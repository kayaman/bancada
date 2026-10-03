use crate::agent_session::AgentSession;
use crate::chatlog;
use em_agent::AgentProvider;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::broadcast;

/// Shared state behind one project's embedded chat session. FreeCAD itself
/// -- driven by the agent through its own MCP server -- is the live model
/// and the only "preview" there is; this state's whole job is the chat
/// session plumbing (spawning/resuming the provider process, broadcasting
/// its events to every connected browser tab, and the per-project chat
/// history on disk), the same responsibility this type had before this
/// refactor, minus everything that used to exist to serve a 3D mesh.
pub struct ChatState {
    project_dir: PathBuf,
    agent: tokio::sync::Mutex<Option<AgentSession>>,
    chat_tx: broadcast::Sender<String>,
    /// The chat file the *next* message appends to. `None` means "no
    /// thread chosen yet" -- the next `send_chat_message` starts a fresh
    /// one. Set by `send_chat_message` itself (first message of a new
    /// thread) or by `continue_chat` (resuming a past one).
    current_chat_file: Mutex<Option<String>>,
    current_chat_provider: Mutex<Option<AgentProvider>>,
}

impl ChatState {
    pub fn new(project_dir: PathBuf) -> Arc<Self> {
        let (chat_tx, _) = broadcast::channel(256);
        Arc::new(ChatState {
            project_dir,
            agent: tokio::sync::Mutex::new(None),
            chat_tx,
            current_chat_file: Mutex::new(None),
            current_chat_provider: Mutex::new(None),
        })
    }

    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    pub fn subscribe_chat(&self) -> broadcast::Receiver<String> {
        self.chat_tx.subscribe()
    }

    /// Sends a pre-serialized JSON text message to every connected client.
    pub fn broadcast(&self, json: String) {
        let _ = self.chat_tx.send(json);
    }

    /// Sends `text` to the assistant, starting a session (confined to
    /// `self.project_dir()`) on the first call of a new thread, or resuming
    /// natively (`--resume`) if `current_chat_file` was set by
    /// [`continue_chat`](Self::continue_chat). Every event the session
    /// produces is both persisted to that thread's `.ndjson` file and
    /// broadcast live -- the same file backs both the live view and later
    /// history browsing, so there's nothing to keep in sync between them.
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
                        let lines = chatlog::load_chat(&self.project_dir, &file).unwrap_or_default();
                        let provider = chatlog::extract_provider(&lines);
                        (file, chatlog::extract_session_id(&lines), provider)
                    }
                    None => {
                        let file = chatlog::new_chat_filename(SystemTime::now());
                        let meta = serde_json::json!({
                            "op": "meta", "provider": requested_provider,
                            "startedAtUnix": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
                        });
                        let _ = chatlog::append_line(&self.project_dir, &file, &meta.to_string());
                        chatlog::prune_default(&self.project_dir);
                        *current = Some(file.clone());
                        (file, None, requested_provider)
                    }
                }
            };
            *self.current_chat_provider.lock().unwrap() = Some(provider);

            match AgentSession::spawn(provider, &self.project_dir, resume_id).await {
                Ok((session, mut events)) => {
                    *agent = Some(session);
                    let chat_tx = self.chat_tx.clone();
                    let project_dir = self.project_dir.clone();
                    tokio::spawn(async move {
                        while let Some(line) = events.recv().await {
                            let event_value = serde_json::from_str::<serde_json::Value>(&line)
                                .unwrap_or(serde_json::Value::Null);
                            let op =
                                serde_json::json!({ "op": "event", "event": event_value.clone() });
                            let _ = chatlog::append_line(&project_dir, &file, &op.to_string());

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
            let _ = chatlog::append_line(&self.project_dir, &file, &op.to_string());
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
        let lines = chatlog::load_chat(&self.project_dir, &file).unwrap_or_default();
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
        chatlog::list_chats(&self.project_dir)
    }

    pub fn load_chat_history(&self, file: &str) -> Result<Vec<String>, String> {
        chatlog::load_chat(&self.project_dir, file).map_err(|e| e.to_string())
    }

    pub fn delete_chat_history(&self, file: &str) -> Result<(), String> {
        chatlog::delete_chat(&self.project_dir, file).map_err(|e| e.to_string())
    }
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

    /// Exercises the history list/load/delete wiring against real files on
    /// disk (written directly through `chatlog`, the same module
    /// `send_chat_message` itself uses to persist) without spawning a real
    /// provider session -- that plumbing is covered by the agent argument,
    /// event-normalization, and chatlog extraction tests.
    #[test]
    fn history_list_load_and_delete_round_trip_through_chat_state() {
        let dir = tempdir();
        let state = ChatState::new(dir.clone());

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
        let state = ChatState::new(dir);

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
}
