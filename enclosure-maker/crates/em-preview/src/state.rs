use crate::agent_session::AgentSession;
use crate::chatlog;
use crate::protocol::mesh_to_wire;
use crate::transforms::{self, PartTransform, Transforms};
use em_agent::AgentProvider;
use em_core::Mesh;
use em_script::ScriptEngine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::{broadcast, watch};

#[derive(Clone, Debug, serde::Serialize)]
pub struct ParamSchema {
    pub name: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub value: f64,
}

/// One push to every connected preview client: the mesh to render (binary
/// wire format) and the current parameter schema (JSON), sent together so a
/// client always has a consistent pair. `export_parts` is the named meshes
/// the download should contain — one binary STL per part — kept here so
/// export does not re-run CSG.
pub struct PreviewUpdate {
    pub mesh_wire: Vec<u8>,
    pub params_json: String,
    pub parts_json: String,
    pub export_parts: Vec<(String, Mesh)>,
}

pub struct PreparedDownload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

impl PreviewUpdate {
    /// One binary STL per printable part. A single part uses `{stem}.stl`,
    /// or `{stem}-{part}.stl` when `--part` selected it. Several parts each
    /// get `{stem}-{name}.stl`. `None` before the first successful eval.
    pub fn build_download(
        &self,
        stem: &str,
        selected_part: Option<&str>,
    ) -> Option<Vec<PreparedDownload>> {
        if self.export_parts.is_empty() {
            return None;
        }
        let single = self.export_parts.len() == 1;
        Some(
            self.export_parts
                .iter()
                .map(|(name, mesh)| {
                    let filename = if single {
                        match selected_part {
                            Some(part) => format!("{stem}-{part}.stl"),
                            None => format!("{stem}.stl"),
                        }
                    } else {
                        format!("{stem}-{name}.stl")
                    };
                    PreparedDownload {
                        filename,
                        bytes: em_export::to_binary_stl(mesh),
                    }
                })
                .collect(),
        )
    }
}

/// Shared state behind a running `preview` session. Both the file watcher
/// (on a script/library edit) and the WebSocket handler (on a slider move
/// from the browser) call [`reeval`](Self::reeval) through this same state,
/// so either trigger produces a consistent push to every connected client.
pub struct PreviewState {
    script_path: PathBuf,
    script_dir: PathBuf,
    part: Option<String>,
    overrides: Mutex<HashMap<String, f64>>,
    evaluation_revision: Mutex<u64>,
    evaluation_status: Mutex<Option<String>>,
    model: Mutex<ModelState>,
    tx: watch::Sender<Arc<PreviewUpdate>>,
    agent: tokio::sync::Mutex<Option<AgentSession>>,
    chat_tx: broadcast::Sender<String>,
    /// The chat file the *next* message appends to. `None` means "no
    /// thread chosen yet" -- the next `send_chat_message` starts a fresh
    /// one. Set by `send_chat_message` itself (first message of a new
    /// thread) or by `continue_chat` (resuming a past one).
    current_chat_file: Mutex<Option<String>>,
    current_chat_provider: Mutex<Option<AgentProvider>>,
}

impl PreviewState {
    pub fn new(
        script_path: PathBuf,
        part: Option<String>,
    ) -> (Arc<Self>, watch::Receiver<Arc<PreviewUpdate>>) {
        let script_dir = script_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let initial = Arc::new(PreviewUpdate {
            mesh_wire: mesh_to_wire(&Mesh::default()),
            params_json: "[]".to_string(),
            parts_json: serde_json::json!({ "kind": "parts", "parts": [] }).to_string(),
            export_parts: Vec::new(),
        });
        let (tx, rx) = watch::channel(initial);
        let (chat_tx, _) = broadcast::channel(256);
        let (transforms, load_error) = match transforms::load(&script_path) {
            Ok(transforms) => (transforms, None),
            Err(error) => (Transforms::new(), Some(error)),
        };
        let state = Arc::new(PreviewState {
            script_path,
            script_dir,
            part,
            overrides: Mutex::new(HashMap::new()),
            evaluation_revision: Mutex::new(0),
            evaluation_status: Mutex::new(Some(
                serde_json::json!({ "kind": "script_evaluating" }).to_string(),
            )),
            tx,
            model: Mutex::new(ModelState {
                raw: None,
                transforms,
                load_error,
            }),
            agent: tokio::sync::Mutex::new(None),
            chat_tx,
            current_chat_file: Mutex::new(None),
            current_chat_provider: Mutex::new(None),
        });
        (state, rx)
    }

