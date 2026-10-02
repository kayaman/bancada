use crate::state::{PreviewState, PreviewUpdate};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, watch};

#[derive(Clone)]
struct AppState {
    preview: Arc<PreviewState>,
    rx: watch::Receiver<Arc<PreviewUpdate>>,
}

#[derive(serde::Deserialize)]
struct ParamUpdate {
    name: String,
    value: f64,
}

#[derive(serde::Deserialize)]
struct ChatMessage {
    text: String,
    #[serde(default)]
    provider: em_agent::AgentProvider,
}

#[derive(serde::Deserialize)]
struct ChatHistoryFile {
    file: String,
}

#[derive(serde::Deserialize)]
struct ScriptEdit {
    source: String,
}

#[derive(serde::Deserialize)]
struct PartEdit {
    name: String,
    #[serde(default)]
    request_id: String,
    #[serde(flatten)]
    transform: crate::transforms::PartTransform,
}

#[derive(serde::Deserialize)]
struct NumberEdit {
    source: String,
    id: String,
    value: f64,
}

/// Parses a persisted chat's `.ndjson` lines into JSON values for the
/// client, dropping (not failing on) any line that somehow isn't valid
/// JSON -- the file is append-only and each line was validated at write
/// time, but a client replaying it shouldn't choke on one bad line.
fn parse_chat_lines(lines: &[String]) -> Vec<serde_json::Value> {
    lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

fn build_router(preview: Arc<PreviewState>, rx: watch::Receiver<Arc<PreviewUpdate>>) -> Router {
    let app_state = AppState { preview, rx };
    Router::new()
        .route("/", get(index))
        .route("/preview.js", get(preview_js))
        .route("/model-tools.js", get(model_tools_js))
        .route("/ws", get(ws_handler))
        .route("/export.stl", get(export_stl))
        .with_state(app_state)
}

pub async fn serve(
    port: u16,
    preview: Arc<PreviewState>,
    rx: watch::Receiver<Arc<PreviewUpdate>>,
) -> anyhow::Result<()> {
    let app = build_router(preview, rx);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Preview server running at http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Binds to an OS-assigned free port and starts serving in the background
/// (does not block). Returns the bound address once the listener is ready,
/// for an embedder (e.g. a desktop wrapper) that wants a URL to point a
/// window at, as opposed to [`serve`], which blocks forever on a fixed port
/// and is what the CLI's `preview` subcommand uses.
pub async fn spawn(
    preview: Arc<PreviewState>,
    rx: watch::Receiver<Arc<PreviewUpdate>>,
) -> anyhow::Result<std::net::SocketAddr> {
    let app = build_router(preview, rx);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("preview server error: {e}");
        }
    });
    Ok(addr)
}

async fn index() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../../../static/index.html"),
    )
}

async fn preview_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../../static/preview.js"),
    )
}

async fn model_tools_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../../static/model-tools.js"),
    )
}

#[derive(serde::Serialize)]
struct ExportFile {
    filename: String,
    data: String,
}

#[derive(serde::Serialize)]
struct ExportBody {
    files: Vec<ExportFile>,
}

/// Downloads the current model without re-running CSG. The body is one binary
/// STL per printable part, base64-encoded so a multi-part script can save
/// each file on its own.
async fn export_stl(State(app_state): State<AppState>) -> impl IntoResponse {
    let update = app_state.rx.borrow().clone();
    let Some(downloads) = update.build_download(
        &app_state.preview.export_stem(),
        app_state.preview.selected_part(),
    ) else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "no model to export yet",
        )
            .into_response();
    };
    let body = ExportBody {
        files: downloads
            .into_iter()
            .map(|file| ExportFile {
                filename: file.filename,
                data: base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    file.bytes,
                ),
            })
            .collect(),
    };
    (
        [(header::CONTENT_TYPE, "application/json".to_string())],
        serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string()),
    )
        .into_response()
}

