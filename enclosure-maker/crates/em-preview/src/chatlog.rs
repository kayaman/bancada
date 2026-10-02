//! Per-project, persistent assistant chat history -- adapted from
//! bancada's chat-history design (that project's `core/src/chatlog.rs`):
//! one NDJSON file per chat thread, each line a single op. Replaying the
//! lines reproduces the exact live rendering, so there's no second message
//! schema to keep in sync with the live WS protocol.
//!
//! Storage lives *inside* the project directory (`.enclosure-maker/chats/`,
//! refused to the agent's own Edit/Write via `em_agent::guard`) rather than
//! a central app-config directory: chat history then travels with the
//! project if it's moved, copied, or backed up, and "per project" falls out
//! for free from the project directory already being the scoping key --
//! no separate path-hash registry needed.
//!
//! Op shapes, one JSON object per line:
//! - `{"op":"meta","startedAt":"<RFC3339>"}` -- always the first line.
//! - `{"op":"userSent","text":"..."}` -- a message the user sent.
//! - `{"op":"event","event":{...}}` -- one normalized provider event
//!   (the same shape relayed to the browser as a `chat_event` envelope).

use em_agent::AgentProvider;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CHATS_SUBDIR: &str = ".enclosure-maker/chats";
const MAX_KEPT_CHATS: usize = 30;
/// How far into a chat file to scan for its title (the first `userSent`
/// line) before giving up -- every real file has it within the first few
/// lines (right after `meta`), so this keeps listing many chats cheap even
/// if one of them is huge.
const TITLE_SCAN_LINES: usize = 20;
const TITLE_MAX_CHARS: usize = 80;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ChatEntry {
    pub file: String,
    pub title: String,
    pub modified: u64,
    pub provider: AgentProvider,
}

fn chats_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(CHATS_SUBDIR)
}

/// A chat filename must be a single safe path component ending in
/// `.ndjson` -- no separators, no `..`, nothing that could climb out of
/// the chats directory when joined onto it.
fn validate_filename(file: &str) -> Result<(), String> {
    if file.is_empty() || file == "." || file == ".." {
        return Err("invalid chat filename".to_string());
    }
    if file.contains('/') || file.contains('\\') {
        return Err("chat filename can't contain a path separator".to_string());
    }
    if !file.ends_with(".ndjson") {
        return Err("chat filename must end in .ndjson".to_string());
    }
    Ok(())
}

/// A fresh, timestamp-derived filename for a new chat thread.
pub fn new_chat_filename(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // A manual RFC3339-ish stamp (no chrono dependency): days/time derived
    // from the Unix epoch via straightforward civil-calendar arithmetic.
    let (y, mo, d, h, mi, s) = civil_from_unix(secs as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}-{mi:02}-{s:02}.ndjson")
}

/// Civil (Gregorian) date/time from a Unix timestamp, UTC. Standard
/// days-since-epoch -> civil-date algorithm (Howard Hinnant's
/// `civil_from_days`), since pulling in a whole date/time crate for "turn a
/// timestamp into a sortable filename" isn't worth the dependency.
fn civil_from_unix(unix_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = unix_secs.div_euclid(86400);
    let secs_of_day = unix_secs.rem_euclid(86400);
    let (h, mi, s) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };

    (y, m, d, h as u32, mi as u32, s as u32)
}

fn modified_unix_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Appends one line to `file` in `project_dir`'s chats directory, creating
/// the directory and file as needed. Fire-and-forget by convention at the
/// call site -- a failed append must never break a live chat.
pub fn append_line(project_dir: &Path, file: &str, line: &str) -> std::io::Result<()> {
    validate_filename(file).map_err(std::io::Error::other)?;
    let dir = chats_dir(project_dir);
    std::fs::create_dir_all(&dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(file))?;
    writeln!(f, "{line}")
}

/// Every chat file in `project_dir`, newest-modified first. A missing
/// chats directory is "no chats yet", not an error.
pub fn list_chats(project_dir: &Path) -> Vec<ChatEntry> {
    let dir = chats_dir(project_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut chats: Vec<ChatEntry> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("ndjson"))
        .map(|e| {
            let path = e.path();
            let file = e.file_name().to_string_lossy().into_owned();
            ChatEntry {
                title: title_for(&path),
                modified: modified_unix_secs(&path),
                provider: provider_for(&path),
                file,
            }
        })
        .collect();
    chats.sort_by(|a, b| b.modified.cmp(&a.modified));
    chats
}

fn provider_for(path: &Path) -> AgentProvider {
    let Ok(content) = std::fs::read_to_string(path) else {
        return AgentProvider::Claude;
    };
    extract_provider(
        &content
            .lines()
            .take(TITLE_SCAN_LINES)
            .map(str::to_string)
            .collect::<Vec<_>>(),
    )
}

fn title_for(path: &Path) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    };
    for line in content.lines().take(TITLE_SCAN_LINES) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v.get("op").and_then(|o| o.as_str()) == Some("userSent") {
                if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                    let trimmed: String = text.chars().take(TITLE_MAX_CHARS).collect();
                    return if text.chars().count() > TITLE_MAX_CHARS {
                        format!("{trimmed}…")
                    } else {
                        trimmed
                    };
                }
            }
        }
    }
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every line of `file`, for replaying into a read-only history view.
pub fn load_chat(project_dir: &Path, file: &str) -> std::io::Result<Vec<String>> {
    validate_filename(file).map_err(std::io::Error::other)?;
    let content = std::fs::read_to_string(chats_dir(project_dir).join(file))?;
    Ok(content.lines().map(str::to_string).collect())
}