    pub fn script_dir(&self) -> &Path {
        &self.script_dir
    }

    /// Replayed on connect so loading and startup errors are visible even
    /// when evaluation begins or finishes before a client subscribes.
    pub fn evaluation_status(&self) -> Option<String> {
        self.evaluation_status.lock().unwrap().clone()
    }

    /// JSON text frame for the code pane: the main script's filename and
    /// its current contents. Missing file reads as an empty source so a
    /// client can still open the pane and create it by typing.
    pub fn script_message(&self) -> String {
        let name = self
            .script_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("script.rhai");
        let source = std::fs::read_to_string(&self.script_path).unwrap_or_default();
        serde_json::json!({ "kind": "script", "name": name, "controls": crate::controls::numbers(&source), "source": source }).to_string()
    }

    pub fn broadcast_script(&self) {
        self.broadcast(self.script_message());
    }

    pub fn edit_number(&self, source: &str, id: &str, value: f64) -> Result<(), String> {
        let current = std::fs::read_to_string(&self.script_path).map_err(|e| e.to_string())?;
        if current != source {
            return Err("The code changed. Use the refreshed controls and try again.".into());
        }
        let edited = crate::controls::edit(&current, id, value)?;
        self.write_script(&edited).map_err(|e| e.to_string())?;
        self.broadcast_script();
        Ok(())
    }

    /// Replaces the main script on disk when `source` differs from what's
    /// already there. Returns whether a write happened. Does not re-evaluate;
    /// the file watcher picks up the change and reloads.
    pub fn write_script(&self, source: &str) -> std::io::Result<bool> {
        match std::fs::read_to_string(&self.script_path) {
            Ok(current) if current == source => Ok(false),
            _ => {
                // Write aside and rename so the watcher never reloads a
                // half-written script. The temp name is not `*.rhai`, so the
                // watcher ignores it.
                let tmp = self.script_path.with_extension("rhai.tmp");
                std::fs::write(&tmp, source)?;
                std::fs::rename(&tmp, &self.script_path)?;
                Ok(true)
            }
        }
    }

    pub fn subscribe_chat(&self) -> broadcast::Receiver<String> {
        self.chat_tx.subscribe()
    }

    /// Sends a pre-serialized JSON text message to every connected client,
    /// over the same channel chat events use (it's a general "push a text
    /// message to every tab" broadcast at this point, not chat-exclusive).
    pub fn broadcast(&self, json: String) {
        let _ = self.chat_tx.send(json);
    }

