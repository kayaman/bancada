# Project Git Checkpoint & Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A toolbar git pill with two actions — Commit (checkpoint-all with editable suggested message) and Sync (fetch → rebase → push) — plus repo initialization and gh-based remote creation.

**Architecture:** Extend `core/src/git.rs` (arriving via the `fix-git-detection` merge) with pure porcelain-v2 parsing, pure arg builders, and thin runners; six thin Tauri commands in the `spawn_blocking` + `err_str` shape; one `GitPill` toolbar component using the `Menu.tsx` popover primitive. Git and gh are shelled out to as free functions resolved from PATH — never a struct, never libgit2.

**Tech Stack:** Rust (bancada-core + Tauri 2), React + TypeScript, vitest, `git` and `gh` CLIs.

**Spec:** `docs/superpowers/specs/2026-08-09-project-git-sync-design.md`

## Global Constraints

- cwd is always expressed as `-C <dir>` in git args, never `Command::current_dir`.
- One `Command::new("git")` site in the whole crate: `core::git::run` (streaming gets its own, in the same module). Same rule for `gh`.
- `ErrorKind::NotFound` → `Error::ToolMissing("git")` / `ToolMissing("gh")`; non-zero exit → `Error::ToolFailed { tool, status, stderr }` with trimmed stderr.
- Pure arg builders + pure output parsers, unit-tested inline; thin `pub fn`s only compose builder → runner → parser.
- Rust field names cross the Tauri boundary in snake_case verbatim; invoke *arguments* are camelCase in TS.
- Never leave the repo mid-operation: any failed rebase is aborted before returning.
- The status bar (`notify`) is transient; sync/remote output streams to the Build console via `build://line`.
- `core/src/git.rs` unit tests run real `git` in tempdirs without `#[ignore]` (the module's existing convention — git is a README prerequisite); only tests needing *network* are `#[ignore]`d.
- Commit after every green step. All commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` (blank line before the trailer; shown abbreviated as `[trailer]` in tasks).

---

### Task 1: Merge `fix-git-detection` into main

**Files:**
- Modify: merge commit touching `core/src/ghlib.rs`, `core/src/git.rs` (new), `core/src/lib.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `bancada_core::git::{run, is_under_git, repo_root, init_repo, ensure_under_git}` — `run(args: &[&str]) -> Result<String>`, `is_under_git(dir: &Path) -> bool`, `repo_root(dir: &Path) -> Option<PathBuf>`, `init_repo(dir: &Path) -> Result<()>`, `ensure_under_git(dir: &Path) -> Result<bool>`.

- [ ] **Step 1: Check the shared checkout, then merge**

This checkout hosts concurrent sessions. Confirm a clean tree first; if dirty, stop and report rather than merging over someone's work-in-progress.

```bash
cd /home/kayaman/Projects/bancada
git status --short   # must be empty
git merge fix-git-detection
```

Expected: a conflict in `src-tauri/src/lib.rs` — both sides extended `create_project` (main added required-libs injection near `library_errors`; the branch added `ensure_under_git` and two `CreatedProject` fields).

- [ ] **Step 2: Resolve the conflict — keep both sides**

In `CreatedProject`, keep main's `library_errors` field and add the branch's two:

```rust
    /// Libraries that could not be added; the project is still usable.
    library_errors: Vec<String>,
    /// Whether the new project ended up under git — either because it was
    /// created inside an existing work tree, or because it was initialised.
    under_git: bool,
    /// Why `git init` did not happen, when it was attempted and failed.
    /// Non-fatal: the sketch exists and builds either way.
    git_error: Option<String>,
```

In `create_project`, keep main's required-libs loop exactly as it is, and insert the branch's git block after it, before the `Ok(CreatedProject { ... })`:

```rust
        // Put the project under git so the Assistant's auto-applied edits have
        // something to undo against. Skipped when the parent is already a work
        // tree — initialising there would nest a second repository inside one
        // the user already keeps. Non-fatal for the same reason the library
        // failures are: the sketch exists and builds without it.
        let mut git_error = None;
        let under_git = match bancada_core::git::ensure_under_git(&dir) {
            Ok(_) => true,
            Err(e) => {
                git_error = Some(e.to_string());
                false
            }
        };

        Ok(CreatedProject {
            dir: dir.to_string_lossy().into_owned(),
            name,
            profile,
            library_errors,
            under_git,
            git_error,
        })
```

Also verify the merged `sketch_has_git` body is the branch's (`bancada_core::git::is_under_git(Path::new(&sketch_dir))`).

- [ ] **Step 3: Run the Rust suites**

```bash
cargo test -p bancada-core --lib
cargo test -p bancada --lib
```

Expected: PASS (the branch brings its own `core::git` tests; main's tests unaffected).

- [ ] **Step 4: Commit the merge**

```bash
git add -A && git commit --no-edit || git commit -m "merge: fix-git-detection — core::git module, ancestry-aware detection

[trailer]"
```

---

### Task 2: Credential `.gitignore` written before the baseline commit

**Files:**
- Modify: `core/src/git.rs` (add gitignore logic to `init_repo`)
- Modify: `core/src/clone.rs` (delete its private copies, import from `git`)

**Interfaces:**
- Consumes: `git::run`, `git::init_repo` from Task 1.
- Produces: `git::GITIGNORE_REQUIRED: &[&str]`, `git::merged_gitignore(existing: &str) -> String`, `git::write_gitignore(dir: &Path) -> Result<()>` — and `init_repo` now writes the `.gitignore` before `add --all`.

- [ ] **Step 1: Move the constant and merge function from `clone.rs` into `git.rs`, public**

Cut `GITIGNORE_REQUIRED` (clone.rs:29-37) and `merged_gitignore` (clone.rs:310-336) verbatim into `core/src/git.rs`, making both `pub`, and add:

```rust
/// Write (or extend) `dir/.gitignore` so every [`GITIGNORE_REQUIRED`] entry is
/// present. Reads what exists and appends only what is missing — a
/// hand-maintained ignore file is preserved, not replaced.
pub fn write_gitignore(dir: &Path) -> Result<()> {
    let p = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&p).unwrap_or_default();
    std::fs::write(&p, merged_gitignore(&existing))?;
    Ok(())
}
```

In `clone.rs`, replace the two private definitions with `use crate::git::{merged_gitignore, GITIGNORE_REQUIRED};` (the symlink-guard write in clone.rs stays as-is — it has clone-specific semantics). Move `merged_gitignore`'s unit tests from clone.rs's test module into git.rs's.

- [ ] **Step 2: Write the failing test — init writes the ignore file before the baseline**

In `core/src/git.rs` `mod tests`:

```rust
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
```

- [ ] **Step 3: Run it — expect FAIL** (`secrets.h` gets committed today)

```bash
cargo test -p bancada-core --lib git::tests::init_repo_writes_credential_gitignore -- --nocapture
```

- [ ] **Step 4: Implement — `init_repo` writes the ignore file first**

In `init_repo`, immediately after the `run(&["init", "--quiet", path])?;` line:

```rust
    write_gitignore(dir)?;
```

- [ ] **Step 5: Run the full core suite — expect PASS (including moved clone tests)**

```bash
cargo test -p bancada-core --lib
```

- [ ] **Step 6: Commit**

```bash
git add core/src/git.rs core/src/clone.rs
git commit -m "feat: init_repo writes the credential .gitignore before the baseline commit

[trailer]"
```

---

### Task 3: Porcelain-v2 parsing and `RepoState` (pure)

**Files:**
- Modify: `core/src/git.rs`

**Interfaces:**
- Produces (all pure, no subprocess):
  - `pub struct ChangedPath { pub path: String, pub status: String }`
  - `pub struct StatusV2 { pub branch: String, pub detached: bool, pub upstream: Option<String>, pub ahead: u32, pub behind: u32, pub dirty: Vec<ChangedPath> }`
  - `pub fn parse_status_v2(out: &str) -> StatusV2`
  - `pub fn suggested_message(dirty: &[ChangedPath]) -> String`
  - `pub fn tracked_secrets(ls_files_out: &str) -> Vec<String>`
  - `#[serde(tag = "kind", rename_all = "snake_case")] pub enum RepoState { NoGit, Root { branch, detached, dirty, remote, has_upstream, ahead, behind, tracked_secrets, suggested_message }, Nested { root: String, dirty: Vec<ChangedPath> } }` — `Root` fields: `branch: String, detached: bool, dirty: Vec<ChangedPath>, remote: Option<String>, has_upstream: bool, ahead: u32, behind: u32, tracked_secrets: Vec<String>, suggested_message: String`. `ChangedPath`, `StatusV2`, `RepoState` all `#[derive(Debug, Clone, Serialize)]`; `ChangedPath` and `StatusV2` also `PartialEq, Eq`.

- [ ] **Step 1: Write the failing tests**

In `core/src/git.rs` `mod tests`, with a real captured fixture:

```rust
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
        assert_eq!(paths, ["sketch.ino", "web/index.html", "renamed.h", "notes.txt"]);
        assert_eq!(s.dirty[0].status, ".M");
        assert_eq!(s.dirty[3].status, "??");
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
```

- [ ] **Step 2: Run — expect FAIL (nothing defined)**

```bash
cargo test -p bancada-core --lib git::tests -- --nocapture
```

- [ ] **Step 3: Implement the types and the three pure functions**

```rust
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
            let path = rest.split('\t').next().unwrap_or("").to_string();
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
```

And the state enum (fields as in **Interfaces** above):

```rust
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
```

(`use serde::Serialize;` at the top of git.rs.)

- [ ] **Step 4: Run — expect PASS**

```bash
cargo test -p bancada-core --lib git::tests
```

- [ ] **Step 5: Commit**

```bash
git add core/src/git.rs
git commit -m "feat: porcelain-v2 parser, RepoState, suggested message, tracked-secrets check

[trailer]"
```

---

### Task 4: `repo_state` runner

**Files:**
- Modify: `core/src/git.rs`

**Interfaces:**
- Consumes: `run`, `repo_root`, `parse_status_v2`, `tracked_secrets`, `suggested_message`, `RepoState` (Tasks 1, 3).
- Produces: `pub fn repo_state(dir: &Path) -> Result<RepoState>`.

- [ ] **Step 1: Write the failing tests** (real git in tempdirs, same as the module's existing tests)

```rust
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
        std::fs::write(sketch.join("inner.ino"), "void loop() {}\n").unwrap();
        std::fs::write(root.join("outside.txt"), "not the sketch's business\n").unwrap();

        match repo_state(&sketch).unwrap() {
            RepoState::Nested { root: r, dirty } => {
                assert_eq!(r, root.to_string_lossy());
                let paths: Vec<&str> = dirty.iter().map(|c| c.path.as_str()).collect();
                assert_eq!(paths, ["inner.ino"], "outside.txt must not leak in");
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
```

- [ ] **Step 2: Run — expect FAIL** (`repo_state` undefined)

- [ ] **Step 3: Implement**

```rust
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
```

- [ ] **Step 4: Run — expect PASS.** Then run the whole core suite.

```bash
cargo test -p bancada-core --lib
```

- [ ] **Step 5: Commit**

```bash
git add core/src/git.rs
git commit -m "feat: repo_state — the git pill's one read call

[trailer]"
```

---

### Task 5: Commit operation

**Files:**
- Modify: `core/src/git.rs`

**Interfaces:**
- Consumes: `run`, `repo_state` (Tasks 1, 4).
- Produces: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "snake_case")] pub enum CommitOutcome { Committed, NothingToCommit }` and `pub fn commit(dir: &Path, message: &str) -> Result<CommitOutcome>`.

- [ ] **Step 1: Write the failing tests**

```rust
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
```

- [ ] **Step 2: Run — expect FAIL**

- [ ] **Step 3: Implement**

```rust
/// Outcome of a checkpoint commit. "Nothing to commit" is a state the UI
/// reports quietly, not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitOutcome {
    Committed,
    NothingToCommit,
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
```

- [ ] **Step 4: Run — expect PASS**

- [ ] **Step 5: Commit**

```bash
git add core/src/git.rs
git commit -m "feat: git commit op — checkpoint-all scoped to the sketch subtree

[trailer]"
```

---

### Task 6: Streaming runner and Sync

**Files:**
- Modify: `core/src/git.rs`

**Interfaces:**
- Consumes: `run`, `repo_state`, `RepoState`, `parse_status_v2` (Tasks 1, 3, 4); `OutputLine`, `OutputStream` from `crate::types`.
- Produces:
  - `pub fn run_streaming(args: &[&str], on_line: impl FnMut(OutputLine)) -> Result<bool>` (true = exit 0)
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "snake_case")] pub enum SyncOutcome { Synced, DirtyTree, Diverged, NoRemote, NotRoot }`
  - `pub fn sync(dir: &Path, on_line: impl FnMut(OutputLine)) -> Result<SyncOutcome>`

- [ ] **Step 1: Implement the streaming runner** (mirrors `cli::run_streaming`; no test of its own — it is exercised by every sync test below)

```rust
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
```

(`use crate::types::{OutputLine, OutputStream};` at the top.)

- [ ] **Step 2: Write the failing sync tests** — two local clones stand in for "another machine"; file remotes need no network, so these stay un-`#[ignore]`d like the rest of the module:

```rust
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
```

- [ ] **Step 3: Run — expect FAIL**

- [ ] **Step 4: Implement sync**

```rust
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
```

- [ ] **Step 5: Run — expect PASS.** Full core suite too.

```bash
cargo test -p bancada-core --lib
```

- [ ] **Step 6: Commit**

```bash
git add core/src/git.rs
git commit -m "feat: git sync — fetch/rebase/push with conflict auto-abort

[trailer]"
```

---

### Task 7: gh runner and remote setup

**Files:**
- Modify: `core/src/git.rs`

**Interfaces:**
- Consumes: `run`, `run_streaming` (Tasks 1, 6).
- Produces: `pub fn gh_available() -> bool`, `pub fn create_remote_args(name: &str, dir: &str) -> Vec<String>`, `pub fn create_remote(dir: &Path, name: &str, on_line: impl FnMut(OutputLine)) -> Result<()>`, `pub fn set_remote(dir: &Path, url: &str, on_line: impl FnMut(OutputLine)) -> Result<()>`.

- [ ] **Step 1: Write the failing tests** (arg builders and validation are pure; gh itself is exercised only in the manual smoke — network)

```rust
    #[test]
    fn create_remote_args_are_private_source_push() {
        assert_eq!(
            create_remote_args("teste-uno-veia", "/home/u/Projects/teste-uno-veia"),
            ["repo", "create", "teste-uno-veia", "--private",
             "--source", "/home/u/Projects/teste-uno-veia", "--push"]
        );
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
```

- [ ] **Step 2: Run — expect FAIL**

- [ ] **Step 3: Implement**

```rust
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

/// `gh repo create <name> --private --source <dir> --push` — private by
/// default; `--push` last so nothing partial happens on auth/name errors.
pub fn create_remote_args(name: &str, dir: &str) -> Vec<String> {
    ["repo", "create", name, "--private", "--source", dir, "--push"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Create a private GitHub repo for `dir` and push. Streams gh's output.
pub fn create_remote(dir: &Path, name: &str, mut on_line: impl FnMut(OutputLine)) -> Result<()> {
    let d = dir
        .to_str()
        .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", dir.display())))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other("repository name is empty".into()));
    }
    let args = create_remote_args(name, d);
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
```

- [ ] **Step 4: Run — expect PASS**

- [ ] **Step 5: Commit**

```bash
git add core/src/git.rs
git commit -m "feat: gh repo creation and set-remote with first-push rollback

[trailer]"
```

---

### Task 8: Tauri commands; retire `sketch_has_git`

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `bancada_core::git::{repo_state, commit, init_repo, write_gitignore, sync, create_remote, set_remote, gh_available, RepoState, CommitOutcome, SyncOutcome, is_under_git}`.
- Produces (commands, exact names the frontend invokes): `git_state(sketch_dir) -> RepoState`, `git_commit(sketch_dir, message) -> CommitOutcome`, `git_init(sketch_dir) -> RepoState`, `git_sync(sketch_dir) -> SyncOutcome` (streams `build://line`), `git_create_remote(sketch_dir, name) -> ()` (streams), `git_set_remote(sketch_dir, url) -> ()` (streams), `gh_available() -> bool`. `sketch_has_git` is deleted.

- [ ] **Step 1: Add the commands** (next to the build & flash section)

```rust
// ---------- project git (checkpoint & sync) ----------

#[tauri::command]
async fn git_state(sketch_dir: String) -> Result<bancada_core::git::RepoState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::git::repo_state(Path::new(&sketch_dir)).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn git_commit(
    sketch_dir: String,
    message: String,
) -> Result<bancada_core::git::CommitOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::git::commit(Path::new(&sketch_dir), &message).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

/// Initialize a repository (with the credential .gitignore and a baseline
/// commit) and return the fresh state, so the pill updates in one round trip.
#[tauri::command]
async fn git_init(sketch_dir: String) -> Result<bancada_core::git::RepoState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = Path::new(&sketch_dir);
        if bancada_core::git::is_under_git(dir) {
            return Err("already under git".to_string());
        }
        bancada_core::git::init_repo(dir).map_err(err_str)?;
        bancada_core::git::repo_state(dir).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn git_sync(
    app: AppHandle,
    sketch_dir: String,
) -> Result<bancada_core::git::SyncOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::git::sync(Path::new(&sketch_dir), |line| {
            let _ = app.emit("build://line", &line);
        })
        .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn git_create_remote(
    app: AppHandle,
    sketch_dir: String,
    name: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::git::create_remote(Path::new(&sketch_dir), &name, |line| {
            let _ = app.emit("build://line", &line);
        })
        .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn git_set_remote(app: AppHandle, sketch_dir: String, url: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::git::set_remote(Path::new(&sketch_dir), &url, |line| {
            let _ = app.emit("build://line", &line);
        })
        .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
fn gh_available() -> bool {
    bancada_core::git::gh_available()
}
```

Delete the `sketch_has_git` command (and its doc comment). In `tauri::generate_handler![...]`, remove `sketch_has_git` and add the seven new names.

- [ ] **Step 2: Update the src-tauri tests**

Replace the two `sketch_has_git_*` tests (added by the merge in Task 1) with the same scenarios through `git_state`'s core call:

```rust
    /// The reported bug the ancestry walk fixed, now answered by repo_state:
    /// a sketch inside ~/Projects (itself a checkout) is Nested, not NoGit.
    #[test]
    fn git_state_sees_a_repo_above_the_sketch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        bancada_core::git::init_repo(&root).unwrap();
        let sketch = root.join("notundergit");
        std::fs::create_dir_all(&sketch).unwrap();

        let state = bancada_core::git::repo_state(&sketch).unwrap();
        assert!(
            matches!(state, bancada_core::git::RepoState::Nested { .. }),
            "got {state:?}"
        );
    }

    #[test]
    fn git_state_is_no_git_outside_any_repo() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("loose");
        std::fs::create_dir_all(&dir).unwrap();
        let state = bancada_core::git::repo_state(&dir).unwrap();
        assert!(matches!(state, bancada_core::git::RepoState::NoGit), "got {state:?}");
    }
```

- [ ] **Step 3: Build and test**

```bash
cargo check -p bancada && cargo test -p bancada --lib
```

Expected: PASS. (The frontend still calls `sketch_has_git` until Task 9 — the dev app is briefly broken between these two commits, which is why they land back-to-back.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: git_state/commit/init/sync/remote commands; retire sketch_has_git

[trailer]"
```

---

### Task 9: `api.ts` wrappers, `gitStatus.ts` helpers, tests

**Files:**
- Modify: `src/api.ts` (remove `sketchHasGit`, add git section)
- Create: `src/gitStatus.ts`
- Test: `src/__tests__/gitStatus.test.ts`; modify `src/__tests__/api.test.ts`

**Interfaces:**
- Consumes: Task 8's command names and serde shapes.
- Produces:
  - `api.ts`: `RepoState`, `ChangedPath`, `CommitOutcome`, `SyncOutcome` types; `gitState(sketchDir)`, `gitCommit(sketchDir, message)`, `gitInit(sketchDir)`, `gitSync(sketchDir)`, `gitCreateRemote(sketchDir, name)`, `gitSetRemote(sketchDir, url)`, `ghAvailable()`.
  - `gitStatus.ts`: `pillLabel(state: RepoState | null): string | null`, `type PopoverMode = "actions" | "setup_remote" | "init" | "nested"`, `popoverMode(state: RepoState): PopoverMode`, `syncDisabledReason(state: RepoState): string | null`, `parentName(root: string): string`.

- [ ] **Step 1: Replace `sketchHasGit` in `api.ts`**

Delete the `sketchHasGit` wrapper (api.ts:281-283) and add, in the same region:

```ts
// ---------- project git (checkpoint & sync) ----------

/** One dirty path with its porcelain XY status ("??" = untracked). */
export interface ChangedPath {
  path: string;
  status: string;
}
/** Mirror of core::git::RepoState (serde tag = "kind"). */
export type RepoState =
  | { kind: "no_git" }
  | {
      kind: "root";
      branch: string;
      detached: boolean;
      dirty: ChangedPath[];
      remote: string | null;
      has_upstream: boolean;
      ahead: number;
      behind: number;
      tracked_secrets: string[];
      suggested_message: string;
    }
  | { kind: "nested"; root: string; dirty: ChangedPath[] };

export type CommitOutcome = "committed" | "nothing_to_commit";
export type SyncOutcome = "synced" | "dirty_tree" | "diverged" | "no_remote" | "not_root";

/** The git pill's whole world — see core::git::repo_state. */
export const gitState = (sketchDir: string) =>
  invoke<RepoState>("git_state", { sketchDir });
export const gitCommit = (sketchDir: string, message: string) =>
  invoke<CommitOutcome>("git_commit", { sketchDir, message });
/** Init + credential .gitignore + baseline commit; returns the fresh state. */
export const gitInit = (sketchDir: string) =>
  invoke<RepoState>("git_init", { sketchDir });
/** fetch → rebase → push; output streams to build://line. */
export const gitSync = (sketchDir: string) =>
  invoke<SyncOutcome>("git_sync", { sketchDir });
export const gitCreateRemote = (sketchDir: string, name: string) =>
  invoke<void>("git_create_remote", { sketchDir, name });
export const gitSetRemote = (sketchDir: string, url: string) =>
  invoke<void>("git_set_remote", { sketchDir, url });
export const ghAvailable = () => invoke<boolean>("gh_available");
```

- [ ] **Step 2: Write the failing vitest tests**

`src/__tests__/gitStatus.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { parentName, pillLabel, popoverMode, syncDisabledReason } from "../gitStatus";
import type { RepoState } from "../api";

const root = (over: Partial<Extract<RepoState, { kind: "root" }>> = {}): RepoState => ({
  kind: "root",
  branch: "main",
  detached: false,
  dirty: [],
  remote: "git@github.com:m/x.git",
  has_upstream: true,
  ahead: 0,
  behind: 0,
  tracked_secrets: [],
  suggested_message: "checkpoint",
  ...over,
});

describe("pillLabel", () => {
  it("is null with no state (no sketch open — no placeholder pill)", () => {
    expect(pillLabel(null)).toBeNull();
  });
  it("shows clean, changed, and ahead/behind states", () => {
    expect(pillLabel(root())).toBe("✓ clean");
    expect(
      pillLabel(root({ dirty: [{ path: "a.ino", status: ".M" }] })),
    ).toBe("1 changed");
    expect(pillLabel(root({ ahead: 2, behind: 1 }))).toBe("✓ clean ↑2 ↓1");
    expect(pillLabel(root({ ahead: 2 }))).toBe("✓ clean ↑2");
  });
  it("names the other states", () => {
    expect(pillLabel({ kind: "no_git" })).toBe("no git");
    expect(
      pillLabel({ kind: "nested", root: "/home/u/Projects", dirty: [] }),
    ).toBe("tracked by Projects");
  });
});

describe("popoverMode", () => {
  it("routes each state to its popover", () => {
    expect(popoverMode({ kind: "no_git" })).toBe("init");
    expect(popoverMode({ kind: "nested", root: "/p", dirty: [] })).toBe("nested");
    expect(popoverMode(root({ remote: null }))).toBe("setup_remote");
    expect(popoverMode(root())).toBe("actions");
  });
});

describe("syncDisabledReason", () => {
  it("explains a dirty tree and a detached head", () => {
    expect(
      syncDisabledReason(root({ dirty: [{ path: "a", status: ".M" }] })),
    ).toMatch(/commit first/i);
    expect(syncDisabledReason(root({ detached: true }))).toMatch(/detached/i);
    expect(syncDisabledReason(root())).toBeNull();
  });
});

describe("parentName", () => {
  it("takes the last path segment", () => {
    expect(parentName("/home/u/Projects")).toBe("Projects");
    expect(parentName("/")).toBe("/");
  });
});
```

In `src/__tests__/api.test.ts`, delete the `sketchHasGit` case and add (same arg-assertion style):

```ts
  it("gitState passes sketchDir", async () => {
    await api.gitState("/s");
    expect(called()).toEqual(["git_state", { sketchDir: "/s" }]);
  });

  it("gitCommit passes sketchDir and message", async () => {
    await api.gitCommit("/s", "checkpoint: x");
    expect(called()).toEqual(["git_commit", { sketchDir: "/s", message: "checkpoint: x" }]);
  });

  it("gitSync passes sketchDir", async () => {
    await api.gitSync("/s");
    expect(called()).toEqual(["git_sync", { sketchDir: "/s" }]);
  });

  it("gitCreateRemote passes sketchDir and name", async () => {
    await api.gitCreateRemote("/s", "proj");
    expect(called()).toEqual(["git_create_remote", { sketchDir: "/s", name: "proj" }]);
  });

  it("gitSetRemote passes sketchDir and url", async () => {
    await api.gitSetRemote("/s", "git@host:r.git");
    expect(called()).toEqual(["git_set_remote", { sketchDir: "/s", url: "git@host:r.git" }]);
  });
```

- [ ] **Step 3: Run — expect FAIL** (`../gitStatus` missing)

```bash
npx vitest run src/__tests__/gitStatus.test.ts
```

- [ ] **Step 4: Implement `src/gitStatus.ts`**

```ts
// Pure derivations for the toolbar git pill — kept out of the component so
// the pill's whole vocabulary is unit-testable, like ports.ts is for ports.

import type { RepoState } from "./api";

/** Last path segment, for "tracked by <parent>". */
export function parentName(root: string): string {
  const seg = root.split("/").filter(Boolean);
  return seg.length ? seg[seg.length - 1] : root;
}

/** The pill's text, or null when there is nothing to say (no open sketch). */
export function pillLabel(state: RepoState | null): string | null {
  if (!state) return null;
  switch (state.kind) {
    case "no_git":
      return "no git";
    case "nested":
      return `tracked by ${parentName(state.root)}`;
    case "root": {
      let label = state.dirty.length === 0 ? "✓ clean" : `${state.dirty.length} changed`;
      if (state.ahead > 0) label += ` ↑${state.ahead}`;
      if (state.behind > 0) label += ` ↓${state.behind}`;
      return label;
    }
  }
}

export type PopoverMode = "actions" | "setup_remote" | "init" | "nested";

/** Which popover the pill opens. Remote setup replaces the actions until an
 *  origin exists — Commit still works there via the actions in that pane. */
export function popoverMode(state: RepoState): PopoverMode {
  switch (state.kind) {
    case "no_git":
      return "init";
    case "nested":
      return "nested";
    case "root":
      return state.remote ? "actions" : "setup_remote";
  }
}

/** Why Sync is disabled right now, or null when it can run. The profile
 *  silently winning over the port taught us (2026-08-09) that disabled
 *  buttons must say why. */
export function syncDisabledReason(state: RepoState): string | null {
  if (state.kind !== "root") return "sync works from the repository root";
  if (state.detached) return "HEAD is detached — check out a branch first";
  if (state.dirty.length > 0) return "uncommitted changes — commit first";
  return null;
}
```

- [ ] **Step 5: Run all frontend tests — expect PASS**

```bash
npx vitest run
```

- [ ] **Step 6: Commit**

```bash
git add src/api.ts src/gitStatus.ts src/__tests__/gitStatus.test.ts src/__tests__/api.test.ts
git commit -m "feat: git api wrappers and pure pill-state helpers

[trailer]"
```

---

### Task 10: GitPill component, Toolbar & App wiring, CSS

**Files:**
- Create: `src/components/GitPill.tsx`
- Modify: `src/components/Toolbar.tsx` (render the pill), `src/App.tsx` (state + refresh + handlers), `src/styles.css`

**Interfaces:**
- Consumes: Task 9's api wrappers and helpers; `Menu.tsx` popover; `notify`; `openBottomTab("build")`.
- Produces: `<GitPill state={RepoState | null} busy={boolean} onCommit={(msg) => void} onSync={() => void} onInit={() => void} onCreateRemote={(name) => void} onSetRemote={(url) => void} ghAvailable={boolean} defaultRepoName={string} />`.

- [ ] **Step 1: Write `GitPill.tsx`** (RecentsMenu anchor pattern + Menu shell; commit row is an input like ProfileInit)

```tsx
import { useCallback, useRef, useState } from "react";
import Menu from "./Menu";
import type { RepoState } from "../api";
import { parentName, pillLabel, popoverMode, syncDisabledReason } from "../gitStatus";

interface Props {
  state: RepoState | null;
  busy: boolean;
  ghAvailable: boolean;
  /** Prefill for the gh repo name: the sketch folder's name. */
  defaultRepoName: string;
  onCommit: (message: string) => void;
  onSync: () => void;
  onInit: () => void;
  onCreateRemote: (name: string) => void;
  onSetRemote: (url: string) => void;
}

/** Toolbar git pill: one glanceable state, and a popover holding the two
 *  actions (Commit / Sync) or the setup that must come first. Rendered only
 *  while a sketch is open — the caller passes state=null otherwise. */
export default function GitPill(props: Props) {
  const btnRef = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const [message, setMessage] = useState("");
  const [repoName, setRepoName] = useState("");
  const [url, setUrl] = useState("");

  const label = pillLabel(props.state);
  const close = useCallback(() => setAnchor(null), []);
  if (!props.state || label === null) return null;
  const state = props.state;

  const toggle = () => {
    if (anchor) {
      setAnchor(null);
      return;
    }
    if (state.kind === "root") setMessage(state.suggested_message);
    setRepoName(props.defaultRepoName);
    const r = btnRef.current?.getBoundingClientRect();
    if (r) setAnchor({ x: r.left, y: r.bottom + 4 });
  };

  const mode = popoverMode(state);
  const syncReason = syncDisabledReason(state);
  const secrets = state.kind === "root" ? state.tracked_secrets : [];
  const dirtyCount = state.kind === "root" || state.kind === "nested" ? state.dirty.length : 0;

  const commitRow = (
    <div className="git-pop-row">
      <input
        className="input"
        value={message}
        onChange={(e) => setMessage(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && message.trim() && dirtyCount > 0) {
            props.onCommit(message.trim());
            close();
          }
        }}
        placeholder="commit message"
        title="Checkpoint everything in this sketch"
      />
      <button
        className="btn small primary"
        disabled={props.busy || !message.trim() || dirtyCount === 0}
        onClick={() => {
          props.onCommit(message.trim());
          close();
        }}
      >
        Commit
      </button>
    </div>
  );

  return (
    <>
      <button
        ref={btnRef}
        className={`git-pill ${dirtyCount > 0 || state.kind === "no_git" ? "attention" : ""}`}
        onClick={toggle}
        title="Git — checkpoint & sync"
        aria-haspopup="menu"
        aria-expanded={anchor !== null}
      >
        {label}
      </button>
      {anchor && (
        <Menu x={anchor.x} y={anchor.y} onClose={close} anchorRef={btnRef}>
          {secrets.length > 0 && (
            <div className="git-pop-warning" role="note">
              ⚠ tracked despite .gitignore: {secrets.join(", ")}
            </div>
          )}
          {mode === "init" && (
            <button
              className="ctx-item"
              disabled={props.busy}
              onClick={() => {
                props.onInit();
                close();
              }}
            >
              Initialize repository
            </button>
          )}
          {mode === "nested" && state.kind === "nested" && (
            <>
              {commitRow}
              <div
                className="git-pop-note"
                title="Pushing would publish the whole parent repository — more than this button promises."
              >
                sync is up to the {parentName(state.root)} repo
              </div>
            </>
          )}
          {(mode === "actions" || mode === "setup_remote") && commitRow}
          {mode === "actions" && (
            <button
              className="ctx-item"
              disabled={props.busy || syncReason !== null}
              title={syncReason ?? "fetch, rebase, push"}
              onClick={() => {
                props.onSync();
                close();
              }}
            >
              ⇅ Sync
            </button>
          )}
          {mode === "setup_remote" && (
            <>
              {props.ghAvailable && (
                <div className="git-pop-row">
                  <input
                    className="input"
                    value={repoName}
                    onChange={(e) => setRepoName(e.target.value)}
                    placeholder="repository name"
                    title="Created private on your GitHub account"
                  />
                  <button
                    className="btn small"
                    disabled={props.busy || !repoName.trim()}
                    onClick={() => {
                      props.onCreateRemote(repoName.trim());
                      close();
                    }}
                  >
                    Create on GitHub
                  </button>
                </div>
              )}
              <div className="git-pop-row">
                <input
                  className="input"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="or paste a git remote URL"
                />
                <button
                  className="btn small"
                  disabled={props.busy || !url.trim()}
                  onClick={() => {
                    props.onSetRemote(url.trim());
                    close();
                  }}
                >
                  Set remote
                </button>
              </div>
            </>
          )}
        </Menu>
      )}
    </>
  );
}
```

- [ ] **Step 2: Wire into `Toolbar.tsx`**

Add to `Props`:

```tsx
  gitState: RepoState | null;
  ghAvailable: boolean;
  onGitCommit: (message: string) => void;
  onGitSync: () => void;
  onGitInit: () => void;
  onGitCreateRemote: (name: string) => void;
  onGitSetRemote: (url: string) => void;
```

(plus `import GitPill from "./GitPill"; import type { RepoState } from "../api";`). Render right after the port `toolbar-pair`'s closing `</div>`, before `<div className="spacer" />`:

```tsx
      {props.sketchDir && (
        <GitPill
          state={props.gitState}
          busy={props.busy}
          ghAvailable={props.ghAvailable}
          defaultRepoName={props.sketchDir.split("/").filter(Boolean).pop() ?? ""}
          onCommit={props.onGitCommit}
          onSync={props.onGitSync}
          onInit={props.onGitInit}
          onCreateRemote={props.onGitCreateRemote}
          onSetRemote={props.onGitSetRemote}
        />
      )}
```

- [ ] **Step 3: Wire into `App.tsx`**

State + refresh (near the `gitWarning` state, which this replaces):

```tsx
  // The toolbar pill's whole world; null while no sketch is open.
  const [gitState, setGitState] = useState<api.RepoState | null>(null);
  const [ghOk, setGhOk] = useState(false);
  const gitStateRefreshRef = useRef<(dir: string) => void>(() => {});

  const refreshGitState = useCallback((dir: string) => {
    // Fire-and-forget like fleetSync: a hint, not load-bearing.
    api
      .gitState(dir)
      .then(setGitState)
      .catch(() => setGitState(null));
  }, []);
  gitStateRefreshRef.current = refreshGitState;
```

In `loadSketch`, replace the `sketchHasGit` block (App.tsx:491-495) with:

```tsx
      refreshGitState(dir);
```

Derive the Assistant warning instead of fetching it — where `gitWarning` was set, compute it from state; delete the `gitWarning` useState and pass to `<AgentPanel gitWarning={gitState?.kind === "no_git"} ...>`.

Refresh triggers: at the end of `saveAll` (after buffers flush) and in `handleAgentFileChange` (refs only — use `gitStateRefreshRef.current(dir)` beside the `listSketchFiles` call). And one `useEffect(() => { api.ghAvailable().then(setGhOk).catch(() => setGhOk(false)); }, [])`.

Action handlers (beside `upload`):

```tsx
  const gitCommit = async (message: string) => {
    if (!sketchDir) return;
    await saveAll();
    try {
      const outcome = await api.gitCommit(sketchDir, message);
      notify(outcome === "committed" ? "✓ Committed" : "Nothing to commit");
    } catch (e) {
      notify(String(e), true);
    } finally {
      refreshGitState(sketchDir);
    }
  };

  const gitSync = async () => {
    if (!sketchDir) return;
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    notify("Syncing…");
    try {
      const outcome = await api.gitSync(sketchDir);
      const msg: Record<api.SyncOutcome, [string, boolean]> = {
        synced: ["✓ Synced with origin", false],
        dirty_tree: ["Uncommitted changes — commit first", true],
        diverged: [
          "Diverged with conflicts — rebase aborted; resolve in a terminal, then Sync again",
          true,
        ],
        no_remote: ["No remote configured", true],
        not_root: ["Sync works from the repository root", true],
      };
      const [text, isErr] = msg[outcome];
      notify(text, isErr);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      refreshGitState(sketchDir);
    }
  };

  const gitInit = async () => {
    if (!sketchDir) return;
    try {
      setGitState(await api.gitInit(sketchDir));
      notify("✓ Repository initialized (credential .gitignore + baseline commit)");
    } catch (e) {
      notify(String(e), true);
    }
  };

  const gitCreateRemote = async (name: string) => {
    if (!sketchDir) return;
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    notify(`Creating private GitHub repo ${name}…`);
    try {
      await api.gitCreateRemote(sketchDir, name);
      notify(`✓ Created and pushed to ${name}`);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      refreshGitState(sketchDir);
    }
  };

  const gitSetRemote = async (url: string) => {
    if (!sketchDir) return;
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    notify("Setting remote and pushing…");
    try {
      await api.gitSetRemote(sketchDir, url);
      notify("✓ Remote set and pushed");
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      refreshGitState(sketchDir);
    }
  };
```

Pass all of it at the `<Toolbar>` call site:

```tsx
        gitState={gitState}
        ghAvailable={ghOk}
        onGitCommit={gitCommit}
        onGitSync={gitSync}
        onGitInit={gitInit}
        onGitCreateRemote={gitCreateRemote}
        onGitSetRemote={gitSetRemote}
```

Also clear on close: wherever `sketchDir` is reset to null, `setGitState(null)`.

- [ ] **Step 4: CSS** (feature-prefixed, `.obs-chip` lineage)

```css
/* ---------- toolbar git pill ---------- */

.git-pill {
  font-size: 10px;
  font-family: var(--font-ui);
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  padding: 1px 8px;
  height: 20px;
  color: var(--text-dim);
  background: none;
  cursor: pointer;
  flex: none;
  white-space: nowrap;
}

.git-pill.attention {
  color: var(--warn);
  border-color: var(--warn);
}

.git-pill:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

/* Rows inside the pill's popover: input + button as one unit. */
.git-pop-row {
  display: flex;
  gap: 4px;
  padding: 5px 12px;
}

.git-pop-row .input {
  width: 200px;
}

.git-pop-warning {
  padding: 5px 12px;
  font-size: 11px;
  color: var(--warn);
}

.git-pop-note {
  padding: 5px 12px;
  font-size: 11px;
  color: var(--text-dim);
}
```

- [ ] **Step 5: Full check — types, tests, dev run**

```bash
npx tsc --noEmit && npx vitest run && cargo check -p bancada
```

Expected: PASS / clean.

- [ ] **Step 6: Commit**

```bash
git add src/components/GitPill.tsx src/components/Toolbar.tsx src/App.tsx src/styles.css
git commit -m "feat: toolbar git pill — checkpoint & sync popover

[trailer]"
```

---

### Task 11: Full verification, README, smoke checklist

**Files:**
- Modify: `README.md` (prerequisites)

**Interfaces:** none new.

- [ ] **Step 1: README prerequisite note**

In the prerequisites section, after the git line, add:

```markdown
# gh (GitHub CLI) — optional: powers the one-button "create private repo"
# in the git pill; without it, paste any git remote URL instead.
sudo zypper install gh   # then: gh auth login
```

- [ ] **Step 2: Run everything**

```bash
cargo test -p bancada-core --lib && cargo test -p bancada --lib && npx vitest run && npx tsc --noEmit
```

Expected: all PASS.

- [ ] **Step 3: Manual smoke** (run `npm run tauri dev`; needs a human or the bench)

1. Fresh project → pill reads `✓ clean` (created under git by Task 1's merge).
2. Open `~/Projects/teste-uno-veia` (repo, no remote) → edit a file → pill `1 changed` → Commit with the suggested message → `✓ clean` → popover shows remote setup → Create on GitHub (or paste URL) → Sync → `✓ Synced`.
3. Open a sketch nested under the `~/Projects` checkout → pill `tracked by Projects`, Commit works, Sync explains itself.
4. Clone the GitHub repo elsewhere, commit there, push; back in bancada, commit locally too (non-conflicting) → Sync → rebase output in Build console, `✓ Synced`. Repeat with a conflicting edit → "Diverged … rebase aborted", `git status` clean, local commit intact.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: gh CLI as an optional prerequisite for the git pill

[trailer]"
```
