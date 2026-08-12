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
//! - `commit` (the checkpoint path) always disables gpg-signing and hooks,
//!   the same as `init_repo` and for the same reason: a checkpoint is an
//!   app-internal safety commit, and a broken gpg agent or a harmless global
//!   hooks directory must never turn "save a checkpoint" into an error. The
//!   identity fallback, though, is conditional — used only when the
//!   repository has no `user.email` of its own — so a checkpoint with a
//!   configured identity keeps the user's own authorship, and only a
//!   genuinely identity-less repo falls back to Bancada's.

use crate::{Error, Result};
use crate::types::{OutputLine, OutputStream};
use serde::{Deserialize, Serialize};
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

/// Parse `git status --porcelain=v2 --branch -z` output. Records are
/// NUL-terminated rather than newline-terminated — including the `#
/// branch.*` headers — and pathnames are emitted verbatim, unquoted, per
/// git's porcelain-v2 `-z` spec. That matters: with the default
/// `core.quotePath`, a non-ASCII filename would otherwise arrive C-quoted
/// (`"leitura-t\303\251rmica.ino"`, literal backslashes and quotes) in the
/// plain newline-terminated form, breaking display, checkpoint messages and
/// [`tracked_secrets`] matching. `-z` sidesteps quoting entirely instead of
/// parsing it.
///
/// Entries start with `1` (ordinary), `2` (rename/copy), `u` (unmerged) or
/// `?` (untracked). Unknown record kinds are skipped: git adds header kinds
/// over time and a status pill must not break on them. A `2` (rename/copy)
/// entry's path field is followed by a *second* NUL-terminated record, the
/// original path — under `-z` there is no tab-joined pair on one line, so
/// that second record is consumed here and not surfaced (call sites want the
/// new path, same as the non-`-z` behaviour this replaces).
pub fn parse_status_v2(out: &str) -> StatusV2 {
    let mut s = StatusV2 {
        branch: String::new(),
        detached: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        dirty: Vec::new(),
    };
    let mut records = out.split('\0');
    while let Some(rec) = records.next() {
        if rec.is_empty() {
            continue;
        }
        if let Some(rest) = rec.strip_prefix("# branch.head ") {
            s.detached = rest == "(detached)";
            s.branch = rest.to_string();
        } else if let Some(rest) = rec.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.to_string());
        } else if let Some(rest) = rec.strip_prefix("# branch.ab ") {
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    s.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    s.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = rec.strip_prefix("? ") {
            s.dirty.push(ChangedPath {
                path: rest.to_string(),
                status: "??".to_string(),
            });
        } else if rec.starts_with("1 ") || rec.starts_with("2 ") || rec.starts_with("u ") {
            let status = rec.get(2..4).unwrap_or("").to_string();
            // Fields are space-separated; the path is everything from field 8
            // (ordinary/rename) / field 10 (unmerged has extra mode columns)
            // onward. Taking the tail after the Nth space is simpler than a
            // fixed-width split and survives spaces in filenames.
            let n_meta = if rec.starts_with("u ") { 10 } else { 8 };
            let mut rest = rec;
            for _ in 0..n_meta {
                rest = match rest.split_once(' ') {
                    Some((_, r)) => r,
                    None => "",
                };
            }
            // For renames/copies the remaining text is "<score> <path>"
            // (e.g. "R100 renamed.h"); strip the score. The path itself is
            // the whole rest of the record now — no tab to split on under
            // `-z`, since the original path is a separate NUL record.
            let path = if rec.starts_with("2 ") {
                match rest.split_once(' ') {
                    Some((prefix, tail))
                        if (prefix.starts_with('R') || prefix.starts_with('C'))
                            && prefix[1..].chars().all(|c| c.is_ascii_digit()) =>
                    {
                        tail.to_string()
                    }
                    _ => rest.to_string(),
                }
            } else {
                rest.to_string()
            };
            if !path.is_empty() {
                s.dirty.push(ChangedPath { path, status });
            }
            if rec.starts_with("2 ") {
                // Consume the paired origPath record; it is not surfaced.
                let _ = records.next();
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

/// Credential-named files among `git ls-files -z` output — files
/// `.gitignore` can no longer protect, because they are already tracked.
/// Expects NUL-separated, unquoted paths (`-z`): with plain `ls-files`, a
/// non-ASCII path like `configuração/secrets.h` would arrive C-quoted under
/// the default `core.quotePath` and the basename match below would miss it.
pub fn tracked_secrets(ls_files_out: &str) -> Vec<String> {
    let secret_names = ["secrets.h", "arduino_secrets.h", ".env"];
    ls_files_out
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let base = p.rsplit('/').next().unwrap_or(p);
            secret_names.contains(&base)
        })
        .map(str::to_string)
        .collect()
}

/// Strip `dir`'s root-relative prefix from each of `dirty`'s paths.
///
/// `git status -z` reports paths relative to the repository root regardless
/// of `-C`/cwd (unlike the non-`-z` form, which is cwd-relative) — see
/// `git-status(1)`'s porcelain v2 section. A nested sketch's dirty paths
/// therefore arrive as `inner/tracked.ino`, not `tracked.ino`; callers that
/// want them scoped to the sketch (matching how they're already scoped to
/// exclude the rest of the repository) need the prefix removed.
fn strip_root_prefix(root: &Path, dir: &Path, dirty: Vec<ChangedPath>) -> Vec<ChangedPath> {
    let Ok(rel) = dir.strip_prefix(root) else {
        return dirty;
    };
    let prefix = rel.to_string_lossy().replace('\\', "/");
    if prefix.is_empty() {
        return dirty;
    }
    dirty
        .into_iter()
        .map(|mut c| {
            if let Some(stripped) = c.path.strip_prefix(&prefix).and_then(|s| s.strip_prefix('/')) {
                c.path = stripped.to_string();
            }
            c
        })
        .collect()
}

/// Everything the git pill needs, in one call.
///
/// Root-vs-nested comes from [`repo_root`] ancestry. Dirty paths are always
/// scoped to the sketch directory (`status ... -- .` with `-C <sketch>`), so a
/// nested sketch never reports its parent repository's unrelated noise. Under
/// `-z` those paths come back root-relative (see [`strip_root_prefix`]), so
/// the Nested arm strips the sketch's prefix back off before returning.
pub fn repo_state(dir: &Path) -> Result<RepoState> {
    let Some(root) = repo_root(dir) else {
        return Ok(RepoState::NoGit);
    };
    let dir_s = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;

    // `-z`: NUL-terminated records with unquoted paths, so a non-ASCII
    // filename comes back exact instead of C-quoted. See `parse_status_v2`.
    let status = run(&["-C", dir_s, "status", "--porcelain=v2", "--branch", "-z", "--", "."])?;
    let parsed = parse_status_v2(&status);

    if root != dir {
        return Ok(RepoState::Nested {
            root: root.to_string_lossy().into_owned(),
            dirty: strip_root_prefix(&root, dir, parsed.dirty),
        });
    }

    // A missing remote is a state, not an error: `remote get-url` exits 2.
    let remote = run(&["-C", dir_s, "remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let ls = run(&["-C", dir_s, "ls-files", "-z", "--", "."])?;
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
///
/// gpg-signing and hooks are always disabled (`-c commit.gpgsign=false -c
/// core.hooksPath= --no-verify`), for the same reason as [`init_repo`]: a
/// checkpoint is an app-internal safety commit triggered by the Assistant
/// panel, and a broken gpg agent or a harmless global hooks directory must
/// never be able to turn "save a checkpoint" into an error.
///
/// The `user.name`/`user.email` fallback, unlike those, is conditional: it
/// is added only when [`has_usable_identity`] finds the repository has none
/// configured. When an identity exists, a checkpoint is the user's own
/// commit and keeps their authorship — a pushed checkpoint must read as
/// theirs, not as "Bancada". When none exists — a fresh machine, a sandboxed
/// build environment — the Bancada identity is a fallback so checkpointing
/// itself still never fails for lack of configuration, mirroring
/// `init_repo`'s rationale for its own baseline commit. `init_repo`'s
/// identity fallback stays unconditional on purpose: there is no prior
/// authorship to preserve for a repository that doesn't exist yet.
pub fn commit(dir: &Path, message: &str) -> Result<CommitOutcome> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let status = run(&["-C", d, "status", "--porcelain", "--", "."])?;
    if status.trim().is_empty() {
        return Ok(CommitOutcome::NothingToCommit);
    }
    run(&["-C", d, "add", "-A", "--", "."])?;

    let mut args: Vec<&str> = vec!["-C", d];
    if !has_usable_identity(d) {
        args.extend(["-c", "user.name=Bancada", "-c", "user.email=bancada@localhost"]);
    }
    args.extend([
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=",
        "commit",
        "--no-verify",
        "--quiet",
        "-m",
        message,
        "--",
        ".",
    ]);
    run(&args)?;
    Ok(CommitOutcome::Committed)
}

/// Whether `dir`'s effective git config already resolves a usable
/// `user.email` — local config, global config, or environment, whatever git
/// itself would use. Probed with `git config user.email` rather than reading
/// config files directly, so it honours exactly the same resolution order
/// `git commit` would.
fn has_usable_identity(dir: &str) -> bool {
    run(&["-C", dir, "config", "user.email"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
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

// ---------- flash tags ----------

/// `flash/2026-08-12T1430` for the instant `unix_secs`, in UTC.
///
/// Clock-free on purpose: `core` never reads the clock, so the Tauri layer
/// passes the seconds in exactly as it does for `fleet` (see that module's
/// doc). That is what makes a date-boundary case testable at all — a
/// function calling `SystemTime::now()` internally could not be pinned to a
/// leap day or to a second before midnight.
///
/// The civil-date conversion (Howard Hinnant's `civil_from_days`) is inlined
/// for the same reason `chatlog`'s fnv1a is — "it is eight lines and not
/// worth a dependency" (`chatlog.rs:33`). This is about fifteen, and the
/// alternative is putting `chrono` or `time` into a workspace that has
/// neither, to format one string.
///
/// UTC rather than local time so the tag reads the same in a log, in a
/// remote's tag list, and on the other machine of a two-machine bench;
/// minute resolution because two flashes within one minute are the same
/// bench moment, and the tag is a label, not an identifier.
pub fn flash_tag_name(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (hour, minute) = (rem / 3_600, (rem % 3_600) / 60);

    // civil_from_days: shift the epoch to 0000-03-01 so a leap day lands at
    // the end of the era and no month needs a special case. `era` is a
    // 400-year Gregorian cycle (146097 days); `doe`/`yoe`/`doy` are the day
    // of the era, year of the era and day of the year within it.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(month <= 2);

    format!("flash/{year:04}-{month:02}-{day:02}T{hour:02}{minute:02}")
}

/// Tag HEAD of `dir` with an annotated tag `name` carrying `message`.
///
/// Annotated (`-a`/`-m`) rather than lightweight because the message *is*
/// the provenance record — which board, which port, which sketch — and a
/// lightweight tag is just a ref with nowhere to put it.
///
/// The flag discipline is [`commit`]'s, one step later and for the same
/// reasons. gpg-signing and hooks are forced off (`-c tag.gpgSign=false -c
/// core.hooksPath=`) because tagging a flash is an app-internal write and a
/// broken gpg agent or a global hooks directory must never turn "record what
/// was flashed" into an error. The `user.name`/`user.email` fallback stays
/// conditional on [`has_usable_identity`]: an annotated tag carries a tagger,
/// and a pushed flash tag must read as the user's own, not as "Bancada" —
/// the fallback exists only so an identity-less machine still gets its tag.
pub fn tag_annotated(dir: &Path, name: &str, message: &str) -> Result<()> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other("tag name is empty".into()));
    }
    let mut args: Vec<&str> = vec!["-C", d];
    if !has_usable_identity(d) {
        args.extend(["-c", "user.name=Bancada", "-c", "user.email=bancada@localhost"]);
    }
    args.extend(["-c", "tag.gpgSign=false", "-c", "core.hooksPath=", "tag", "-a", "-m", message, name]);
    run(&args)?;
    Ok(())
}

/// Names of the `flash/*` tags pointing at HEAD, empty when there are none.
///
/// Scoped to `flash/*` and to HEAD so the answer is "what this exact commit
/// was flashed as", which is the only question the UI asks — a release tag
/// on the same commit, or an older flash of an earlier one, is not it.
pub fn flash_tags_at_head(dir: &Path) -> Result<Vec<String>> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let out = run(&["-C", d, "tag", "--points-at", "HEAD", "-l", "flash/*"])?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// `git push origin <tag>`, streaming git's output the way [`sync`] does.
///
/// A non-zero exit is `Ok(false)`, not an error, matching [`run_streaming`]:
/// no remote, no auth, or a tag the remote already has are all states the UI
/// reports quietly — the tag is already in the local history either way, so
/// nothing was lost.
pub fn push_tag(dir: &Path, tag: &str, on_line: impl FnMut(OutputLine)) -> Result<bool> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let tag = tag.trim();
    if tag.is_empty() {
        return Err(Error::Other("tag name is empty".into()));
    }
    run_streaming(&["-C", d, "push", "origin", tag], on_line)
}

/// The commit HEAD points at, full 40-char sha.
///
/// Exactly one subprocess, deliberately: this is called on the flash path,
/// inside the build gate, where every extra `git` spawn is time between the
/// press and the board being written. `rev-parse HEAD` answers it alone —
/// there is nothing to add without paying for another process.
///
/// A directory that is not a repository is left to git to refuse, matching
/// [`commit`] rather than [`repo_state`]'s `NoGit`. The difference is what
/// the two are for: `repo_state` exists to *describe* a directory, so "no
/// repository" is one of the answers it owes its caller, whereas this
/// question — which commit is on the board — simply has no answer there, and
/// the caller must record nothing rather than record something invented.
pub fn head_sha(dir: &Path) -> Result<String> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    Ok(run(&["-C", d, "rev-parse", "HEAD"])?.trim().to_string())
}

/// How many commits `dir`'s HEAD is ahead of `commit` — `None` when the two
/// cannot be compared at all.
///
/// `Option` rather than a documented `0` because "cannot compare" and
/// "identical" are genuinely different answers and the banner words them
/// differently: `Some(0)` is "this board is running what you are looking at",
/// `None` is "the commit this board was flashed from is not in this history"
/// — an invitation to re-flash, not a reassurance. Collapsing both to `0`
/// would make a stale board read as up to date, which is the one wrong thing
/// this function is able to say.
///
/// "Cannot compare" is `merge-base --is-ancestor` failing, which covers both
/// halves of the problem in one probe: a sha naming nothing (the `flash/*`
/// tag deleted, the project re-cloned) and a sha that still exists but no
/// longer leads to HEAD (branch force-pushed, history rebased). The second is
/// why a mere existence check would not be enough — `<commit>..HEAD` happily
/// counts the whole rewritten branch and returns a large, confident,
/// meaningless number.
///
/// HEAD is resolved first, via [`head_sha`], so that a directory which is not
/// a repository stays an error instead of degrading into `None`: a failed
/// comparison is only evidence about the *recorded commit* once the
/// repository itself is known good. Unlike [`head_sha`] this runs for a
/// banner rather than under the build gate, so the extra process costs
/// nothing anyone is waiting on.
///
/// `commit` must be a hex object name of 4–40 characters — the shape
/// [`head_sha`] returns, and the only thing ever recorded against a board.
/// Anything else is refused before git runs, the way [`create_remote`] guards
/// an empty repository name and [`tag_annotated`] an empty tag name: a stored
/// value that has been blanked, truncated or mangled must never reach a
/// command line where git could read it as an option or as a range.
pub fn project_drift(dir: &Path, commit: &str) -> Result<Option<u32>> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let rev = commit.trim();
    if rev.is_empty() {
        return Err(Error::Other("commit sha is empty".into()));
    }
    if !(4..=40).contains(&rev.len()) || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Other(format!("not a commit sha: {rev:?}")));
    }

    head_sha(dir)?;
    if run(&["-C", d, "merge-base", "--is-ancestor", rev, "HEAD"]).is_err() {
        return Ok(None);
    }

    let range = format!("{rev}..HEAD");
    let out = run(&["-C", d, "rev-list", "--count", &range])?;
    let n = out
        .trim()
        .parse()
        .map_err(|_| Error::Other(format!("git rev-list --count printed {out:?}")))?;
    Ok(Some(n))
}