    /// Sends `text` to the assistant, starting a session (confined to
    /// `self.script_dir()`) on the first call of a new thread, or resuming
    /// natively (`--resume`) if `current_chat_file` was set by
    /// [`continue_chat`](Self::continue_chat). Every event the session
    /// produces is both persisted to that thread's `.ndjson` file and
    /// broadcast live -- the same file backs both the live view and later
    /// history browsing, so there's nothing to keep in sync between them.
    /// Any file edit the assistant makes is picked up automatically by the
    /// existing file watcher -- no extra wiring needed to refresh the
    /// preview.
    pub async fn send_chat_message(&self, requested_provider: AgentProvider, text: String) {
        let mut agent = self.agent.lock().await;
        if agent
            .as_ref()
            .is_some_and(|session| session.provider() != requested_provider)
        {
            *agent = None;
            *self.current_chat_file.lock().unwrap() = None;
            *self.current_chat_provider.lock().unwrap() = None;
        }
        if agent.is_none() {
            let (file, resume_id, provider) = {
                let mut current = self.current_chat_file.lock().unwrap();
                match current.clone() {
                    Some(file) => {
                        let lines = chatlog::load_chat(&self.script_dir, &file).unwrap_or_default();
                        let provider = chatlog::extract_provider(&lines);
                        (file, chatlog::extract_session_id(&lines), provider)
                    }
                    None => {
                        let file = chatlog::new_chat_filename(SystemTime::now());
                        let meta = serde_json::json!({
                            "op": "meta", "provider": requested_provider,
                            "startedAtUnix": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
                        });
                        let _ = chatlog::append_line(&self.script_dir, &file, &meta.to_string());
                        chatlog::prune_default(&self.script_dir);
                        *current = Some(file.clone());
                        (file, None, requested_provider)
                    }
                }
            };
            *self.current_chat_provider.lock().unwrap() = Some(provider);

            match AgentSession::spawn(provider, &self.script_dir, resume_id).await {
                Ok((session, mut events)) => {
                    *agent = Some(session);
                    let chat_tx = self.chat_tx.clone();
                    let script_dir = self.script_dir.clone();
                    tokio::spawn(async move {
                        while let Some(line) = events.recv().await {
                            let event_value = serde_json::from_str::<serde_json::Value>(&line)
                                .unwrap_or(serde_json::Value::Null);
                            let op =
                                serde_json::json!({ "op": "event", "event": event_value.clone() });
                            let _ = chatlog::append_line(&script_dir, &file, &op.to_string());

                            let envelope =
                                serde_json::json!({ "kind": "chat_event", "event": event_value });
                            let _ = chat_tx.send(envelope.to_string());
                        }
                    });
                }
                Err(e) => {
                    self.broadcast(
                        serde_json::json!({ "kind": "chat_error", "message": e.to_string() })
                            .to_string(),
                    );
                    return;
                }
            }
        }

        if let Some(file) = self.current_chat_file.lock().unwrap().clone() {
            let op = serde_json::json!({ "op": "userSent", "text": text });
            let _ = chatlog::append_line(&self.script_dir, &file, &op.to_string());
        }

        if let Some(session) = agent.as_mut() {
            if let Err(e) = session.send_user_message(&text).await {
                self.broadcast(
                    serde_json::json!({ "kind": "chat_error", "message": e.to_string() })
                        .to_string(),
                );
            }
        }
    }

    /// Ends the current live thread (if any) without deleting its history,
    /// so the next message starts a brand new thread/file.
    pub async fn new_chat(&self) {
        *self.agent.lock().await = None;
        *self.current_chat_file.lock().unwrap() = None;
        *self.current_chat_provider.lock().unwrap() = None;
    }

    /// Arms `file` to be resumed by the *next* `send_chat_message` call:
    /// stops whatever thread is currently live (its own file is untouched
    /// -- it can be continued again later) and points at `file` instead.
    /// Does not itself send anything or start a process; the caller
    /// typically also pushes `file`'s replayed lines to the client so the
    /// visible chat log matches what's about to be resumed.
    pub async fn continue_chat(&self, file: String) {
        *self.agent.lock().await = None;
        let lines = chatlog::load_chat(&self.script_dir, &file).unwrap_or_default();
        *self.current_chat_provider.lock().unwrap() = Some(chatlog::extract_provider(&lines));
        *self.current_chat_file.lock().unwrap() = Some(file);
    }

    pub fn current_chat_provider(&self) -> AgentProvider {
        self.current_chat_provider
            .lock()
            .unwrap()
            .unwrap_or_default()
    }

    pub fn list_chat_history(&self) -> Vec<chatlog::ChatEntry> {
        chatlog::list_chats(&self.script_dir)
    }

    pub fn load_chat_history(&self, file: &str) -> Result<Vec<String>, String> {
        chatlog::load_chat(&self.script_dir, file).map_err(|e| e.to_string())
    }

    pub fn delete_chat_history(&self, file: &str) -> Result<(), String> {
        chatlog::delete_chat(&self.script_dir, file).map_err(|e| e.to_string())
    }

    pub fn set_param(&self, name: String, value: f64) {
        self.overrides.lock().unwrap().insert(name, value);
        self.reeval();
    }

    pub fn set_part_transform(&self, name: &str, transform: PartTransform) -> Result<(), String> {
        transform.validate()?;
        let mut model = self.model.lock().unwrap();
        if let Some(error) = &model.load_error {
            return Err(error.clone());
        }
        let raw = model
            .raw
            .as_ref()
            .ok_or("Wait for the model to finish building.")?;
        if !raw.export_parts.iter().any(|(part, _)| part == name) {
            return Err(format!("Part '{name}' is no longer in the model."));
        }
        let mut updated = model.transforms.clone();
        if transform == PartTransform::default() {
            updated.remove(name);
        } else {
            updated.insert(name.to_string(), transform);
        }
        transforms::save(&self.script_path, &updated)?;
        model.transforms = updated;
        let update = model
            .raw
            .as_ref()
            .unwrap()
            .with_transforms(&model.transforms);
        self.tx.send_replace(Arc::new(update));
        Ok(())
    }

