//! Protocol types for driving the `claude` CLI as a supervised child process.
//!
//! This module is pure Rust with no knowledge of Tauri, processes, or
//! threads — it only turns bytes into typed values and back. The src-tauri
//! layer owns spawning `claude` with `--input-format stream-json
//! --output-format stream-json`, writing NDJSON lines built here to its
//! stdin, and feeding its stdout lines through [`parse_event`].
//!
//! ## Why the parser is tolerant (risk R1)
//!
//! `claude`'s stdin/stdout stream-json wire protocol is **undocumented** —
//! the Claude Agent SDK is the documented surface, and the CLI merely shells
//! the same protocol internally. That means the exact set of `type` values
//! and their fields is not a contract Bancada can rely on staying fixed
//! across CLI releases, and fixtures recorded against 2.1.220 already show
//! shapes the spec never anticipated: hook lifecycle events
//! (`hook_started`/`hook_response`), a `status` subtype under `system`
//! alongside `init`, and a completely separate top-level `rate_limit_event`
//! type that isn't `system`/`assistant`/`user`/`stream_event`/`result` at
//! all (see `testdata/agent_stream_pong.ndjson`, line with
//! `"type":"rate_limit_event"`).
//!
//! [`parse_event`] therefore never fails on a `type` it doesn't recognise —
//! any such line becomes [`AgentEvent::Unknown`] carrying the raw
//! [`serde_json::Value`]. The five typed variants also use
//! `#[serde(default)]` on every field so a *known* type with an unexpected
//! or missing field degrades gracefully; the only way [`parse_event`]
//! returns `Err` is a line that isn't valid JSON at all. A future CLI
//! release adding fields, adding event types, or a session that emits
//! nothing but errors must never crash or wedge an embedded agent turn.

use serde::Deserialize;
use serde_json::Value;

use crate::types::{OutputLine, OutputStream};
use crate::{Error, Result};

// ---------- AgentEvent ----------

/// One parsed line of `claude --output-format stream-json` output.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    System(SystemEvent),
    Assistant(AssistantEvent),
    User(UserEvent),
    StreamEvent(StreamEvent),
    Result(ResultEvent),
    /// Any line whose `type` is missing or not one of the five known
    /// values, or whose value structurally failed to decode as its known
    /// variant. See the module doc for why this can never become an `Err`.
    Unknown(Value),
}

/// `{"type":"system",...}` — session lifecycle. Real traffic includes more
/// subtypes than `init` (`status`, hook lifecycle events, ...); only `init`
/// carries `model`/`tools`, so those are empty/absent on the others.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct SystemEvent {
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub tools: Vec<String>,
}

/// `{"type":"assistant","message":{...}}`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AssistantEvent {
    #[serde(default)]
    pub message: AssistantMessage,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

/// A content block inside an assistant message.
///
/// `thinking` blocks (and anything else future CLI versions add) fall into
/// `Other` rather than failing the surrounding message — the panel has no
/// use for thinking content today, but a message that *did* contain a
/// tool_use alongside it must still parse.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolUse {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        input: Value,
    },
    #[serde(other)]
    Other,
}

/// `{"type":"user","message":{...}}` — carries tool results back to the
/// model; also what `user_message_json` builds the mirror image of.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct UserEvent {
    #[serde(default)]
    pub message: UserMessage,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct UserMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Vec<UserContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserContentBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolResult {
        #[serde(default)]
        tool_use_id: String,
        /// The tool's output. Kept as raw JSON: the Anthropic content
        /// format allows this to be either a plain string or a block
        /// array, and Bancada only ever displays it, never inspects it.
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

/// `{"type":"stream_event","event":{...}}` — a partial-message delta
/// wrapper around the Anthropic Messages streaming format. `event` is kept
/// as a raw [`Value`] rather than modelled in full (message_start,
/// content_block_start/delta/stop, message_delta, message_stop, and their
/// several delta shapes); [`StreamEvent::text_delta`] pulls out the one
/// piece the panel actually renders incrementally.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct StreamEvent {
    #[serde(default)]
    pub event: Value,
}

