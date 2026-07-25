//! App-level settings persisted between runs (`settings.json`).
//!
//! Path-agnostic on purpose: the Tauri layer decides where the file lives
//! (`app_config_dir`), this module only reads/writes it. Loading never fails —
//! a missing or corrupt file yields defaults, because settings must never
//! block startup.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct AppSettings {
    #[serde(default)]
    pub last_sketch_dir: Option<String>,
    #[serde(default)]
    pub last_open_file: Option<String>,
    /// Parent directory the last new project was created in, so the New Project
    /// form follows where the user actually keeps sketches rather than always
    /// defaulting to the sketchbook.
    #[serde(default)]
    pub last_new_project_parent: Option<String>,
}

pub fn load(path: &Path) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Atomic write (temp file + rename) so a crash mid-write can't corrupt.
pub fn save(path: &Path, s: &AppSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(s).expect("AppSettings always serializes");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("settings.json");
        let s = AppSettings {
            last_sketch_dir: Some("/home/me/sketch".into()),
            last_open_file: Some("src/x.cpp".into()),
            last_new_project_parent: Some("/home/me/Projects".into()),
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
    }

    #[test]
    fn missing_file_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(&tmp.path().join("nope.json")), AppSettings::default());
    }

    #[test]
    fn corrupt_file_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("settings.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), AppSettings::default());
    }

    #[test]
    fn save_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("deep/nested/settings.json");
        save(&p, &AppSettings::default()).unwrap();
        assert!(p.is_file());
    }
}
