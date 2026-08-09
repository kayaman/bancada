# Usage Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist per-project Assistant token/cost totals in a cumulative `usage.json` and show them on a full-area dashboard with links into individual session replays.

**Architecture:** A new path-agnostic `core/src/usage.rs` store accumulates result-event usage at `chat_append` time (surviving the 50-chat prune and chat deletion), seeded once from surviving chat logs. Two read commands feed a new `UsageDashboard` React screen opened from the toolbar; session links reuse the existing `chat_load` → `replayChat` → `ReplayView` pipeline.

**Tech Stack:** Rust (serde_json, tempfile for tests), Tauri 2 commands, React + TypeScript, vitest. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-09-usage-dashboard-design.md`

## Global Constraints

- Branch: `feat/usage-dashboard` (already checked out in this worktree). Commit after every task.
- No new crate or npm dependencies.
- Rust structs cross IPC in `snake_case` (no `rename_all` on structs); every non-key field carries `#[serde(default)]`; persisted JSON uses `to_string_pretty`; saves are atomic (`.json.tmp` + rename).
- Core stays path-agnostic and clock-free: functions take `&Path`; no clock reads anywhere in this feature (recency comes from chat file stems).
- Tauri commands return `Result<T, String>` via `err_str`; file-mutating commands stay sync (main-thread serialization).
- TS wrappers: camelCase arrow consts returning `invoke<T>` directly; interfaces mirror Rust field names verbatim.
- Run Rust tests with `cargo test -p bancada-core` from the repo root. Frontend: `npm test` (vitest). Type check + bundle: `npm run build`.
- The dev server may not be running; do NOT launch `npm run tauri dev` — manual smoke testing is the final task's (human) step.

---

### Task 1: Core usage store (`core/src/usage.rs`)

**Files:**
- Create: `core/src/usage.rs`
- Modify: `core/src/lib.rs` (the module list is alphabetical and ends with `pub mod types;` — append `pub mod usage;` after it)

**Interfaces:**
- Consumes: `crate::{Error, Result}` (existing `thiserror` enum with `Json { what, source }` variant).
- Produces (used by Tasks 2, 4):
  - `pub const USAGE_VERSION: u32 = 1;`
  - `pub struct ProjectUsage { sketch_dir: String, cost_usd: f64, input_tokens: u64, output_tokens: u64, turns: u64, sessions: u64, last_chat: Option<String> }` (all pub fields)
  - `UsageStore::load(path: &Path) -> Result<UsageStore>` / `save(&self, path: &Path) -> Result<()>`
  - `UsageStore::note_new_chat(&mut self, key: &str, sketch_dir: &str) -> bool`
  - `UsageStore::record_line(&mut self, key: &str, sketch_dir: &str, file: &str, line: &str) -> bool`
  - `UsageStore::overview(&self) -> Vec<ProjectUsage>`

- [ ] **Step 1: Write the failing tests**

