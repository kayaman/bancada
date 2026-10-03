//! Project CRUD: a "project" is a directory directly under the projects
//! root containing a `model.FCStd` entry-point document (that file's
//! presence is what distinguishes an enclosure-maker project from the many
//! other, unrelated project folders that also live under `~/Projects`).

use std::path::{Path, PathBuf};

pub const ENTRY_FILE: &str = "model.FCStd";

/// A minimal, blank FreeCAD document, embedded at compile time and copied
/// to `<dir>/model.FCStd` for every new project. Generated once via
/// FreeCAD's own `freecadcmd` (`FreeCAD.newDocument(...).saveAs(...)`), not
/// hand-built, so it's a real, valid `.FCStd` zip container rather than a
/// guess at FreeCAD's internal XML schema.
const STARTER_TEMPLATE: &[u8] = include_bytes!("../../../assets/starter.FCStd");

#[derive(Clone, serde::Serialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    /// Seconds since the Unix epoch, best-effort (0 if unavailable) --
    /// enough to sort "most recently touched first".
    pub modified: u64,
}

/// `~/Projects`, or `$ENCLOSURE_MAKER_PROJECTS_ROOT` if set (used by tests,
/// and available as an escape hatch for anyone who wants projects
/// somewhere else).
pub fn projects_root() -> PathBuf {
    if let Ok(over) = std::env::var("ENCLOSURE_MAKER_PROJECTS_ROOT") {
        return PathBuf::from(over);
    }
    dirs_home().join("Projects")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// A project name must be usable as a single path component: no
/// separators, no `..`/`.`, not empty, and restricted to a safe charset so
/// it can't be misread as a flag or contain control characters. This is
/// deliberately stricter than the filesystem actually requires -- project
/// names only need to be *readable*, not maximally permissive.
pub(crate) fn sanitize_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("project name can't be empty".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("that name is reserved".to_string());
    }
    let ok = trimmed
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ' ');
    if !ok {
        return Err("project names can only contain letters, numbers, spaces, '-', and '_'".to_string());
    }
    if trimmed.len() > 100 {
        return Err("project name is too long".to_string());
    }
    Ok(trimmed.to_string())
}

fn modified_unix_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Every immediate subdirectory of `root` that contains a `main.rhai`,
/// newest-modified first. Missing `root` is treated as "no projects yet",
/// not an error.
pub fn list_projects(root: &Path) -> Vec<ProjectInfo> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut projects: Vec<ProjectInfo> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let main = e.path().join(ENTRY_FILE);
            if !main.is_file() {
                return None;
            }
            Some(ProjectInfo {
                name: e.file_name().to_string_lossy().into_owned(),
                path: main.to_string_lossy().into_owned(),
                modified: modified_unix_secs(&main),
            })
        })
        .collect();
    projects.sort_by(|a, b| b.modified.cmp(&a.modified));
    projects
}

