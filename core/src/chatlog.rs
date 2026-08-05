//! Persisted Assistant chat transcripts, one NDJSON file per chat.
//!
//! Each line is one mutating `AgentStore` operation recorded by the frontend;
//! replaying the lines through a fresh store reproduces the live rendering
//! exactly, so there is no second message schema to keep in sync. This module
//! only manages the files — it never interprets ops beyond peeking at the
//! first line for a list title.
//!
//! Path-agnostic and clock-free like `settings`/`fleet`: the Tauri layer
//! supplies the chats root (`app_config_dir()/chats`) and the frontend names
//! files after its own wall clock, so every operation here is deterministic
//! under test. `file` arguments arrive from the webview, so they are validated
//! as plain `*.ndjson` basenames before touching the filesystem.

use std::path::Path;

use serde::Serialize;

use crate::{Error, Result};

/// One row of the history list: the on-disk file name plus a human title
/// derived from the chat's first user message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatEntry {
    pub file: String,
    pub title: String,
}

/// Directory name for one sketch's chats: `<fnv1a-64-hex>-<sanitized-basename>`.
///
/// The hash makes the key collision-safe across sketches that share a
/// basename; the basename keeps the directory recognisable to a human
/// poking around the config dir. fnv1a is inlined because it is eight lines
/// and not worth a dependency.
pub fn sketch_key(sketch_dir: &str) -> String {
    // fnv1a-64 over the full path, so equal basenames in different parents
    // land in different directories.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in sketch_dir.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let basename = Path::new(sketch_dir)
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let sanitized: String = basename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();

    format!("{hash:016x}-{sanitized}")
}

/// Whether `file` is a plain `*.ndjson` basename — the only shape the
/// commands accept, because the webview chooses these names.
pub fn valid_chat_file(file: &str) -> bool {
    file.len() > ".ndjson".len()
        && file.ends_with(".ndjson")
        && !file.contains('/')
        && !file.contains('\\')
        && !file.contains("..")
}

/// Validate-then-join, shared by everything that touches a webview-named file.
fn chat_path(chats_root: &Path, key: &str, file: &str) -> Result<std::path::PathBuf> {
    if !valid_chat_file(file) {
        return Err(Error::Other(format!("invalid chat file name: `{file}`")));
    }
    Ok(chats_root.join(key).join(file))
}

/// Append one op line to a chat file, creating directories as needed.
///
/// Returns `true` when the file did not exist before, so the caller knows a
/// new chat was born and can prune the directory — pruning on every append
/// would be wasted work.
pub fn append_line(chats_root: &Path, key: &str, file: &str, line: &str) -> Result<bool> {
    let path = chat_path(chats_root, key, file)?;
    std::fs::create_dir_all(path.parent().expect("chat path always has a parent"))?;
    let created = !path.exists();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    use std::io::Write;
    writeln!(f, "{line}")?;
    Ok(created)
}

/// List a sketch's chats newest first.
///
/// Filenames are frontend timestamps (`YYYY-MM-DDTHH-MM-SS.ndjson`), so
/// descending name order *is* reverse-chronological — no mtime reads, which
/// backups and copies would falsify. Unreadable files are skipped rather
/// than failing the whole listing: one damaged chat must not hide the rest.
pub fn list_chats(chats_root: &Path, key: &str) -> Vec<ChatEntry> {
    let mut files: Vec<String> = chat_file_names(chats_root, key);
    files.sort_unstable_by(|a, b| b.cmp(a));
    files
        .into_iter()
        .filter_map(|file| {
            // Unreadable file → skipped; no userSent in the head → stem title.
            let f = std::fs::File::open(chats_root.join(key).join(&file)).ok()?;
            let title = title_from_head(std::io::BufReader::new(f))
                .unwrap_or_else(|| file.trim_end_matches(".ndjson").to_string());
            Some(ChatEntry { file, title })
        })
        .collect()
}

/// How deep into a chat file the title scan looks. The recorder writes
/// meta → sessionStarted → userSent, so the first user text sits on line 2
/// or 3 of every real file; the cap only exists so that listing 50 chats
/// whose events run to megabytes never reads past the head of any of them.
const TITLE_SCAN_LINES: usize = 10;

