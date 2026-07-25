//! Typed views over `arduino-cli --json` output.
//!
//! All structs are tolerant (`#[serde(default)]` everywhere) because the CLI's
//! JSON schema gains fields between releases; we only fail on missing data we
//! actually need.

use serde::{Deserialize, Serialize};

// ---------- `arduino-cli board list --json` ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BoardListResponse {
    #[serde(default)]
    pub detected_ports: Vec<DetectedPort>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DetectedPort {
    #[serde(default)]
    pub port: Port,
    #[serde(default)]
    pub matching_boards: Vec<MatchingBoard>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Port {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub protocol_label: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MatchingBoard {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fqbn: String,
}

// ---------- `arduino-cli lib search <q> --json` ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibSearchResponse {
    #[serde(default)]
    pub libraries: Vec<IndexedLibrary>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct IndexedLibrary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub latest: LibraryRelease,
    /// Every published version, newest last (map keys in the raw JSON).
    #[serde(default)]
    pub available_versions: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibraryRelease {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub paragraph: String,
    #[serde(default)]
    pub website: String,
}

// ---------- `arduino-cli lib list --json` ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibListResponse {
    #[serde(default)]
    pub installed_libraries: Vec<InstalledLibraryEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InstalledLibraryEntry {
    #[serde(default)]
    pub library: InstalledLibrary,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InstalledLibrary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub sentence: String,
    /// "user" (sketchbook), "platform", or "ide" — lets the UI badge
    /// personal/local libraries differently from bundled ones.
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub install_dir: String,
}

// ---------- `arduino-cli core list --json` ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CoreListResponse {
    #[serde(default)]
    pub platforms: Vec<Platform>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Platform {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub installed_version: String,
    #[serde(default)]
    pub latest_version: String,
}

// ---------- Bancada's own composite types ----------

/// A line of process output, tagged so the UI can colour stderr.
#[derive(Debug, Clone, Serialize)]
pub struct OutputLine {
    pub stream: OutputStream,
    pub line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Result of a compile/upload run.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub success: bool,
    pub exit_code: i32,
}

// ---------- board listall ----------
//
// `board listall --json` nests the entire platform — including every one of its
// boards — inside each board entry. Only the fields below are read; the rest is
// ignored, and `ArduinoCli::board_listall` flattens the result into
// [`BoardOption`] before it reaches the frontend.

#[derive(Debug, Clone, Deserialize)]
pub struct BoardListAllResponse {
    #[serde(default)]
    pub boards: Vec<BoardListAllEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoardListAllEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fqbn: String,
    #[serde(default)]
    pub platform: BoardListAllPlatform,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BoardListAllPlatform {
    #[serde(default)]
    pub metadata: BoardListAllPlatformMeta,
    #[serde(default)]
    pub release: BoardListAllPlatformRelease,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BoardListAllPlatformMeta {
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BoardListAllPlatformRelease {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub installed: bool,
}

/// One selectable board, flattened for the New Project picker.
#[derive(Debug, Clone, Serialize)]
pub struct BoardOption {
    pub fqbn: String,
    pub name: String,
    /// e.g. `esp32:esp32`
    pub platform_id: String,
    /// e.g. `esp32` — used to group the picker.
    pub platform_name: String,
}