// ---------- gh (GitHub CLI) ----------

/// Is the GitHub CLI on PATH? Its absence only hides the create-repo button.
pub fn gh_available() -> bool {
    std::process::Command::new("gh")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Who can see a repository `gh repo create` makes. Private stays the
/// default at every call site: a bench sketch usually carries credentials
/// before it carries a README, and publishing is a decision to take
/// deliberately rather than to inherit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Private,
    Public,
}

/// `gh repo create <name> --private|--public [--description <text>] --source
/// <dir> --push` — `--push` last so nothing partial happens on auth/name
/// errors, which is why `--description` rides ahead of `--source` rather
/// than being appended.
///
/// An empty or whitespace-only description is dropped entirely instead of
/// being passed as `--description ''`: a blank field is a user declining to
/// write one, and gh would happily record the blank.
pub fn create_remote_args(
    name: &str,
    dir: &str,
    visibility: Visibility,
    description: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["repo".into(), "create".into(), name.into()];
    args.push(
        match visibility {
            Visibility::Private => "--private",
            Visibility::Public => "--public",
        }
        .into(),
    );
    if let Some(text) = description.map(str::trim).filter(|t| !t.is_empty()) {
        args.push("--description".into());
        args.push(text.into());
    }
    args.push("--source".into());
    args.push(dir.into());
    args.push("--push".into());
    args
}

