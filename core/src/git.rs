//! The small slice of git Bancada needs: telling whether a directory is under
//! version control, and putting a fresh project under it.
//!
//! Two decisions worth knowing:
//!
//! - `is_under_git` walks the ancestors looking for `.git` rather than shelling
//!   out to `git rev-parse`. A sketch usually lives *inside* a repository
//!   rather than being one — `~/Projects/embedded/my-sketch` under a single
//!   repo is the normal shape — so a check that only looked in the sketch
//!   directory itself reported "not under git" for files that were tracked all
//!   along. Walking up matches what git itself considers a work tree, stays
//!   cheap, needs no subprocess, and still answers when git is not installed.
//!   It does not honour `GIT_DIR`/`GIT_CEILING_DIRECTORIES`; for a warning
//!   about whether edits can be undone, ancestry is the question being asked.
//! - `init_repo` makes an initial commit rather than leaving an empty
//!   repository. An empty repo has no baseline to restore to, which is the one
//!   thing the Assistant panel's warning is about.

use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Entries every repository's `.gitignore` must carry: build output, vendored
/// libraries, and the credential files that must never reach a commit.
pub const GITIGNORE_REQUIRED: &[&str] = &[
    "build/",
    ".bancada/",
    ".env",
    "secrets.h",
    "arduino_secrets.h",
];

/// Append the [`GITIGNORE_REQUIRED`] entries `existing` lacks, using the same
/// trimmed-line comparison as git's ignore logic: `build`, `build/`,
/// `/build` and `/build/` all count as the entry being present. Existing
/// content is preserved, gaining a trailing newline when it is missing one.
pub fn merged_gitignore(existing: &str) -> String {
    let has = |content: &str, entry: &str| {
        let base = entry.trim_end_matches('/');
        content.lines().any(|l| {
            let t = l.trim();
            t == base
                || t == format!("{base}/")
                || t == format!("/{base}")
                || t == format!("/{base}/")
        })
    };
    let mut out = existing.to_string();
    for entry in GITIGNORE_REQUIRED {
        if !has(&out, entry) {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(entry);
            out.push('\n');
        }
    }
    out
}

/// Write (or extend) `dir/.gitignore` so every [`GITIGNORE_REQUIRED`] entry is
/// present. Reads what exists and appends only what is missing — a
/// hand-maintained ignore file is preserved, not replaced.
pub fn write_gitignore(dir: &Path) -> Result<()> {
    let p = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&p).unwrap_or_default();
    std::fs::write(&p, merged_gitignore(&existing))?;
    Ok(())
}