impl StreamEvent {
    /// The text fragment of a `content_block_delta` / `text_delta` event,
    /// i.e. `event.delta.text` — `None` for every other event shape
    /// (thinking/signature deltas, block start/stop, message_delta, ...).
    pub fn text_delta(&self) -> Option<&str> {
        if self.event.get("type").and_then(Value::as_str) != Some("content_block_delta") {
            return None;
        }
        let delta = self.event.get("delta")?;
        if delta.get("type").and_then(Value::as_str) != Some("text_delta") {
            return None;
        }
        delta.get("text").and_then(Value::as_str)
    }
}

/// `{"type":"result",...}` — the final line of a turn.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ResultEvent {
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub session_id: String,
}

/// Helper used only to pull the raw `event` value out of a `stream_event`
/// line without modelling the rest of the envelope.
#[derive(Deserialize)]
struct StreamEventEnvelope {
    #[serde(default)]
    event: Value,
}

/// Parse one line of `claude --output-format stream-json` output.
///
/// Never fails on valid JSON — see the module doc (risk R1). The only `Err`
/// case is a line that doesn't parse as JSON at all.
pub fn parse_event(line: &str) -> Result<AgentEvent> {
    let value: Value = serde_json::from_str(line).map_err(|source| Error::Json {
        what: "agent stream-json line".to_string(),
        source,
    })?;

    let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

    let event = match event_type {
        "system" => serde_json::from_value(value.clone())
            .ok()
            .map(AgentEvent::System),
        "assistant" => serde_json::from_value(value.clone())
            .ok()
            .map(AgentEvent::Assistant),
        "user" => serde_json::from_value(value.clone())
            .ok()
            .map(AgentEvent::User),
        "stream_event" => serde_json::from_value::<StreamEventEnvelope>(value.clone())
            .ok()
            .map(|raw| AgentEvent::StreamEvent(StreamEvent { event: raw.event })),
        "result" => serde_json::from_value(value.clone())
            .ok()
            .map(AgentEvent::Result),
        _ => None,
    };

    Ok(event.unwrap_or(AgentEvent::Unknown(value)))
}

// ---------- stdin payload builders ----------

/// Builds one NDJSON line (no trailing newline) sending a user text message
/// on the agent's stdin.
pub fn user_message_json(text: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": text}]
        }
    })
    .to_string()
}

/// Builds one NDJSON line requesting an interrupt via the (undocumented)
/// `control_request` protocol. Best-effort only — see the spec's note that
/// killing the child after a short grace period is the reliable path.
pub fn interrupt_json(request_id: &str) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "interrupt"}
    })
    .to_string()
}

// ---------- agent_args ----------

pub struct AgentCfg {
    /// Path to a JSON file holding the `--mcp-config` payload (the loopback
    /// server's URL plus its bearer token). A *path*, never the token
    /// itself: argv is visible to any local process via `/proc/<pid>/cmdline`
    /// on Linux, so an inline `--mcp-config <json-with-the-token>` would leak
    /// the one thing gating the `verify` listener. The caller (src-tauri's
    /// `agent_start`) owns writing this file at 0600 and deleting it when
    /// the session stops — core only embeds the path; it does no file I/O
    /// of its own.
    pub mcp_config_path: String,
    /// Project dir/profile context, composed by the caller and appended to
    /// the agent's system prompt via `--append-system-prompt`.
    pub system_prompt_extra: String,
}

