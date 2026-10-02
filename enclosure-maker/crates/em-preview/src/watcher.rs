use crate::state::PreviewState;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Watches the script's directory tree recursively and re-evaluates on
/// every `.rhai` file change (not just the main script itself, so edits to
/// an `import`ed shared library are picked up too). Returns the watcher,
/// which must be kept alive for watching to continue.
pub fn spawn_watcher(state: Arc<PreviewState>) -> anyhow::Result<RecommendedWatcher> {
    state.reeval();
    watch_script(state)
}

/// Registers file watching without evaluating the script first, so an
/// embedded preview can open its window while the initial model builds.
pub(crate) fn watch_script(state: Arc<PreviewState>) -> anyhow::Result<RecommendedWatcher> {
    let watch_dir = state.script_dir().to_path_buf();

    let (changes, pending) = mpsc::sync_channel(1);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if !matches!(
            event.kind,
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
        ) {
            return;
        }
        if !event
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|ext| ext == "rhai"))
        {
            return;
        }
        // One queued signal is enough: the worker reads the latest files.
        // Keep notification delivery running while geometry is evaluating.
        let _ = changes.try_send(());
    })?;

    watcher.watch(&watch_dir, RecursiveMode::Recursive)?;
    std::thread::Builder::new()
        .name("script-watch".into())
        .spawn(move || {
            while pending.recv().is_ok() {
                // Render after the final event in a burst, rather than
                // dropping saves that arrive soon after the first event.
                loop {
                    match pending.recv_timeout(Duration::from_millis(50)) {
                        Ok(()) => continue,
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                state.broadcast_script();
                state.reeval();
            }
        })?;
    // Dropping the watcher drops its callback's sender and stops the worker.
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn project(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("em-watch-{label}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        dir
    }

    fn max_x(update: &crate::state::PreviewUpdate) -> f32 {
        update.mesh_wire[4..]
            .chunks_exact(12)
            .map(|point| f32::from_le_bytes(point[..4].try_into().unwrap()))
            .fold(f32::NEG_INFINITY, f32::max)
    }

    #[tokio::test]
    async fn rapid_atomic_editor_saves_render_the_final_source() {
        let dir = project("editor");
        let script = dir.join("main.rhai");
        std::fs::write(&script, "emit(cuboid(10.0, 10.0, 10.0));").unwrap();
        let (state, mut rx) = PreviewState::new(script, None);
        let watcher = spawn_watcher(state.clone()).unwrap();
        rx.borrow_and_update();

        state
            .write_script("emit(cuboid(20.0, 10.0, 10.0));")
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            rx.wait_for(|mesh| max_x(mesh) == 10.0),
        )
        .await
        .unwrap()
        .unwrap();
        // A second save just after the first render must not be discarded
        // by the debounce window, even though each save replaces the file.
        state
            .write_script("emit(cuboid(40.0, 10.0, 10.0));")
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            rx.wait_for(|mesh| max_x(mesh) == 20.0),
        )
        .await
        .expect("the last editor save did not render")
        .unwrap();

        drop(watcher);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn imported_library_changes_render_without_touching_the_main_script() {
        let dir = project("import");
        let script = dir.join("main.rhai");
        let library = dir.join("lib/shape.rhai");
        std::fs::write(&library, "fn shape() { cuboid(10.0, 10.0, 10.0) }").unwrap();
        std::fs::write(
            &script,
            "import \"lib/shape\" as shape; emit(shape::shape());",
        )
        .unwrap();
        let (state, mut rx) = PreviewState::new(script, None);
        let watcher = spawn_watcher(state).unwrap();
        rx.borrow_and_update();

        std::fs::write(&library, "fn shape() { cuboid(30.0, 10.0, 10.0) }").unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            rx.wait_for(|mesh| max_x(mesh) == 15.0),
        )
        .await
        .expect("the imported library edit did not render")
        .unwrap();

        drop(watcher);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