/// The first `userSent` op's text within the head of a chat, 80 chars max.
fn title_from_head(reader: impl std::io::BufRead) -> Option<String> {
    reader
        .lines()
        .take(TITLE_SCAN_LINES)
        .map_while(|l| l.ok())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(&l).ok())
        .find(|v| v.get("op").and_then(|o| o.as_str()) == Some("userSent"))
        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
        .map(|t| t.chars().take(80).collect())
}

/// Everything the Assistant has cost one sketch, summed from its chats.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProjectTotals {
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub chats: usize,
    pub turns: u64,
    /// Newest chat's file stem, e.g. `2026-08-05T09-00-00`.
    pub last_chat: Option<String>,
}

/// Sum every recorded `result` event across a sketch's chat files.
///
/// Per-call semantics, verified against the Agent SDK docs: each result's
/// `total_cost_usd`/`usage`/`num_turns` cover only its own query call, so
/// plain summation is the correct aggregation. Whole files are read, but
/// only when the History view opens — never on a poll. Never errors:
/// unreadable files and malformed lines are skipped, same as `list_chats`.
pub fn project_totals(chats_root: &Path, key: &str) -> ProjectTotals {
    let mut files = chat_file_names(chats_root, key);
    files.sort_unstable();
    let mut t = ProjectTotals {
        cost_usd: 0.0,
        input_tokens: 0,
        output_tokens: 0,
        chats: files.len(),
        turns: 0,
        last_chat: files
            .last()
            .map(|f| f.trim_end_matches(".ndjson").to_string()),
    };
    for file in &files {
        let Ok(f) = std::fs::File::open(chats_root.join(key).join(file)) else {
            continue;
        };
        use std::io::BufRead;
        for line in std::io::BufReader::new(f).lines().map_while(|l| l.ok()) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if v.get("op").and_then(|o| o.as_str()) != Some("push") {
                continue;
            }
            let Some(ev) = v.get("ev") else { continue };
            if ev.get("type").and_then(|t| t.as_str()) != Some("result") {
                continue;
            }
            t.cost_usd += ev
                .get("total_cost_usd")
                .and_then(|c| c.as_f64())
                .unwrap_or(0.0);
            t.turns += ev.get("num_turns").and_then(|n| n.as_u64()).unwrap_or(0);
            if let Some(u) = ev.get("usage") {
                t.input_tokens +=
                    u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                t.output_tokens +=
                    u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
            }
        }
    }
    t
}

/// The `*.ndjson` basenames under one sketch's chat directory, unsorted.
/// A missing directory is simply an empty history, never an error.
fn chat_file_names(chats_root: &Path, key: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(chats_root.join(key)) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| valid_chat_file(name))
        .collect()
}

/// Read one chat back as its raw op lines, ready for the frontend replayer.
pub fn load_chat(chats_root: &Path, key: &str, file: &str) -> Result<Vec<String>> {
    let path = chat_path(chats_root, key, file)?;
    let text = std::fs::read_to_string(path)?;
    Ok(text.lines().map(String::from).collect())
}

/// Delete one chat file.
pub fn delete_chat(chats_root: &Path, key: &str, file: &str) -> Result<()> {
    let path = chat_path(chats_root, key, file)?;
    std::fs::remove_file(path)?;
    Ok(())
}