/// Create a GitHub repo for `dir` at `visibility` and push. Streams gh's
/// output.
pub fn create_remote(
    dir: &Path,
    name: &str,
    visibility: Visibility,
    description: Option<&str>,
    mut on_line: impl FnMut(OutputLine),
) -> Result<()> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other("repository name is empty".into()));
    }
    let args = create_remote_args(name, d, visibility, description);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut child = std::process::Command::new("gh")
        .args(&arg_refs)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::ToolMissing("gh".into())
            } else {
                Error::Io(e)
            }
        })?;
    // Same interleave shape as run_streaming, inlined for the gh binary.
    use std::io::BufRead;
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
    let mut tail = Vec::new();
    for line in rx {
        if line.stream == OutputStream::Stderr {
            tail.push(line.line.clone());
        }
        on_line(line);
    }
    let _ = t_out.join();
    let _ = t_err.join();
    let status = child.wait()?;
    if !status.success() {
        return Err(Error::ToolFailed {
            tool: "gh repo create".into(),
            status: status.code().unwrap_or(-1),
            // gh explains auth problems on stderr ("run: gh auth login").
            stderr: tail.join("\n"),
        });
    }
    Ok(())
}

/// Wire an existing remote repository as origin and push the current branch.
pub fn set_remote(dir: &Path, url: &str, mut on_line: impl FnMut(OutputLine)) -> Result<()> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::Other("remote URL is empty".into()));
    }
    run(&["-C", d, "remote", "add", "origin", url])?;
    if !run_streaming(&["-C", d, "push", "-u", "origin", "HEAD"], &mut on_line)? {
        // Leave no half-configured origin behind a failed first push.
        let _ = run(&["-C", d, "remote", "remove", "origin"]);
        return Err(Error::Other(
            "first push failed — origin was not kept; check the URL and access".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Restores a process environment variable to whatever it held before,
    /// when dropped — including on an unwinding panic. A couple of fixtures
    /// below need to point `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` at
    /// `/dev/null` so the `commit()` call under test can't see this
    /// machine's ambient git identity; `commit()` calls `run()`, which
    /// spawns `git` inheriting the process environment, so there is no
    /// per-invocation `.env()` to attach the override to instead — the
    /// mutation has to be process-wide. Wrapping it in a guard means a
    /// failed assertion mid-test can never leave the override leaked into
    /// whatever test runs next in the same process.
    struct EnvGuard {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var_os(key);
            std::env::set_var(key, value);
            EnvGuard { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

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

        // Add a tracked file inside the sketch directory. Identity is set
        // locally (rather than relying on the host's ambient git config, as
        // the rest of this module's fixtures do) so this test is hermetic —
        // in particular, immune to another test in the suite isolating
        // itself from ambient identity via GIT_CONFIG_GLOBAL/SYSTEM.
        std::fs::write(sketch.join("tracked.ino"), "void setup() {}\n").unwrap();
        run(&["-C", sketch.to_str().unwrap(), "config", "user.name", "T"]).unwrap();
        run(&["-C", sketch.to_str().unwrap(), "config", "user.email", "t@t"]).unwrap();
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

    /// `git status -z` reports paths root-relative regardless of `-C`,
    /// unlike the non-`-z` form which is cwd-relative — confirmed by hand
    /// against a real nested checkout (`git -C <subdir> status --porcelain=v2
    /// -z -- .` returns `inner/tracked.ino`, not `tracked.ino`). Direct unit
    /// coverage of the stripping helper, independent of the full
    /// `repo_state` round-trip above.
    #[test]
    fn strip_root_prefix_removes_the_sketchs_root_relative_prefix() {
        let root = Path::new("/repo");
        let dirty = vec![
            ChangedPath { path: "inner/tracked.ino".into(), status: ".M".into() },
            ChangedPath { path: "inner/new.ino".into(), status: "??".into() },
        ];
        let stripped = strip_root_prefix(root, Path::new("/repo/inner"), dirty);
        let paths: Vec<&str> = stripped.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["tracked.ino", "new.ino"]);
    }

    #[test]
    fn strip_root_prefix_is_a_no_op_at_the_repo_root() {
        let root = Path::new("/repo");
        let dirty = vec![ChangedPath { path: "a.ino".into(), status: ".M".into() }];
        let stripped = strip_root_prefix(root, root, dirty.clone());
        assert_eq!(stripped, dirty);
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

    /// The bug this test exists for: with the default `core.quotePath`, a
    /// non-ASCII filename comes back from `status --porcelain=v2` (and
    /// `ls-files`) C-quoted — `"t\303\251rmica.ino"`, literal backslashes and
    /// quotes — which broke display, checkpoint messages, and
    /// `tracked_secrets` matching a quoted `configuração/secrets.h`. `-z`
    /// (NUL-terminated, unquoted paths) fixes all three at once.
    #[test]
    fn repo_state_reports_non_ascii_paths_unquoted_and_flags_a_non_ascii_secret() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("Naovoce");
        std::fs::create_dir_all(dir.join("configuração")).unwrap();
        std::fs::write(dir.join("térmica.ino"), "void setup() {}\n").unwrap();
        std::fs::write(dir.join("configuração/secrets.h"), "#define X\n").unwrap();
        run(&["init", "--quiet", dir.to_str().unwrap()]).unwrap();
        let d = dir.to_str().unwrap();
        run(&["-C", d, "config", "user.name", "T"]).unwrap();
        run(&["-C", d, "config", "user.email", "t@t"]).unwrap();
        // Simulate a repo from before the ignore rules: track the secret by
        // force, same as `repo_state_flags_a_tracked_secret`. Committed (not
        // left staged) so only `térmica.ino`'s later edit shows up as dirty.
        run(&["-C", d, "add", "-f", "configuração/secrets.h"]).unwrap();
        run(&["-C", d, "add", "térmica.ino"]).unwrap();
        run(&["-C", d, "commit", "--quiet", "-m", "base"]).unwrap();

        std::fs::write(dir.join("térmica.ino"), "void setup() { }\n").unwrap();

        match repo_state(&dir).unwrap() {
            RepoState::Root { dirty, tracked_secrets, .. } => {
                let paths: Vec<&str> = dirty.iter().map(|c| c.path.as_str()).collect();
                assert_eq!(
                    paths,
                    ["térmica.ino"],
                    "path must come back exact, not C-quoted: {paths:?}"
                );
                assert_eq!(
                    tracked_secrets,
                    vec!["configuração/secrets.h"],
                    "a quoted path would not have matched the secrets basename check"
                );
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

    // Real `git status --porcelain=v2 --branch -z` output shapes: every
    // record (including headers) is NUL-terminated instead of newline
    // terminated. `1` = ordinary change, `2` = rename (path record, then a
    // *separate* NUL-terminated origPath record — no tab under `-z`), `?` =
    // untracked, `u` = unmerged.
    const STATUS_V2: &str = "\
# branch.oid 4e83dc1f24864aa68013ed2de6b6d4be42179445\0\
# branch.head main\0\
# branch.upstream origin/main\0\
# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaaa bbbb sketch.ino\0\
1 A. N... 000000 100644 100644 0000 cccc web/index.html\0\
2 R. N... 100644 100644 100644 dddd eeee R100 renamed.h\0old.h\0\
u UU N... 100644 100644 100644 100644 aaaa bbbb cccc conflict.txt\0\
? notes.txt\0";

    const STATUS_V2_DETACHED: &str = "\
# branch.oid 4e83dc1f24864aa68013ed2de6b6d4be42179445\0\
# branch.head (detached)\0";

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
        let s = parse_status_v2("# branch.oid aaaa\0# branch.head main\0");
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
        let out = "sketch.ino\0secrets.h\0web/.env\0notes/secrets.h.example\0";
        assert_eq!(tracked_secrets(out), vec!["secrets.h", "web/.env"]);
        assert!(tracked_secrets("sketch.ino\0").is_empty());
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

    /// The bug this test exists for: `commit` had none of `init_repo`'s
    /// fallbacks, so a checkpoint on a machine with no git identity failed
    /// "Please tell me who you are", a repo with `commit.gpgsign=true` and no
    /// usable agent hung/failed, and a broken global hook aborted the
    /// checkpoint. GIT_CONFIG_GLOBAL/SYSTEM are pointed at `/dev/null` for the
    /// duration (via `EnvGuard`, restored on drop even if an assertion below
    /// panics) so the assertion exercises the fixture's (lack of) identity,
    /// not whatever happens to be configured on the machine running the test.
    #[test]
    fn commit_falls_back_to_bancada_identity_and_skips_gpg_and_hooks() {
        let _global = EnvGuard::set("GIT_CONFIG_GLOBAL", "/dev/null");
        let _system = EnvGuard::set("GIT_CONFIG_SYSTEM", "/dev/null");

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("NoIdentity");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();

        let d = dir.to_str().unwrap();
        // Explicitly unset any identity (init_repo never persists one to
        // local config — it only ever used `-c` for its own commit) and force
        // a gpg signature this sandbox has no agent to produce.
        let _ = run(&["-C", d, "config", "--unset-all", "user.name"]);
        let _ = run(&["-C", d, "config", "--unset-all", "user.email"]);
        run(&["-C", d, "config", "commit.gpgsign", "true"]).unwrap();

        std::fs::write(dir.join("b.h"), "#pragma once\n").unwrap();
        let result = commit(&dir, "checkpoint: b.h");

        assert_eq!(result.unwrap(), CommitOutcome::Committed);
        let log = run(&["-C", d, "log", "--format=%an <%ae>", "-1"]).unwrap();
        assert!(log.contains("Bancada <bancada@localhost>"), "log {log:?}");
    }

    /// FINDING 1 fix: when the repository already has a usable identity, a
    /// checkpoint commit must carry it — not Bancada's — so a pushed
    /// checkpoint reads as the user's own commit, not as authored by the
    /// app. Identity is set on local config only, so this is hermetic
    /// regardless of what the other fixture in this module does with
    /// GIT_CONFIG_GLOBAL/SYSTEM: local config always wins.
    #[test]
    fn commit_uses_the_repos_own_identity_when_one_is_configured() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("HasIdentity");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();

        let d = dir.to_str().unwrap();
        run(&["-C", d, "config", "user.name", "Test User"]).unwrap();
        run(&["-C", d, "config", "user.email", "t@example.com"]).unwrap();

        std::fs::write(dir.join("b.h"), "#pragma once\n").unwrap();
        assert_eq!(commit(&dir, "checkpoint: b.h").unwrap(), CommitOutcome::Committed);

        let author_name = run(&["-C", d, "log", "-1", "--format=%an"]).unwrap();
        let author_email = run(&["-C", d, "log", "-1", "--format=%ae"]).unwrap();
        assert_eq!(author_name.trim(), "Test User");
        assert_eq!(author_email.trim(), "t@example.com");
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

    // ----- flash tags -----

    /// Expected values were read off `date -u -d @<secs>`, not derived from
    /// the implementation, so the inlined civil-date arithmetic is checked
    /// against the system's own calendar rather than against itself.
    #[test]
    fn flash_tag_name_formats_the_utc_minute_of_a_unix_instant() {
        // date -u -d @0            -> 1970-01-01T0000
        assert_eq!(flash_tag_name(0), "flash/1970-01-01T0000");
        // date -u -d @1786545000   -> 2026-08-12T1430
        assert_eq!(flash_tag_name(1_786_545_000), "flash/2026-08-12T1430");
        // date -u -d @2147483647   -> 2038-01-19T0314 (past the 32-bit cliff)
        assert_eq!(flash_tag_name(2_147_483_647), "flash/2038-01-19T0314");
        // date -u -d @4102444799   -> 2099-12-31T2359
        assert_eq!(flash_tag_name(4_102_444_799), "flash/2099-12-31T2359");
    }

    /// A leap day is where a hand-rolled days-from-civil conversion goes
    /// wrong first, so it gets its own case.
    #[test]
    fn flash_tag_name_handles_a_leap_day() {
        // date -u -d @1709164800 -> 2024-02-29T0000
        assert_eq!(flash_tag_name(1_709_164_800), "flash/2024-02-29T0000");
        // Seconds within the minute are dropped, not rounded up.
        assert_eq!(flash_tag_name(1_709_164_837), "flash/2024-02-29T0000");
    }

    /// The rollover the tag name must get right: two seconds apart across
    /// midnight UTC are two different days, and 2024-02-29 is followed by
    /// 2024-03-01, not 2024-03-00.
    #[test]
    fn flash_tag_name_rolls_the_day_at_midnight_utc() {
        // date -u -d @1709251140 -> 2024-02-29T2359
        assert_eq!(flash_tag_name(1_709_251_140), "flash/2024-02-29T2359");
        // date -u -d @1709251199 -> 2024-02-29T2359 (last second of the day)
        assert_eq!(flash_tag_name(1_709_251_199), "flash/2024-02-29T2359");
        // date -u -d @1709251200 -> 2024-03-01T0000
        assert_eq!(flash_tag_name(1_709_251_200), "flash/2024-03-01T0000");
    }

    /// A repository with its own identity, so annotated tags are possible
    /// without exercising the fallback.
    fn repo_with_identity(tmp: &TempDir, name: &str) -> PathBuf {
        let dir = tmp.path().canonicalize().unwrap().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();
        let d = dir.to_str().unwrap();
        run(&["-C", d, "config", "user.name", "Test User"]).unwrap();
        run(&["-C", d, "config", "user.email", "t@example.com"]).unwrap();
        dir
    }

    /// Annotated, not lightweight: the message is the provenance record of
    /// what was flashed, and a lightweight tag has nowhere to keep it.
    #[test]
    fn tag_annotated_writes_an_annotated_tag_carrying_its_message() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "Tagged");
        let d = dir.to_str().unwrap();

        tag_annotated(&dir, "flash/2026-08-12T1430", "esp32:esp32:esp32c6 on /dev/ttyACM0").unwrap();

        let kind = run(&["-C", d, "cat-file", "-t", "flash/2026-08-12T1430"]).unwrap();
        assert_eq!(kind.trim(), "tag", "must be an annotated tag object");
        let msg = run(&["-C", d, "tag", "-l", "--format=%(contents)", "flash/2026-08-12T1430"]).unwrap();
        assert!(msg.contains("esp32:esp32:esp32c6 on /dev/ttyACM0"), "message was {msg:?}");
        let tagger = run(&["-C", d, "tag", "-l", "--format=%(taggeremail)", "flash/2026-08-12T1430"]).unwrap();
        assert!(tagger.contains("t@example.com"), "own identity must be kept: {tagger:?}");
    }

    /// Same failure modes `commit` guards against, one step later: tagging a
    /// flash must not fail because the machine has no git identity, because
    /// the repo asks for a gpg signature no agent can produce, or because a
    /// global hooks directory has something to say.
    ///
    /// Identity is emptied in *local* config rather than by pointing
    /// `GIT_CONFIG_GLOBAL` at `/dev/null` the way the `commit` fixture does.
    /// An empty `user.email` is exactly what [`has_usable_identity`] reads as
    /// "none configured", and git itself refuses it ("empty ident name (for
    /// <>) not allowed"), so the failure mode is reproduced faithfully — and
    /// hermetically, without a second test mutating the same process-wide
    /// environment variable concurrently and restoring it out from under this
    /// one.
    #[test]
    fn tag_annotated_falls_back_to_bancada_identity_and_skips_gpg_and_hooks() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("NoIdentityTag");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();

        let d = dir.to_str().unwrap();
        run(&["-C", d, "config", "user.name", ""]).unwrap();
        run(&["-C", d, "config", "user.email", ""]).unwrap();
        run(&["-C", d, "config", "tag.gpgSign", "true"]).unwrap();

        tag_annotated(&dir, "flash/1970-01-01T0000", "flashed").unwrap();

        let tagger = run(&["-C", d, "tag", "-l", "--format=%(taggeremail)", "flash/1970-01-01T0000"]).unwrap();
        assert!(tagger.contains("bancada@localhost"), "tagger {tagger:?}");
        // And the override really took: an unsigned tag has no signature,
        // however well-configured the gpg agent on this machine happens to be.
        let sig = run(&["-C", d, "tag", "-l", "--format=%(contents:signature)", "flash/1970-01-01T0000"]).unwrap();
        assert!(sig.trim().is_empty(), "tag.gpgSign=false was not honoured: {sig:?}");
    }

    #[test]
    fn empty_tag_names_are_refused_before_git_runs() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "EmptyName");
        let err = tag_annotated(&dir, "   ", "m").unwrap_err().to_string();
        assert!(err.contains("empty"), "{err}");
        let err = push_tag(&dir, "", |_| {}).unwrap_err().to_string();
        assert!(err.contains("empty"), "{err}");
    }

    /// Only `flash/*`, and only at HEAD: an older flash tag and a release
    /// tag sharing the repository must not be reported as this commit's.
    #[test]
    fn flash_tags_at_head_lists_only_flash_tags_pointing_at_head() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "Points");
        let d = dir.to_str().unwrap();

        assert!(flash_tags_at_head(&dir).unwrap().is_empty(), "premise: no tags yet");

        // The fixture's own lightweight tags carry `-c tag.gpgSign=false`
        // because the machine running the tests may well sign tags by
        // default (this repository's release ritual does) — under an ambient
        // `tag.gpgsign=true`, a bare `git tag <name>` becomes a signed
        // annotated tag and fails with "no tag message?". That ambient
        // setting is precisely what `tag_annotated` overrides, so the calls
        // to it below are left alone to exercise the override.
        let no_sign = ["-C", d, "-c", "tag.gpgSign=false", "tag"];

        // An older commit carries a flash tag of its own.
        run(&[&no_sign[..], &["flash/2020-01-01T0000"]].concat()).unwrap();
        std::fs::write(dir.join("b.h"), "#pragma once\n").unwrap();
        commit(&dir, "second").unwrap();

        tag_annotated(&dir, "flash/2026-08-12T1430", "flashed").unwrap();
        tag_annotated(&dir, "flash/2026-08-12T1435", "flashed again").unwrap();
        run(&[&no_sign[..], &["v1.0.0"]].concat()).unwrap();

        let mut tags = flash_tags_at_head(&dir).unwrap();
        tags.sort();
        assert_eq!(tags, ["flash/2026-08-12T1430", "flash/2026-08-12T1435"]);
    }

    #[test]
    fn push_tag_publishes_the_tag_to_origin() {
        let tmp = TempDir::new().unwrap();
        let (a, _b) = make_pair(&tmp);
        let base = tmp.path().canonicalize().unwrap();
        commit_file(&a, "flashed.ino", "void setup() {}\n", "flashed");
        assert_eq!(sync(&a, |_| {}).unwrap(), SyncOutcome::Synced);

        tag_annotated(&a, "flash/2026-08-12T1430", "esp32:esp32:esp32c6").unwrap();
        assert!(push_tag(&a, "flash/2026-08-12T1430", |_| {}).unwrap());

        let refs = run(&["-C", base.join("origin.git").to_str().unwrap(), "for-each-ref", "refs/tags"]).unwrap();
        assert!(refs.contains("flash/2026-08-12T1430"), "origin tags: {refs:?}");
    }

    /// A non-zero exit is a `false`, not an `Err` — same contract as
    /// [`run_streaming`], so the caller can report "not pushed" quietly.
    #[test]
    fn push_tag_reports_false_when_git_refuses() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "NoOrigin");
        tag_annotated(&dir, "flash/2026-08-12T1430", "flashed").unwrap();
        // No origin remote at all: git exits non-zero.
        assert!(!push_tag(&dir, "flash/2026-08-12T1430", |_| {}).unwrap());
    }

    // ----- flashed-commit provenance -----

    #[test]
    fn head_sha_is_the_full_forty_char_sha_rev_parse_reports() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "Head");
        let sha = head_sha(&dir).unwrap();
        assert_eq!(sha.len(), 40, "sha was {sha:?}");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()), "sha was {sha:?}");
        let expected = run(&["-C", dir.to_str().unwrap(), "rev-parse", "HEAD"]).unwrap();
        assert_eq!(sha, expected.trim());
    }

    /// Unlike [`repo_state`], which answers `NoGit` because describing a
    /// directory is its whole job, this one lets git refuse: there is no sha
    /// to hand back, and the flash path records nothing rather than something
    /// invented.
    #[test]
    fn head_sha_lets_git_refuse_a_directory_that_is_not_a_repository() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("loose");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(head_sha(&dir).is_err());
    }

    #[test]
    fn project_drift_is_zero_against_head_itself() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "Same");
        let sha = head_sha(&dir).unwrap();
        assert_eq!(project_drift(&dir, &sha).unwrap(), Some(0));
    }

    #[test]
    fn project_drift_counts_the_commits_made_since() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "Moved");
        let flashed = head_sha(&dir).unwrap();
        for n in 0..3 {
            std::fs::write(dir.join(format!("f{n}.h")), "#pragma once\n").unwrap();
            assert_eq!(commit(&dir, "since").unwrap(), CommitOutcome::Committed);
        }
        assert_eq!(project_drift(&dir, &flashed).unwrap(), Some(3));
        // An abbreviated sha names the same commit and gets the same answer.
        assert_eq!(project_drift(&dir, &flashed[..12]).unwrap(), Some(3));
    }

    /// The recorded sha names nothing here — the repository was re-cloned, or
    /// the sha was never this project's to begin with. `None`, not an error
    /// and not a reassuring `0`.
    #[test]
    fn project_drift_cannot_compare_an_unknown_commit() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "Unknown");
        assert_eq!(
            project_drift(&dir, "0123456789abcdef0123456789abcdef01234567").unwrap(),
            None
        );
    }

    /// The other half of "cannot compare", and the reason an existence check
    /// alone would not do: after a force-push or a rebase the recorded commit
    /// is still in the object store but no longer leads to HEAD, and
    /// `<commit>..HEAD` would answer with the size of the rewritten branch —
    /// a confident, meaningless number.
    #[test]
    fn project_drift_cannot_compare_a_commit_no_longer_reachable_from_head() {
        let tmp = TempDir::new().unwrap();
        let dir = repo_with_identity(&tmp, "Rewritten");
        let d = dir.to_str().unwrap();
        let baseline = head_sha(&dir).unwrap();
        std::fs::write(dir.join("b.h"), "#pragma once\n").unwrap();
        commit(&dir, "flashed from here").unwrap();
        let flashed = head_sha(&dir).unwrap();

        run(&["-C", d, "reset", "--hard", "--quiet", &baseline]).unwrap();
        assert!(
            run(&["-C", d, "cat-file", "-t", &flashed]).is_ok(),
            "premise: the object itself survives the rewind"
        );
        assert_eq!(project_drift(&dir, &flashed).unwrap(), None);
    }

    /// Refused in Rust before git is spawned, the same shape as
    /// [`tag_annotated`]'s empty-name guard. The fixture directory is not a
    /// repository at all, so anything that leaked through to git would come
    /// back as a `ToolFailed` about *that* instead of the messages asserted
    /// here — which is what makes this an assertion that git never ran.
    #[test]
    fn project_drift_refuses_an_empty_or_malformed_commit_before_git_runs() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("never-used");
        std::fs::create_dir_all(&dir).unwrap();

        for blank in ["", "   "] {
            let err = project_drift(&dir, blank).unwrap_err().to_string();
            assert!(err.contains("empty"), "{blank:?} gave {err}");
        }
        for bad in [
            "HEAD",
            "abc",
            "--upload-pack=touch /tmp/pwned",
            "deadbeef..HEAD",
            "v1.0.0",
            "0123456789abcdef0123456789abcdef012345678",
        ] {
            let err = project_drift(&dir, bad).unwrap_err().to_string();
            assert!(err.contains("not a commit sha"), "{bad:?} gave {err}");
        }
    }

    // ----- gh remote -----

    #[test]
    fn create_remote_args_are_private_source_push() {
        assert_eq!(
            create_remote_args(
                "teste-uno-veia",
                "/home/u/Projects/teste-uno-veia",
                Visibility::Private,
                None,
            ),
            ["repo", "create", "teste-uno-veia", "--private",
             "--source", "/home/u/Projects/teste-uno-veia", "--push"]
        );
    }

    #[test]
    fn create_remote_args_can_publish_a_public_repo() {
        assert_eq!(
            create_remote_args("aberto", "/home/u/Projects/aberto", Visibility::Public, None),
            ["repo", "create", "aberto", "--public",
             "--source", "/home/u/Projects/aberto", "--push"]
        );
    }

    /// `--description` rides between visibility and `--source`, so `--push`
    /// stays last no matter which options are present.
    #[test]
    fn create_remote_args_carry_a_description_before_source_and_push() {
        assert_eq!(
            create_remote_args(
                "termometro",
                "/home/u/Projects/termometro",
                Visibility::Public,
                Some("Leitura térmica com AHT20"),
            ),
            ["repo", "create", "termometro", "--public",
             "--description", "Leitura térmica com AHT20",
             "--source", "/home/u/Projects/termometro", "--push"]
        );
    }

    /// An empty description is omitted entirely rather than passed as an
    /// empty flag: `gh repo create --description ''` is a valid but pointless
    /// argument, and a whitespace-only one is a user leaving the field blank.
    #[test]
    fn create_remote_args_omit_a_blank_description() {
        let expected = ["repo", "create", "x", "--private", "--source", "/d", "--push"];
        assert_eq!(create_remote_args("x", "/d", Visibility::Private, Some("")), expected);
        assert_eq!(create_remote_args("x", "/d", Visibility::Private, Some("   ")), expected);
        assert_eq!(create_remote_args("x", "/d", Visibility::Private, None), expected);
    }

    #[test]
    fn set_remote_rejects_an_obviously_bad_url() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("R");
        std::fs::create_dir_all(&dir).unwrap();
        init_repo(&dir).unwrap();
        let err = set_remote(&dir, "   ", |_| {}).unwrap_err().to_string();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn set_remote_records_origin_before_attempting_the_push() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let origin = base.join("srv.git");
        run(&["init", "--bare", "--quiet", origin.to_str().unwrap()]).unwrap();
        let dir = base.join("local");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.ino"), "void setup() {}\n").unwrap();
        init_repo(&dir).unwrap();

        set_remote(&dir, origin.to_str().unwrap(), |_| {}).unwrap();
        let url = run(&["-C", dir.to_str().unwrap(), "remote", "get-url", "origin"]).unwrap();
        assert_eq!(url.trim(), origin.to_str().unwrap());
        // And the push -u actually landed the baseline.
        let refs = run(&["-C", origin.to_str().unwrap(), "for-each-ref"]).unwrap();
        assert!(!refs.trim().is_empty(), "remote should have the branch: {refs:?}");
    }
}
