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
    /// The `sketch_key` this project is stored under — its identity, and the
    /// only safe way to reach its chat directory.
    ///
    /// Filled by [`UsageStore::overview`] and **never persisted**: it would
    /// duplicate the map key it is copied from, and a stored copy could drift
    /// from it. Empty on a value read straight out of `projects`.
    ///
    /// It exists because `sketch_dir` cannot do this job. `backfill` recovers
    /// that field from transcripts' `meta` lines, which record where a
    /// conversation happened rather than where the project is now — so after
    /// a rename it can name a directory that no longer exists. Re-hashing it
    /// yields a key nothing is stored under.
    #[serde(default, skip_serializing)]
    pub key: String,
    /// Original sketch path — display only. See `key` for addressing.
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
        let text = serde_json::to_string_pretty(self).map_err(|source| Error::Json {
            what: "the usage record".to_string(),
            source,
        })?;
        crate::replace_file_atomically(path, &text)?;
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

    /// Follow a renamed project: move its entry to `new_key` and repoint
    /// `sketch_dir`, which is both the dashboard's label and what the chat
    /// commands re-hash into a key.
    ///
    /// Totals are a lifetime record, so an occupied target is **merged into**
    /// rather than clobbered — renaming a project back to a name it once had
    /// must not delete what that name already spent. `last_chat` keeps the
    /// newer stem (they are frontend timestamps, so ordering is lexical).
    /// An unrecorded project has nothing to move and must not gain an empty
    /// entry, so it is a no-op.
    pub fn rename_project_key(&mut self, old_key: &str, new_key: &str, new_sketch_dir: &str) {
        let Some(moved) = self.projects.remove(old_key) else {
            return;
        };
        let p = self.projects.entry(new_key.to_string()).or_default();
        p.sketch_dir = new_sketch_dir.to_string();
        p.cost_usd += moved.cost_usd;
        p.input_tokens += moved.input_tokens;
        p.output_tokens += moved.output_tokens;
        p.turns += moved.turns;
        p.sessions += moved.sessions;
        if moved.last_chat > p.last_chat {
            p.last_chat = moved.last_chat;
        }
    }

    /// Dashboard rows: every project, most expensive first (path as the
    /// deterministic tie-break).
    /// Every project, dearest first, each row carrying the key it is stored
    /// under. Callers drill into chat history with `key`, never by re-hashing
    /// `sketch_dir` — see [`ProjectUsage::key`].
    pub fn overview(&self) -> Vec<ProjectUsage> {
        let mut rows: Vec<ProjectUsage> = self
            .projects
            .iter()
            .map(|(key, p)| ProjectUsage {
                key: key.clone(),
                ..p.clone()
            })
            .collect();
        rows.sort_by(|a, b| {
            b.cost_usd
                .partial_cmp(&a.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.sketch_dir.cmp(&b.sketch_dir))
        });
        rows
    }
}

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
                // Not persisted, and not the map key's home — `overview`
                // fills it from the map on the way out.
                key: String::new(),
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
        for line in std::io::BufReader::new(f)
            .lines()
            .take(3)
            .map_while(|l| l.ok())
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A recorded `result` push op, mirroring chatlog's test helper.
    fn result_line(cost: f64, input: u64, output: u64, turns: u64) -> String {
        format!(
            r#"{{"op":"push","ev":{{"type":"result","total_cost_usd":{cost},"num_turns":{turns},"usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}"#
        )
    }

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
            &[
                r#"{"op":"meta","sketchDir":"/home/me/Blink","startedAt":"t"}"#,
                &r1,
            ],
        );
        write_chat(
            tmp.path(),
            "aaaa-blink",
            "2026-08-09T09-00-00.ndjson",
            &[
                r#"{"op":"meta","sketchDir":"/home/me/Blink","startedAt":"t"}"#,
                &r2,
            ],
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
        assert!(backfill(&tmp.path().join("never-created"))
            .projects
            .is_empty());
    }

    #[test]
    fn backfill_without_meta_falls_back_to_the_key() {
        // Old or hand-damaged chats may lack the meta line; the key is at
        // least recognisable (it ends with the sanitized basename).
        let tmp = tempfile::tempdir().unwrap();
        write_chat(
            tmp.path(),
            "cccc-mystery",
            "2026-08-01T08-00-00.ndjson",
            &[&result_line(0.01, 1, 1, 1)],
        );
        let s = backfill(tmp.path());
        assert_eq!(s.projects["cccc-mystery"].sketch_dir, "cccc-mystery");
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
        s.record_line(
            "k1",
            "/home/me/Blink",
            "2026-08-09T10-00-00.ndjson",
            &result_line(0.05, 500, 50, 3),
        );
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
        assert!(!s.record_line(
            "k",
            "/s",
            "a.ndjson",
            r#"{"op":"push","ev":{"type":"assistant"}}"#
        ));
        assert!(!s.record_line("k", "/s", "a.ndjson", "{not json"));
        assert!(s.projects.is_empty());

        assert!(s.record_line(
            "k",
            "/s",
            "2026-08-09T10-00-00.ndjson",
            &result_line(0.01, 100, 10, 2)
        ));
        assert!(s.record_line(
            "k",
            "/s",
            "2026-08-09T11-00-00.ndjson",
            &result_line(0.02, 200, 20, 3)
        ));
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
    fn rename_project_key_moves_the_entry_and_repoints_its_path() {
        let mut s = UsageStore::default();
        s.note_new_chat("old", "/home/me/Old");
        s.record_line(
            "old",
            "/home/me/Old",
            "2026-08-09T10-00-00.ndjson",
            &result_line(0.05, 500, 50, 3),
        );

        s.rename_project_key("old", "new", "/home/me/New");

        assert!(!s.projects.contains_key("old"), "the old key must be gone");
        let p = &s.projects["new"];
        assert_eq!(p.sketch_dir, "/home/me/New");
        assert!((p.cost_usd - 0.05).abs() < 1e-9);
        assert_eq!(p.input_tokens, 500);
        assert_eq!(p.output_tokens, 50);
        assert_eq!(p.turns, 3);
        assert_eq!(p.sessions, 1);
        assert_eq!(p.last_chat.as_deref(), Some("2026-08-09T10-00-00"));
    }

    #[test]
    fn rename_project_key_merges_into_an_occupied_target() {
        // A project renamed back to a name it once had already has totals
        // under the target key; clobbering them would lose recorded spend.
        let mut s = UsageStore::default();
        s.note_new_chat("old", "/home/me/Old");
        s.record_line(
            "old",
            "/home/me/Old",
            "2026-08-09T10-00-00.ndjson",
            &result_line(0.05, 500, 50, 3),
        );
        s.note_new_chat("new", "/home/me/New");
        s.record_line(
            "new",
            "/home/me/New",
            "2026-08-01T10-00-00.ndjson",
            &result_line(0.01, 100, 10, 1),
        );

        s.rename_project_key("old", "new", "/home/me/New");

        assert!(!s.projects.contains_key("old"));
        let p = &s.projects["new"];
        assert!((p.cost_usd - 0.06).abs() < 1e-9);
        assert_eq!(p.input_tokens, 600);
        assert_eq!(p.output_tokens, 60);
        assert_eq!(p.turns, 4);
        assert_eq!(p.sessions, 2);
        assert_eq!(
            p.last_chat.as_deref(),
            Some("2026-08-09T10-00-00"),
            "the newer of the two stems wins"
        );
        assert_eq!(p.sketch_dir, "/home/me/New");
    }

    #[test]
    fn rename_project_key_of_an_unrecorded_project_changes_nothing() {
        // A project that never cost anything has no entry to move, and must
        // not gain an empty one.
        let mut s = UsageStore::default();
        s.note_new_chat("other", "/home/me/Other");
        s.rename_project_key("old", "new", "/home/me/New");
        assert_eq!(s.projects.len(), 1);
        assert!(!s.projects.contains_key("new"));
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
    fn overview_carries_the_store_key_for_every_row() {
        let mut s = UsageStore::default();
        s.record_line("aaaa-one", "/one", "x.ndjson", &result_line(0.01, 1, 1, 1));
        s.record_line("bbbb-two", "/two", "y.ndjson", &result_line(0.90, 1, 1, 1));
        let rows = s.overview();
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            ["bbbb-two", "aaaa-one"],
            "sorted by cost, key preserved"
        );
    }

    #[test]
    fn overview_key_is_right_even_when_sketch_dir_is_stale() {
        // The regression this field exists for. `backfill` recovers
        // `sketch_dir` from a transcript's meta line, which records where the
        // conversation happened — so after a project rename it names a
        // directory that is gone. Re-hashing it gives a key nothing is stored
        // under; the key must come from the map, not the path.
        let tmp = tempfile::tempdir().unwrap();
        let key = crate::chatlog::sketch_key("/home/u/Projects/porch-light");
        write_chat(
            tmp.path(),
            &key,
            "2026-08-01T08-00-00.ndjson",
            &[
                r#"{"op":"meta","sketchDir":"/home/u/Projects/led-test"}"#,
                &result_line(0.25, 1, 1, 1),
            ],
        );
        let rows = backfill(tmp.path()).overview();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].sketch_dir, "/home/u/Projects/led-test",
            "stale, by design"
        );
        assert_eq!(
            rows[0].key, key,
            "addressing must not go through sketch_dir"
        );
        assert_ne!(
            crate::chatlog::sketch_key(&rows[0].sketch_dir),
            rows[0].key,
            "re-hashing the display path is exactly the bug"
        );
    }

    #[test]
    fn overview_key_is_not_persisted() {
        // It would duplicate the map key it is copied from, and a stored copy
        // could drift from it.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usage.json");
        let mut s = UsageStore::default();
        s.record_line("kkkk-proj", "/p", "x.ndjson", &result_line(0.01, 1, 1, 1));
        s.save(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("\"key\""), "{raw}");
    }

    #[test]
    fn older_json_without_new_fields_loads() {
        // Forward compat rides #[serde(default)], like AppSettings/Fleet.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usage.json");
        std::fs::write(
            &path,
            r#"{"version":1,"projects":{"k":{"sketch_dir":"/s","cost_usd":1.0}}}"#,
        )
        .unwrap();
        let s = UsageStore::load(&path).unwrap();
        assert_eq!(s.projects["k"].input_tokens, 0);
        assert_eq!(s.projects["k"].sessions, 0);
    }
}
