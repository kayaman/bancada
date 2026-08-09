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
use crate::types::{OutputLine, OutputStream};
use serde::Serialize;
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

/// Run git streaming stdout+stderr lines into `on_line`; returns exit success.
/// Sync's fetch/rebase/push go through this so the Build console shows
/// progress the way compiles do.
pub fn run_streaming(args: &[&str], mut on_line: impl FnMut(OutputLine)) -> Result<bool> {
    use std::io::BufRead;
    let mut child = std::process::Command::new("git")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::ToolMissing("git".into())
            } else {
                Error::Io(e)
            }
        })?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let (tx, rx) = std::sync::mpsc::channel::<OutputLine>();
    let tx_err = tx.clone();
    let t_out = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            let _ = tx.send(OutputLine { stream: OutputStream::Stdout, line });
        }
    });
    let t_err = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            let _ = tx_err.send(OutputLine { stream: OutputStream::Stderr, line });
        }
    });
    for line in rx {
        on_line(line);
    }
    let _ = t_out.join();
    let _ = t_err.join();
    Ok(child.wait()?.success())
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

/// One dirty path, with its two-letter porcelain XY status (`??` untracked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedPath {
    pub path: String,
    pub status: String,
}

/// Everything `git status --porcelain=v2 --branch` says, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusV2 {
    pub branch: String,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: Vec<ChangedPath>,
}

/// Parse porcelain v2. Headers are `# branch.*` lines; entries start with
/// `1` (ordinary), `2` (rename/copy — new path first, then a tab and the
/// original), `u` (unmerged) or `?` (untracked). Unknown lines are skipped:
/// git adds header kinds over time and a status pill must not break on them.
pub fn parse_status_v2(out: &str) -> StatusV2 {
    let mut s = StatusV2 {
        branch: String::new(),
        detached: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        dirty: Vec::new(),
    };
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            s.detached = rest == "(detached)";
            s.branch = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    s.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    s.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = line.strip_prefix("? ") {
            s.dirty.push(ChangedPath {
                path: rest.to_string(),
                status: "??".to_string(),
            });
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            let status = line.get(2..4).unwrap_or("").to_string();
            // Fields are space-separated; the path is everything from field 8
            // (ordinary) / 9 (unmerged has an extra mode) onward. Taking the
            // tail after the 8th space and splitting a rename's `\t` is
            // simpler and survives spaces in filenames.
            let n_meta = if line.starts_with("u ") { 10 } else { 8 };
            let mut rest = line;
            for _ in 0..n_meta {
                rest = match rest.split_once(' ') {
                    Some((_, r)) => r,
                    None => "",
                };
            }
            let mut path = rest.split('\t').next().unwrap_or("").to_string();
            // For renames/copies, strip the score prefix (e.g., "R100 " or "C75 ")
            if line.starts_with("2 ") {
                if let Some(space_idx) = path.find(' ') {
                    let prefix = &path[..space_idx];
                    if (prefix.starts_with('R') || prefix.starts_with('C'))
                        && prefix[1..].chars().all(|c| c.is_ascii_digit())
                    {
                        path = path[space_idx + 1..].to_string();
                    }
                }
            }
            if !path.is_empty() {
                s.dirty.push(ChangedPath { path, status });
            }
        }
    }
    s
}

/// `checkpoint: a, b (+N)` — the first two paths by name, the rest counted.
pub fn suggested_message(dirty: &[ChangedPath]) -> String {
    let names: Vec<&str> = dirty.iter().map(|c| c.path.as_str()).collect();
    match names.len() {
        0 => "checkpoint".to_string(),
        1 => format!("checkpoint: {}", names[0]),
        2 => format!("checkpoint: {}, {}", names[0], names[1]),
        n => format!("checkpoint: {}, {} (+{})", names[0], names[1], n - 2),
    }
}

