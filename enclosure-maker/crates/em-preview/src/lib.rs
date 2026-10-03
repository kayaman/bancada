pub mod agent_session;
pub mod chatlog;
pub mod server;
pub mod state;

pub use state::ChatState;

use std::path::PathBuf;
use std::sync::Arc;

/// Starts the chat server bound to an OS-assigned free port, for embedding
/// in another process -- e.g. a desktop wrapper that wants a URL to point a
/// native window at, instead of the CLI's `chat` subcommand, which blocks
/// forever on a fixed port via [`server::serve`].
///
/// `entry_path` is the project's entry file (`<project_dir>/model.FCStd`,
/// see `em_projects::projects::ENTRY_FILE`) -- only its parent directory
/// matters here, since FreeCAD's own document is what the embedded agent
/// opens and saves through its MCP tools; this crate never touches it
/// directly. The second parameter is unused (kept for call-site
/// compatibility with the single-part-preview selector this function took
/// before this refactor, now meaningless with no mesh to select a part
/// from).
pub async fn spawn_preview(
    entry_path: PathBuf,
    _part: Option<String>,
) -> anyhow::Result<(std::net::SocketAddr, Arc<ChatState>)> {
    let project_dir = entry_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(entry_path);
    let chat = ChatState::new(project_dir);
    let addr = server::spawn(chat.clone()).await?;
    Ok((addr, chat))
}
