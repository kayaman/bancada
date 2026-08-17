//! bancada-core — the engine behind Bancada.
//!
//! This crate deliberately contains **no** Tauri/UI code so it can be unit
//! tested headlessly and reused from a CLI or a different frontend later.
//!
//! Design: we do not reimplement toolchain logic. `arduino-cli` (JSON output)
//! is the build/board/library engine — the same engine Arduino IDE 2.x uses —
//! and `esptool` provides ESP-specific utilities such as reading the MAC
//! address. This crate shells out to them, parses their output into typed
//! structs, and manages the project-level `sketch.yaml` file.

pub mod agent;
pub mod boards;
pub mod chatlog;
pub mod cli;
pub mod clone;
pub mod devproxy;
pub mod esptool;
pub mod files;
pub mod fleet;
pub mod ghlib;
pub mod git;
pub mod library;
pub mod mcp;
pub mod mqtt;
pub mod ports;
pub mod project;
pub mod scope;
pub mod serialring;
pub mod settings;
pub mod sketch;
pub mod types;
pub mod usage;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("`{tool}` exited with status {status}:\n{stderr}")]
    ToolFailed {
        tool: String,
        status: i32,
        stderr: String,
    },
    #[error("could not find `{0}` on PATH — is it installed?")]
    ToolMissing(String),
    #[error("failed to parse {what}: {source}")]
    Json {
        what: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to parse sketch.yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A private staging path beside `path`, for the temp-file-then-rename
/// writes every JSON store in this crate does.
///
/// The name carries the process id *and* a per-call counter, so neither two
/// Bancada windows nor two saves racing inside one process can collide. It
/// must stay a sibling of `path`: `fs::rename` is only atomic within a
/// filesystem, and staging through `/tmp` would silently give that up.
pub(crate) fn staging_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{n}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// Write `text` over `path` through a staging file, atomically.
///
/// The staging name is unique per call ([`staging_path`]), which is what
/// stops two writers colliding — and is also why the failure path has to
/// unlink explicitly. With a fixed name a failed attempt was tidied up by
/// the next save reusing it; with unique names nothing ever would, so a
/// config directory that cannot be written would collect one orphan per
/// attempt.
///
/// Callers serialise their own value, so the error they raise for bad JSON
/// stays theirs to name.
pub(crate) fn replace_file_atomically(path: &std::path::Path, text: &str) -> Result<()> {
    let tmp = staging_path(path);
    let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    Ok(())
}

#[cfg(test)]
mod staging_tests {
    use super::staging_path;
    use std::path::Path;

    #[test]
    fn a_staging_path_is_unique_per_call_and_sits_beside_its_target() {
        // Every JSON store here writes to a temp file and renames it over
        // the target. With a *fixed* temp name — `fleet.json.tmp` — two
        // Bancada windows could interleave: one truncates and starts
        // writing while the other renames the same file into place, and
        // what lands is a half-written record. The loaders correctly refuse
        // a corrupt file rather than emptying it, so the user sees the
        // panel error out, which reads as losing the registry.
        let target = Path::new("/home/me/.config/dev.magj.bancada/fleet.json");
        let a = staging_path(target);
        let b = staging_path(target);
        assert_ne!(a, b, "two writers must not choose the same staging name");

        // Same directory, or the final rename crosses a filesystem and
        // stops being atomic — which is the whole point of staging.
        assert_eq!(a.parent(), target.parent());
        assert_ne!(a, target.to_path_buf());
    }

    #[test]
    fn a_failed_replace_leaves_no_staging_file_behind() {
        // Unique staging names fixed a collision, but they also mean a
        // failure can no longer be cleaned up by the *next* save reusing
        // the same name. Without an explicit unlink, a config directory
        // that cannot be written — read-only, full — silently accumulates
        // one `.tmp` per attempt.
        let dir = tempfile::tempdir().unwrap();
        // Rename onto a non-empty directory fails, which is the cheapest
        // portable way to fail the second half of the write.
        let target = dir.path().join("settings.json");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("occupied"), "x").unwrap();

        assert!(super::replace_file_atomically(&target, "{}").is_err());

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files left behind: {leftovers:?}"
        );
    }
}
