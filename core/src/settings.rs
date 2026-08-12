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
    /// Recently opened project directories, most-recent-first, deduped,
    /// capped at [`MAX_RECENT`].
    #[serde(default)]
    pub recent_projects: Vec<String>,
}

pub const MAX_RECENT: usize = 10;

impl AppSettings {
    /// Sets both last-sketch fields, touching nothing else.
    pub fn set_last_sketch(&mut self, dir: String, open_file: Option<String>) {
        self.last_sketch_dir = Some(dir);
        self.last_open_file = open_file;
    }

    pub fn set_last_project_parent(&mut self, dir: String) {
        self.last_new_project_parent = Some(dir);
    }

    pub fn push_recent(&mut self, dir: String) {
        self.recent_projects.retain(|d| *d != dir);
        self.recent_projects.insert(0, dir);
        self.recent_projects.truncate(MAX_RECENT);
    }

    pub fn remove_recent(&mut self, dir: &str) {
        self.recent_projects.retain(|d| d != dir);
    }

    /// Point a recent entry at a renamed project's new directory, **in
    /// place** — the list is ordered by how recently a project was opened,
    /// and renaming one is not opening it. Absent `old`: nothing to do.
    ///
    /// A pre-existing entry for `new` (the name was used before) would
    /// otherwise appear twice, so the surviving occurrence is the earlier —
    /// i.e. more recent — of the two.
    pub fn replace_recent(&mut self, old: &str, new: &str) {
        let Some(at) = self.recent_projects.iter().position(|d| d == old) else {
            return;
        };
        self.recent_projects[at] = new.to_string();
        let mut seen = false;
        self.recent_projects.retain(|d| {
            if d != new {
                return true;
            }
            let first = !seen;
            seen = true;
            first
        });
    }
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
            recent_projects: vec!["/home/me/sketch".into(), "/home/me/other".into()],
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
    }

    #[test]
    fn push_recent_dedupes_to_front() {
        let mut s = AppSettings::default();
        s.push_recent("/a".into());
        s.push_recent("/b".into());
        s.push_recent("/a".into());
        assert_eq!(s.recent_projects, vec!["/a".to_string(), "/b".to_string()]);
    }

    #[test]
    fn push_recent_caps_at_ten() {
        let mut s = AppSettings::default();
        for i in 0..=MAX_RECENT {
            s.push_recent(format!("/p{i}"));
        }
        assert_eq!(s.recent_projects.len(), MAX_RECENT);
        assert_eq!(s.recent_projects[0], format!("/p{MAX_RECENT}"));
        assert!(!s.recent_projects.contains(&"/p0".to_string()));
    }

    #[test]
    fn remove_recent_present_and_absent() {
        let mut s = AppSettings::default();
        s.push_recent("/a".into());
        s.push_recent("/b".into());
        s.remove_recent("/a");
        assert_eq!(s.recent_projects, vec!["/b".to_string()]);
        s.remove_recent("/nope");
        assert_eq!(s.recent_projects, vec!["/b".to_string()]);
    }

    #[test]
    fn replace_recent_keeps_the_position() {
        // A rename must not promote the project to the top of the list — the
        // ordering is "recently opened", and renaming is not opening.
        let mut s = AppSettings::default();
        s.push_recent("/c".into());
        s.push_recent("/b".into());
        s.push_recent("/a".into());
        s.replace_recent("/b", "/renamed");
        assert_eq!(
            s.recent_projects,
            vec!["/a".to_string(), "/renamed".to_string(), "/c".to_string()]
        );
    }

    #[test]
    fn replace_recent_ignores_an_absent_entry() {
        let mut s = AppSettings::default();
        s.push_recent("/a".into());
        s.replace_recent("/nope", "/new");
        assert_eq!(s.recent_projects, vec!["/a".to_string()]);
    }

    #[test]
    fn replace_recent_does_not_duplicate_an_existing_entry() {
        let mut s = AppSettings::default();
        s.push_recent("/b".into());
        s.push_recent("/a".into());
        s.replace_recent("/b", "/a");
        assert_eq!(s.recent_projects, vec!["/a".to_string()]);
    }

    #[test]
    fn mutators_preserve_other_fields() {
        let mut s = AppSettings {
            last_sketch_dir: Some("/sketch".into()),
            last_open_file: Some("x.cpp".into()),
            last_new_project_parent: Some("/parent".into()),
            recent_projects: vec!["/r".into()],
        };

        s.set_last_sketch("/sketch2".into(), Some("y.cpp".into()));
        assert_eq!(s.last_new_project_parent, Some("/parent".to_string()));
        assert_eq!(s.recent_projects, vec!["/r".to_string()]);

        s.set_last_project_parent("/parent2".into());
        assert_eq!(s.last_sketch_dir, Some("/sketch2".to_string()));
        assert_eq!(s.last_open_file, Some("y.cpp".to_string()));
        assert_eq!(s.recent_projects, vec!["/r".to_string()]);

        s.push_recent("/r2".into());
        s.remove_recent("/r");
        assert_eq!(s.last_sketch_dir, Some("/sketch2".to_string()));
        assert_eq!(s.last_open_file, Some("y.cpp".to_string()));
        assert_eq!(s.last_new_project_parent, Some("/parent2".to_string()));
    }

    #[test]
    fn json_without_recent_projects_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("settings.json");
        std::fs::write(
            &p,
            r#"{
                "last_sketch_dir": "/home/me/sketch",
                "last_open_file": "src/x.cpp",
                "last_new_project_parent": "/home/me/Projects"
            }"#,
        )
        .unwrap();
        let s = load(&p);
        assert_eq!(s.last_sketch_dir, Some("/home/me/sketch".to_string()));
        assert_eq!(s.last_open_file, Some("src/x.cpp".to_string()));
        assert_eq!(s.last_new_project_parent, Some("/home/me/Projects".to_string()));
        assert!(s.recent_projects.is_empty());
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