/// Runs git and returns stdout, mapping a missing binary to `ToolMissing`.
pub(crate) fn run(args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::ToolMissing("git".into())
        } else {
            Error::Io(e)
        }
    })?;
    if !out.status.success() {
        return Err(Error::ToolFailed {
            tool: "git".into(),
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether `dir` — or any directory above it — is a git work tree.
///
/// `.git` is matched whether it is a directory or a file, so linked worktrees
/// and submodules count. An empty or relative path answers `false`: the caller
/// always has an absolute sketch directory, and guessing from the process's
/// current directory would be a surprising answer to give.
pub fn is_under_git(dir: &Path) -> bool {
    if dir.as_os_str().is_empty() || !dir.is_absolute() {
        return false;
    }
    let mut cur: Option<&Path> = Some(dir);
    while let Some(d) = cur {
        if d.join(".git").exists() {
            return true;
        }
        cur = d.parent();
    }
    false
}

/// Puts `dir` under git with one commit containing everything in it.
///
/// Callers should skip this when [`is_under_git`] already answers `true` —
/// initialising inside an existing work tree would nest a second repository,
/// which is rarely what anyone wants.
///
/// The commit is made with `-c` overrides so a machine with no `user.name`
/// configured still gets a baseline instead of an error, and with
/// `--no-verify`/`core.hooksPath=` so a global hooks directory cannot run
/// during project creation. The credential `.gitignore` is written before
/// `add --all`, ensuring that no secrets can reach the baseline commit.
pub fn init_repo(dir: &Path) -> Result<()> {
    let path = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;

    run(&["init", "--quiet", path])?;
    write_gitignore(dir)?;
    run(&["-C", path, "add", "--all"])?;
    run(&[
        "-C",
        path,
        "-c",
        "user.name=Bancada",
        "-c",
        "user.email=bancada@localhost",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=",
        "commit",
        "--no-verify",
        "--quiet",
        // An empty directory would otherwise fail with "nothing to commit";
        // a baseline to restore to is the point, so make one either way.
        "--allow-empty",
        "-m",
        "Initial sketch",
    ])?;
    Ok(())
}

/// Puts `dir` under git unless it already is, and says whether it had to.
///
/// This is the decision a freshly created project wants: give the sketch an
/// undo path, but never nest a repository inside one the user already keeps —
/// a sketch created under `~/Projects` when that folder is itself a checkout
/// is already covered by it.
pub fn ensure_under_git(dir: &Path) -> Result<bool> {
    if is_under_git(dir) {
        return Ok(false);
    }
    init_repo(dir)?;
    Ok(true)
}

/// The nearest ancestor of `dir` (inclusive) holding a `.git`, if any.
/// Useful for explaining *which* repository a sketch belongs to.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    if dir.as_os_str().is_empty() || !dir.is_absolute() {
        return None;
    }
    let mut cur: Option<&Path> = Some(dir);
    while let Some(d) = cur {
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The bug this module exists for: a sketch inside a repository was
    /// reported as "not under git" because only its own directory was checked.
    #[test]
    fn a_sketch_nested_in_a_repo_is_under_git() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        run(&["init", "--quiet", root.to_str().unwrap()]).unwrap();

        let sketch = root.join("sketches").join("MySketch");
        std::fs::create_dir_all(&sketch).unwrap();

        assert!(!sketch.join(".git").exists(), "premise: no .git of its own");
        assert!(is_under_git(&sketch));
        assert_eq!(repo_root(&sketch).as_deref(), Some(root.as_path()));
    }

    #[test]
    fn a_repo_root_itself_is_under_git() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        run(&["init", "--quiet", root.to_str().unwrap()]).unwrap();
        assert!(is_under_git(&root));
    }

    #[test]
    fn a_standalone_directory_is_not_under_git() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Standalone");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_under_git(&dir));
        assert_eq!(repo_root(&dir), None);
    }

    /// A linked worktree or submodule has `.git` as a *file*, not a directory.
    #[test]
    fn a_dot_git_file_counts_as_under_git() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("linked");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".git"), "gitdir: /elsewhere/.git/worktrees/x").unwrap();
        assert!(is_under_git(&dir));
    }

    #[test]
    fn empty_and_relative_paths_answer_false() {
        assert!(!is_under_git(Path::new("")));
        assert!(!is_under_git(Path::new("relative/dir")));
        assert_eq!(repo_root(Path::new("relative/dir")), None);
    }

    #[test]
    fn init_repo_leaves_a_committed_baseline() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Fresh");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Fresh.ino"), "void setup() {}\n").unwrap();

        assert!(!is_under_git(&dir), "premise: not a repo yet");
        init_repo(&dir).unwrap();

        assert!(is_under_git(&dir));
        // A baseline to restore to — an empty repo would not give one.
        let log = run(&["-C", dir.to_str().unwrap(), "log", "--oneline"]).unwrap();
        assert!(log.contains("Initial sketch"), "log was {log:?}");
        let tracked = run(&["-C", dir.to_str().unwrap(), "ls-files"]).unwrap();
        assert!(tracked.contains("Fresh.ino"), "tracked {tracked:?}");
    }

    /// The user's reported case: a sketch created inside `~/Projects` when
    /// that folder is itself a checkout. It is already under git, so nothing
    /// should be initialised — a nested repository would be a worse bug than
    /// the warning it replaced.
    #[test]
    fn ensure_under_git_does_not_nest_a_repo_inside_an_existing_one() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        run(&["init", "--quiet", root.to_str().unwrap()]).unwrap();

        let sketch = root.join("notundergit");
        std::fs::create_dir_all(&sketch).unwrap();

        assert!(!ensure_under_git(&sketch).unwrap(), "should not have acted");
        assert!(!sketch.join(".git").exists(), "must not nest a repo");
        assert_eq!(repo_root(&sketch).as_deref(), Some(root.as_path()));
    }

    #[test]
    fn ensure_under_git_initialises_a_standalone_project() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Alone");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Alone.ino"), "void setup() {}\n").unwrap();

        assert!(ensure_under_git(&dir).unwrap(), "should have initialised");
        assert!(dir.join(".git").exists());
        assert_eq!(repo_root(&dir).as_deref(), Some(dir.as_path()));
        // Idempotent: a second call is a no-op, not a second repo.
        assert!(!ensure_under_git(&dir).unwrap());
    }

    /// An empty directory still gets a baseline commit; git would otherwise
    /// refuse with "nothing to commit" and leave a repo with no history.
    #[test]
    fn init_repo_handles_an_empty_directory() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Empty");
        std::fs::create_dir_all(&dir).unwrap();
        init_repo(&dir).unwrap();
        let log = run(&["-C", dir.to_str().unwrap(), "log", "--oneline"]).unwrap();
        assert!(log.contains("Initial sketch"), "log was {log:?}");
    }

    #[test]
    fn init_repo_commits_even_without_a_configured_identity() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("NoIdentity");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();
        let log = run(&["-C", dir.to_str().unwrap(), "log", "--format=%an <%ae>"]).unwrap();
        assert!(log.contains("Bancada <bancada@localhost>"), "log {log:?}");
    }

    // ----- gitignore merging -----

    #[test]
    fn merged_gitignore_adds_all_required_entries_to_empty_file() {
        let result = merged_gitignore("");
        assert_eq!(
            result,
            "build/\n.bancada/\n.env\nsecrets.h\narduino_secrets.h\n"
        );
    }

    #[test]
    fn merged_gitignore_preserves_existing_content_and_adds_missing_entries() {
        let existing = "custom.txt\n";
        let result = merged_gitignore(existing);
        assert_eq!(
            result,
            "custom.txt\nbuild/\n.bancada/\n.env\nsecrets.h\narduino_secrets.h\n"
        );
    }

    #[test]
    fn merged_gitignore_fixes_missing_trailing_newline() {
        let existing = "custom.txt";
        let result = merged_gitignore(existing);
        assert_eq!(
            result,
            "custom.txt\nbuild/\n.bancada/\n.env\nsecrets.h\narduino_secrets.h\n"
        );
    }

    #[test]
    fn merged_gitignore_variant_spellings_are_not_duplicated() {
        let existing = "build\n.env\n";
        let result = merged_gitignore(existing);
        assert_eq!(result, "build\n.env\n.bancada/\nsecrets.h\narduino_secrets.h\n");
        let build_lines = result
            .lines()
            .filter(|l| matches!(l.trim(), "build" | "build/" | "/build" | "/build/"))
            .count();
        assert_eq!(build_lines, 1, "{result}");
    }

    /// The credential rules must exist before any commit can happen — a
    /// baseline that includes secrets.h is worse than no baseline.
    #[test]
    fn init_repo_writes_credential_gitignore_and_keeps_secrets_out() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("WithSecrets");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.ino"), "void setup() {}\n").unwrap();
        std::fs::write(dir.join("secrets.h"), "#define WIFI_PASS \"hunter2\"\n").unwrap();

        init_repo(&dir).unwrap();

        let gi = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(gi.contains("secrets.h"), ".gitignore was {gi:?}");
        let tracked = run(&["-C", dir.to_str().unwrap(), "ls-files"]).unwrap();
        assert!(tracked.contains("x.ino"));
        assert!(!tracked.contains("secrets.h"), "tracked {tracked:?}");
        assert!(tracked.contains(".gitignore"));
    }
}