/// Credential-named files among `git ls-files` output — files `.gitignore`
/// can no longer protect, because they are already tracked.
pub fn tracked_secrets(ls_files_out: &str) -> Vec<String> {
    let secret_names = ["secrets.h", "arduino_secrets.h", ".env"];
    ls_files_out
        .lines()
        .filter(|p| {
            let base = p.rsplit('/').next().unwrap_or(p);
            secret_names.contains(&base)
        })
        .map(str::to_string)
        .collect()
}

/// Everything the git pill needs, in one call.
///
/// Root-vs-nested comes from [`repo_root`] ancestry. Dirty paths are always
/// scoped to the sketch directory (`status ... -- .` with `-C <sketch>`), so a
/// nested sketch never reports its parent repository's unrelated noise.
pub fn repo_state(dir: &Path) -> Result<RepoState> {
    let Some(root) = repo_root(dir) else {
        return Ok(RepoState::NoGit);
    };
    let dir_s = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;

    let status = run(&["-C", dir_s, "status", "--porcelain=v2", "--branch", "--", "."])?;
    let parsed = parse_status_v2(&status);

    if root != dir {
        return Ok(RepoState::Nested {
            root: root.to_string_lossy().into_owned(),
            dirty: parsed.dirty,
        });
    }

    // A missing remote is a state, not an error: `remote get-url` exits 2.
    let remote = run(&["-C", dir_s, "remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let ls = run(&["-C", dir_s, "ls-files", "--", "."])?;
    let secrets = tracked_secrets(&ls);
    let msg = suggested_message(&parsed.dirty);

    Ok(RepoState::Root {
        branch: parsed.branch,
        detached: parsed.detached,
        has_upstream: parsed.upstream.is_some(),
        ahead: parsed.ahead,
        behind: parsed.behind,
        dirty: parsed.dirty,
        remote,
        tracked_secrets: secrets,
        suggested_message: msg,
    })
}

/// Checkpoint everything under `dir`: `add -A -- .` scoped to the sketch,
/// then commit. In a nested sketch this commits only the subtree's paths —
/// the parent repository's other changes are not swept in.
pub fn commit(dir: &Path, message: &str) -> Result<CommitOutcome> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let status = run(&["-C", d, "status", "--porcelain", "--", "."])?;
    if status.trim().is_empty() {
        return Ok(CommitOutcome::NothingToCommit);
    }
    run(&["-C", d, "add", "-A", "--", "."])?;
    run(&["-C", d, "commit", "--quiet", "-m", message, "--", "."])?;
    Ok(CommitOutcome::Committed)
}

/// Outcome of a checkpoint commit. "Nothing to commit" is a state the UI
/// reports quietly, not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitOutcome {
    Committed,
    NothingToCommit,
}

/// What a sync attempt came to. Everything except `Synced` is a state the
/// UI explains; none of them is a Rust-level error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncOutcome {
    Synced,
    DirtyTree,
    Diverged,
    NoRemote,
    NotRoot,
}