    /// The script's file stem, used as the download name before the extension.
    pub fn export_stem(&self) -> String {
        self.script_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("model")
            .to_string()
    }

    /// The `--part` selection, if the preview is showing a single part.
    pub fn selected_part(&self) -> Option<&str> {
        self.part.as_deref()
    }

    /// Rewrites the script file's `param(...)` calls so the current slider
    /// overrides become the new defaults, then clears those overrides
    /// (they're now redundant with the script's own default) and triggers
    /// the normal reeval/broadcast path. Returns the names actually baked
    /// in, or an I/O error.
    pub fn save_params(&self) -> std::io::Result<Vec<String>> {
        let overrides = self.overrides.lock().unwrap().clone();
        if overrides.is_empty() {
            return Ok(Vec::new());
        }
        let source = std::fs::read_to_string(&self.script_path)?;
        let (rewritten, baked) = crate::save::bake_param_defaults(&source, &overrides);
        if !baked.is_empty() {
            std::fs::write(&self.script_path, rewritten)?;
            let mut guard = self.overrides.lock().unwrap();
            for name in &baked {
                guard.remove(name);
            }
        }
        Ok(baked)
    }

    pub fn reeval(&self) {
        let revision = {
            let mut revision = self.evaluation_revision.lock().unwrap();
            *revision += 1;
            *revision
        };
        let overrides = self.overrides.lock().unwrap().clone();
        let script_path = self.script_path.clone();
        let script_dir = self.script_dir.clone();
        let part = self.part.clone();

        // Boolean CSG recurses deeply enough to overflow a Tokio worker's
        // 2MB stack, which aborts the whole process. Evaluate on an 8MB
        // thread and turn a panic into a script error.
        let built = std::thread::Builder::new()
            .name("csg-eval".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    evaluate_script(&script_path, &script_dir, part.as_deref(), &overrides)
                }))
            });
        let built = match built {
            Ok(handle) => match handle.join() {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_)) | Err(_) => Err("geometry evaluation crashed".to_string()),
            },
            Err(e) => Err(format!("could not start geometry evaluation: {e}")),
        };

        // Startup, file changes, and explicit previews may overlap. Only
        // the newest request may publish a model or an error. Hold this
        // guard through publication so a newer request cannot slip between
        // the revision check and sending the update.
        let latest = self.evaluation_revision.lock().unwrap();
        if revision != *latest {
            return;
        }
        match built {
            Ok(Some(update)) => {
                *self.evaluation_status.lock().unwrap() = None;
                println!(
                    "loaded {} ({} triangles{})",
                    self.script_path.display(),
                    update.triangle_count,
                    self.part
                        .as_deref()
                        .map(|n| format!(", part={n}"))
                        .unwrap_or_default()
                );
                let mut model = self.model.lock().unwrap();
                let transformed = update.with_transforms(&model.transforms);
                model.raw = Some(update);
                self.tx.send_replace(Arc::new(transformed));
            }
            Ok(None) => {}
            Err(message) => {
                eprintln!("script error: {message}");
                let status =
                    serde_json::json!({ "kind": "script_error", "message": message }).to_string();
                *self.evaluation_status.lock().unwrap() = Some(status.clone());
                self.broadcast(status);
            }
        }
    }
}

struct EvalUpdate {
    triangle_count: usize,
    params_json: String,
    export_parts: Vec<(String, Mesh)>,
}

struct ModelState {
    raw: Option<EvalUpdate>,
    transforms: Transforms,
    load_error: Option<String>,
}

impl EvalUpdate {
    fn with_transforms(&self, transforms: &Transforms) -> PreviewUpdate {
        let mut triangles = Vec::new();
        let mut parts = Vec::new();
        let mut export_parts = Vec::new();
        for (name, mesh) in &self.export_parts {
            let transform = transforms.get(name).cloned().unwrap_or_default();
            parts.push(serde_json::json!({
                "name": name, "start_triangle": triangles.len(),
                "triangle_count": mesh.triangle_count(), "center": transforms::center(mesh),
                "translation": transform.translation, "rotation": transform.rotation,
            }));
            triangles.extend(mesh.triangles.iter().copied());
            export_parts.push((name.clone(), transform.apply(mesh)));
        }
        PreviewUpdate {
            mesh_wire: mesh_to_wire(&Mesh { triangles }),
            params_json: self.params_json.clone(),
            parts_json: serde_json::json!({ "kind": "parts", "parts": parts }).to_string(),
            export_parts,
        }
    }
}