async fn ws_handler(ws: WebSocketUpgrade, State(app_state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

async fn send_update(
    sender: &mut SplitSink<WebSocket, Message>,
    update: &PreviewUpdate,
) -> Result<(), axum::Error> {
    sender
        .send(Message::Text(update.params_json.clone().into()))
        .await?;
    sender
        .send(Message::Binary(update.mesh_wire.clone().into()))
        .await?;
    sender
        .send(Message::Text(update.parts_json.clone().into()))
        .await?;
    Ok(())
}

async fn handle_socket(socket: WebSocket, app_state: AppState) {
    let AppState { preview, mut rx } = app_state;
    let mut chat_rx = preview.subscribe_chat();
    let (mut sender, mut receiver) = socket.split();

    // `borrow_and_update` (not plain `borrow`) marks this receiver as having
    // seen the current value -- otherwise the send_task's first
    // `rx.changed()` below fires immediately as a spurious duplicate of the
    // value just sent here, shifting every later update out of sync with
    // what a client expects to receive next.
    let current = rx.borrow_and_update().clone();
    if send_update(&mut sender, &current).await.is_err() {
        return;
    }
    // After the schema/mesh pair, so clients that key off the first text
    // frame still see the parameter array. Slider moves do not repeat this;
    // only a connect and a later disk change (via broadcast_script) do.
    if sender
        .send(Message::Text(preview.script_message().into()))
        .await
        .is_err()
    {
        return;
    }
    if let Some(status) = preview.evaluation_status() {
        if sender.send(Message::Text(status.into())).await.is_err() {
            return;
        }
    }

    // One task drives both outgoing streams (mesh/param updates and chat
    // events) since they share the same WebSocket sender half.
    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let update = rx.borrow_and_update().clone();
                    if send_update(&mut sender, &update).await.is_err() {
                        break;
                    }
                }
                chat = chat_rx.recv() => {
                    match chat {
                        Ok(msg) => {
                            if sender.send(Message::Text(msg.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            let Message::Text(text) = msg else { continue };
            let kind = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string));

            match kind.as_deref() {
                Some("code_control") => {
                    let result = serde_json::from_str::<NumberEdit>(&text)
                        .map_err(|e| e.to_string())
                        .and_then(|edit| preview.edit_number(&edit.source, &edit.id, edit.value));
                    preview.broadcast(serde_json::json!({ "kind": "code_control_result", "ok": result.is_ok(), "message": result.err() }).to_string());
                    preview.broadcast_script();
                }
                Some("part_transform") => {
                    let request_id = serde_json::from_str::<PartEdit>(&text)
                        .map(|edit| edit.request_id)
                        .unwrap_or_default();
                    let result = serde_json::from_str::<PartEdit>(&text)
                        .map_err(|e| e.to_string())
                        .and_then(|edit| preview.set_part_transform(&edit.name, edit.transform));
                    preview.broadcast(
                        serde_json::json!({
                            "kind": "part_transform_result", "ok": result.is_ok(),
                            "message": result.err(),
                            "request_id": request_id,
                        })
                        .to_string(),
                    );
                }
                Some("chat") => {
                    if let Ok(chat) = serde_json::from_str::<ChatMessage>(&text) {
                        preview.send_chat_message(chat.provider, chat.text).await;
                    }
                }
                Some("save") => {
                    let envelope = match preview.save_params() {
                        Ok(baked) if baked.is_empty() => {
                            serde_json::json!({ "kind": "save_result", "ok": true, "baked": [], "message": "nothing to save" })
                        }
                        Ok(baked) => {
                            serde_json::json!({ "kind": "save_result", "ok": true, "baked": baked })
                        }
                        Err(e) => {
                            serde_json::json!({ "kind": "save_result", "ok": false, "message": e.to_string() })
                        }
                    };
                    preview.broadcast(envelope.to_string());
                }
                Some("history_list") => {
                    let chats = preview.list_chat_history();
                    preview.broadcast(
                        serde_json::json!({ "kind": "history_list_result", "chats": chats })
                            .to_string(),
                    );
                }
                Some("history_load") => {
                    if let Ok(req) = serde_json::from_str::<ChatHistoryFile>(&text) {
                        let envelope = match preview.load_chat_history(&req.file) {
                            Ok(lines) => serde_json::json!({
                                "kind": "history_load_result", "ok": true,
                                "file": req.file, "lines": parse_chat_lines(&lines),
                            }),
                            Err(e) => serde_json::json!({
                                "kind": "history_load_result", "ok": false,
                                "file": req.file, "message": e,
                            }),
                        };
                        preview.broadcast(envelope.to_string());
                    }
                }
                Some("history_delete") => {
                    if let Ok(req) = serde_json::from_str::<ChatHistoryFile>(&text) {
                        let ok = preview.delete_chat_history(&req.file).is_ok();
                        preview.broadcast(serde_json::json!({ "kind": "history_delete_result", "ok": ok, "file": req.file }).to_string());
                        let chats = preview.list_chat_history();
                        preview.broadcast(
                            serde_json::json!({ "kind": "history_list_result", "chats": chats })
                                .to_string(),
                        );
                    }
                }
                Some("script") => {
                    if let Ok(edit) = serde_json::from_str::<ScriptEdit>(&text) {
                        if let Err(e) = preview.write_script(&edit.source) {
                            preview.broadcast(
                                serde_json::json!({ "kind": "script_error", "message": e.to_string() }).to_string(),
                            );
                        }
                    }
                }
                Some("preview") => {
                    if let Ok(edit) = serde_json::from_str::<ScriptEdit>(&text) {
                        match preview.write_script(&edit.source) {
                            Ok(_) => preview.reeval(),
                            Err(e) => preview.broadcast(
                                serde_json::json!({ "kind": "script_error", "message": e.to_string() }).to_string(),
                            ),
                        }
                    }
                }
                Some("history_new") => {
                    preview.new_chat().await;
                    preview
                        .broadcast(serde_json::json!({ "kind": "history_new_result" }).to_string());
                }
                Some("history_continue") => {
                    if let Ok(req) = serde_json::from_str::<ChatHistoryFile>(&text) {
                        let lines = preview.load_chat_history(&req.file).unwrap_or_default();
                        preview.continue_chat(req.file.clone()).await;
                        let provider = preview.current_chat_provider();
                        preview.broadcast(
                            serde_json::json!({
                                "kind": "history_continue_result", "file": req.file,
                                "lines": parse_chat_lines(&lines), "provider": provider,
                            })
                            .to_string(),
                        );
                    }
                }
                _ => {
                    if let Ok(update) = serde_json::from_str::<ParamUpdate>(&text) {
                        preview.set_param(update.name, update.value);
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    #[tokio::test]
    async fn connecting_during_startup_receives_loading_then_model_or_error() {
        let dir =
            std::env::temp_dir().join(format!("em-preview-startup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.rhai");
        std::fs::write(&path, "invalid rhai {").unwrap();
        let (preview, rx) = PreviewState::new(path.clone(), None);
        let addr = spawn(preview.clone(), rx).await.unwrap();
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();
        for _ in 0..4 {
            ws.next().await.unwrap().unwrap();
        }
        let loading = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&loading).unwrap()["kind"],
            "script_evaluating"
        );

        preview.reeval();
        let error = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&error).unwrap()["kind"],
            "script_error"
        );

        // A client arriving after startup failed must receive that error too.
        let (mut late, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();
        for _ in 0..4 {
            late.next().await.unwrap().unwrap();
        }
        assert_eq!(
            late.next().await.unwrap().unwrap().into_text().unwrap(),
            error
        );

        std::fs::write(&path, "emit(cuboid(10.0, 10.0, 10.0));").unwrap();
        preview.reeval();
        ws.next().await.unwrap().unwrap(); // parameter schema
        let mesh = ws.next().await.unwrap().unwrap().into_data();
        assert!(mesh.len() > 4);
        assert!(preview.evaluation_status().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn preview_renders_submitted_code_even_when_unchanged_and_reports_errors() {
        let dir =
            std::env::temp_dir().join(format!("em-preview-button-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.rhai");
        let initial_source = "emit(cuboid(10.0, 10.0, 10.0));";
        std::fs::write(&path, initial_source).unwrap();
        let (preview, rx) = PreviewState::new(path.clone(), None);
        preview.reeval();
        let initial_mesh = rx.borrow().mesh_wire.clone();
        // No file watcher: a Preview request must evaluate directly,
        // including when the source is already identical to the file.
        let addr = spawn(preview, rx.clone()).await.unwrap();
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();
        for _ in 0..4 {
            ws.next().await.unwrap().unwrap();
        }

        let source = "emit(cuboid(25.0, 10.0, 10.0));";
        for _ in 0..2 {
            ws.send(WsMessage::text(
                serde_json::json!({ "kind": "preview", "source": source }).to_string(),
            ))
            .await
            .unwrap();
            let schema = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(
                serde_json::from_str::<serde_json::Value>(&schema.into_text().unwrap())
                    .unwrap()
                    .is_array()
            );
            let mesh = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .into_data();
            assert_ne!(mesh.as_ref(), initial_mesh.as_slice());
            assert_eq!(mesh.as_ref(), rx.borrow().mesh_wire.as_slice());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
            let parts = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&parts).unwrap()["kind"],
                "parts"
            );
        }

        let last_good = rx.borrow().mesh_wire.clone();
        ws.send(WsMessage::text(
            serde_json::json!({ "kind": "preview", "source": "invalid rhai {" }).to_string(),
        ))
        .await
        .unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let error: serde_json::Value = serde_json::from_str(&error.into_text().unwrap()).unwrap();
        assert_eq!(error["kind"], "script_error");
        assert!(!error["message"].as_str().unwrap().is_empty());
        assert_eq!(rx.borrow().mesh_wire, last_good);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Regression test for a real bug: the WS handler's initial send used
    /// `rx.borrow()` instead of `rx.borrow_and_update()`, so the send loop's
    /// first `rx.changed()` fired immediately on a spurious "duplicate" of
    /// the just-sent initial value, shifting every subsequent client message
    /// out of sync with the actual parameter state by one update.
    #[tokio::test]
    async fn param_override_reaches_the_next_message_not_a_stale_duplicate() {
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/enclosure_assembly.rhai"
        );
        let (preview, rx) = PreviewState::new(script.into(), Some("base".to_string()));
        preview.reeval();

        let app = build_router(preview, rx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();

        // Initial pair: schema (text) then mesh (binary), reflecting defaults.
        // The script frame follows that pair; it must not land in front of it.
        let msg1 = ws.next().await.unwrap().unwrap();
        let _msg2 = ws.next().await.unwrap().unwrap();
        let schema: serde_json::Value =
            serde_json::from_str(msg1.into_text().unwrap().as_str()).unwrap();
        let width = schema
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Width")
            .unwrap();
        assert_eq!(width["value"], 60.0);

        let parts_msg = ws.next().await.unwrap().unwrap();
        let parts: serde_json::Value =
            serde_json::from_str(&parts_msg.into_text().unwrap()).unwrap();
        assert_eq!(parts["kind"], "parts");
        let script_msg = ws.next().await.unwrap().unwrap();
        let script: serde_json::Value =
            serde_json::from_str(script_msg.into_text().unwrap().as_str()).unwrap();
        assert_eq!(script["kind"], "script");

        ws.send(WsMessage::text(r#"{"name":"Width","value":90.0}"#))
            .await
            .unwrap();

        // The very next pair received must already reflect the override --
        // not a stale repeat of the initial state.
        let msg3 = ws.next().await.unwrap().unwrap();
        let _msg4 = ws.next().await.unwrap().unwrap();
        let schema2: serde_json::Value =
            serde_json::from_str(msg3.into_text().unwrap().as_str()).unwrap();
        let width2 = schema2
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Width")
            .unwrap();
        assert_eq!(
            width2["value"], 90.0,
            "expected the override to land in the very next update, got a stale duplicate instead"
        );
    }
}