/// Builds the `claude` argv (everything after the binary name) for an
/// embedded headless agent session.
///
/// The `mcp__bancada__verify` entry in `--allowedTools` is load-bearing: a
/// headless (`-p`) session with an MCP tool absent from the allow-list
/// stalls forever on a permission prompt it has no way to answer.
/// `--strict-mcp-config` keeps the user's personal MCP servers out of the
/// embedded session, leaving only the loopback `bancada` server from
/// `--mcp-config`. `--mcp-config` takes `cfg.mcp_config_path` — a *file
/// path*, not inline JSON — because inline JSON would put the loopback
/// server's bearer token straight into argv, world-readable via
/// `/proc/<pid>/cmdline` on Linux (fixed post-review; see `AgentCfg::mcp_config_path`).
///
/// ## Why `--bare` is *not* here (risk R3, resolved by the Task 5 prototype)
///
/// The design spec called for `--bare` as the isolation mechanism (it skips
/// auto-discovery of the user's own hooks/skills/plugins). It cannot be
/// used: `claude --help` for 2.1.220 states that under `--bare` "Anthropic
/// auth is strictly `ANTHROPIC_API_KEY` or `apiKeyHelper` via `--settings`
/// (OAuth and keychain are never read)", and the Task 5 prototype confirmed
/// it end to end — an otherwise identical argv with `--bare` produced
/// `"error":"authentication_failed"` / `"Not logged in · Please run
/// /login"` on the very first turn, while the same argv without it
/// completed a full `mcp__bancada__verify` tool round-trip.
///
/// Bancada's whole runtime premise (spec decision #2) is that auth comes
/// free from the user's existing Claude Code login, which is exactly the
/// OAuth/keychain credential `--bare` refuses to read. So R3's isolation is
/// deliberately traded for working auth: the remaining isolation mechanisms
/// are `--strict-mcp-config` (no personal MCP servers) plus the explicit
/// `--allowedTools`/`--disallowedTools` pair (no Bash/network/subagents).
/// The residue is that the user's own hooks, skills and plugins do load
/// into the embedded session.
pub fn agent_args(cfg: &AgentCfg) -> Vec<String> {
    vec![
        "-p".to_string(),
        "--verbose".to_string(),
        "--include-partial-messages".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
        "--allowedTools".to_string(),
        "Read,Edit,Write,Glob,Grep,mcp__bancada__verify".to_string(),
        "--disallowedTools".to_string(),
        "Bash,WebFetch,WebSearch,Task,NotebookEdit,KillShell,BashOutput".to_string(),
        "--mcp-config".to_string(),
        cfg.mcp_config_path.clone(),
        "--strict-mcp-config".to_string(),
        "--append-system-prompt".to_string(),
        cfg.system_prompt_extra.clone(),
    ]
}

// ---------- summarize_build_output ----------

/// Summarises a compile's captured output for the agent's `verify` tool
/// result: **all** stderr lines (that's where gcc/toolchain errors live)
/// plus the **tail** of stdout, under a line count and a total byte size
/// cap.
///
/// Strategy (binding, decided by the user — see spec §Decisions #7): stderr
/// is never dropped from the front — if it alone exceeds the caps, its
/// *tail* is kept (errors cluster at the end of a failing build) and a
/// `[... N stderr lines dropped]` marker replaces the front. Whatever line
/// budget remains after stderr goes to the tail of stdout, with the same
/// drop-the-front-keep-the-tail marker convention. `max_bytes` is enforced
/// as a genuine hard cap on top of the line cap by shrinking stdout's tail
/// first (least essential — it's context, not errors), then stderr's; if
/// even the bare section headers/markers (no content lines left to drop)
/// still exceed `max_bytes` — e.g. `max_lines` near zero with a tiny
/// `max_bytes` — the rendered text is hard-truncated to `max_bytes` as a
/// last resort, so the return value's byte length never exceeds the cap.
///
/// Empty input returns an empty string.
pub fn summarize_build_output(lines: &[OutputLine], max_lines: usize, max_bytes: usize) -> String {
    let stderr: Vec<&str> = lines
        .iter()
        .filter(|l| l.stream == OutputStream::Stderr)
        .map(|l| l.line.as_str())
        .collect();
    let stdout: Vec<&str> = lines
        .iter()
        .filter(|l| l.stream == OutputStream::Stdout)
        .map(|l| l.line.as_str())
        .collect();

    let (mut stderr_kept, mut stderr_dropped) = keep_tail(&stderr, max_lines);
    let remaining = max_lines.saturating_sub(stderr_kept.len());
    let (mut stdout_kept, mut stdout_dropped) = keep_tail(&stdout, remaining);

    // Byte cap, phase 1: shrink stdout's tail first, then stderr's, one
    // line at a time from the front (the tail is what's worth keeping in
    // both sections), until the rendered text fits or there is no whole
    // line left to drop.
    loop {
        let rendered = render_summary(&stderr_kept, stderr_dropped, &stdout_kept, stdout_dropped);
        if rendered.len() <= max_bytes {
            return rendered;
        }
        if !stdout_kept.is_empty() {
            stdout_kept.remove(0);
            stdout_dropped += 1;
        } else if !stderr_kept.is_empty() {
            stderr_kept.remove(0);
            stderr_dropped += 1;
        } else {
            // Phase 2: even the bare headers/markers (no content lines
            // left at all) exceed max_bytes — a pathologically tiny cap,
            // or max_lines left nothing to keep in the first place. Hard-
            // truncate so max_bytes is a genuine hard limit, not merely a
            // best-effort one.
            return truncate_to_byte_budget(rendered, max_bytes);
        }
    }
}