/// fetch → rebase → push, refusing anything that could lose work.
///
/// Dirty trees are refused rather than autostashed: in a checkpoint model,
/// "commit first" is one honest click, and it keeps rebase away from
/// uncommitted changes. A conflicted rebase is aborted on the spot — local
/// commits intact, tree clean — because a half-finished rebase in a GUI with
/// no conflict UI is the one trap this feature must never set.
pub fn sync(dir: &Path, mut on_line: impl FnMut(OutputLine)) -> Result<SyncOutcome> {
    let state = repo_state(dir)?;
    let (branch, detached, dirty, remote, has_upstream) = match state {
        RepoState::NoGit | RepoState::Nested { .. } => return Ok(SyncOutcome::NotRoot),
        RepoState::Root { branch, detached, dirty, remote, has_upstream, .. } => {
            (branch, detached, dirty, remote, has_upstream)
        }
    };
    if remote.is_none() {
        return Ok(SyncOutcome::NoRemote);
    }
    if detached {
        return Err(Error::Other(
            "HEAD is detached — check out a branch before syncing".into(),
        ));
    }
    if !dirty.is_empty() {
        return Ok(SyncOutcome::DirtyTree);
    }
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;

    if !run_streaming(&["-C", d, "fetch", "origin"], &mut on_line)? {
        return Err(Error::Other("git fetch failed — see the Build console".into()));
    }

    // Rebase only onto a branch the remote actually has: a fresh remote (or
    // first push of a new branch) has nothing to rebase onto.
    let target = format!("origin/{branch}");
    let remote_has_branch = run(&["-C", d, "rev-parse", "--verify", "--quiet", &target]).is_ok();
    if remote_has_branch && !run_streaming(&["-C", d, "rebase", &target], &mut on_line)? {
        // Best-effort abort; the repo must never be left mid-rebase.
        let _ = run(&["-C", d, "rebase", "--abort"]);
        return Ok(SyncOutcome::Diverged);
    }

    let pushed = if has_upstream {
        run_streaming(&["-C", d, "push"], &mut on_line)?
    } else {
        run_streaming(&["-C", d, "push", "-u", "origin", "HEAD"], &mut on_line)?
    };
    if !pushed {
        return Err(Error::Other("git push failed — see the Build console".into()));
    }
    Ok(SyncOutcome::Synced)
}

