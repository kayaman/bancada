pub mod agent_session;
pub mod chatlog;
pub mod controls;
pub mod protocol;
pub mod save;
pub mod server;
pub mod state;
pub mod transforms;
pub mod watcher;

pub use state::PreviewState;

use std::path::PathBuf;
use std::sync::Arc;

/// Starts the preview server (file watcher + axum server) bound to an
/// OS-assigned free port, for embedding in another process -- e.g. a
/// desktop wrapper that wants a URL to point a native window at, instead of
/// the CLI's `preview` subcommand, which blocks forever on a fixed port via
/// [`server::serve`].
///
/// The watcher is deliberately leaked: it must live for the whole process
/// lifetime, same as the CLI implicitly keeps it alive by never returning
/// from `serve`.
pub async fn spawn_preview(
    script: PathBuf,
    part: Option<String>,
) -> anyhow::Result<(std::net::SocketAddr, Arc<PreviewState>)> {
    let (state, rx) = PreviewState::new(script, part);
    let watcher = watcher::watch_script(state.clone())?;
    let addr = server::spawn(state.clone(), rx).await?;
    std::mem::forget(watcher);
    let evaluating = state.clone();
    tokio::task::spawn_blocking(move || evaluating.reeval());
    Ok((addr, state))
}