/// Best-effort cap on chats per sketch, deleting oldest beyond `keep`.
///
/// Silent by design: history is a convenience, and a failed cleanup must
/// never surface into the chat flow that triggered it.
pub fn prune(chats_root: &Path, key: &str, keep: usize) {
    let mut files = chat_file_names(chats_root, key);
    files.sort_unstable_by(|a, b| b.cmp(a));
    for file in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(chats_root.join(key).join(file));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_chat(root: &Path, key: &str, file: &str, lines: &[&str]) {
        let dir = root.join(key);
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(dir.join(file), text).unwrap();
    }

    /// A recorded `result` push op, per-call semantics (verified: each
    /// result's cost/usage/turns cover only its own query call).
    fn result_line(cost: f64, input: u64, output: u64, turns: u64) -> String {
        format!(
            r#"{{"op":"push","ev":{{"type":"result","total_cost_usd":{cost},"num_turns":{turns},"usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}"#
        )
    }

    #[test]
    fn project_totals_sums_every_result_across_files() {
        let tmp = tempfile::tempdir().unwrap();
        let r1 = result_line(0.01, 100, 10, 2);
        let r2 = result_line(0.02, 200, 20, 3);
        let r3 = result_line(0.04, 400, 40, 1);
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
                &r3,
            ],
        );
        let t = project_totals(tmp.path(), "k");
        assert_eq!(t.chats, 2);
        assert!((t.cost_usd - 0.07).abs() < 1e-9);
        assert_eq!(t.input_tokens, 700);
        assert_eq!(t.output_tokens, 70);
        assert_eq!(t.turns, 6);
        assert_eq!(t.last_chat.as_deref(), Some("2026-08-05T09-00-00"));
    }

    #[test]
    fn project_totals_of_nothing_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let t = project_totals(tmp.path(), "missing");
        assert_eq!(
            t,
            ProjectTotals {
                cost_usd: 0.0,
                input_tokens: 0,
                output_tokens: 0,
                chats: 0,
                turns: 0,
                last_chat: None,
            }
        );
    }

    #[test]
    fn sketch_key_is_stable_and_sanitized() {
        let a = sketch_key("/home/me/sketches/my sketch");
        assert_eq!(a, sketch_key("/home/me/sketches/my sketch"));
        assert!(a.ends_with("-my-sketch"), "spaces become dashes: {a}");
        // 16 hex digits of fnv1a-64, then the separator.
        let (hash, _) = a.split_once('-').unwrap();
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        // Same basename under a different parent must not collide.
        assert_ne!(a, sketch_key("/somewhere/else/my sketch"));
    }

    #[test]
    fn valid_chat_file_accepts_only_plain_ndjson_basenames() {
        assert!(valid_chat_file("2026-08-05T14-02-31.ndjson"));
        assert!(!valid_chat_file("../x.ndjson"));
        assert!(!valid_chat_file("a/b.ndjson"));
        assert!(!valid_chat_file("a\\b.ndjson"));
        assert!(!valid_chat_file("x.txt"));
        assert!(!valid_chat_file(".ndjson"));
        assert!(!valid_chat_file(""));
    }

    #[test]
    fn append_creates_dirs_and_reports_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("chats");
        let created = append_line(&root, "k", "a.ndjson", r#"{"op":"userSent","text":"hi"}"#);
        assert!(created.unwrap(), "first append creates the file");
        let created = append_line(&root, "k", "a.ndjson", r#"{"op":"closed","reason":"done"}"#);
        assert!(!created.unwrap(), "second append does not");
        let text = std::fs::read_to_string(root.join("k/a.ndjson")).unwrap();
        assert_eq!(
            text,
            "{\"op\":\"userSent\",\"text\":\"hi\"}\n{\"op\":\"closed\",\"reason\":\"done\"}\n"
        );
    }

    #[test]
    fn append_rejects_invalid_file_names() {
        let tmp = tempfile::tempdir().unwrap();
        for bad in ["../x.ndjson", "a/b.ndjson", "x.txt"] {
            assert!(
                append_line(tmp.path(), "k", bad, "{}").is_err(),
                "{bad} must be rejected"
            );
        }
        // Nothing may have been written anywhere under the root.
        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    }

    #[test]
    fn list_is_newest_first_with_titles_from_first_user_message() {
        let tmp = tempfile::tempdir().unwrap();
        let long = "x".repeat(200);
        write_chat(
            tmp.path(),
            "k",
            "2026-08-04T10-00-00.ndjson",
            &[&format!(r#"{{"op":"userSent","text":"{long}"}}"#)],
        );
        write_chat(
            tmp.path(),
            "k",
            "2026-08-05T09-30-00.ndjson",
            &[
                r#"{"op":"meta","sketchDir":"/s","startedAt":"t"}"#,
                r#"{"op":"userSent","text":"fix the wifi loop"}"#,
            ],
        );
        let list = list_chats(tmp.path(), "k");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].file, "2026-08-05T09-30-00.ndjson");
        // Every real file starts with the recorder's meta line — the title
        // must come from the first userSent op, wherever it sits in the head.
        assert_eq!(list[0].title, "fix the wifi loop");
        assert_eq!(list[1].file, "2026-08-04T10-00-00.ndjson");
        assert_eq!(list[1].title, "x".repeat(80), "title truncated to 80 chars");
    }

    #[test]
    fn title_scan_is_bounded_and_reads_only_the_head() {
        // The realistic head: meta, then sessionStarted, then the user.
        let tmp = tempfile::tempdir().unwrap();
        write_chat(
            tmp.path(),
            "k",
            "2026-08-05T10-00-00.ndjson",
            &[
                r#"{"op":"meta","sketchDir":"/s","startedAt":"t"}"#,
                r#"{"op":"sessionStarted","pid":42}"#,
                r#"{"op":"userSent","text":"scan me"}"#,
            ],
        );
        // A file whose first userSent sits beyond the scan cap keeps the stem
        // title — the cap is what keeps listing 50 multi-MB chats cheap.
        let mut filler: Vec<String> = (0..TITLE_SCAN_LINES)
            .map(|i| format!(r#"{{"op":"push","ev":{{"n":{i}}}}}"#))
            .collect();
        filler.push(r#"{"op":"userSent","text":"too deep"}"#.to_string());
        let filler_refs: Vec<&str> = filler.iter().map(String::as_str).collect();
        write_chat(tmp.path(), "k", "2026-08-05T11-00-00.ndjson", &filler_refs);

        let list = list_chats(tmp.path(), "k");
        assert_eq!(list[1].title, "scan me");
        assert_eq!(list[0].title, "2026-08-05T11-00-00");
    }

    #[test]
    fn list_survives_corrupt_and_foreign_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_chat(tmp.path(), "k", "2026-08-05T08-00-00.ndjson", &["{not json"]);
        write_chat(tmp.path(), "k", "notes.txt", &["not a chat"]);
        let list = list_chats(tmp.path(), "k");
        assert_eq!(list.len(), 1, "foreign extensions are not chats");
        assert_eq!(list[0].title, "2026-08-05T08-00-00", "corrupt line falls back to stem");
    }

    #[test]
    fn list_of_missing_sketch_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_chats(tmp.path(), "never-chatted").is_empty());
    }

    #[test]
    fn load_returns_lines() {
        let tmp = tempfile::tempdir().unwrap();
        write_chat(tmp.path(), "k", "a.ndjson", &["l1", "l2"]);
        assert_eq!(load_chat(tmp.path(), "k", "a.ndjson").unwrap(), vec!["l1", "l2"]);
        assert!(load_chat(tmp.path(), "k", "../a.ndjson").is_err());
    }

    #[test]
    fn delete_removes_exactly_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        write_chat(tmp.path(), "k", "a.ndjson", &["l"]);
        write_chat(tmp.path(), "k", "b.ndjson", &["l"]);
        delete_chat(tmp.path(), "k", "a.ndjson").unwrap();
        assert!(!tmp.path().join("k/a.ndjson").exists());
        assert!(tmp.path().join("k/b.ndjson").exists());
        assert!(delete_chat(tmp.path(), "k", "../b.ndjson").is_err());
    }

    #[test]
    fn prune_keeps_the_newest_n() {
        let tmp = tempfile::tempdir().unwrap();
        for name in [
            "2026-08-01T00-00-00.ndjson",
            "2026-08-02T00-00-00.ndjson",
            "2026-08-03T00-00-00.ndjson",
        ] {
            write_chat(tmp.path(), "k", name, &["l"]);
        }
        prune(tmp.path(), "k", 2);
        let list = list_chats(tmp.path(), "k");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].file, "2026-08-03T00-00-00.ndjson");
        assert_eq!(list[1].file, "2026-08-02T00-00-00.ndjson");
        // Pruning a sketch that never chatted must not panic.
        prune(tmp.path(), "ghost", 2);
    }
}
