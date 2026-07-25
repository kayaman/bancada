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

pub mod cli;
pub mod esptool;
pub mod ghlib;
pub mod library;
pub mod project;
pub mod scope;
pub mod settings;
pub mod sketch;
pub mod types;

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
