//! File and folder operations inside an open sketch, for the explorer.
//!
//! All safety decisions live here (repo policy: fs safety logic in core,
//! Tauri commands stay thin): rel-path validation, protected-file rules,
//! collision/descendant guards, and trash-backed deletion.

use std::path::PathBuf;

use crate::sketch::SketchProject;
use crate::{Error, Result};

/// Reject anything that could escape the sketch dir or name nothing:
/// absolute paths, `..` segments, empty/blank segments (which also covers
/// trailing slashes), and the empty string.
pub fn validate_rel_path(rel: &str) -> Result<()> {
    if rel.trim().is_empty() {
        return Err(Error::Other("empty path".into()));
    }
    if rel.starts_with('/') {
        return Err(Error::Other(format!("{rel} is not inside the sketch")));
    }
    for seg in rel.split('/') {
        if seg == ".." {
            return Err(Error::Other(format!("{rel} would leave the sketch (..)")));
        }
        if seg.trim().is_empty() {
            return Err(Error::Other(format!("{rel} has an empty path segment")));
        }
    }
    Ok(())
}

impl SketchProject {
    /// Files the sketch cannot build without: the main `<dirname>.ino` and
    /// the top-level `sketch.yaml`. Name-based — protection applies whether
    /// or not the file currently exists.
    pub fn is_protected(&self, rel_path: &str) -> bool {
        if rel_path == "sketch.yaml" {
            return true;
        }
        match self.dir.file_name().and_then(|n| n.to_str()) {
            Some(name) => rel_path == format!("{name}.ino"),
            None => false,
        }
    }

    /// Absolute path for a validated rel path.
    pub fn abs_path(&self, rel_path: &str) -> Result<PathBuf> {
        validate_rel_path(rel_path)?;
        Ok(self.dir.join(rel_path))
    }

    /// Create an empty file, making parent directories as needed.
    /// Errors if an entry (file or dir) already exists at the path.
    pub fn create_file(&self, rel_path: &str) -> Result<()> {
        let abs = self.abs_path(rel_path)?;
        if abs.symlink_metadata().is_ok() {
            return Err(Error::Other(format!("{rel_path} already exists")));
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs, "")?;
        Ok(())
    }

    /// Create a directory (and missing parents).
    /// Errors if an entry already exists at the path.
    pub fn create_dir(&self, rel_path: &str) -> Result<()> {
        let abs = self.abs_path(rel_path)?;
        if abs.symlink_metadata().is_ok() {
            return Err(Error::Other(format!("{rel_path} already exists")));
        }
        std::fs::create_dir_all(&abs)?;
        Ok(())
    }

    /// Rename or move a file or directory. Creates the target's parent
    /// directories, so rename-as-move can name a folder that doesn't exist
    /// yet. Guards: unchanged path, protected source, missing source,
    /// directory moved into its own descendant, target collision (with a
    /// case-only-rename exemption on case-insensitive filesystems).
    pub fn rename_entry(&self, from: &str, to: &str) -> Result<()> {
        let from_abs = self.abs_path(from)?;
        let to_abs = self.abs_path(to)?;

        // Guard order: rule violations before disk state (the user should
        // learn the rule, not the symptom), cheapest first.
        if from == to {
            return Err(Error::Other(format!("{from} is unchanged")));
        }
        if self.is_protected(from) {
            return Err(Error::Other(format!(
                "{from} is required by the sketch and cannot be renamed or moved"
            )));
        }
        if from_abs.symlink_metadata().is_err() {
            return Err(Error::Other(format!("{from} does not exist")));
        }
        if to.starts_with(&format!("{from}/")) {
            return Err(Error::Other(format!("cannot move {from} inside itself")));
        }
        if to_abs.symlink_metadata().is_ok() && !same_entry(&from_abs, &to_abs, from, to) {
            return Err(Error::Other(format!("{to} already exists")));
        }

        if let Some(parent) = to_abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&from_abs, &to_abs)?;
        Ok(())
    }

    /// Send a file or directory (recursively) to the OS trash.
    /// Errors on protected or missing paths before touching the trash.
    pub fn delete_entry(&self, rel_path: &str) -> Result<()> {
        let abs = self.abs_path(rel_path)?;
        if self.is_protected(rel_path) {
            return Err(Error::Other(format!(
                "{rel_path} is required by the sketch and cannot be deleted"
            )));
        }
        if abs.symlink_metadata().is_err() {
            return Err(Error::Other(format!("{rel_path} does not exist")));
        }
        trash::delete(&abs).map_err(|e| Error::Other(format!("could not trash {rel_path}: {e}")))
    }
}

