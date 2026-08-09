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