/// Creates `dir/model.FCStd` from the starter template. Refuses only if
/// `dir` already has a `model.FCStd` -- `dir` itself may already exist and
/// hold unrelated files, since the bancada-import path uses this directly
/// against a bancada project's own (already populated) directory, not the
/// launcher's `projects_root()`.
pub(crate) fn create_project_at(dir: &Path) -> Result<PathBuf, String> {
    let main = dir.join(ENTRY_FILE);
    if main.is_file() {
        return Err(format!("{} already exists", main.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create the project folder: {e}"))?;
    std::fs::write(&main, STARTER_TEMPLATE).map_err(|e| format!("could not write {ENTRY_FILE}: {e}"))?;
    Ok(main)
}

/// Creates `root/<name>/main.rhai`. Refuses if a project with that name
/// already exists under `root`.
pub fn create_project(root: &Path, name: &str) -> Result<PathBuf, String> {
    let name = sanitize_name(name)?;
    create_project_at(&root.join(&name)).map_err(|_| format!("a project named '{name}' already exists"))
}

/// Permanently deletes `root/<name>` and everything in it. The caller (the
/// UI) is responsible for confirming with the user first -- this function
/// does not ask.
pub fn delete_project(root: &Path, name: &str) -> Result<(), String> {
    let name = sanitize_name(name)?;
    let dir = root.join(&name);
    if !dir.join(ENTRY_FILE).is_file() {
        return Err(format!("'{name}' doesn't look like an enclosure-maker project (no {ENTRY_FILE})"));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("could not delete '{name}': {e}"))
}

/// Renames `root/<old_name>` to `root/<new_name>`.
pub fn rename_project(root: &Path, old_name: &str, new_name: &str) -> Result<PathBuf, String> {
    let old_name = sanitize_name(old_name)?;
    let new_name = sanitize_name(new_name)?;
    let old_dir = root.join(&old_name);
    let new_dir = root.join(&new_name);
    if !old_dir.join(ENTRY_FILE).is_file() {
        return Err(format!("'{old_name}' doesn't look like an enclosure-maker project (no {ENTRY_FILE})"));
    }
    if new_dir.exists() {
        return Err(format!("a project named '{new_name}' already exists"));
    }
    std::fs::rename(&old_dir, &new_dir).map_err(|e| format!("could not rename '{old_name}' to '{new_name}': {e}"))?;
    Ok(new_dir.join(ENTRY_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("em-projects-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_then_list_finds_the_new_project() {
        let root = tempdir();
        let main = create_project(&root, "My Enclosure").unwrap();
        assert!(main.ends_with("My Enclosure/model.FCStd"));
        assert!(main.is_file());
        assert!(std::fs::metadata(&main).unwrap().len() > 0);

        let projects = list_projects(&root);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "My Enclosure");
    }

    #[test]
    fn create_rejects_a_duplicate_name() {
        let root = tempdir();
        create_project(&root, "dup").unwrap();
        let result = create_project(&root, "dup");
        assert!(result.is_err());
    }

    #[test]
    fn create_rejects_unsafe_names() {
        let root = tempdir();
        assert!(create_project(&root, "").is_err());
        assert!(create_project(&root, "..").is_err());
        assert!(create_project(&root, "../escape").is_err());
        assert!(create_project(&root, "a/b").is_err());
        assert!(create_project(&root, "a;rm -rf /").is_err());
    }

    #[test]
    fn list_ignores_folders_without_model_fcstd() {
        let root = tempdir();
        create_project(&root, "real-project").unwrap();
        std::fs::create_dir_all(root.join("unrelated-project")).unwrap();
        std::fs::write(root.join("unrelated-project/notes.txt"), "hi").unwrap();

        let projects = list_projects(&root);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "real-project");
    }

    #[test]
    fn list_on_a_missing_root_is_empty_not_an_error() {
        let root = tempdir().join("does-not-exist-yet");
        assert_eq!(list_projects(&root).len(), 0);
    }

    #[test]
    fn delete_removes_the_whole_project_folder() {
        let root = tempdir();
        create_project(&root, "to-delete").unwrap();
        assert!(root.join("to-delete").exists());
        delete_project(&root, "to-delete").unwrap();
        assert!(!root.join("to-delete").exists());
    }

    #[test]
    fn delete_refuses_a_folder_that_is_not_a_project() {
        let root = tempdir();
        std::fs::create_dir_all(root.join("not-a-project")).unwrap();
        let result = delete_project(&root, "not-a-project");
        assert!(result.is_err());
        assert!(root.join("not-a-project").exists(), "refused delete must not touch the folder");
    }

    #[test]
    fn rename_moves_the_folder_and_keeps_contents() {
        let root = tempdir();
        create_project(&root, "old-name").unwrap();
        std::fs::write(root.join("old-name/notes.txt"), "some project note").unwrap();

        let new_main = rename_project(&root, "old-name", "new-name").unwrap();
        assert!(!root.join("old-name").exists());
        assert!(new_main.exists());
        assert!(root.join("new-name/notes.txt").exists());
    }

    #[test]
    fn rename_refuses_when_the_target_name_is_taken() {
        let root = tempdir();
        create_project(&root, "a").unwrap();
        create_project(&root, "b").unwrap();
        let result = rename_project(&root, "a", "b");
        assert!(result.is_err());
        assert!(root.join("a").exists(), "refused rename must not touch either folder");
    }
}