/// "Target exists" is not a collision when it is the same underlying file —
/// a case-only rename on a case-insensitive filesystem.
#[cfg(unix)]
fn same_entry(a: &std::path::Path, b: &std::path::Path, _from: &str, _to: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (a.symlink_metadata(), b.symlink_metadata()) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_entry(_a: &std::path::Path, _b: &std::path::Path, from: &str, to: &str) -> bool {
    from.eq_ignore_ascii_case(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sketch dir named "demo" with demo.ino, sketch.yaml and src/util.h.
    fn sample() -> (tempfile::TempDir, SketchProject) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("demo");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("demo.ino"), "void setup(){}\n").unwrap();
        std::fs::write(dir.join("sketch.yaml"), "profiles: {}\n").unwrap();
        std::fs::create_dir(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/util.h"), "// util\n").unwrap();
        let proj = SketchProject::open(&dir).unwrap();
        (tmp, proj)
    }

    fn rel_paths(proj: &SketchProject) -> Vec<String> {
        proj.list_files()
            .unwrap()
            .into_iter()
            .map(|f| f.rel_path)
            .collect()
    }

    // ---------- validate_rel_path ----------

    #[test]
    fn validate_accepts_plain_and_nested_paths() {
        assert!(validate_rel_path("a.txt").is_ok());
        assert!(validate_rel_path("src/sensors/Env.cpp").is_ok());
    }

    #[test]
    fn validate_rejects_empty_and_blank() {
        assert!(validate_rel_path("").is_err());
        assert!(validate_rel_path("   ").is_err());
    }

    #[test]
    fn validate_rejects_absolute() {
        assert!(validate_rel_path("/etc/passwd").is_err());
    }

    #[test]
    fn validate_rejects_parent_traversal() {
        assert!(validate_rel_path("..").is_err());
        assert!(validate_rel_path("src/../../x").is_err());
    }

    #[test]
    fn validate_rejects_empty_segments() {
        assert!(validate_rel_path("src//x").is_err());
        assert!(validate_rel_path("src/").is_err());
    }

    // ---------- is_protected ----------

    #[test]
    fn main_ino_and_sketch_yaml_are_protected() {
        let (_t, proj) = sample();
        assert!(proj.is_protected("demo.ino"));
        assert!(proj.is_protected("sketch.yaml"));
    }

    #[test]
    fn nested_or_other_files_are_not_protected() {
        let (_t, proj) = sample();
        assert!(!proj.is_protected("src/sketch.yaml"));
        assert!(!proj.is_protected("src/demo.ino"));
        assert!(!proj.is_protected("other.ino"));
    }

    // ---------- abs_path ----------

    #[test]
    fn abs_path_joins_validated_rel() {
        let (_t, proj) = sample();
        assert_eq!(proj.abs_path("src/util.h").unwrap(), proj.dir.join("src/util.h"));
        assert!(proj.abs_path("../x").is_err());
    }

    // ---------- create_file ----------

    #[test]
    fn create_file_makes_empty_file() {
        let (_t, proj) = sample();
        proj.create_file("notes.txt").unwrap();
        assert_eq!(std::fs::read_to_string(proj.dir.join("notes.txt")).unwrap(), "");
        assert!(rel_paths(&proj).contains(&"notes.txt".to_string()));
    }

    #[test]
    fn create_file_makes_missing_parents() {
        let (_t, proj) = sample();
        proj.create_file("lib/net/Wifi.cpp").unwrap();
        assert!(proj.dir.join("lib/net/Wifi.cpp").is_file());
    }

    #[test]
    fn create_file_rejects_existing_entry() {
        let (_t, proj) = sample();
        let err = proj.create_file("src/util.h").unwrap_err();
        assert!(err.to_string().contains("exists"), "{err}");
        // a dir counts as an existing entry too
        assert!(proj.create_file("src").is_err());
    }

    #[test]
    fn create_file_rejects_bad_path() {
        let (_t, proj) = sample();
        assert!(proj.create_file("../escape.txt").is_err());
    }

    // ---------- create_dir ----------

    #[test]
    fn create_dir_makes_empty_dir_visible_in_listing() {
        let (_t, proj) = sample();
        proj.create_dir("assets").unwrap();
        assert!(proj.dir.join("assets").is_dir());
        // locks in walk() listing empty dirs — the explorer depends on it
        assert!(rel_paths(&proj).contains(&"assets".to_string()));
    }

    #[test]
    fn create_dir_makes_nested_dirs() {
        let (_t, proj) = sample();
        proj.create_dir("a/b/c").unwrap();
        assert!(proj.dir.join("a/b/c").is_dir());
    }

    #[test]
    fn create_dir_rejects_existing_entry() {
        let (_t, proj) = sample();
        assert!(proj.create_dir("src").is_err());
        assert!(proj.create_dir("demo.ino").is_err());
    }

    // ---------- rename_entry ----------

    #[test]
    fn rename_file_preserves_content() {
        let (_t, proj) = sample();
        proj.rename_entry("src/util.h", "src/helpers.h").unwrap();
        assert!(!proj.dir.join("src/util.h").exists());
        assert_eq!(
            std::fs::read_to_string(proj.dir.join("src/helpers.h")).unwrap(),
            "// util\n"
        );
    }

    #[test]
    fn rename_dir_carries_children() {
        let (_t, proj) = sample();
        proj.rename_entry("src", "lib").unwrap();
        assert!(proj.dir.join("lib/util.h").is_file());
        assert!(!proj.dir.join("src").exists());
    }

    #[test]
    fn rename_into_fresh_parent_creates_it() {
        let (_t, proj) = sample();
        proj.rename_entry("src/util.h", "deep/nested/util.h").unwrap();
        assert!(proj.dir.join("deep/nested/util.h").is_file());
    }

    #[test]
    fn rename_rejects_unchanged_path() {
        let (_t, proj) = sample();
        let err = proj.rename_entry("src/util.h", "src/util.h").unwrap_err();
        assert!(err.to_string().contains("unchanged"), "{err}");
    }

    #[test]
    fn rename_rejects_missing_source() {
        let (_t, proj) = sample();
        let err = proj.rename_entry("ghost.h", "spirit.h").unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn rename_rejects_protected_source() {
        let (_t, proj) = sample();
        let err = proj.rename_entry("demo.ino", "renamed.ino").unwrap_err();
        assert!(err.to_string().contains("required by the sketch"), "{err}");
        assert!(proj.rename_entry("sketch.yaml", "cfg.yaml").is_err());
    }

    #[test]
    fn rename_rejects_target_collision() {
        let (_t, proj) = sample();
        proj.create_file("existing.h").unwrap();
        let err = proj.rename_entry("src/util.h", "existing.h").unwrap_err();
        assert!(err.to_string().contains("exists"), "{err}");
    }

    #[test]
    fn rename_rejects_dir_into_own_descendant() {
        let (_t, proj) = sample();
        let err = proj.rename_entry("src", "src/inner").unwrap_err();
        assert!(err.to_string().contains("inside itself"), "{err}");
    }

    #[test]
    fn case_only_rename_succeeds() {
        let (_t, proj) = sample();
        proj.rename_entry("src/util.h", "src/Util.h").unwrap();
        assert!(rel_paths(&proj).contains(&"src/Util.h".to_string()));
    }

    // ---------- delete_entry (guards run everywhere; actual trashing
    // needs a trash-capable mount, so those are #[ignore]d) ----------

    #[test]
    fn delete_rejects_protected() {
        let (_t, proj) = sample();
        assert!(proj.delete_entry("demo.ino").is_err());
        assert!(proj.delete_entry("sketch.yaml").is_err());
    }

    #[test]
    fn delete_rejects_missing_and_invalid() {
        let (_t, proj) = sample();
        assert!(proj.delete_entry("ghost.h").is_err());
        assert!(proj.delete_entry("../outside").is_err());
    }

    #[test]
    #[ignore = "needs a trash-capable mount"]
    fn delete_file_removes_it() {
        let (_t, proj) = sample();
        proj.delete_entry("src/util.h").unwrap();
        assert!(!proj.dir.join("src/util.h").exists());
    }

    #[test]
    #[ignore = "needs a trash-capable mount"]
    fn delete_dir_recursive() {
        let (_t, proj) = sample();
        proj.delete_entry("src").unwrap();
        assert!(!proj.dir.join("src").exists());
    }
}