/// Truncates `s` to at most `max_bytes` bytes, backing off to the nearest
/// preceding `char` boundary so the result is never invalid UTF-8.
fn truncate_to_byte_budget(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Keeps at most `max` items from the tail of `items`; returns the kept
/// slice and how many were dropped from the front.
fn keep_tail<'a>(items: &[&'a str], max: usize) -> (Vec<&'a str>, usize) {
    if items.len() <= max {
        (items.to_vec(), 0)
    } else {
        let dropped = items.len() - max;
        (items[dropped..].to_vec(), dropped)
    }
}

fn render_summary(
    stderr_kept: &[&str],
    stderr_dropped: usize,
    stdout_kept: &[&str],
    stdout_dropped: usize,
) -> String {
    let mut out = String::new();
    if !stderr_kept.is_empty() || stderr_dropped > 0 {
        out.push_str("--- stderr ---\n");
        if stderr_dropped > 0 {
            out.push_str(&format!("[... {stderr_dropped} stderr lines dropped]\n"));
        }
        for line in stderr_kept {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !stdout_kept.is_empty() || stdout_dropped > 0 {
        out.push_str("--- stdout ---\n");
        if stdout_dropped > 0 {
            out.push_str(&format!("[... {stdout_dropped} stdout lines dropped]\n"));
        }
        for line in stdout_kept {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- parse_event: fixture-driven ----------

    /// Real output from `claude` 2.1.220, `-p --input-format stream-json
    /// --output-format stream-json --include-partial-messages
    /// --strict-mcp-config --mcp-config '{"mcpServers":{}}'
    /// --disallowedTools Bash,WebFetch,WebSearch,Task`, stdin
    /// `{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Reply
    /// with exactly: pong"}]}}`. `--bare` is omitted: in this sandbox it
    /// made the child report `authentication_failed` immediately (see the
    /// task report) even though a plain `claude -p "..."` in the same shell
    /// works — a real finding, not a fixture-recording shortcut, and the
    /// Task 5 prototype has since reproduced it outside the sandbox and
    /// removed `--bare` from `agent_args` entirely. The four
    /// SessionStart-hook lines that `--bare` would have skipped were
    /// trimmed from the top of the raw capture (this sandbox's own hooks,
    /// irrelevant to the embedded-agent wire shape); everything from the
    /// `system`/`init` line onward is untouched.
    const PONG_FIXTURE: &str = include_str!("testdata/agent_stream_pong.ndjson");

    #[test]
    fn fixture_every_line_parses_without_error() {
        for (i, line) in PONG_FIXTURE.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            assert!(
                parse_event(line).is_ok(),
                "line {} failed to parse: {line}",
                i + 1
            );
        }
    }

    #[test]
    fn fixture_known_types_never_fall_back_to_unknown() {
        for line in PONG_FIXTURE.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let raw: Value = serde_json::from_str(line).unwrap();
            let raw_type = raw.get("type").and_then(Value::as_str).unwrap_or("");
            if !matches!(
                raw_type,
                "system" | "assistant" | "user" | "stream_event" | "result"
            ) {
                continue; // e.g. rate_limit_event — Unknown is correct here.
            }
            let event = parse_event(line).unwrap();
            assert!(
                !matches!(event, AgentEvent::Unknown(_)),
                "a recognised type {raw_type:?} decoded as Unknown: {line}"
            );
        }
    }

    #[test]
    fn fixture_contains_at_least_one_system_init_assistant_and_result() {
        let mut saw_system_init = false;
        let mut saw_assistant = false;
        let mut saw_result = false;
        for line in PONG_FIXTURE.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match parse_event(line).unwrap() {
                AgentEvent::System(s) if s.subtype == "init" => saw_system_init = true,
                AgentEvent::Assistant(_) => saw_assistant = true,
                AgentEvent::Result(_) => saw_result = true,
                _ => {}
            }
        }
        assert!(saw_system_init, "fixture must contain a system/init line");
        assert!(saw_assistant, "fixture must contain an assistant line");
        assert!(saw_result, "fixture must contain a result line");
    }

    #[test]
    fn fixture_genuinely_unrecognised_top_level_type_becomes_unknown() {
        // `rate_limit_event` is real recorded output — proof R1's escape
        // hatch is exercised by actual CLI traffic, not just a hand-authored
        // guess at what an unknown type might look like.
        let mut saw_unknown_rate_limit = false;
        for line in PONG_FIXTURE.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if line.contains(r#""type":"rate_limit_event""#) {
                match parse_event(line).unwrap() {
                    AgentEvent::Unknown(v) => {
                        assert_eq!(
                            v.get("type").and_then(Value::as_str),
                            Some("rate_limit_event")
                        );
                        saw_unknown_rate_limit = true;
                    }
                    other => panic!("expected Unknown, got {other:?}"),
                }
            }
        }
        assert!(
            saw_unknown_rate_limit,
            "fixture must contain a rate_limit_event line"
        );
    }

    #[test]
    fn fixture_stream_event_text_delta_yields_pong() {
        // The fixture's real text delta spells "pong" one content block at
        // a time; text_delta must pull it out of the raw event Value.
        let mut deltas = String::new();
        for line in PONG_FIXTURE.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let AgentEvent::StreamEvent(ev) = parse_event(line).unwrap() {
                if let Some(text) = ev.text_delta() {
                    deltas.push_str(text);
                }
            }
        }
        assert_eq!(deltas, "pong");
    }

    // ---------- parse_event: hand-authored fixtures ----------
    //
    // # hand-authored (not from a CLI recording) — the pong fixture above
    // never happens to exercise a tool_use/tool_result turn, an unknown
    // event *shape* distinct from rate_limit_event, or invalid JSON, so
    // these are written by hand to cover them.

    #[test]
    fn hand_authored_assistant_with_no_message_field_still_parses_as_assistant() {
        // The brief's own tolerant-parser example: a bare {"type":"assistant"}
        // must decode through the #[serde(default)] scaffolding into
        // AgentEvent::Assistant with an empty message, never Unknown or Err
        // — pins the regression the review flagged against a future field
        // becoming required by accident.
        let line = r#"{"type":"assistant"}"#;
        match parse_event(line).unwrap() {
            AgentEvent::Assistant(a) => {
                assert_eq!(a.message.role, "");
                assert!(a.message.content.is_empty());
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn hand_authored_tool_use_block_is_captured() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/s/Blink.ino"}}]}}"#;
        let event = parse_event(line).unwrap();
        match event {
            AgentEvent::Assistant(a) => {
                assert_eq!(a.message.content.len(), 1);
                match &a.message.content[0] {
                    ContentBlock::ToolUse { id, name, input } => {
                        assert_eq!(id, "toolu_01");
                        assert_eq!(name, "Read");
                        assert_eq!(input.get("file_path").unwrap(), "/s/Blink.ino");
                    }
                    other => panic!("expected ToolUse, got {other:?}"),
                }
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn hand_authored_tool_result_block_is_captured() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"file contents","is_error":false}]}}"#;
        let event = parse_event(line).unwrap();
        match event {
            AgentEvent::User(u) => {
                assert_eq!(u.message.content.len(), 1);
                match &u.message.content[0] {
                    UserContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        assert_eq!(tool_use_id, "toolu_01");
                        assert_eq!(content, "file contents");
                        assert!(!is_error);
                    }
                    other => panic!("expected ToolResult, got {other:?}"),
                }
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn hand_authored_tool_result_error_flag_is_captured() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_02","content":"boom","is_error":true}]}}"#;
        match parse_event(line).unwrap() {
            AgentEvent::User(u) => match &u.message.content[0] {
                UserContentBlock::ToolResult { is_error, .. } => assert!(is_error),
                other => panic!("expected ToolResult, got {other:?}"),
            },
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn hand_authored_unknown_type_becomes_unknown_not_an_error() {
        let line = r#"{"type":"some_future_event","fancy_new_field":42}"#;
        match parse_event(line).unwrap() {
            AgentEvent::Unknown(v) => {
                assert_eq!(v.get("fancy_new_field").unwrap(), 42);
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn hand_authored_missing_type_field_becomes_unknown_not_an_error() {
        let line = r#"{"no_type_here":true}"#;
        assert!(matches!(parse_event(line).unwrap(), AgentEvent::Unknown(_)));
    }

    #[test]
    fn hand_authored_invalid_json_is_the_only_error_case() {
        match parse_event("not json at all") {
            Err(Error::Json { what, .. }) => assert!(what.contains("stream-json"), "{what}"),
            other => panic!("expected Json error, got {other:?}"),
        }
    }

    #[test]
    fn a_system_init_line_captures_the_documented_fields() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc-123","model":"claude-fable-5","tools":["Read","Edit"],"cwd":"/ignored"}"#;
        match parse_event(line).unwrap() {
            AgentEvent::System(s) => {
                assert_eq!(s.subtype, "init");
                assert_eq!(s.session_id, "abc-123");
                assert_eq!(s.model, "claude-fable-5");
                assert_eq!(s.tools, vec!["Read".to_string(), "Edit".to_string()]);
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    #[test]
    fn a_result_line_captures_the_documented_fields() {
        let line = r#"{"type":"result","is_error":false,"subtype":"success","result":"pong","total_cost_usd":0.0332,"num_turns":1,"session_id":"abc-123"}"#;
        match parse_event(line).unwrap() {
            AgentEvent::Result(r) => {
                assert!(!r.is_error);
                assert_eq!(r.subtype, "success");
                assert_eq!(r.result.as_deref(), Some("pong"));
                assert_eq!(r.total_cost_usd, Some(0.0332));
                assert_eq!(r.num_turns, Some(1));
                assert_eq!(r.session_id, "abc-123");
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn a_result_line_tolerates_a_missing_result_text_on_error() {
        // A denied/aborted turn can have is_error true with no `result`.
        let line =
            r#"{"type":"result","is_error":true,"subtype":"error_max_turns","session_id":"x"}"#;
        match parse_event(line).unwrap() {
            AgentEvent::Result(r) => {
                assert!(r.is_error);
                assert_eq!(r.result, None);
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn text_delta_returns_none_for_a_non_text_delta_event() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"xyz"}}}"#;
        match parse_event(line).unwrap() {
            AgentEvent::StreamEvent(ev) => assert_eq!(ev.text_delta(), None),
            other => panic!("expected StreamEvent, got {other:?}"),
        }
    }

    #[test]
    fn text_delta_returns_none_for_a_non_delta_event() {
        let line = r#"{"type":"stream_event","event":{"type":"message_stop"}}"#;
        match parse_event(line).unwrap() {
            AgentEvent::StreamEvent(ev) => assert_eq!(ev.text_delta(), None),
            other => panic!("expected StreamEvent, got {other:?}"),
        }
    }

    // ---------- stdin payload builders ----------

    #[test]
    fn user_message_json_escapes_and_wraps_the_text() {
        let json = user_message_json("say \"hi\" and \\backslash\\, then\nnewline");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["role"], "user");
        assert_eq!(
            value["message"]["content"][0]["text"],
            "say \"hi\" and \\backslash\\, then\nnewline"
        );
        assert_eq!(value["message"]["content"][0]["type"], "text");
        assert!(
            !json.ends_with('\n'),
            "must be a bare line, no trailing newline"
        );
    }

    #[test]
    fn interrupt_json_carries_the_request_id() {
        let json = interrupt_json("req-42");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "control_request");
        assert_eq!(value["request_id"], "req-42");
        assert_eq!(value["request"]["subtype"], "interrupt");
    }

    // ---------- agent_args ----------

    #[test]
    fn agent_args_allows_the_verify_tool() {
        // Load-bearing: without this in --allowedTools, a headless
        // tools/call for mcp__bancada__verify stalls on a permission
        // prompt that can never be answered.
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/bancada-agent-mcp-abc.json".to_string(),
            system_prompt_extra: "project at /s, profile esp32s3".to_string(),
        };
        let args = agent_args(&cfg);
        let allowed_idx = args.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(
            args[allowed_idx + 1],
            "Read,Edit,Write,Glob,Grep,mcp__bancada__verify"
        );
        assert!(args[allowed_idx + 1].contains("mcp__bancada__verify"));
    }

    #[test]
    fn agent_args_disallows_bash_and_friends() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            system_prompt_extra: String::new(),
        };
        let args = agent_args(&cfg);
        let idx = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert_eq!(
            args[idx + 1],
            "Bash,WebFetch,WebSearch,Task,NotebookEdit,KillShell,BashOutput"
        );
    }

    #[test]
    fn agent_args_isolates_the_session_with_strict_mcp_config() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            system_prompt_extra: String::new(),
        };
        let args = agent_args(&cfg);
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn agent_args_never_passes_bare_because_it_disables_keychain_auth() {
        // Regression guard for the Task 5 prototype finding: `--bare` makes
        // the CLI read auth *only* from ANTHROPIC_API_KEY/apiKeyHelper, so
        // an embedded session relying on the user's Claude Code login dies
        // with `authentication_failed` on turn one. See the `agent_args`
        // doc comment for the full rationale.
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            system_prompt_extra: String::new(),
        };
        let args = agent_args(&cfg);
        assert!(
            !args.iter().any(|a| a == "--bare"),
            "--bare breaks stored-credential auth; isolation is --strict-mcp-config \
             plus the explicit tool allow/deny lists"
        );
    }

    #[test]
    fn agent_args_mcp_config_is_a_path_not_inline_json() {
        // F5 (post-review security fix): the bearer token must never ride
        // argv (world-readable via /proc/<pid>/cmdline) — --mcp-config takes
        // the path to a file the caller wrote it into instead. Building and
        // parsing that file's JSON is now src-tauri's job (it owns the
        // token/port and the temp file), so this only pins that core embeds
        // the given path verbatim and does not turn it into JSON itself.
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/bancada-agent-mcp-sekrit-nonce.json".to_string(),
            system_prompt_extra: String::new(),
        };
        let args = agent_args(&cfg);
        let idx = args.iter().position(|a| a == "--mcp-config").unwrap();
        assert_eq!(args[idx + 1], "/tmp/bancada-agent-mcp-sekrit-nonce.json");
        assert!(
            serde_json::from_str::<Value>(&args[idx + 1]).is_err(),
            "the --mcp-config argv value must be a bare path, not JSON"
        );
    }

    #[test]
    fn agent_args_carries_the_system_prompt_extra_verbatim() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            system_prompt_extra: "project at /home/me/Blink, profile esp32s3".to_string(),
        };
        let args = agent_args(&cfg);
        let idx = args
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert_eq!(args[idx + 1], "project at /home/me/Blink, profile esp32s3");
    }

    #[test]
    fn agent_args_never_contains_a_cwd_flag() {
        // cwd is a std::process::Command concern set by the caller, not an
        // argv entry — arduino-cli's spec has no such flag either.
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            system_prompt_extra: String::new(),
        };
        let args = agent_args(&cfg);
        assert!(!args.iter().any(|a| a == "--cwd" || a == "-cwd"));
    }

    #[test]
    fn agent_args_starts_with_the_documented_flag_sequence() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            system_prompt_extra: String::new(),
        };
        let args = agent_args(&cfg);
        assert_eq!(
            &args[0..9],
            &[
                "-p",
                "--verbose",
                "--include-partial-messages",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--permission-mode",
                "acceptEdits",
            ]
        );
    }

    // ---------- summarize_build_output ----------

    fn out(stream: OutputStream, line: &str) -> OutputLine {
        OutputLine {
            stream,
            line: line.to_string(),
        }
    }

    #[test]
    fn summarize_empty_input_is_empty() {
        assert_eq!(summarize_build_output(&[], 200, 50_000), "");
    }

    #[test]
    fn summarize_under_cap_passes_everything_through_in_order() {
        let lines = vec![
            out(OutputStream::Stdout, "compiling Blink.ino"),
            out(OutputStream::Stderr, "warning: unused variable 'x'"),
            out(OutputStream::Stdout, "Sketch uses 1234 bytes"),
        ];
        let summary = summarize_build_output(&lines, 200, 50_000);
        assert_eq!(
            summary,
            "--- stderr ---\n\
             warning: unused variable 'x'\n\
             --- stdout ---\n\
             compiling Blink.ino\n\
             Sketch uses 1234 bytes\n"
        );
    }

    #[test]
    fn summarize_truncates_stdout_before_ever_touching_stderr() {
        let stdout_lines: Vec<OutputLine> = (0..10)
            .map(|i| out(OutputStream::Stdout, &format!("stdout-{i}")))
            .collect();
        let mut lines = stdout_lines;
        lines.push(out(OutputStream::Stderr, "error: expected ';'"));

        let summary = summarize_build_output(&lines, 3, 50_000);
        // 1 line goes to stderr, so 2 remain for stdout's tail.
        assert!(summary.contains("[... 8 stdout lines dropped]"));
        assert!(summary.contains("stdout-8"));
        assert!(summary.contains("stdout-9"));
        assert!(!summary.contains("stdout-7"));
        assert!(summary.contains("error: expected ';'"));
        assert!(!summary.contains("stderr lines dropped"));
    }

    #[test]
    fn summarize_keeps_the_tail_of_stderr_when_stderr_alone_exceeds_the_cap() {
        let lines: Vec<OutputLine> = (0..10)
            .map(|i| out(OutputStream::Stderr, &format!("err-{i}")))
            .collect();

        let summary = summarize_build_output(&lines, 4, 50_000);
        assert!(summary.contains("[... 6 stderr lines dropped]"));
        for i in 6..10 {
            assert!(summary.contains(&format!("err-{i}")), "{summary}");
        }
        for i in 0..6 {
            assert!(!summary.contains(&format!("err-{i}\n")), "{summary}");
        }
        // No line budget left over for stdout at all.
        assert!(!summary.contains("--- stdout ---"));
    }

    #[test]
    fn summarize_enforces_a_byte_cap_even_when_under_the_line_cap() {
        // 5 stdout lines of 50 bytes each comfortably clear the line cap
        // (200) but not a byte cap tight enough to hold only a couple.
        let lines: Vec<OutputLine> = (0..5)
            .map(|i| out(OutputStream::Stdout, &format!("{i}{}", "x".repeat(48))))
            .collect();

        let summary = summarize_build_output(&lines, 200, 120);
        assert!(
            summary.len() <= 120,
            "summary was {} bytes: {summary}",
            summary.len()
        );
        assert!(summary.contains("stdout lines dropped"));
        // The tail (highest-numbered lines) must be what survives.
        assert!(summary.contains("4xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"));
    }

    #[test]
    fn summarize_byte_cap_shrinks_stdout_before_stderr() {
        let mut lines = vec![out(OutputStream::Stderr, &"e".repeat(40))];
        lines.extend((0..5).map(|_| out(OutputStream::Stdout, &"o".repeat(40))));

        // Enough room for stderr's one line plus a bit of stdout, not all of it.
        let summary = summarize_build_output(&lines, 200, 150);
        assert!(summary.len() <= 150, "{}", summary.len());
        assert!(
            summary.contains(&"e".repeat(40)),
            "stderr must survive: {summary}"
        );
        assert!(!summary.contains("stderr lines dropped"), "{summary}");
    }

    /// Regression: with `max_lines` at zero (or near it), the line-cap
    /// phase alone empties every section, leaving only the `--- stderr ---`
    /// / dropped-count header text to render — and that header text can
    /// itself exceed a tiny `max_bytes`. Before the fix the function
    /// returned early as soon as nothing more could be *dropped*, without
    /// ever re-checking the rendered length against `max_bytes`, so the
    /// "hard cap" in the doc comment didn't actually hold.
    #[test]
    fn summarize_hard_caps_bytes_when_max_lines_is_zero() {
        let lines: Vec<OutputLine> = (0..10)
            .map(|i| out(OutputStream::Stderr, &format!("err-{i}")))
            .collect();

        let summary = summarize_build_output(&lines, 0, 5);
        assert!(
            summary.len() <= 5,
            "summary was {} bytes against a 5-byte cap: {summary:?}",
            summary.len()
        );
    }

    /// Same failure mode approached from the other side: a non-zero
    /// `max_lines` that still leaves nothing kept once the line budget runs
    /// out for stdout, paired with a `max_bytes` too small even for the
    /// section headers.
    #[test]
    fn summarize_hard_caps_bytes_with_near_zero_line_and_byte_caps() {
        let lines: Vec<OutputLine> = (0..3)
            .map(|i| out(OutputStream::Stdout, &format!("out-{i}")))
            .collect();

        let summary = summarize_build_output(&lines, 1, 3);
        assert!(
            summary.len() <= 3,
            "summary was {} bytes against a 3-byte cap: {summary:?}",
            summary.len()
        );
    }

    #[test]
    fn summarize_zero_byte_cap_is_always_empty() {
        let lines = vec![out(OutputStream::Stderr, "boom")];
        assert_eq!(summarize_build_output(&lines, 200, 0), "");
    }
}