pub fn delete_chat(project_dir: &Path, file: &str) -> std::io::Result<()> {
    validate_filename(file).map_err(std::io::Error::other)?;
    std::fs::remove_file(chats_dir(project_dir).join(file))
}

/// The most recent `session_id` recorded in `lines` (from a `system`/`init`
/// event) -- what the selected provider needs to continue this exact thread.
/// `None` if the thread never got far enough to start a real session (e.g.
/// it only ever recorded a `userSent` before the app closed).
pub fn extract_session_id(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .rev()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|v| {
            if v.get("op").and_then(|o| o.as_str()) != Some("event") {
                return None;
            }
            let event = v.get("event")?;
            let id = event.get("session_id").and_then(|s| s.as_str())?;
            if id.is_empty() {
                return None;
            }
            Some(id.to_string())
        })
}

/// The provider pinned to a chat. Threads written before multi-provider
/// support have no field and remain Claude threads for compatibility.
pub fn extract_provider(lines: &[String]) -> AgentProvider {
    lines
        .iter()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            if value.get("op").and_then(|op| op.as_str()) != Some("meta") {
                return None;
            }
            serde_json::from_value(value.get("provider")?.clone()).ok()
        })
        .unwrap_or_default()
}

/// Best-effort: deletes the oldest chats beyond `keep`, newest-modified
/// first order (matches [`list_chats`]).
pub fn prune(project_dir: &Path, keep: usize) {
    let chats = list_chats(project_dir);
    for entry in chats.into_iter().skip(keep) {
        let _ = delete_chat(project_dir, &entry.file);
    }
}

/// [`prune`] with this module's own default retention.
pub fn prune_default(project_dir: &Path) {
    prune(project_dir, MAX_KEPT_CHATS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("em-chatlog-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn civil_date_matches_known_timestamps() {
        // 2026-09-30T15:03:05Z, spot-checked against `date -u -d @<secs>`.
        assert_eq!(civil_from_unix(1790780585), (2026, 9, 30, 15, 3, 5));
        // The Unix epoch itself.
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn append_then_list_finds_the_chat_with_a_title() {
        let root = tempdir();
        let file = new_chat_filename(SystemTime::now());
        append_line(
            &root,
            &file,
            r#"{"op":"meta","startedAt":"2026-09-29T00:00:00Z"}"#,
        )
        .unwrap();
        append_line(&root, &file, r#"{"op":"userSent","text":"add a lid"}"#).unwrap();

        let chats = list_chats(&root);
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].file, file);
        assert_eq!(chats[0].title, "add a lid");
    }

    #[test]
    fn list_on_a_missing_project_is_empty_not_an_error() {
        let root = tempdir().join("does-not-exist");
        assert_eq!(list_chats(&root).len(), 0);
    }

    #[test]
    fn load_chat_returns_every_line_in_order() {
        let root = tempdir();
        let file = new_chat_filename(SystemTime::now());
        append_line(&root, &file, "line1").unwrap();
        append_line(&root, &file, "line2").unwrap();
        let lines = load_chat(&root, &file).unwrap();
        assert_eq!(lines, vec!["line1", "line2"]);
    }

    #[test]
    fn delete_removes_the_file() {
        let root = tempdir();
        let file = new_chat_filename(SystemTime::now());
        append_line(&root, &file, "x").unwrap();
        assert_eq!(list_chats(&root).len(), 1);
        delete_chat(&root, &file).unwrap();
        assert_eq!(list_chats(&root).len(), 0);
    }

    #[test]
    fn rejects_unsafe_filenames() {
        let root = tempdir();
        assert!(append_line(&root, "../escape.ndjson", "x").is_err());
        assert!(append_line(&root, "a/b.ndjson", "x").is_err());
        assert!(append_line(&root, "no-extension", "x").is_err());
        assert!(load_chat(&root, "../../etc/passwd").is_err());
    }

    #[test]
    fn extract_session_id_finds_the_most_recent_init_event() {
        let lines = vec![
            r#"{"op":"meta","startedAt":"x"}"#.to_string(),
            r#"{"op":"userSent","text":"hi"}"#.to_string(),
            r#"{"op":"event","event":{"type":"system","subtype":"init","session_id":"first"}}"#
                .to_string(),
            r#"{"op":"event","event":{"type":"assistant","message":{}}}"#.to_string(),
            r#"{"op":"userSent","text":"continue"}"#.to_string(),
            r#"{"op":"event","event":{"type":"system","subtype":"init","session_id":"second"}}"#
                .to_string(),
        ];
        assert_eq!(extract_session_id(&lines), Some("second".to_string()));
    }

    #[test]
    fn extract_session_id_is_none_when_no_session_ever_started() {
        let lines = vec![
            r#"{"op":"meta","startedAt":"x"}"#.to_string(),
            r#"{"op":"userSent","text":"hi"}"#.to_string(),
        ];
        assert_eq!(extract_session_id(&lines), None);
    }

    #[test]
    fn provider_defaults_old_threads_to_claude() {
        assert_eq!(
            extract_provider(&[r#"{"op":"meta"}"#.into()]),
            AgentProvider::Claude
        );
        assert_eq!(
            extract_provider(&[r#"{"op":"meta","provider":"codex"}"#.into()]),
            AgentProvider::Codex
        );
    }

    #[test]
    fn prune_keeps_only_the_newest_n() {
        let root = tempdir();
        for i in 0..5 {
            let file = format!("2026-01-01T00-00-0{i}.ndjson");
            append_line(&root, &file, r#"{"op":"meta"}"#).unwrap();
        }
        assert_eq!(list_chats(&root).len(), 5);
        prune(&root, 2);
        assert_eq!(list_chats(&root).len(), 2);
    }
}