/// `Ok(None)` when `--part` names something the script did not emit: that
/// case is reported on stderr and leaves the previous preview in place.
fn evaluate_script(
    script_path: &Path,
    script_dir: &Path,
    part: Option<&str>,
    overrides: &HashMap<String, f64>,
) -> Result<Option<EvalUpdate>, String> {
    // A fresh engine per reload avoids Rhai's FileModuleResolver caching
    // a now-stale copy of an imported shared library file.
    let engine = ScriptEngine::with_import_root(script_dir);
    let (scene, params) = engine
        .eval_file_with_params(script_path, overrides)
        .map_err(|e| e.to_string())?;

    let (mesh, export_parts) = match part {
        Some(name) => match scene.part(name) {
            Some(m) => {
                let mesh = m.clone();
                (mesh.clone(), vec![(name.to_string(), mesh)])
            }
            None => {
                eprintln!(
                    "no part named '{name}' (available: {})",
                    scene.part_names().collect::<Vec<_>>().join(", ")
                );
                return Ok(None);
            }
        },
        None => (scene.assembly_mesh(), scene.printable_parts().to_vec()),
    };

    let triangle_count = mesh.triangle_count();
    let schema: Vec<ParamSchema> = params
        .into_iter()
        .map(|p| ParamSchema {
            name: p.name,
            default: p.default,
            min: p.min,
            max: p.max,
            value: p.value,
        })
        .collect();
    let params_json = serde_json::to_string(&schema).unwrap_or_else(|_| "[]".to_string());
    Ok(Some(EvalUpdate {
        triangle_count,
        params_json,
        export_parts,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("em-preview-state-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn part_controls_persist_and_affect_exports_without_rebuilding_geometry() {
        let dir = tempdir();
        let script = dir.join("main.rhai");
        let source =
            "emit(\"base\", cuboid(10.0, 20.0, 6.0)); emit(\"lid\", cuboid(4.0, 4.0, 2.0));";
        std::fs::write(&script, source).unwrap();
        let (state, rx) = PreviewState::new(script.clone(), None);
        state.reeval();
        let before = rx.borrow().clone();
        // A transform operates on cached geometry even if a code edit is
        // still invalid, and must preserve all other parts exactly.
        std::fs::write(&script, "invalid rhai {").unwrap();
        let transform = PartTransform {
            translation: [3.25, -2.0, 1.0],
            rotation: [0.0, 0.0, 90.0],
        };
        state.set_part_transform("base", transform.clone()).unwrap();
        let after = rx.borrow().clone();
        assert_eq!(before.mesh_wire, after.mesh_wire); // raw geometry retained
        assert_eq!(
            after.export_parts[1].1.triangles[0][0].pos,
            before.export_parts[1].1.triangles[0][0].pos
        );
        assert_eq!(
            transforms::center(&after.export_parts[0].1),
            transform.translation
        );
        let downloads = after.build_download("main", None).unwrap();
        assert_ne!(
            downloads[0].bytes,
            before.build_download("main", None).unwrap()[0].bytes
        );
        std::fs::write(&script, source).unwrap();
        let (reopened, reopened_rx) = PreviewState::new(script.clone(), None);
        reopened.reeval();
        assert_eq!(
            reopened_rx.borrow().build_download("main", None).unwrap()[0].bytes,
            downloads[0].bytes
        );
        reopened
            .set_part_transform("base", PartTransform::default())
            .unwrap();
        assert_eq!(
            reopened_rx.borrow().build_download("main", None).unwrap()[0].bytes,
            before.build_download("main", None).unwrap()[0].bytes
        );
        assert!(reopened.set_part_transform("missing", transform).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn numeric_boss_control_updates_code_and_rejects_stale_edits() {
        let dir = tempdir();
        let script = dir.join("main.rhai");
        let source = "let boss = screw_boss(3, 12.0, 3, 2.4); emit(boss);";
        std::fs::write(&script, source).unwrap();
        let (state, rx) = PreviewState::new(script.clone(), None);
        state.reeval();
        let old_mesh = rx.borrow().mesh_wire.clone();
        let controls = crate::controls::numbers(source);
        let height = controls
            .iter()
            .find(|c| c.label.starts_with("Boss height"))
            .unwrap();
        state.edit_number(source, &height.id, 18.75).unwrap();
        assert_eq!(
            std::fs::read_to_string(&script).unwrap(),
            source.replace("12.0", "18.75")
        );
        state.reeval();
        assert_ne!(rx.borrow().mesh_wire, old_mesh);
        assert!(state.edit_number(source, &height.id, 20.0).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Exercises the history list/load/delete wiring against real files on
    /// disk (written directly through `chatlog`, the same module
    /// `send_chat_message` itself uses to persist) without spawning a real
    /// provider session -- that plumbing is covered by the agent argument,
    /// event-normalization, and chatlog extraction tests.
    #[test]
    fn history_list_load_and_delete_round_trip_through_preview_state() {
        let dir = tempdir();
        let (state, _rx) = PreviewState::new(dir.join("main.rhai"), None);

        assert!(state.list_chat_history().is_empty());

        let file = chatlog::new_chat_filename(SystemTime::now());
        chatlog::append_line(&dir, &file, r#"{"op":"meta","startedAtUnix":0}"#).unwrap();
        chatlog::append_line(&dir, &file, r#"{"op":"userSent","text":"add a lid"}"#).unwrap();

        let chats = state.list_chat_history();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].file, file);
        assert_eq!(chats[0].title, "add a lid");

        let lines = state.load_chat_history(&file).unwrap();
        assert_eq!(lines.len(), 2);

        state.delete_chat_history(&file).unwrap();
        assert!(state.list_chat_history().is_empty());
    }

    /// `continue_chat` must clear any live agent session (so the next
    /// message spawns a fresh one that resumes the chosen file, rather than
    /// talking to whatever session happened to be live) and `new_chat` must
    /// clear the pinned file (so the next message starts a brand new
    /// thread instead of appending to the old one).
    #[tokio::test]
    async fn continue_and_new_chat_clear_the_live_agent_session() {
        let dir = tempdir();
        let (state, _rx) = PreviewState::new(dir.join("main.rhai"), None);

        state
            .continue_chat("2020-01-01T00-00-00.ndjson".to_string())
            .await;
        assert_eq!(
            *state.current_chat_file.lock().unwrap(),
            Some("2020-01-01T00-00-00.ndjson".to_string())
        );
        assert!(state.agent.lock().await.is_none());

        state.new_chat().await;
        assert_eq!(*state.current_chat_file.lock().unwrap(), None);
        assert!(state.agent.lock().await.is_none());
    }

    #[test]
    fn write_script_writes_when_changed_and_skips_when_unchanged() {
        let dir = tempdir();
        let path = dir.join("main.rhai");
        std::fs::write(&path, "let x = 1;\n").unwrap();
        let (state, _rx) = PreviewState::new(path.clone(), None);

        assert!(state.write_script("let x = 2;\n").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "let x = 2;\n");

        assert!(!state.write_script("let x = 2;\n").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "let x = 2;\n");
    }

    /// A failed eval reports the error to clients and leaves the last mesh
    /// in place, so the view doesn't blank while the script is mid-edit.
    #[test]
    fn reeval_error_broadcasts_script_error_without_replacing_the_mesh() {
        let dir = tempdir();
        let path = dir.join("main.rhai");
        std::fs::write(&path, "this is not valid rhai {\n").unwrap();
        let (state, rx) = PreviewState::new(path, None);
        let mut chat_rx = state.subscribe_chat();
        let before = rx.borrow().mesh_wire.clone();

        state.reeval();

        let msg = chat_rx.try_recv().unwrap();
        let value: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(value["kind"], "script_error");
        assert!(!value["message"].as_str().unwrap_or("").is_empty());
        assert_eq!(rx.borrow().mesh_wire, before);
    }

    #[test]
    fn geometry_panic_keeps_the_last_preview_and_recovers_after_an_edit() {
        let dir = tempdir();
        let path = dir.join("main.rhai");
        std::fs::write(&path, "emit(cuboid(10.0, 10.0, 10.0));").unwrap();
        let (state, mut rx) = PreviewState::new(path.clone(), None);
        let mut chat_rx = state.subscribe_chat();
        state.reeval();
        let before = rx.borrow_and_update().clone();
        assert!(!before.export_parts.is_empty());

        std::fs::write(
            &path,
            "let width = 10.0; emit(rounded_box(width, 10.0, 10.0, 6.0, 8));",
        )
        .unwrap();
        state.reeval();

        let message: serde_json::Value =
            serde_json::from_str(&chat_rx.try_recv().unwrap()).unwrap();
        assert_eq!(message["kind"], "script_error");
        assert!(message["message"]
            .as_str()
            .unwrap()
            .contains("rounded_box: radius"));
        assert!(!rx.has_changed().unwrap());
        assert!(Arc::ptr_eq(&rx.borrow(), &before));

        std::fs::write(&path, "emit(cuboid(20.0, 10.0, 10.0));").unwrap();
        state.reeval();
        assert!(rx.has_changed().unwrap());
        assert_ne!(rx.borrow().mesh_wire, before.mesh_wire);
        assert!(chat_rx.try_recv().is_err());
    }

    #[test]
    fn multi_part_preview_exports_base_and_lid_as_separate_stl_files() {
        let dir = tempdir();
        let path = dir.join("box.rhai");
        std::fs::write(
            &path,
            r#"
            let base = cuboid(20.0, 20.0, 5.0);
            let lid = cuboid(20.0, 20.0, 2.0).translate(0.0, 0.0, 10.0);
            emit("base", base);
            emit("lid", lid);
            view("assembly", base.union(lid));
            "#,
        )
        .unwrap();
        let (state, rx) = PreviewState::new(path, None);
        state.reeval();

        let update = rx.borrow().clone();
        let names: Vec<&str> = update
            .export_parts
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(names, vec!["base", "lid"]);
        let wire_tris = u32::from_le_bytes(update.mesh_wire[0..4].try_into().unwrap()) as usize;
        let part_tris: usize = update
            .export_parts
            .iter()
            .map(|(_, m)| m.triangle_count())
            .sum();
        assert_eq!(
            wire_tris, part_tris,
            "the preview mesh is the printable parts only"
        );

        let downloads = update.build_download(&state.export_stem(), None).unwrap();
        let filenames: Vec<&str> = downloads.iter().map(|d| d.filename.as_str()).collect();
        assert_eq!(filenames, vec!["box-base.stl", "box-lid.stl"]);
        for download in &downloads {
            assert!(download.filename.ends_with(".stl"));
            let tris = u32::from_le_bytes(download.bytes[80..84].try_into().unwrap());
            assert!(tris > 0);
        }
    }

    #[test]
    fn single_part_preview_exports_a_binary_stl() {
        let dir = tempdir();
        let path = dir.join("box.rhai");
        std::fs::write(&path, "emit(cuboid(10.0, 10.0, 10.0));\n").unwrap();
        let (state, rx) = PreviewState::new(path, None);
        state.reeval();

        let downloads = rx
            .borrow()
            .build_download(&state.export_stem(), None)
            .unwrap();
        assert_eq!(downloads.len(), 1);
        let download = &downloads[0];
        assert_eq!(download.filename, "box.stl");
        assert!(download.bytes.len() > 84);
        let tris = u32::from_le_bytes(download.bytes[80..84].try_into().unwrap());
        assert!(tris > 0);
    }

    #[test]
    fn selected_part_exports_that_part_as_stl() {
        let dir = tempdir();
        let path = dir.join("box.rhai");
        std::fs::write(
            &path,
            r#"
            emit("base", cuboid(20.0, 20.0, 5.0));
            emit("lid", cuboid(20.0, 20.0, 2.0));
            view("assembly", cuboid(20.0, 20.0, 8.0));
            "#,
        )
        .unwrap();
        let (state, rx) = PreviewState::new(path, Some("assembly".to_string()));
        state.reeval();

        let update = rx.borrow().clone();
        assert_eq!(update.export_parts.len(), 1);
        assert_eq!(update.export_parts[0].0, "assembly");
        let downloads = update
            .build_download(&state.export_stem(), state.selected_part())
            .unwrap();
        assert_eq!(downloads.len(), 1);
        assert_eq!(downloads[0].filename, "box-assembly.stl");
    }
}