/// The git pill's whole world, computed in one place.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepoState {
    /// No repository here or above.
    NoGit,
    /// The sketch directory is itself the repository root.
    Root {
        branch: String,
        detached: bool,
        dirty: Vec<ChangedPath>,
        remote: Option<String>,
        has_upstream: bool,
        ahead: u32,
        behind: u32,
        tracked_secrets: Vec<String>,
        suggested_message: String,
    },
    /// The sketch lives inside a bigger repository (e.g. ~/Projects).
    Nested {
        root: String,
        dirty: Vec<ChangedPath>,
    },
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

    // ----- repo_state -----

    #[test]
    fn repo_state_no_git_outside_any_repo() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("loose");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(repo_state(&dir).unwrap(), RepoState::NoGit));
    }

    #[test]
    fn repo_state_root_reports_dirty_remote_and_suggestion() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Proj");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Proj.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();
        std::fs::write(dir.join("Proj.ino"), "void setup() { }\n").unwrap();
        std::fs::write(dir.join("new.h"), "#pragma once\n").unwrap();

        match repo_state(&dir).unwrap() {
            RepoState::Root { dirty, remote, has_upstream, tracked_secrets,
                              suggested_message: msg, detached, .. } => {
                let paths: Vec<&str> = dirty.iter().map(|c| c.path.as_str()).collect();
                assert_eq!(paths, ["Proj.ino", "new.h"]);
                assert_eq!(remote, None);
                assert!(!has_upstream);
                assert!(!detached);
                assert!(tracked_secrets.is_empty());
                assert_eq!(msg, "checkpoint: Proj.ino, new.h");
            }
            other => panic!("expected Root, got {other:?}"),
        }
    }

    #[test]
    fn repo_state_nested_reports_the_root_and_scopes_dirt_to_the_sketch() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        init_repo(&root).unwrap();
        let sketch = root.join("inner");
        std::fs::create_dir_all(&sketch).unwrap();

        // Add a tracked file inside the sketch directory
        std::fs::write(sketch.join("tracked.ino"), "void setup() {}\n").unwrap();
        run(&["-C", sketch.to_str().unwrap(), "add", "tracked.ino"]).unwrap();
        run(&["-C", sketch.to_str().unwrap(), "commit", "-m", "add tracked file"]).unwrap();

        // Modify the tracked file
        std::fs::write(sketch.join("tracked.ino"), "void setup() { }\n").unwrap();

        // Add an untracked file
        std::fs::write(sketch.join("inner.ino"), "void loop() {}\n").unwrap();

        // Add a file outside the sketch (should not appear in dirty)
        std::fs::write(root.join("outside.txt"), "not the sketch's business\n").unwrap();

        match repo_state(&sketch).unwrap() {
            RepoState::Nested { root: r, dirty } => {
                assert_eq!(r, root.to_string_lossy());
                let paths: Vec<&str> = dirty.iter().map(|c| c.path.as_str()).collect();
                // Both modified tracked file and untracked file should be included
                // Order may vary, so sort for comparison
                let mut paths_sorted = paths.clone();
                paths_sorted.sort();
                let expected = vec!["inner.ino", "tracked.ino"];
                assert_eq!(paths_sorted, expected, "outside.txt must not leak in; tracked and untracked files in sketch must both be present");
            }
            other => panic!("expected Nested, got {other:?}"),
        }
    }

    #[test]
    fn repo_state_flags_a_tracked_secret() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Leaky");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("secrets.h"), "#define X\n").unwrap();
        // Simulate a repo from before the ignore rules: track it by force.
        run(&["init", "--quiet", dir.to_str().unwrap()]).unwrap();
        run(&["-C", dir.to_str().unwrap(), "add", "-f", "secrets.h"]).unwrap();

        match repo_state(&dir).unwrap() {
            RepoState::Root { tracked_secrets, .. } => {
                assert_eq!(tracked_secrets, vec!["secrets.h"]);
            }
            other => panic!("expected Root, got {other:?}"),
        }
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

    // ----- porcelain-v2 parsing -----

    // Real `git status --porcelain=v2 --branch` output shapes. `1` = ordinary
    // change, `2` = rename (path\tab origPath), `?` = untracked, `u` = unmerged.
    const STATUS_V2: &str = "\
# branch.oid 4e83dc1f24864aa68013ed2de6b6d4be42179445
# branch.head main
# branch.upstream origin/main
# branch.ab +2 -1
1 .M N... 100644 100644 100644 aaaa bbbb sketch.ino
1 A. N... 000000 100644 100644 0000 cccc web/index.html
2 R. N... 100644 100644 100644 dddd eeee R100 renamed.h\told.h
u UU N... 100644 100644 100644 100644 aaaa bbbb cccc conflict.txt
? notes.txt
";

    const STATUS_V2_DETACHED: &str = "\
# branch.oid 4e83dc1f24864aa68013ed2de6b6d4be42179445
# branch.head (detached)
";

    #[test]
    fn parses_branch_upstream_and_ahead_behind() {
        let s = parse_status_v2(STATUS_V2);
        assert_eq!(s.branch, "main");
        assert!(!s.detached);
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
    }

    #[test]
    fn parses_ordinary_rename_and_untracked_entries() {
        let s = parse_status_v2(STATUS_V2);
        let paths: Vec<&str> = s.dirty.iter().map(|c| c.path.as_str()).collect();
        // The rename reports its *new* path, not the tab-joined pair.
        assert_eq!(paths, ["sketch.ino", "web/index.html", "renamed.h", "conflict.txt", "notes.txt"]);
        assert_eq!(s.dirty[0].status, ".M");
        assert_eq!(s.dirty[3].status, "UU");
        assert_eq!(s.dirty[4].status, "??");
    }

    #[test]
    fn detached_head_is_reported_with_no_upstream() {
        let s = parse_status_v2(STATUS_V2_DETACHED);
        assert!(s.detached);
        assert_eq!(s.upstream, None);
        assert_eq!((s.ahead, s.behind), (0, 0));
        assert!(s.dirty.is_empty());
    }

    #[test]
    fn a_clean_repo_without_upstream_parses_to_empty() {
        let s = parse_status_v2("# branch.oid aaaa\n# branch.head main\n");
        assert_eq!(s.branch, "main");
        assert_eq!(s.upstream, None);
        assert!(s.dirty.is_empty());
    }

    #[test]
    fn suggested_message_names_two_files_and_counts_the_rest() {
        let dirty: Vec<ChangedPath> = ["a.ino", "sketch.yaml", "web/x.py", "y.h"]
            .iter()
            .map(|p| ChangedPath { path: p.to_string(), status: ".M".into() })
            .collect();
        assert_eq!(
            suggested_message(&dirty),
            "checkpoint: a.ino, sketch.yaml (+2)"
        );
        assert_eq!(suggested_message(&dirty[..1]), "checkpoint: a.ino");
        assert_eq!(suggested_message(&[]), "checkpoint");
    }

    #[test]
    fn tracked_secrets_flags_only_credential_basenames() {
        let out = "sketch.ino\nsecrets.h\nweb/.env\nnotes/secrets.h.example\n";
        assert_eq!(tracked_secrets(out), vec!["secrets.h", "web/.env"]);
        assert!(tracked_secrets("sketch.ino\n").is_empty());
    }

    // ----- commit -----

    #[test]
    fn commit_checkpoints_everything_in_the_sketch() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Chk");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();
        std::fs::write(dir.join("b.h"), "#pragma once\n").unwrap();

        assert_eq!(commit(&dir, "checkpoint: b.h").unwrap(), CommitOutcome::Committed);
        let d = dir.to_str().unwrap();
        let log = run(&["-C", d, "log", "--oneline"]).unwrap();
        assert!(log.contains("checkpoint: b.h"), "log {log:?}");
        let status = run(&["-C", d, "status", "--porcelain"]).unwrap();
        assert!(status.trim().is_empty(), "tree should be clean: {status:?}");
    }

    #[test]
    fn commit_on_a_clean_tree_reports_nothing_to_commit() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Clean");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();
        assert_eq!(commit(&dir, "no-op").unwrap(), CommitOutcome::NothingToCommit);
    }

    /// A nested sketch's commit takes the sketch subtree only — the parent
    /// repository's other dirt must survive untouched.
    #[test]
    fn commit_in_a_nested_sketch_stays_inside_the_subtree() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        init_repo(&root).unwrap();
        let sketch = root.join("nested");
        std::fs::create_dir_all(&sketch).unwrap();
        std::fs::write(sketch.join("n.ino"), "void loop() {}\n").unwrap();
        std::fs::write(root.join("unrelated.txt"), "leave me be\n").unwrap();

        assert_eq!(commit(&sketch, "checkpoint: n.ino").unwrap(), CommitOutcome::Committed);
        let status = run(&["-C", root.to_str().unwrap(), "status", "--porcelain"]).unwrap();
        assert!(status.contains("unrelated.txt"), "must stay dirty: {status:?}");
        assert!(!status.contains("n.ino"), "must be committed: {status:?}");
    }

    // ----- sync -----

    fn commit_file(dir: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        let d = dir.to_str().unwrap();
        run(&["-C", d, "add", "-A"]).unwrap();
        run(&["-C", d, "commit", "--quiet", "-m", msg]).unwrap();
    }

    /// Two clones of one file:// remote — the two-machine bench story. The
    /// remote is seeded with a baseline before `b` clones, because every real
    /// bancada project has one (init_repo guarantees it): an unborn-HEAD clone
    /// is a test-harness artifact, not a state sync needs to handle.
    fn make_pair(tmp: &TempDir) -> (PathBuf, PathBuf) {
        let base = tmp.path().canonicalize().unwrap();
        let origin = base.join("origin.git");
        run(&["init", "--bare", "--quiet", origin.to_str().unwrap()]).unwrap();
        // Bare repos default to whatever HEAD says; pin the branch name.
        run(&["-C", origin.to_str().unwrap(), "symbolic-ref", "HEAD", "refs/heads/main"]).unwrap();
        let a = base.join("a");
        run(&["clone", "--quiet", origin.to_str().unwrap(), a.to_str().unwrap()]).unwrap();
        run(&["-C", a.to_str().unwrap(), "checkout", "--quiet", "-B", "main"]).unwrap();
        run(&["-C", a.to_str().unwrap(), "config", "user.name", "T"]).unwrap();
        run(&["-C", a.to_str().unwrap(), "config", "user.email", "t@t"]).unwrap();
        commit_file(&a, "seed.ino", "void setup() {}\n", "seed");
        run(&["-C", a.to_str().unwrap(), "push", "--quiet", "-u", "origin", "main"]).unwrap();
        let b = base.join("b");
        run(&["clone", "--quiet", origin.to_str().unwrap(), b.to_str().unwrap()]).unwrap();
        run(&["-C", b.to_str().unwrap(), "config", "user.name", "T"]).unwrap();
        run(&["-C", b.to_str().unwrap(), "config", "user.email", "t@t"]).unwrap();
        (a, b)
    }

    #[test]
    fn sync_pushes_local_commits_and_pulls_remote_ones() {
        let tmp = TempDir::new().unwrap();
        let (a, b) = make_pair(&tmp);
        commit_file(&a, "one.ino", "void setup() {}\n", "one");
        assert_eq!(sync(&a, |_| {}).unwrap(), SyncOutcome::Synced);

        // b gains a's commit via its own sync; then b commits and a re-syncs.
        assert_eq!(sync(&b, |_| {}).unwrap(), SyncOutcome::Synced);
        assert!(b.join("one.ino").exists());
        commit_file(&b, "two.ino", "void loop() {}\n", "two");
        assert_eq!(sync(&b, |_| {}).unwrap(), SyncOutcome::Synced);
        assert_eq!(sync(&a, |_| {}).unwrap(), SyncOutcome::Synced);
        assert!(a.join("two.ino").exists());
    }

    #[test]
    fn sync_rebases_non_conflicting_divergence() {
        let tmp = TempDir::new().unwrap();
        let (a, b) = make_pair(&tmp);
        commit_file(&a, "from_a.h", "// a\n", "a's");
        assert_eq!(sync(&a, |_| {}).unwrap(), SyncOutcome::Synced);
        commit_file(&b, "from_b.h", "// b\n", "b's"); // b is now diverged
        assert_eq!(sync(&b, |_| {}).unwrap(), SyncOutcome::Synced);
        assert!(b.join("from_a.h").exists());
    }

    /// The one trap sync must never set: a conflict leaves NO rebase in
    /// progress — aborted, local commits intact, tree clean.
    #[test]
    fn sync_conflict_aborts_the_rebase_and_reports_diverged() {
        let tmp = TempDir::new().unwrap();
        let (a, b) = make_pair(&tmp);
        commit_file(&a, "same.ino", "original\n", "base");
        assert_eq!(sync(&a, |_| {}).unwrap(), SyncOutcome::Synced);
        assert_eq!(sync(&b, |_| {}).unwrap(), SyncOutcome::Synced);

        commit_file(&a, "same.ino", "a's version\n", "a's");
        assert_eq!(sync(&a, |_| {}).unwrap(), SyncOutcome::Synced);
        commit_file(&b, "same.ino", "b's version\n", "b's");

        assert_eq!(sync(&b, |_| {}).unwrap(), SyncOutcome::Diverged);
        let d = b.to_str().unwrap();
        assert!(
            !b.join(".git").join("rebase-merge").exists()
                && !b.join(".git").join("rebase-apply").exists(),
            "no rebase may be left in progress"
        );
        let status = run(&["-C", d, "status", "--porcelain"]).unwrap();
        assert!(status.trim().is_empty(), "tree must be clean: {status:?}");
        let log = run(&["-C", d, "log", "--oneline"]).unwrap();
        assert!(log.contains("b's"), "local commit must survive: {log:?}");
    }

    #[test]
    fn sync_refuses_a_dirty_tree() {
        let tmp = TempDir::new().unwrap();
        let (a, _) = make_pair(&tmp);
        commit_file(&a, "x.ino", "void setup() {}\n", "base");
        std::fs::write(a.join("x.ino"), "changed but not committed\n").unwrap();
        assert_eq!(sync(&a, |_| {}).unwrap(), SyncOutcome::DirtyTree);
    }

    #[test]
    fn sync_without_a_remote_reports_no_remote() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Lonely");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("l.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();
        assert_eq!(sync(&dir, |_| {}).unwrap(), SyncOutcome::NoRemote);
    }
}
