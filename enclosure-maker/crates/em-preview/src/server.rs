use crate::state::ChatState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
struct AppState {
    chat: Arc<ChatState>,
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

fn build_router(chat: Arc<ChatState>) -> Router {
    let app_state = AppState { chat };
    Router::new()
        .route("/", get(index))
        .route("/preview.js", get(preview_js))
        .route("/ws", get(ws_handler))
        .with_state(app_state)
}

/// Starts the chat server, blocking forever on a fixed port -- what the
/// CLI's `chat` subcommand uses.
pub async fn serve(port: u16, chat: Arc<ChatState>) -> anyhow::Result<()> {
    let app = build_router(chat);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Chat server running at http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Binds to an OS-assigned free port and starts serving in the background
/// (does not block). Returns the bound address once the listener is ready,
/// for an embedder (e.g. a desktop wrapper) that wants a URL to point a
/// window at, as opposed to [`serve`], which blocks forever on a fixed port.
pub async fn spawn(chat: Arc<ChatState>) -> anyhow::Result<std::net::SocketAddr> {
    let app = build_router(chat);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("chat server error: {e}");
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

async fn ws_handler(ws: WebSocketUpgrade, State(app_state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: AppState) {
    let AppState { chat } = app_state;
    let mut chat_rx = chat.subscribe_chat();
    let (mut sender, mut receiver) = socket.split();

    let mut send_task = tokio::spawn(async move {
        loop {
            match chat_rx.recv().await {
                Ok(msg) => {
                    if sender.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
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
                Some("chat") => {
                    if let Ok(msg) = serde_json::from_str::<ChatMessage>(&text) {
                        chat.send_chat_message(msg.provider, msg.text).await;
                    }
                }
                Some("history_list") => {
                    let chats = chat.list_chat_history();
                    chat.broadcast(
                        serde_json::json!({ "kind": "history_list_result", "chats": chats })
                            .to_string(),
                    );
                }
                Some("history_load") => {
                    if let Ok(req) = serde_json::from_str::<ChatHistoryFile>(&text) {
                        let envelope = match chat.load_chat_history(&req.file) {
                            Ok(lines) => serde_json::json!({
                                "kind": "history_load_result", "ok": true,
                                "file": req.file, "lines": parse_chat_lines(&lines),
                            }),
                            Err(e) => serde_json::json!({
                                "kind": "history_load_result", "ok": false,
                                "file": req.file, "message": e,
                            }),
                        };
                        chat.broadcast(envelope.to_string());
                    }
                }
                Some("history_delete") => {
                    if let Ok(req) = serde_json::from_str::<ChatHistoryFile>(&text) {
                        let ok = chat.delete_chat_history(&req.file).is_ok();
                        chat.broadcast(serde_json::json!({ "kind": "history_delete_result", "ok": ok, "file": req.file }).to_string());
                        let chats = chat.list_chat_history();
                        chat.broadcast(
                            serde_json::json!({ "kind": "history_list_result", "chats": chats })
                                .to_string(),
                        );
                    }
                }
                Some("history_new") => {
                    chat.new_chat().await;
                    chat.broadcast(serde_json::json!({ "kind": "history_new_result" }).to_string());
                }
                Some("history_continue") => {
                    if let Ok(req) = serde_json::from_str::<ChatHistoryFile>(&text) {
                        let lines = chat.load_chat_history(&req.file).unwrap_or_default();
                        chat.continue_chat(req.file.clone()).await;
                        let provider = chat.current_chat_provider();
                        chat.broadcast(
                            serde_json::json!({
                                "kind": "history_continue_result", "file": req.file,
                                "lines": parse_chat_lines(&lines), "provider": provider,
                            })
                            .to_string(),
                        );
                    }
                }
                _ => {}
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
    async fn chat_message_round_trips_history_list_over_the_socket() {
        let dir = std::env::temp_dir().join(format!("em-preview-server-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let chat = ChatState::new(dir.clone());
        let addr = spawn(chat).await.unwrap();
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();

        ws.send(WsMessage::text(
            serde_json::json!({ "kind": "history_list" }).to_string(),
        ))
        .await
        .unwrap();
        let msg = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let value: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(value["kind"], "history_list_result");
        assert_eq!(value["chats"].as_array().unwrap().len(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn history_new_clears_the_live_thread_and_reports_back() {
        let dir = std::env::temp_dir().join(format!("em-preview-server-new-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let chat = ChatState::new(dir.clone());
        let addr = spawn(chat).await.unwrap();
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();

        ws.send(WsMessage::text(
            serde_json::json!({ "kind": "history_new" }).to_string(),
        ))
        .await
        .unwrap();
        let msg = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let value: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(value["kind"], "history_new_result");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