Create `core/src/usage.rs` containing only the `mod tests` block below (plus the `use` lines it needs); the types and functions it references come in Step 3, so the first run fails to compile — that is the failing state:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A recorded `result` push op, mirroring chatlog's test helper.
    fn result_line(cost: f64, input: u64, output: u64, turns: u64) -> String {
        format!(
            r#"{{"op":"push","ev":{{"type":"result","total_cost_usd":{cost},"num_turns":{turns},"usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}"#
        )
    }

    #[test]
    fn roundtrip_and_missing_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usage.json");
        let empty = UsageStore::load(&path).unwrap();
        assert!(empty.projects.is_empty());
        assert_eq!(empty.version, USAGE_VERSION);

        let mut s = UsageStore::default();
        s.note_new_chat("k1", "/home/me/Blink");
        s.record_line("k1", "/home/me/Blink", "2026-08-09T10-00-00.ndjson",
            &result_line(0.05, 500, 50, 3));
        s.save(&path).unwrap();
        let back = UsageStore::load(&path).unwrap();
        assert_eq!(back.projects, s.projects);
    }

    #[test]
    fn corrupt_file_is_an_error_not_a_default() {
        // This file is the user's accumulated record — swallowing corruption
        // would destroy it on the next save (fleet::load policy, not settings).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usage.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(UsageStore::load(&path).is_err());
    }

    #[test]
    fn record_line_accumulates_results_and_ignores_everything_else() {
        let mut s = UsageStore::default();
        // Non-result ops and garbage change nothing and report no change.
        assert!(!s.record_line("k", "/s", "a.ndjson", r#"{"op":"userSent","text":"hi"}"#));
        assert!(!s.record_line("k", "/s", "a.ndjson", r#"{"op":"push","ev":{"type":"assistant"}}"#));
        assert!(!s.record_line("k", "/s", "a.ndjson", "{not json"));
        assert!(s.projects.is_empty());

        assert!(s.record_line("k", "/s", "2026-08-09T10-00-00.ndjson",
            &result_line(0.01, 100, 10, 2)));
        assert!(s.record_line("k", "/s", "2026-08-09T11-00-00.ndjson",
            &result_line(0.02, 200, 20, 3)));
        let p = &s.projects["k"];
        assert!((p.cost_usd - 0.03).abs() < 1e-9);
        assert_eq!(p.input_tokens, 300);
        assert_eq!(p.output_tokens, 30);
        assert_eq!(p.turns, 5);
        assert_eq!(p.sketch_dir, "/s");
        assert_eq!(p.last_chat.as_deref(), Some("2026-08-09T11-00-00"));
        assert_eq!(p.sessions, 0, "results alone are not sessions");
    }

    #[test]
    fn note_new_chat_counts_sessions() {
        let mut s = UsageStore::default();
        assert!(s.note_new_chat("k", "/s"));
        assert!(s.note_new_chat("k", "/s"));
        assert_eq!(s.projects["k"].sessions, 2);
        assert_eq!(s.projects["k"].sketch_dir, "/s");
    }

    #[test]
    fn overview_is_sorted_by_cost_descending() {
        let mut s = UsageStore::default();
        s.record_line("a", "/cheap", "x.ndjson", &result_line(0.01, 1, 1, 1));
        s.record_line("b", "/pricey", "y.ndjson", &result_line(0.90, 1, 1, 1));
        s.record_line("c", "/mid", "z.ndjson", &result_line(0.50, 1, 1, 1));
        let rows = s.overview();
        let dirs: Vec<&str> = rows.iter().map(|r| r.sketch_dir.as_str()).collect();
        assert_eq!(dirs, ["/pricey", "/mid", "/cheap"]);
    }

    #[test]
    fn older_json_without_new_fields_loads() {
        // Forward compat rides #[serde(default)], like AppSettings/Fleet.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usage.json");
        std::fs::write(&path,
            r#"{"version":1,"projects":{"k":{"sketch_dir":"/s","cost_usd":1.0}}}"#).unwrap();
        let s = UsageStore::load(&path).unwrap();
        assert_eq!(s.projects["k"].input_tokens, 0);
        assert_eq!(s.projects["k"].sessions, 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bancada-core usage`
Expected: compile error (module/types don't exist yet) or test failures.

- [ ] **Step 3: Implement the store**

Full implementation of `core/src/usage.rs` above the tests:

```rust
//! Cumulative per-project Assistant usage, persisted as `usage.json`.
//!
//! Chat NDJSON files are pruned at 50 per sketch and user-deletable, so
//! totals computed by scanning them silently shrink over time. This store
//! accumulates at append time instead and survives both. Path-agnostic and
//! clock-free like `settings`/`fleet`: the Tauri layer supplies the path,
//! and recency comes from chat file stems (frontend timestamps), never from
//! a clock read here.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Bumped only for a breaking change to the on-disk shape. Additive fields
/// ride on `#[serde(default)]`, the same way `Fleet` grows.
pub const USAGE_VERSION: u32 = 1;

fn usage_version() -> u32 {
    USAGE_VERSION
}

/// Everything the Assistant has ever cost one project. `sessions` counts
/// chat files created, whether or not they are still on disk; the dashboard
/// shows the difference against surviving files as "older sessions pruned".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectUsage {
    /// Original sketch path — display, and re-hashed by chat commands.
    #[serde(default)]
    pub sketch_dir: String,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub sessions: u64,
    /// Stem of the newest chat that contributed a result, e.g.
    /// `2026-08-09T10-00-00`.
    #[serde(default)]
    pub last_chat: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStore {
    #[serde(default = "usage_version")]
    pub version: u32,
    /// Keyed by `chatlog::sketch_key(sketch_dir)`.
    #[serde(default)]
    pub projects: BTreeMap<String, ProjectUsage>,
}

impl Default for UsageStore {
    fn default() -> Self {
        Self {
            version: USAGE_VERSION,
            projects: BTreeMap::new(),
        }
    }
}

impl UsageStore {
    /// A missing file is an empty record; a corrupt one is an **error**.
    ///
    /// Same policy as `Fleet::load`, for the same reason: this file is the
    /// user's accumulated record, and silently replacing it with an empty
    /// store would destroy it on the next save. Deliberately unlike
    /// `settings::load`.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|source| Error::Json {
            what: format!("the usage record at {}", path.display()),
            source,
        })
    }

    /// Atomic write (temp file + rename), matching `settings::save`.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(|source| Error::Json {
            what: "the usage record".to_string(),
            source,
        })?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// A chat file was newly created: one more session on this project's
    /// lifetime tally. Always a change.
    pub fn note_new_chat(&mut self, key: &str, sketch_dir: &str) -> bool {
        let p = self.projects.entry(key.to_string()).or_default();
        p.sketch_dir = sketch_dir.to_string();
        p.sessions += 1;
        true
    }

    /// Accumulate one recorded op line iff it is a pushed `result` event —
    /// the same filter and fields as `chatlog::project_totals`, applied
    /// incrementally. Returns whether the store changed, so the caller can
    /// skip the save on the torrent of non-result lines.
    pub fn record_line(&mut self, key: &str, sketch_dir: &str, file: &str, line: &str) -> bool {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        if v.get("op").and_then(|o| o.as_str()) != Some("push") {
            return false;
        }
        let Some(ev) = v.get("ev") else { return false };
        if ev.get("type").and_then(|t| t.as_str()) != Some("result") {
            return false;
        }
        let p = self.projects.entry(key.to_string()).or_default();
        p.sketch_dir = sketch_dir.to_string();
        p.cost_usd += ev
            .get("total_cost_usd")
            .and_then(|c| c.as_f64())
            .unwrap_or(0.0);
        p.turns += ev.get("num_turns").and_then(|n| n.as_u64()).unwrap_or(0);
        if let Some(u) = ev.get("usage") {
            p.input_tokens += u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
            p.output_tokens += u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
        }
        p.last_chat = Some(file.trim_end_matches(".ndjson").to_string());
        true
    }

    /// Dashboard rows: every project, most expensive first (path as the
    /// deterministic tie-break).
    pub fn overview(&self) -> Vec<ProjectUsage> {
        let mut rows: Vec<ProjectUsage> = self.projects.values().cloned().collect();
        rows.sort_by(|a, b| {
            b.cost_usd
                .partial_cmp(&a.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.sketch_dir.cmp(&b.sketch_dir))
        });
        rows
    }
}
```

Register the module in `core/src/lib.rs` after `pub mod types;`:

```rust
pub mod usage;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p bancada-core usage`
Expected: all 6 new tests PASS. Then `cargo test -p bancada-core` — no regressions.

- [ ] **Step 5: Commit**

```bash
git add core/src/usage.rs core/src/lib.rs
git commit -m "feat: cumulative per-project usage store (usage.json)"
```

---

### Task 2: Backfill from surviving chat logs (`usage::backfill`)

**Files:**
- Modify: `core/src/usage.rs` (add free functions + tests)

**Interfaces:**
- Consumes: `chatlog::project_totals(chats_root, key) -> ProjectTotals` (existing), `UsageStore`/`ProjectUsage` from Task 1.
- Produces (used by Task 4): `pub fn backfill(chats_root: &Path) -> UsageStore`

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `core/src/usage.rs`:

```rust
    fn write_chat(root: &Path, key: &str, file: &str, lines: &[&str]) {
        let dir = root.join(key);
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(dir.join(file), text).unwrap();
    }

    #[test]
    fn backfill_seeds_from_surviving_chats() {
        let tmp = tempfile::tempdir().unwrap();
        let r1 = result_line(0.01, 100, 10, 2);
        let r2 = result_line(0.04, 400, 40, 1);
        write_chat(
            tmp.path(),
            "aaaa-blink",
            "2026-08-08T10-00-00.ndjson",
            &[r#"{"op":"meta","sketchDir":"/home/me/Blink","startedAt":"t"}"#, &r1],
        );
        write_chat(
            tmp.path(),
            "aaaa-blink",
            "2026-08-09T09-00-00.ndjson",
            &[r#"{"op":"meta","sketchDir":"/home/me/Blink","startedAt":"t"}"#, &r2],
        );
        // A directory with chats but no results still counts its sessions.
        write_chat(
            tmp.path(),
            "bbbb-idle",
            "2026-08-01T08-00-00.ndjson",
            &[r#"{"op":"meta","sketchDir":"/home/me/Idle","startedAt":"t"}"#],
        );

        let s = backfill(tmp.path());
        let p = &s.projects["aaaa-blink"];
        assert_eq!(p.sketch_dir, "/home/me/Blink");
        assert!((p.cost_usd - 0.05).abs() < 1e-9);
        assert_eq!(p.input_tokens, 500);
        assert_eq!(p.output_tokens, 50);
        assert_eq!(p.turns, 3);
        assert_eq!(p.sessions, 2);
        assert_eq!(p.last_chat.as_deref(), Some("2026-08-09T09-00-00"));
        assert_eq!(s.projects["bbbb-idle"].sessions, 1);
        assert!((s.projects["bbbb-idle"].cost_usd).abs() < 1e-9);
    }

    #[test]
    fn backfill_of_missing_root_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(backfill(&tmp.path().join("never-created")).projects.is_empty());
    }

    #[test]
    fn backfill_without_meta_falls_back_to_the_key() {
        // Old or hand-damaged chats may lack the meta line; the key is at
        // least recognisable (it ends with the sanitized basename).
        let tmp = tempfile::tempdir().unwrap();
        write_chat(tmp.path(), "cccc-mystery", "2026-08-01T08-00-00.ndjson",
            &[&result_line(0.01, 1, 1, 1)]);
        let s = backfill(tmp.path());
        assert_eq!(s.projects["cccc-mystery"].sketch_dir, "cccc-mystery");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bancada-core usage`
Expected: compile error — `backfill` not defined.

- [ ] **Step 3: Implement backfill**

Add above the tests in `core/src/usage.rs`:

```rust
/// Seed a store from whatever chat files still exist under `chats_root`.
///
/// Used exactly once, when `usage.json` is absent: re-running after new
/// appends would double-count, so the caller must save the result
/// immediately. Best-effort like `project_totals`: unreadable directories,
/// files and lines are skipped, never an error.
pub fn backfill(chats_root: &Path) -> UsageStore {
    let mut store = UsageStore::default();
    let Ok(entries) = std::fs::read_dir(chats_root) else {
        return store;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.path().is_dir() {
            continue;
        }
        let Ok(key) = entry.file_name().into_string() else {
            continue;
        };
        let t = crate::chatlog::project_totals(chats_root, &key);
        if t.chats == 0 {
            continue;
        }
        let sketch_dir = sketch_dir_from_meta(chats_root, &key).unwrap_or_else(|| key.clone());
        store.projects.insert(
            key,
            ProjectUsage {
                sketch_dir,
                cost_usd: t.cost_usd,
                input_tokens: t.input_tokens,
                output_tokens: t.output_tokens,
                turns: t.turns,
                sessions: t.chats as u64,
                last_chat: t.last_chat,
            },
        );
    }
    store
}

/// The original sketch path, recovered from any chat's `meta` op. The
/// recorder writes meta as line 1 of every file, but scan a few lines of
/// every file rather than trusting one — a truncated first file must not
/// erase the path for the whole project.
fn sketch_dir_from_meta(chats_root: &Path, key: &str) -> Option<String> {
    let entries = std::fs::read_dir(chats_root.join(key)).ok()?;
    for e in entries.filter_map(|e| e.ok()) {
        let Ok(f) = std::fs::File::open(e.path()) else {
            continue;
        };
        use std::io::BufRead;
        for line in std::io::BufReader::new(f).lines().take(3).map_while(|l| l.ok()) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if v.get("op").and_then(|o| o.as_str()) == Some("meta") {
                if let Some(d) = v.get("sketchDir").and_then(|s| s.as_str()) {
                    return Some(d.to_string());
                }
            }
        }
    }
    None
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p bancada-core usage`
Expected: PASS (9 usage tests total).

- [ ] **Step 5: Commit**

```bash
git add core/src/usage.rs
git commit -m "feat: seed the usage record from surviving chat logs"
```

---

### Task 3: Per-chat usage listing (`chatlog::list_chats_with_usage`)

**Files:**
- Modify: `core/src/chatlog.rs`

**Interfaces:**
- Consumes: existing private helpers `chat_file_names`, `TITLE_SCAN_LINES`.
- Produces (used by Task 4):
  - `pub struct SessionEntry { file: String, title: String, cost_usd: f64, input_tokens: u64, output_tokens: u64, turns: u64 }` (all pub, `Serialize`)
  - `pub fn list_chats_with_usage(chats_root: &Path, key: &str) -> Vec<SessionEntry>`

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `core/src/chatlog.rs` (reuses the existing `write_chat` and `result_line` helpers already in that module):

```rust
    #[test]
    fn list_with_usage_sums_per_file_and_keeps_titles_and_order() {
        let tmp = tempfile::tempdir().unwrap();
        let r1 = result_line(0.01, 100, 10, 2);
        let r2 = result_line(0.02, 200, 20, 3);
        write_chat(
            tmp.path(),
            "k",
            "2026-08-04T10-00-00.ndjson",
            &[
                r#"{"op":"meta","sketchDir":"/s","startedAt":"t"}"#,
                r#"{"op":"userSent","text":"one"}"#,
                &r1,
                &r2,
            ],
        );
        write_chat(
            tmp.path(),
            "k",
            "2026-08-05T09-00-00.ndjson",
            &[
                r#"{"op":"meta","sketchDir":"/s","startedAt":"t"}"#,
                "{not json",
                r#"{"op":"push","ev":{"type":"assistant"}}"#,
            ],
        );
        let list = list_chats_with_usage(tmp.path(), "k");
        assert_eq!(list.len(), 2);
        // Newest first, like list_chats.
        assert_eq!(list[0].file, "2026-08-05T09-00-00.ndjson");
        assert_eq!(list[0].title, "2026-08-05T09-00-00", "no userSent → stem");
        assert!((list[0].cost_usd).abs() < 1e-9, "corrupt/non-result lines add nothing");
        assert_eq!(list[1].file, "2026-08-04T10-00-00.ndjson");
        assert_eq!(list[1].title, "one");
        assert!((list[1].cost_usd - 0.03).abs() < 1e-9);
        assert_eq!(list[1].input_tokens, 300);
        assert_eq!(list[1].output_tokens, 30);
        assert_eq!(list[1].turns, 5);
    }

    #[test]
    fn list_with_usage_of_missing_sketch_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_chats_with_usage(tmp.path(), "ghost").is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bancada-core chatlog`
Expected: compile error — `list_chats_with_usage` not defined.

- [ ] **Step 3: Implement**

Add to `core/src/chatlog.rs`, after `project_totals`:

```rust
/// One saved chat with its own usage summed — the history list enriched
/// into the usage dashboard's session rows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionEntry {
    pub file: String,
    pub title: String,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub turns: u64,
}

/// `list_chats` and per-file totals in one streaming pass per file.
///
/// Newest first; unreadable files are skipped and malformed lines add
/// nothing, same tolerance as `list_chats`/`project_totals`. The title scan
/// keeps `TITLE_SCAN_LINES` semantics, but the whole file is read anyway
/// for the sums, so there is no extra head-only optimisation to preserve.
pub fn list_chats_with_usage(chats_root: &Path, key: &str) -> Vec<SessionEntry> {
    let mut files = chat_file_names(chats_root, key);
    files.sort_unstable_by(|a, b| b.cmp(a));
    files
        .into_iter()
        .filter_map(|file| {
            let f = std::fs::File::open(chats_root.join(key).join(&file)).ok()?;
            let mut e = SessionEntry {
                title: String::new(),
                file,
                cost_usd: 0.0,
                input_tokens: 0,
                output_tokens: 0,
                turns: 0,
            };
            use std::io::BufRead;
            for (idx, line) in std::io::BufReader::new(f)
                .lines()
                .map_while(|l| l.ok())
                .enumerate()
            {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if e.title.is_empty()
                    && idx < TITLE_SCAN_LINES
                    && v.get("op").and_then(|o| o.as_str()) == Some("userSent")
                {
                    if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                        e.title = t.chars().take(80).collect();
                    }
                }
                if v.get("op").and_then(|o| o.as_str()) != Some("push") {
                    continue;
                }
                let Some(ev) = v.get("ev") else { continue };
                if ev.get("type").and_then(|t| t.as_str()) != Some("result") {
                    continue;
                }
                e.cost_usd += ev
                    .get("total_cost_usd")
                    .and_then(|c| c.as_f64())
                    .unwrap_or(0.0);
                e.turns += ev.get("num_turns").and_then(|n| n.as_u64()).unwrap_or(0);
                if let Some(u) = ev.get("usage") {
                    e.input_tokens +=
                        u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                    e.output_tokens +=
                        u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                }
            }
            if e.title.is_empty() {
                e.title = e.file.trim_end_matches(".ndjson").to_string();
            }
            Some(e)
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p bancada-core chatlog`
Expected: PASS, including all pre-existing chatlog tests.

- [ ] **Step 5: Commit**

```bash
git add core/src/chatlog.rs
git commit -m "feat: per-chat usage listing for the dashboard's session rows"
```

---

### Task 4: Tauri layer — append hook and read commands

**Files:**
- Modify: `src-tauri/src/lib.rs` (chat section around lines 1501–1573, and the `generate_handler!` list around line 3163)

**Interfaces:**
- Consumes: `bancada_core::usage::{UsageStore, backfill, ProjectUsage}` (Tasks 1–2), `bancada_core::chatlog::{sketch_key, list_chats_with_usage, SessionEntry}` (Task 3), existing `chats_root(app)`, `err_str`.
- Produces (used by Task 5): Tauri commands `usage_overview() -> Vec<ProjectUsage>` and `chat_list_usage(sketch_dir: String) -> Vec<SessionEntry>`.

- [ ] **Step 1: Add path helper and lazy-backfill loader**

After the `chat_delete` command (`src-tauri/src/lib.rs:1573`), add:

```rust
// ---------- assistant usage record ----------

fn usage_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("usage.json"))
        .map_err(err_str)
}

/// Load the usage record, seeding it from surviving chat files the first
/// time. The seed is saved immediately so backfill can never run twice —
/// running it again after new appends would double-count.
fn load_usage(app: &AppHandle) -> Result<(PathBuf, bancada_core::usage::UsageStore), String> {
    let path = usage_path(app)?;
    if !path.exists() {
        let store = bancada_core::usage::backfill(&chats_root(app)?);
        store.save(&path).map_err(err_str)?;
        return Ok((path, store));
    }
    bancada_core::usage::UsageStore::load(&path)
        .map(|s| (path, s))
        .map_err(err_str)
}
```

- [ ] **Step 2: Hook `chat_append`**

Replace the body of `chat_append` (`src-tauri/src/lib.rs:1510-1527`) with:

```rust
#[tauri::command]
fn chat_append(
    app: AppHandle,
    sketch_dir: String,
    file: String,
    line: String,
) -> Result<(), String> {
    let root = chats_root(&app)?;
    let key = bancada_core::chatlog::sketch_key(&sketch_dir);
    // Load (or first-time seed) the usage record BEFORE the append, so a
    // backfill can never count the file/line this call is about to add and
    // then count it again below.
    let usage = load_usage(&app).ok();
    let created =
        bancada_core::chatlog::append_line(&root, &key, &file, &line).map_err(err_str)?;
    // A new chat is the moment to bound the directory. Prune is silent and
    // best-effort, so a failed cleanup can never cost the append. The usage
    // record is why pruning is safe: totals were banked at append time.
    if created {
        bancada_core::chatlog::prune(&root, &key, 50);
    }
    // Usage bookkeeping must not turn a good append into an error
    // (note_board_fqbn precedent) — errors are logged and swallowed.
    if let Some((path, mut store)) = usage {
        let mut changed = false;
        if created {
            changed |= store.note_new_chat(&key, &sketch_dir);
        }
        changed |= store.record_line(&key, &sketch_dir, &file, &line);
        if changed {
            if let Err(e) = store.save(&path) {
                eprintln!("usage record not saved: {e}");
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Add the two read commands**

Below `load_usage`, add:

```rust
#[tauri::command]
fn usage_overview(
    app: AppHandle,
) -> Result<Vec<bancada_core::usage::ProjectUsage>, String> {
    let (_path, store) = load_usage(&app)?;
    Ok(store.overview())
}

#[tauri::command]
fn chat_list_usage(
    app: AppHandle,
    sketch_dir: String,
) -> Result<Vec<bancada_core::chatlog::SessionEntry>, String> {
    let root = chats_root(&app)?;
    Ok(bancada_core::chatlog::list_chats_with_usage(
        &root,
        &bancada_core::chatlog::sketch_key(&sketch_dir),
    ))
}
```

Register both in the `generate_handler![...]` list directly after `chat_totals,` (line ~3170):

```rust
            chat_totals,
            usage_overview,
            chat_list_usage,
```

- [ ] **Step 4: Verify it compiles and nothing regressed**

Run: `cargo check -p bancada` (from repo root; the Tauri crate) and `cargo test -p bancada-core`
Expected: clean check, all core tests PASS. (The append-hook ordering is core-tested via Tasks 1–2; the Tauri wrapper is thin delegation, per the repo's convention of keeping logic out of lib.rs commands.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: bank usage at chat append; usage_overview and chat_list_usage commands"
```

---

### Task 5: Frontend API wrappers

**Files:**
- Modify: `src/api.ts` (after the `chatTotals` block, line ~597)
- Modify: `src/__tests__/api.test.ts`

**Interfaces:**
- Consumes: Task 4's commands.
- Produces (used by Tasks 6–7):
  - `export interface ProjectUsage { sketch_dir: string; cost_usd: number; input_tokens: number; output_tokens: number; turns: number; sessions: number; last_chat: string | null }`
  - `export interface SessionEntry { file: string; title: string; cost_usd: number; input_tokens: number; output_tokens: number; turns: number }`
  - `export const usageOverview: () => Promise<ProjectUsage[]>`
  - `export const chatListUsage: (sketchDir: string) => Promise<SessionEntry[]>`

- [ ] **Step 1: Write the failing tests**

Add to `src/__tests__/api.test.ts`, inside a new describe after the `agent commands` block:

```ts
describe("usage dashboard", () => {
  it("usageOverview takes no arguments", async () => {
    await api.usageOverview();
    expect(called()[0]).toBe("usage_overview");
    expect(called()[1]).toBeUndefined();
  });

  it("chatListUsage passes sketchDir", async () => {
    await api.chatListUsage("/s");
    expect(called()).toEqual(["chat_list_usage", { sketchDir: "/s" }]);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- src/__tests__/api.test.ts`
Expected: FAIL — `api.usageOverview is not a function`.

- [ ] **Step 3: Implement the wrappers**

In `src/api.ts` after the `chatTotals` export (line ~597):

```ts
// ---------- usage dashboard ----------

/** Cumulative Assistant usage for one project — survives chat pruning. */
export interface ProjectUsage {
  sketch_dir: string;
  cost_usd: number;
  input_tokens: number;
  output_tokens: number;
  turns: number;
  sessions: number;
  last_chat: string | null;
}

/** One saved chat with its own usage summed. */
export interface SessionEntry {
  file: string;
  title: string;
  cost_usd: number;
  input_tokens: number;
  output_tokens: number;
  turns: number;
}

export const usageOverview = () => invoke<ProjectUsage[]>("usage_overview");
export const chatListUsage = (sketchDir: string) =>
  invoke<SessionEntry[]>("chat_list_usage", { sketchDir });
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- src/__tests__/api.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api.ts src/__tests__/api.test.ts
git commit -m "feat: typed wrappers for the usage dashboard commands"
```

---

### Task 6: Pure dashboard math module

**Files:**
- Create: `src/usageDashboard.ts`
- Create: `src/__tests__/usageDashboard.test.ts`

**Interfaces:**
- Consumes: `ProjectUsage` from Task 5.
- Produces (used by Task 7): `grandTotals(rows)`, `stemToDisplay(stem)`, `prunedCount(row, onDisk)`, `projectName(sketchDir)`.

- [ ] **Step 1: Write the failing tests**

Create `src/__tests__/usageDashboard.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import {
  grandTotals,
  prunedCount,
  projectName,
  stemToDisplay,
} from "../usageDashboard";
import type { ProjectUsage } from "../api";

const row = (over: Partial<ProjectUsage>): ProjectUsage => ({
  sketch_dir: "/s",
  cost_usd: 0,
  input_tokens: 0,
  output_tokens: 0,
  turns: 0,
  sessions: 0,
  last_chat: null,
  ...over,
});

describe("grandTotals", () => {
  it("sums cost and tokens across projects", () => {
    const t = grandTotals([
      row({ cost_usd: 0.5, input_tokens: 100, output_tokens: 10 }),
      row({ cost_usd: 0.25, input_tokens: 50, output_tokens: 5 }),
    ]);
    expect(t.projects).toBe(2);
    expect(t.costUsd).toBeCloseTo(0.75);
    expect(t.inputTokens).toBe(150);
    expect(t.outputTokens).toBe(15);
  });

  it("is all zeros for no projects", () => {
    expect(grandTotals([])).toEqual({
      projects: 0,
      costUsd: 0,
      inputTokens: 0,
      outputTokens: 0,
    });
  });
});

describe("stemToDisplay", () => {
  it("renders a chat file stem as date + hh:mm", () => {
    expect(stemToDisplay("2026-08-05T09-30-00")).toBe("2026-08-05 09:30");
  });

  it("passes through anything that is not a stem", () => {
    expect(stemToDisplay("weird")).toBe("weird");
  });
});

describe("prunedCount", () => {
  it("is the store's excess over files on disk, floored at zero", () => {
    expect(prunedCount(row({ sessions: 60 }), 50)).toBe(10);
    expect(prunedCount(row({ sessions: 3 }), 3)).toBe(0);
    // A chat created before the store existed can leave disk ahead of the
    // record; that must not go negative.
    expect(prunedCount(row({ sessions: 1 }), 2)).toBe(0);
  });
});

describe("projectName", () => {
  it("is the basename, falling back to the full path", () => {
    expect(projectName("/home/me/Projects/Blink")).toBe("Blink");
    expect(projectName("odd")).toBe("odd");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- src/__tests__/usageDashboard.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the module**

Create `src/usageDashboard.ts`:

```ts
// Pure math and formatting for the usage dashboard, kept out of the
// component so it is unit-testable. Ordering is core's job
// (UsageStore::overview sorts by cost); this module never reorders.

import type { ProjectUsage } from "./api";

export interface GrandTotals {
  projects: number;
  costUsd: number;
  inputTokens: number;
  outputTokens: number;
}

export function grandTotals(rows: ProjectUsage[]): GrandTotals {
  return rows.reduce(
    (t, r) => ({
      projects: t.projects + 1,
      costUsd: t.costUsd + r.cost_usd,
      inputTokens: t.inputTokens + r.input_tokens,
      outputTokens: t.outputTokens + r.output_tokens,
    }),
    { projects: 0, costUsd: 0, inputTokens: 0, outputTokens: 0 },
  );
}

/** `2026-08-05T09-30-00` (chat file stem) → `2026-08-05 09:30`. */
export function stemToDisplay(stem: string): string {
  const m = stem.match(/^(\d{4}-\d{2}-\d{2})T(\d{2})-(\d{2})-\d{2}$/);
  return m ? `${m[1]} ${m[2]}:${m[3]}` : stem;
}

/** Sessions banked in the store but no longer on disk (pruned or deleted). */
export function prunedCount(row: ProjectUsage, onDisk: number): number {
  return Math.max(0, row.sessions - onDisk);
}

export function projectName(sketchDir: string): string {
  return sketchDir.split("/").pop() || sketchDir;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- src/__tests__/usageDashboard.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/usageDashboard.ts src/__tests__/usageDashboard.test.ts
git commit -m "feat: pure math module for the usage dashboard"
```

---

### Task 7: Dashboard component + replay exports + styles

**Files:**
- Modify: `src/components/AgentPanel.tsx` (export three existing declarations; no behavior change)
- Create: `src/components/UsageDashboard.tsx`
- Modify: `src/styles.css` (append a new section before the motion-preferences block)

**Interfaces:**
- Consumes: `api.usageOverview`, `api.chatListUsage`, `api.chatLoad` (existing), `replayChat` from `src/agent/chatLog`, `formatTokens` from `src/agent/usage`, Task 6's helpers, and from `AgentPanel.tsx`: `ReplayView` (props `{ store, openBottomTab, onOpenTurn }`), `TurnSummaryView` (props `{ turn, onBack }`), `TurnEnd` type.
- Produces (used by Task 8): `export default function UsageDashboard({ onClose, openBottomTab }: { onClose: () => void; openBottomTab: (tab: BottomTab) => void })`.

- [ ] **Step 1: Export the replay pieces from AgentPanel**

In `src/components/AgentPanel.tsx`, three one-word edits (add `export`):
- Line 32: `type TurnEnd = Extract<...>` → `export type TurnEnd = Extract<...>`
- Line 595: `function ReplayView({` → `export function ReplayView({`
- The `TurnSummaryView` declaration (search `function TurnSummaryView`) → `export function TurnSummaryView(...)`

- [ ] **Step 2: Create the component**

Create `src/components/UsageDashboard.tsx`:

```tsx
// UsageDashboard — app-wide Assistant spend: one row per project from the
// cumulative usage record (usage.json), expandable into per-session rows
// that replay inline. Totals survive chat pruning, so a project can show
// more sessions than it has surviving files — the gap is stated, not
// padded with placeholder rows. Fetches on mount and on ⟳; deliberately no
// live updates while a session streams (project-usage-totals spec).

import { useEffect, useState } from "react";
import * as api from "../api";
import type { ProjectUsage, SessionEntry } from "../api";
import { replayChat } from "../agent/chatLog";
import type { AgentStore } from "../agent/agentStore";
import { formatTokens } from "../agent/usage";
import {
  grandTotals,
  projectName,
  prunedCount,
  stemToDisplay,
} from "../usageDashboard";
import { ReplayView, TurnSummaryView, type TurnEnd } from "./AgentPanel";
import type { BottomTab } from "../bottomTabs";

interface Props {
  onClose: () => void;
  openBottomTab: (tab: BottomTab) => void;
}

export default function UsageDashboard({ onClose, openBottomTab }: Props) {
  const [rows, setRows] = useState<ProjectUsage[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** sketch_dir of the project whose sessions are expanded. */
  const [expanded, setExpanded] = useState<string | null>(null);
  const [sessions, setSessions] = useState<SessionEntry[] | null>(null);
  const [replay, setReplay] = useState<{
    store: AgentStore;
    title: string;
  } | null>(null);
  const [viewTurn, setViewTurn] = useState<TurnEnd | null>(null);

  const refresh = () => {
    setError(null);
    api
      .usageOverview()
      .then(setRows)
      .catch((e) => {
        setRows([]);
        setError(String(e));
      });
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, []);

  const toggleProject = (row: ProjectUsage) => {
    setReplay(null);
    setViewTurn(null);
    if (expanded === row.sketch_dir) {
      setExpanded(null);
      setSessions(null);
      return;
    }
    setExpanded(row.sketch_dir);
    setSessions(null);
    api
      .chatListUsage(row.sketch_dir)
      .then(setSessions)
      .catch(() => setSessions([]));
  };

  const openSession = (row: ProjectUsage, s: SessionEntry) => {
    api
      .chatLoad(row.sketch_dir, s.file)
      .then((lines) => setReplay({ store: replayChat(lines), title: s.title }))
      .catch(() => {});
  };

  if (replay) {
    return (
      <div className="usage-dash">
        <div className="panel-tabs">
          <button
            className="btn small"
            onClick={() =>
              viewTurn ? setViewTurn(null) : setReplay(null)
            }
          >
            ← Back
          </button>
          <span className="usage-title">{replay.title}</span>
          <div className="spacer" />
          <button className="btn small" onClick={onClose} title="Close">
            ✕
          </button>
        </div>
        <div className="usage-replay">
          {viewTurn ? (
            <TurnSummaryView turn={viewTurn} onBack={() => setViewTurn(null)} />
          ) : (
            <ReplayView
              store={replay.store}
              openBottomTab={openBottomTab}
              onOpenTurn={setViewTurn}
            />
          )}
        </div>
      </div>
    );
  }

  const totals = grandTotals(rows ?? []);
  return (
    <div className="usage-dash">
      <div className="panel-tabs">
        <span className="usage-title">📊 Assistant usage</span>
        {rows && rows.length > 0 && (
          <span className="usage-grand">
            {totals.projects} project{totals.projects === 1 ? "" : "s"} · Σ $
            {totals.costUsd.toFixed(4)} · {formatTokens(totals.inputTokens)} in
            / {formatTokens(totals.outputTokens)} out
          </span>
        )}
        <div className="spacer" />
        <button className="btn small" onClick={refresh} title="Refresh">
          ⟳
        </button>
        <button className="btn small" onClick={onClose} title="Close">
          ✕
        </button>
      </div>
      <div className="usage-list">
        {error && <div className="usage-error">{error}</div>}
        {rows && rows.length === 0 && !error && (
          <div className="empty-hint">
            No Assistant usage recorded yet — costs appear here after the
            first chat.
          </div>
        )}
        {(rows ?? []).map((r) => (
          <div key={r.sketch_dir} className="usage-card">
            <button className="usage-row" onClick={() => toggleProject(r)}>
              <span className="usage-name">{projectName(r.sketch_dir)}</span>
              <span className="usage-path">{r.sketch_dir}</span>
              <span className="usage-stats">
                <span>Σ ${r.cost_usd.toFixed(4)}</span>
                <span>
                  {formatTokens(r.input_tokens)} in /{" "}
                  {formatTokens(r.output_tokens)} out
                </span>
                <span>{r.turns} turns</span>
                <span>
                  {r.sessions} session{r.sessions === 1 ? "" : "s"}
                </span>
                {r.last_chat && (
                  <span>last {stemToDisplay(r.last_chat)}</span>
                )}
              </span>
            </button>
            {expanded === r.sketch_dir && sessions && (
              <div className="usage-sessions">
                {sessions.map((s) => (
                  <button
                    key={s.file}
                    className="usage-session-row"
                    onClick={() => openSession(r, s)}
                  >
                    <span className="usage-session-title">{s.title}</span>
                    <span className="usage-session-stats">
                      {stemToDisplay(s.file.replace(/\.ndjson$/, ""))} · $
                      {s.cost_usd.toFixed(4)} · {formatTokens(s.input_tokens)}{" "}
                      in / {formatTokens(s.output_tokens)} out
                    </span>
                  </button>
                ))}
                {prunedCount(r, sessions.length) > 0 && (
                  <div className="usage-pruned">
                    {prunedCount(r, sessions.length)} older session
                    {prunedCount(r, sessions.length) === 1 ? "" : "s"} pruned —
                    still counted in the totals above.
                  </div>
                )}
                {sessions.length === 0 && (
                  <div className="usage-pruned">
                    No surviving chat files — totals above are the banked
                    record.
                  </div>
                )}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Add the stylesheet section**

In `src/styles.css`, immediately before the `/* ---------- motion preferences ---------- */` banner, append:

```css
/* ---------- usage dashboard ---------- */

.usage-dash {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  background: var(--bg);
}
.usage-title {
  font-weight: 600;
  padding: 0 8px;
}
.usage-grand {
  color: var(--text-dim);
  font-size: 12px;
}
.usage-list {
  flex: 1;
  overflow-y: auto;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.usage-error {
  color: var(--error);
  padding: 8px;
}
.usage-card {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-raised);
  overflow: hidden;
}
.usage-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  width: 100%;
  padding: 10px 12px;
  background: none;
  border: none;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.usage-row:hover {
  background: var(--bg-hover);
}
.usage-name {
  font-weight: 600;
}
.usage-path {
  color: var(--text-dim);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0;
}
.usage-stats {
  display: flex;
  gap: 14px;
  color: var(--text-dim);
  font-size: 12px;
  white-space: nowrap;
}
.usage-sessions {
  border-top: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}
.usage-session-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  padding: 7px 12px;
  background: none;
  border: none;
  border-top: 1px solid var(--border);
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.usage-session-row:first-child {
  border-top: none;
}
.usage-session-row:hover {
  background: var(--bg-hover);
}
.usage-session-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.usage-session-stats {
  color: var(--text-dim);
  font-size: 12px;
  white-space: nowrap;
}
.usage-pruned {
  color: var(--text-dim);
  font-style: italic;
  font-size: 12px;
  padding: 7px 12px;
  border-top: 1px solid var(--border);
}
.usage-replay {
  flex: 1;
  overflow-y: auto;
  padding: 12px;
}
```

- [ ] **Step 4: Verify it type-checks**

Run: `npx tsc --noEmit`
Expected: clean. (The component isn't wired into App yet — that's Task 8 — but it must compile standalone.)

- [ ] **Step 5: Commit**

```bash
git add src/components/UsageDashboard.tsx src/components/AgentPanel.tsx src/styles.css
git commit -m "feat: usage dashboard screen with inline session replay"
```

---

### Task 8: Wire into Toolbar and App; full verification

**Files:**
- Modify: `src/components/Toolbar.tsx`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `UsageDashboard` (Task 7), existing `openBottomTab` callback in App.

- [ ] **Step 1: Toolbar button**

In `src/components/Toolbar.tsx`: add `onUsage: () => void;` to `Props` (after `onCloneProject`), and after the Clone button (line ~75) add:

```tsx
        <button
          className="btn"
          onClick={props.onUsage}
          title="Token usage and cost per project"
        >
          📊 Usage
        </button>
```

- [ ] **Step 2: App state + mutual exclusion**

In `src/App.tsx`:

1. Import: `import UsageDashboard from "./components/UsageDashboard";` alongside the other component imports.
2. State, after `cloningProject` (line ~117):

```ts
  /** When true the editor area shows the usage dashboard instead. */
  const [showingUsage, setShowingUsage] = useState(false);
```

3. Toolbar wiring (the `<Toolbar ...>` block at line ~1269): add the prop

```tsx
        onUsage={() => {
          setShowingUsage(true);
          setCreatingProject(false);
          setCloningProject(false);
          setProfileForm(null);
        }}
```

and add `setShowingUsage(false);` inside each of the existing `onNewProject`, `onCloneProject`, `onCreateProfile`, `onAddProfile`, `onRetargetProfile` callbacks (they enforce editor-area mutual exclusion by hand).

4. In `loadSketch` (search `const loadSketch`), add `setShowingUsage(false);` next to its existing UI-state resets — opening a project must land in the editor, not behind the dashboard.
5. Editor-area ternary (line ~1494): insert a branch between `cloningProject` and the editor:

```tsx
          ) : showingUsage ? (
            <UsageDashboard
              onClose={() => setShowingUsage(false)}
              openBottomTab={openBottomTab}
            />
          ) : (
```

- [ ] **Step 3: Full verification**

Run, from the repo root:
- `cargo test -p bancada-core` — expected: all pass (existing 435+ plus ~11 new).
- `cargo check -p bancada` — expected: clean.
- `npm test` — expected: all vitest suites pass, including the two new files.
- `npm run build` — expected: `tsc --noEmit` clean and vite build succeeds.

- [ ] **Step 4: Commit**

```bash
git add src/components/Toolbar.tsx src/App.tsx
git commit -m "feat: usage dashboard entry from the toolbar"
```

- [ ] **Step 5: Manual smoke check (human)**

Marco runs `npm run tauri dev`: open 📊 Usage → projects listed with totals (backfilled from existing chats on first open, `~/.config/dev.magj.bancada/usage.json` created); expand a project → session rows; click one → inline replay; ⟳ after a new Assistant turn → totals grew; delete a chat in History → dashboard totals unchanged.
