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

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

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
    /// Path to a JSON file holding the `--settings` payload: the `PreToolUse`
    /// hook that refuses edits outside the sketch dir (A1). A *path*, like
    /// `mcp_config_path` and for the same reason — and because the hook's
    /// `command` string embeds an absolute path that has no business in argv
    /// where a `--settings` JSON string would put it. Written and deleted by
    /// the caller (src-tauri's `agent_start`/`stop_agent_session`).
    pub settings_path: String,
    /// Project dir/profile context, composed by the caller and appended to
    /// the agent's system prompt via `--append-system-prompt`.
    pub system_prompt_extra: String,
    /// CLI session id to resume via native `--resume`, when continuing a
    /// past chat whose transcript the CLI still has on disk. `None` starts a
    /// fresh session, same as before this field existed. The caller
    /// (src-tauri's `agent_start`) validates the id's shape before it ever
    /// reaches here; `agent_args` trusts it and only decides placement.
    pub resume_session_id: Option<String>,
}

/// The built-in tools the embedded session is given, as a `--tools` value.
///
/// `--tools` is a genuine allow-list over the CLI's built-in set, unlike
/// `--disallowedTools` (see `agent_args`'s doc). Verified against `claude`
/// 2.1.220 by reading the `system`/`init` line's `tools` array: without it a
/// session lists 25 built-ins including `Skill`, `Task*`, `Monitor`,
/// `Workflow`, `SendMessage`, `LSP`, `EnterWorktree` and `Cron*`; with it the
/// array was exactly the allow-listed built-ins plus the `mcp__bancada__*`
/// tools. `WebFetch`/`WebSearch` joined in 0.12.0 (deliberate egress
/// trade-off — see the README's safety section); re-probe the init line
/// whenever this list or the CLI version changes.
pub const BUILTIN_TOOLS: &str = "Read,Edit,Write,Glob,Grep,WebFetch,WebSearch";

/// The complete set of tools an embedded session is expected to report in its
/// `system`/`init` line — [`BUILTIN_TOOLS`] plus the bancada MCP tools.
///
/// Must change in the SAME commit as [`BUILTIN_TOOLS`] and `agent_args`'s
/// `--allowedTools` literal: drift between them either alarms on every
/// session at init (A2 kills it) or stops asserting anything.
pub const EXPECTED_TOOLS: &[&str] = &[
    "Read",
    "Edit",
    "Write",
    "Glob",
    "Grep",
    "WebFetch",
    "WebSearch",
    "mcp__bancada__verify",
    "mcp__bancada__upload",
    "mcp__bancada__serial_read",
    "mcp__bancada__serial_send",
];

/// Tools present in a session's `system`/`init` `tools` array that Bancada did
/// not ask for, sorted and de-duplicated (A2).
///
/// This is the *assertion* behind `--tools`: the flag is what should keep the
/// set narrow, and this is what catches it having failed — a future CLI
/// release changing `--tools` semantics, a policy-managed setting re-adding a
/// tool, or a plugin-provided tool arriving through a channel this argv does
/// not control. A non-empty result means the session has capabilities the
/// safety model never reasoned about, so the host stops it rather than
/// letting it run with them.
///
/// *Missing* expected tools are deliberately not reported: an `init` line
/// emitted before an MCP server finishes connecting legitimately lacks
/// `mcp__bancada__verify`, and a session with too few tools is a usability
/// problem, not a safety one.
pub fn unexpected_tools(tools: &[String]) -> Vec<String> {
    let mut extra: Vec<String> = tools
        .iter()
        .filter(|t| !EXPECTED_TOOLS.contains(&t.as_str()))
        .cloned()
        .collect();
    extra.sort();
    extra.dedup();
    extra
}

// ---------- path confinement (A1) ----------

/// Directory names the agent may never write into, as components *below* the
/// project root.
///
/// Both are code-execution vectors reachable from inside the very subtree the
/// agent is otherwise allowed to edit:
///
/// - `.claude` — a project `settings.json` there can add hooks (shell
///   commands, picked up mid-session by the settings watcher) or, worse, set
///   `disableAllHooks`, which turns off the confinement hook itself. Verified
///   against 2.1.220: with `{"disableAllHooks": true}` in a project
///   `.claude/settings.json`, a `--settings`-supplied `PreToolUse` hook never
///   fires at all.
/// - `.git` — `.git/hooks/*` are shell scripts the next git operation runs.
///
/// This is the second of two layers for these paths: the `permissions.deny`
/// rules Bancada also generates are evaluated *before* hooks and survive
/// `disableAllHooks`, which is what makes them the anchor rather than this.
pub const REFUSED_DIRS: &[&str] = &[".claude", ".git"];

/// Is `candidate` a path the embedded agent is allowed to write?
///
/// Bancada's confinement rule, in one pure function so it is testable without
/// a CLI: a write is allowed only if it lands **inside `sketch_dir`** and no
/// path component *below* `sketch_dir` is one of [`REFUSED_DIRS`].
///
/// - Relative candidates are resolved against `sketch_dir` (the agent's cwd).
/// - `..` traversal is normalised away lexically *before* the prefix test, so
///   `<sketch>/../../etc/passwd` is rejected rather than passing a naive
///   `starts_with`.
/// - Symlinks are followed as far as the filesystem allows: the longest
///   existing prefix of the candidate is `canonicalize`d and the not-yet-
///   existing tail appended. A symlink *inside* the project pointing out of
///   it therefore resolves to its target and is rejected, and a not-yet-
///   created file still gets a meaningful answer (`canonicalize` alone cannot
///   answer for a path that does not exist yet, which is the common case for
///   `Write`).
/// - `sketch_dir` itself is canonicalised the same way, so a project reached
///   through a symlinked parent (`/tmp` → `/private/tmp` on macOS, a
///   symlinked home) compares equal instead of spuriously failing.
///
/// The [`REFUSED_DIRS`] rule is scoped to components *below* the root on
/// purpose: Bancada projects can legitimately live under a `.claude/` path
/// (git worktrees created by Claude Code itself do exactly that) and every
/// git checkout has a `.git` above or at its root, so testing the whole
/// absolute path would refuse every write in such a checkout. What the rule
/// is actually for is the agent rewriting the *project's own* config from
/// inside the session that config constrains.
///
/// Fails closed: an empty candidate, or a `sketch_dir` that cannot be
/// resolved to an absolute path, is not confined.
pub fn path_is_confined(sketch_dir: &Path, candidate: &str) -> bool {
    if candidate.is_empty() {
        return false;
    }
    let root = resolve_as_far_as_possible(sketch_dir);
    if !root.is_absolute() {
        return false;
    }

    let cand = Path::new(candidate);
    let joined = if cand.is_absolute() {
        cand.to_path_buf()
    } else {
        root.join(cand)
    };
    let resolved = resolve_as_far_as_possible(&joined);

    let Ok(below) = resolved.strip_prefix(&root) else {
        return false;
    };
    !below
        .components()
        .any(|c| REFUSED_DIRS.iter().any(|d| c.as_os_str() == *d))
}

/// The `permissions.deny` rules that anchor the confinement (red-team R1/R2).
///
/// The `PreToolUse` hook cannot protect `.claude/` on its own, because a
/// project `.claude/settings.json` containing `{"disableAllHooks": true}`
/// stops the hook firing at all — verified against 2.1.220. Deny rules are
/// evaluated *before* hooks and are unaffected by `disableAllHooks`, so they
/// are what keeps that file out of reach; the hook then provides the subtree
/// containment deny rules cannot express (a denylist has no "everything
/// except here" form).
///
/// Two syntax traps, both verified:
///
/// - **`Edit(...)`, never `Write(...)`.** Since 2.1.210 only `Edit(path)` and
///   `Read(path)` patterns are consulted for file permission checks;
///   `Write(...)` rules are accepted silently and never evaluated. `Edit(...)`
///   covers `Write`/`NotebookEdit`/`MultiEdit` too.
/// - **`//` means the filesystem root.** A single leading `/` anchors to *the
///   directory containing the settings file* — which for Bancada is a temp
///   dir, so a `/`-anchored rule would silently protect nothing. `~/` is the
///   home directory.
///
/// `project_dir` and `temp_dir` must already be absolute and canonical; the
/// caller resolves them. Gitignore-style glob metacharacters in either path
/// (`*`, `?`, `[`) would confuse these patterns — an exotic-enough directory name that
/// the hook layer is the answer for it, not more escaping.
pub fn deny_rules(project_dir: &str, temp_dir: &str) -> Vec<String> {
    // `//` *is* the filesystem-root anchor, so the path is appended without
    // its own leading slash — `///home/me` would be a different (and wrong)
    // pattern.
    let project_dir = project_dir.trim_end_matches('/').trim_start_matches('/');
    let temp_dir = temp_dir.trim_end_matches('/').trim_start_matches('/');
    vec![
        // The session's own 0600 policy files (defence in depth). They live
        // outside the project, so the hook already refuses them — but the
        // hook is exactly what stops working if hooks get disabled, and a
        // mid-session rewrite of the `--settings` file is a chained
        // escalation: edit the policy, and the CLI's settings watcher picks
        // up the new hooks. Anchoring them with a deny rule keeps them
        // protected by the layer that survives `disableAllHooks`. Matches
        // the `bancada-agent-{mcp,settings}-<nonce>.json` naming in
        // src-tauri's `write_mcp_config_file`/`write_agent_settings_file`.
        format!("Edit(//{temp_dir}/bancada-agent-*)"),
        // The project's own config — the disableAllHooks vector, plus any
        // hook a settings file could add.
        format!("Edit(//{project_dir}/.claude/**)"),
        format!("Edit(//{project_dir}/.mcp.json)"),
        format!("Edit(//{project_dir}/.claude.json)"),
        // Git hooks are shell scripts the next git operation runs.
        format!("Edit(//{project_dir}/.git/**)"),
        // The user's config — a hook here would run in every future session
        // of every project, the largest blast radius available.
        "Edit(~/.claude/**)".to_string(),
        "Edit(~/.claude.json)".to_string(),
        "Edit(~/.bashrc)".to_string(),
        "Edit(~/.zshrc)".to_string(),
        "Edit(~/.profile)".to_string(),
        "Edit(//etc/**)".to_string(),
    ]
}

/// Normalise `p` lexically (drop `.`, fold `..`), then canonicalise the
/// longest prefix that actually exists and re-append the rest.
///
/// `Path::canonicalize` fails outright on a path whose leaf does not exist —
/// which is the normal case for a `Write` creating a new file — so it cannot
/// be used directly. Folding `..` lexically *first* is what makes
/// `<sketch>/../../etc/passwd` collapse to `/etc/passwd` before any prefix
/// comparison happens.
fn resolve_as_far_as_possible(p: &Path) -> PathBuf {
    let lexical = normalize_lexically(p);
    let mut prefix = lexical.clone();
    let mut tail: Vec<OsString> = Vec::new();
    loop {
        if let Ok(real) = prefix.canonicalize() {
            let mut out = real;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match prefix.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                if !prefix.pop() {
                    return lexical;
                }
            }
            // A root (or a relative path fully consumed) that still won't
            // canonicalise — nothing left to strip.
            None => return lexical,
        }
    }
}

/// `.` dropped, `..` folded against the preceding component, no filesystem
/// access. A leading `..` on a relative path is kept (there is nothing to
/// fold it into) — callers always join against an absolute root first.
fn normalize_lexically(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in p.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ---------- disableAllHooks pre-flight (A1) ----------

/// Does this settings-file body switch hooks off?
///
/// `{"disableAllHooks": true}` in *any* settings source the CLI loads stops
/// a `--settings`-supplied `PreToolUse` hook from firing at all — verified
/// live against 2.1.220, twice: once by the red-team, once against Bancada's
/// own production configuration, where the `permissions.deny` anchor still
/// held (`.claude/`, `.git/` refused) but the out-of-project write went
/// through because the containment hook never ran.
///
/// The agent cannot *create* such a file — the deny rules see to that — so
/// the only way this arises is a project (or a user config) that already had
/// it before Bancada opened it. That is exactly a case the host can check
/// cheaply *before* spawning, which is why this is a pre-flight refusal
/// rather than a documented residual: a session whose boundary is known in
/// advance not to hold must not start.
///
/// Truthy is deliberately generous (`true`, `"true"`, `1`): this decides
/// whether to *refuse*, so anything that might mean "on" counts. An
/// unparseable settings file returns `false` — the CLI ignores invalid
/// settings files in `-p` mode ("silently ignored", per `--help`), so one
/// cannot disable hooks either.
pub fn settings_disables_hooks(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    match value.get("disableAllHooks") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        Some(Value::Number(n)) => n.as_i64().is_some_and(|i| i != 0),
        _ => false,
    }
}

/// The settings files whose `disableAllHooks` would neutralise the
/// confinement hook, in the order they should be reported.
///
/// Every `.claude/settings.json` and `.claude/settings.local.json` from the
/// sketch dir up to the filesystem root, plus the user's own. Walking all
/// the way up rather than checking only the sketch dir covers the
/// repo-root case: since 2.1.211 `settings.local.json` is loaded from the
/// *git repository root*, which for a sketch inside a larger checkout is
/// above the project — and a settings file above the sketch dir is one the
/// agent cannot write but can still be affected by.
pub fn hook_disabling_settings_paths(sketch_dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let push_for = |dir: &Path, out: &mut Vec<PathBuf>| {
        for name in ["settings.json", "settings.local.json"] {
            let candidate = dir.join(".claude").join(name);
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    };

    let root = resolve_as_far_as_possible(sketch_dir);
    let mut current = Some(root.as_path());
    while let Some(dir) = current {
        push_for(dir, &mut paths);
        current = dir.parent();
    }
    if let Some(home) = home {
        push_for(home, &mut paths);
    }
    paths
}

// ---------- the PreToolUse guard (A1) ----------

/// Tool names whose `file_path` the guard hook adjudicates. Kept beside
/// [`guard_hook_matcher`] so the regex the CLI matches on and the set this
/// module knows how to read cannot drift apart.
const GUARDED_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// The `matcher` regex for the `PreToolUse` hook entry — the CLI-side half of
/// [`GUARDED_TOOLS`].
pub fn guard_hook_matcher() -> String {
    GUARDED_TOOLS.join("|")
}

/// Decide one `PreToolUse` hook invocation.
///
/// `hook_stdin` is the JSON object the CLI writes to the hook's stdin
/// (`{"tool_name": ..., "tool_input": {"file_path": ...}, ...}`). Returns
/// `Some(json)` — the deny payload to print on stdout — when the edit must be
/// refused, and `None` when the hook should stay silent and let the session's
/// normal permission flow (`acceptEdits`) proceed.
///
/// **Fails closed.** Unparseable stdin, a guarded tool with no `file_path`,
/// or a non-string `file_path` all deny: the hook is a boundary, and a
/// boundary that opens when it is confused is not one. A tool name outside
/// [`GUARDED_TOOLS`] passes through — the CLI's `matcher` should mean this
/// never happens, but a widened matcher must not start denying `Read`.
pub fn guard_decision(sketch_dir: &Path, hook_stdin: &str) -> Option<String> {
    let deny = |reason: String| Some(deny_json(&reason));

    let Ok(input) = serde_json::from_str::<Value>(hook_stdin) else {
        return deny("Bancada could not read the tool input and refuses the edit.".to_string());
    };

    let tool = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
    if !tool.is_empty() && !GUARDED_TOOLS.contains(&tool) {
        return None;
    }

    let file_path = input
        .get("tool_input")
        .and_then(|i| i.get("file_path"))
        .and_then(Value::as_str);
    let Some(file_path) = file_path else {
        return deny(format!(
            "Bancada refuses a {tool} with no file_path it can check against the project directory."
        ));
    };

    if path_is_confined(sketch_dir, file_path) {
        return None;
    }
    deny(format!(
        "Bancada refuses this edit: {file_path} is outside the project directory {} \
         (or is part of its {} configuration). Only files inside the project may be edited.",
        sketch_dir.display(),
        REFUSED_DIRS.join("/")
    ))
}

/// The `PreToolUse` deny payload, in the CLI's hook-output shape.
fn deny_json(reason: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

/// Builds the `claude` argv (everything after the binary name) for an
/// embedded headless agent session.
///
/// ## What is, and is not, a boundary
///
/// This argv mixes flags with very different strengths, and the difference
/// matters enough to spell out (it was mis-stated in earlier revisions of
/// this doc and of the design spec):
///
/// - **`--tools BUILTIN_TOOLS` is a real restriction.** It selects from the
///   CLI's built-in set, and a tool left out of it is not offered to the
///   model at all. Probe-verified against 2.1.220 by reading the
///   `system`/`init` `tools` array (see [`BUILTIN_TOOLS`]), *including* that
///   it leaves `mcp__bancada__verify` intact — MCP tools are not built-ins,
///   so `--tools` does not filter them.
/// - **`--disallowedTools` is not a boundary.** With it set and `--tools`
///   absent, the same probe still listed 25 built-in tools including `Skill`,
///   `Task*`, `Monitor`, `Workflow`, `SendMessage` and `LSP`. It is a nudge
///   at the permission layer, not a capability gate; it is kept because
///   defence in depth is free, not because it confines anything.
/// - **`--settings cfg.settings_path` is a real boundary** — it carries the
///   `PreToolUse` hook that refuses out-of-project edits (A1). Probe-verified
///   end to end: an out-of-project `Write` came back as a `tool_result` with
///   `is_error: true`, appeared in the CLI's own `permission_denials`, and
///   left nothing on disk, while an in-project `Write` in the same turn
///   succeeded. A *permissive* hook running alongside it does not override
///   the deny (also probe-verified), so the user's own hooks cannot open it.
/// - **`--strict-mcp-config`** keeps the user's personal MCP servers out,
///   leaving only the loopback `bancada` server from `--mcp-config`.
/// - The `mcp__bancada__*` entries in `--allowedTools` are load-bearing:
///   a headless (`-p`) session with an MCP tool absent from the allow-list
///   stalls forever on a permission prompt it has no way to answer.
/// - `WebFetch`/`WebSearch` are allowed as of 0.12.0. This is a deliberate
///   *egress* trade-off, not a confinement change: reads were never
///   confined, and web access lets what is read leave the machine. Recorded
///   in the README's safety section; `Bash` remains structurally out.
/// - `--mcp-config` and `--settings` both take *file paths*, never inline
///   JSON: inline JSON would put the loopback server's bearer token (and the
///   hook command line) straight into argv, world-readable via
///   `/proc/<pid>/cmdline` on Linux (see [`AgentCfg::mcp_config_path`]).
///
/// ## Why `--bare` and `--safe-mode` are *not* here
///
/// The design spec called for `--bare` as the isolation mechanism (it skips
/// auto-discovery of the user's own hooks/skills/plugins). It cannot be
/// used: `claude --help` for 2.1.220 states that under `--bare` "Anthropic
/// auth is strictly `ANTHROPIC_API_KEY` or `apiKeyHelper` via `--settings`
/// (OAuth and keychain are never read)", and the Task 5 prototype confirmed
/// it end to end — an otherwise identical argv with `--bare` produced
/// `"error":"authentication_failed"` / `"Not logged in · Please run
/// /login"` on the very first turn.
///
/// `--safe-mode` (2.1.220: "all customizations … disabled … Auth … work
/// normally") looks like the missing flag, and the Task 8b probe confirmed it
/// *does* stop the user's `SessionStart` hooks from firing. It was still
/// rejected: the same probe showed it also drops `--mcp-config` servers —
/// `mcp_servers` came back `[]` and `mcp__bancada__verify` was absent from
/// the `tools` array — which removes the compile tool the whole panel exists
/// for.
///
/// So the residue stands and is *not* mitigated by this argv: **the user's
/// own hooks, skills and plugins do load into the embedded session, and
/// hooks are shell commands.** What A1's `--settings` hook does is take away
/// the other half of that pair — an unconfined `Write` — so a hostile or
/// merely careless hook can no longer be *installed* by the agent itself.
///
/// ## `--resume` placement
///
/// When [`AgentCfg::resume_session_id`] is `Some`, `--resume <id>` is
/// appended as the *final* pair, after `--append-system-prompt`'s value —
/// never spliced earlier. This keeps the pinned prefix (`args[0..9]`, the
/// documented flag sequence a test asserts on verbatim) and every other
/// flag's relative position stable regardless of whether a session is
/// resuming or starting fresh, so nothing upstream of this function needs to
/// know resume exists. The id itself is not re-validated here — the command
/// layer (src-tauri's `agent_start`) already rejected anything that is not
/// `^[0-9a-fA-F-]{8,64}$` before it reaches an `AgentCfg`, so by the time
/// this function sees `Some(id)` it cannot be flag-shaped or contain a shell
/// metacharacter.
pub fn agent_args(cfg: &AgentCfg) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--verbose".to_string(),
        "--include-partial-messages".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
        "--tools".to_string(),
        BUILTIN_TOOLS.to_string(),
        "--allowedTools".to_string(),
        "Read,Edit,Write,Glob,Grep,WebFetch,WebSearch,mcp__bancada__verify,\
         mcp__bancada__upload,mcp__bancada__serial_read,mcp__bancada__serial_send"
            .to_string(),
        "--disallowedTools".to_string(),
        "Bash,Task,NotebookEdit,KillShell,BashOutput".to_string(),
        "--mcp-config".to_string(),
        cfg.mcp_config_path.clone(),
        "--strict-mcp-config".to_string(),
        "--settings".to_string(),
        cfg.settings_path.clone(),
        "--append-system-prompt".to_string(),
        cfg.system_prompt_extra.clone(),
    ];
    if let Some(id) = &cfg.resume_session_id {
        args.push("--resume".to_string());
        args.push(id.clone());
    }
    args
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
    fn agent_args_allows_every_bancada_mcp_tool() {
        // Load-bearing: without these in --allowedTools, a headless
        // tools/call for any mcp__bancada__* tool stalls on a permission
        // prompt that can never be answered.
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/bancada-agent-mcp-abc.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: "project at /s, profile esp32s3".to_string(),
            resume_session_id: None,
        };
        let args = agent_args(&cfg);
        let allowed_idx = args.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(
            args[allowed_idx + 1],
            "Read,Edit,Write,Glob,Grep,WebFetch,WebSearch,mcp__bancada__verify,\
             mcp__bancada__upload,mcp__bancada__serial_read,mcp__bancada__serial_send"
        );
    }

    #[test]
    fn agent_args_disallows_bash_and_friends_but_not_the_web() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
        };
        let args = agent_args(&cfg);
        let idx = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert_eq!(args[idx + 1], "Bash,Task,NotebookEdit,KillShell,BashOutput");
        // WebFetch/WebSearch moved to the allow side in 0.12.0 (documented
        // egress trade-off); Bash stays firmly denied.
        assert!(!args[idx + 1].contains("WebFetch"));
        assert!(!args[idx + 1].contains("WebSearch"));
        assert!(args[idx + 1].contains("Bash"));
    }

    #[test]
    fn agent_args_isolates_the_session_with_strict_mcp_config() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
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
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
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
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
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
    fn agent_args_restricts_the_builtin_tool_set_with_the_tools_flag() {
        // A1/A2: `--tools` is the only flag in this argv that actually
        // removes a built-in tool from the session (probe-verified against
        // 2.1.220 — see the `agent_args` doc). Losing it silently would put
        // Skill/Task/Monitor/LSP back in the agent's hands.
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/s.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
        };
        let args = agent_args(&cfg);
        let idx = args
            .iter()
            .position(|a| a == "--tools")
            .expect("--tools must be present");
        assert_eq!(args[idx + 1], "Read,Edit,Write,Glob,Grep,WebFetch,WebSearch");
        assert!(
            !args[idx + 1].contains("Bash"),
            "Bash must never be in the built-in allow-list"
        );
    }

    #[test]
    fn agent_args_passes_the_settings_file_that_carries_the_confinement_hook() {
        // A1: this is the path confinement boundary. Without --settings the
        // session runs with no PreToolUse guard at all, and every out-of-
        // project Write succeeds (Task 5 prototype finding (c)).
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-nonce.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
        };
        let args = agent_args(&cfg);
        let idx = args
            .iter()
            .position(|a| a == "--settings")
            .expect("--settings must be present");
        assert_eq!(args[idx + 1], "/tmp/bancada-agent-settings-nonce.json");
        assert!(
            serde_json::from_str::<Value>(&args[idx + 1]).is_err(),
            "the --settings argv value must be a bare path, not JSON: the hook \
             command line has no business in /proc/<pid>/cmdline"
        );
    }

    #[test]
    fn agent_args_carries_the_system_prompt_extra_verbatim() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: "project at /home/me/Blink, profile esp32s3".to_string(),
            resume_session_id: None,
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
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
        };
        let args = agent_args(&cfg);
        assert!(!args.iter().any(|a| a == "--cwd" || a == "-cwd"));
    }

    #[test]
    fn agent_args_starts_with_the_documented_flag_sequence() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
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

    #[test]
    fn agent_args_appends_resume_id_as_the_final_pair() {
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: "project at /s, profile esp32s3".to_string(),
            resume_session_id: Some("abc123de".to_string()),
        };
        let args = agent_args(&cfg);
        assert_eq!(
            &args[args.len() - 2..],
            &["--resume", "abc123de"],
            "--resume <id> must be the final flag pair"
        );

        let cfg_none = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/bancada-agent-settings-x.json".to_string(),
            system_prompt_extra: "project at /s, profile esp32s3".to_string(),
            resume_session_id: None,
        };
        let args_none = agent_args(&cfg_none);
        assert!(
            !args_none.iter().any(|a| a == "--resume"),
            "no --resume flag should appear when resume_session_id is None"
        );
    }

    // ---------- A1: path_is_confined ----------
    //
    // The confinement rule is the release-blocking security decision, so the
    // traversal cases are pinned explicitly rather than left to the live
    // probe: a live probe proves the *hook* is wired up, these prove the
    // *policy* it enforces.

    /// A real directory to anchor against — `path_is_confined` canonicalises,
    /// so a purely imaginary root would compare against its lexical self and
    /// quietly stop testing symlink behaviour.
    fn sketch() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Blink.ino"), "void setup(){}\n").unwrap();
        dir
    }

    #[test]
    fn confined_accepts_a_plain_file_in_the_project() {
        let dir = sketch();
        let root = dir.path();
        assert!(path_is_confined(root, "Blink.ino"));
        assert!(path_is_confined(root, "./Blink.ino"));
        assert!(path_is_confined(
            root,
            root.join("Blink.ino").to_str().unwrap()
        ));
    }

    #[test]
    fn confined_accepts_a_file_that_does_not_exist_yet() {
        // The common Write case: canonicalize() alone would fail outright on
        // a leaf that isn't there, which must not read as "not confined".
        let dir = sketch();
        assert!(path_is_confined(dir.path(), "src/new/deeply/nested.h"));
        assert!(path_is_confined(dir.path(), "brand-new.ino"));
    }

    #[test]
    fn confined_accepts_a_subdirectory_and_the_root_itself() {
        let dir = sketch();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        assert!(path_is_confined(dir.path(), "src"));
        assert!(path_is_confined(dir.path(), dir.path().to_str().unwrap()));
    }

    #[test]
    fn confined_rejects_an_absolute_path_outside_the_project() {
        let dir = sketch();
        assert!(!path_is_confined(dir.path(), "/etc/passwd"));
        assert!(!path_is_confined(dir.path(), "/tmp/bancada-escape.txt"));
    }

    #[test]
    fn confined_rejects_dot_dot_traversal_out_of_the_project() {
        // The case a naive `starts_with` on the raw string would pass: the
        // path *begins* with the sketch dir and still lands outside it.
        let dir = sketch();
        let root = dir.path();
        assert!(!path_is_confined(root, "../escaped.txt"));
        assert!(!path_is_confined(root, "../../../../etc/passwd"));
        assert!(!path_is_confined(root, "src/../../escaped.txt"));
        assert!(!path_is_confined(
            root,
            root.join("../escaped.txt").to_str().unwrap()
        ));
        assert!(!path_is_confined(
            root,
            &format!("{}/a/b/../../../out.txt", root.display())
        ));
    }

    #[test]
    fn confined_allows_dot_dot_that_stays_inside_the_project() {
        let dir = sketch();
        assert!(path_is_confined(dir.path(), "src/../Blink.ino"));
        assert!(path_is_confined(dir.path(), "./a/b/../../Blink.ino"));
    }

    #[test]
    fn confined_rejects_a_sibling_directory_sharing_the_name_prefix() {
        // `/tmp/proj-evil` starts_with `/tmp/proj` as a *string* but is not
        // inside it. `strip_prefix` is component-wise, which is why this
        // passes — pinned so a future rewrite to string comparison fails.
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let evil = parent.path().join("proj-evil");
        std::fs::create_dir(&evil).unwrap();
        assert!(!path_is_confined(
            &root,
            evil.join("x.txt").to_str().unwrap()
        ));
    }

    #[cfg(unix)]
    #[test]
    fn confined_rejects_a_symlink_inside_the_project_pointing_out_of_it() {
        // The escape a lexical-only check cannot see.
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "s").unwrap();
        let dir = sketch();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();

        assert!(!path_is_confined(dir.path(), "escape/secret.txt"));
        assert!(!path_is_confined(dir.path(), "escape"));
        // ... and a symlink at the leaf itself.
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("leaf-link"),
        )
        .unwrap();
        assert!(!path_is_confined(dir.path(), "leaf-link"));
    }

    #[cfg(unix)]
    #[test]
    fn confined_accepts_a_project_reached_through_a_symlinked_parent() {
        // The false-*negative* direction: canonicalising the candidate but
        // not the root would reject every write in a project opened through
        // a symlinked path (a symlinked home, /tmp on macOS).
        let real = tempfile::tempdir().unwrap();
        let root = real.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let link_parent = tempfile::tempdir().unwrap();
        let link = link_parent.path().join("proj-link");
        std::os::unix::fs::symlink(&root, &link).unwrap();

        assert!(path_is_confined(&link, "Blink.ino"));
        assert!(path_is_confined(
            &link,
            root.join("Blink.ino").to_str().unwrap()
        ));
    }

    #[test]
    fn confined_rejects_a_dot_claude_component_below_the_project() {
        let dir = sketch();
        assert!(!path_is_confined(dir.path(), ".claude/settings.json"));
        assert!(!path_is_confined(dir.path(), ".claude"));
        assert!(!path_is_confined(dir.path(), "src/.claude/hooks/pre.sh"));
        assert!(!path_is_confined(
            dir.path(),
            dir.path()
                .join(".claude/settings.local.json")
                .to_str()
                .unwrap()
        ));
    }

    #[test]
    fn confined_rejects_a_dot_git_component_below_the_project() {
        // `.git/hooks/pre-commit` is a shell script the next git operation
        // runs — a code-execution vector entirely inside the subtree the
        // agent is otherwise allowed to edit.
        let dir = sketch();
        assert!(!path_is_confined(dir.path(), ".git/hooks/pre-commit"));
        assert!(!path_is_confined(dir.path(), ".git/config"));
        assert!(!path_is_confined(
            dir.path(),
            dir.path()
                .join(".git/hooks/post-checkout")
                .to_str()
                .unwrap()
        ));
    }

    #[test]
    fn confined_allows_a_project_inside_a_git_checkout() {
        // Every git project has a `.git` at or above its root; only
        // components *below* the project root are refused, or no Bancada
        // project under version control could be edited at all.
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".git")).unwrap();
        let project = repo.path().join("sketches/Blink");
        std::fs::create_dir_all(&project).unwrap();
        assert!(path_is_confined(&project, "Blink.ino"));
    }

    #[test]
    fn confined_allows_a_project_that_itself_lives_under_a_dot_claude_path() {
        // Claude Code's own git worktrees live at
        // `<repo>/.claude/worktrees/<name>` — testing `.claude` against the
        // whole absolute path would refuse every write in such a checkout,
        // which is why the rule is scoped to components *below* the root.
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join(".claude/worktrees/some-name");
        std::fs::create_dir_all(&root).unwrap();
        assert!(path_is_confined(&root, "Blink.ino"));
        assert!(path_is_confined(&root, "src/thing.h"));
        // ... but its *own* .claude dir is still off limits.
        assert!(!path_is_confined(&root, ".claude/settings.json"));
    }

    #[test]
    fn confined_fails_closed_on_empty_or_unusable_input() {
        let dir = sketch();
        assert!(!path_is_confined(dir.path(), ""));
        // A relative sketch_dir cannot anchor anything.
        assert!(!path_is_confined(Path::new("relative/dir"), "Blink.ino"));
    }

    // ---------- A1: guard_decision ----------

    fn hook_stdin(tool: &str, file_path: &str) -> String {
        serde_json::json!({
            "session_id": "abc",
            "hook_event_name": "PreToolUse",
            "tool_name": tool,
            "tool_input": {"file_path": file_path, "content": "x"}
        })
        .to_string()
    }

    fn deny_reason(json: &str) -> String {
        let v: Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
        v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn guard_stays_silent_for_an_in_project_write() {
        let dir = sketch();
        assert_eq!(
            guard_decision(dir.path(), &hook_stdin("Write", "Blink.ino")),
            None,
            "an in-project edit must fall through to the session's own \
             acceptEdits flow, not be explicitly allowed"
        );
    }

    #[test]
    fn guard_denies_an_out_of_project_write_and_names_the_path() {
        let dir = sketch();
        let json =
            guard_decision(dir.path(), &hook_stdin("Write", "/tmp/escape.txt")).expect("must deny");
        let reason = deny_reason(&json);
        assert!(reason.contains("/tmp/escape.txt"), "{reason}");
        assert!(
            reason.contains(&dir.path().display().to_string()),
            "the reason must name the project dir so the model can correct itself: {reason}"
        );
    }

    #[test]
    fn guard_denies_every_guarded_tool() {
        let dir = sketch();
        for tool in GUARDED_TOOLS {
            assert!(
                guard_decision(dir.path(), &hook_stdin(tool, "/etc/passwd")).is_some(),
                "{tool} must be guarded"
            );
        }
    }

    #[test]
    fn guard_denies_a_dot_claude_edit() {
        let dir = sketch();
        assert!(guard_decision(dir.path(), &hook_stdin("Edit", ".claude/settings.json")).is_some());
    }

    #[test]
    fn guard_fails_closed_on_unparseable_input() {
        // A boundary that opens when it is confused is not a boundary.
        let dir = sketch();
        assert!(guard_decision(dir.path(), "not json at all").is_some());
        assert!(guard_decision(dir.path(), "").is_some());
    }

    #[test]
    fn guard_fails_closed_when_a_guarded_tool_has_no_file_path() {
        let dir = sketch();
        let stdin = r#"{"tool_name":"Write","tool_input":{"content":"x"}}"#;
        assert!(guard_decision(dir.path(), stdin).is_some());
        let non_string = r#"{"tool_name":"Write","tool_input":{"file_path":42}}"#;
        assert!(guard_decision(dir.path(), non_string).is_some());
    }

    #[test]
    fn guard_passes_through_a_tool_it_does_not_adjudicate() {
        // The CLI's `matcher` should keep Read out of here entirely; if a
        // future matcher widens, the guard must not start denying reads.
        let dir = sketch();
        let stdin = r#"{"tool_name":"Read","tool_input":{"file_path":"/etc/passwd"}}"#;
        assert_eq!(guard_decision(dir.path(), stdin), None);
    }

    #[test]
    fn guard_hook_matcher_covers_exactly_the_guarded_tools() {
        // The regex the CLI matches on and the set `guard_decision` knows how
        // to read must not drift apart: a tool in the matcher but not in
        // GUARDED_TOOLS would be waved through by the pass-through branch.
        let matcher = guard_hook_matcher();
        for tool in GUARDED_TOOLS {
            assert!(matcher.contains(tool), "{matcher} is missing {tool}");
        }
        assert_eq!(matcher.split('|').count(), GUARDED_TOOLS.len());
    }

    // ---------- A1: the permissions.deny anchor ----------

    /// Verified against 2.1.220: `Write(...)` path rules are accepted
    /// silently and **never evaluated** — only `Edit(...)` and `Read(...)`
    /// are consulted for file permission checks. A deny rule written as
    /// `Write(...)` is therefore not a weaker rule, it is *no rule*, and
    /// nothing at runtime would say so.
    #[test]
    fn deny_rules_never_use_the_inert_write_pattern() {
        for rule in deny_rules("/home/me/Blink", "/tmp") {
            assert!(
                !rule.starts_with("Write("),
                "Write(...) rules are silently never evaluated: {rule}"
            );
            assert!(
                rule.starts_with("Edit(") || rule.starts_with("Read("),
                "only Edit(...)/Read(...) are consulted: {rule}"
            );
        }
    }

    /// A single leading `/` anchors to the directory containing the settings
    /// file — a temp dir, for Bancada — so a `/`-anchored rule would protect
    /// nothing at all while looking exactly like one that did. Absolute
    /// rules must use `//`.
    #[test]
    fn deny_rules_anchor_absolute_paths_with_a_double_slash() {
        for rule in deny_rules("/home/me/Blink", "/tmp") {
            let pattern = rule
                .trim_start_matches("Edit(")
                .trim_end_matches(')')
                .to_string();
            if pattern.starts_with('~') {
                continue; // home-anchored, correct as-is
            }
            assert!(
                pattern.starts_with("//"),
                "an absolute deny pattern must start with // or it anchors to \
                 the settings file's own directory: {rule}"
            );
        }
    }

    #[test]
    fn deny_rules_cover_the_hook_disabling_vectors() {
        let rules = deny_rules("/home/me/Blink", "/tmp");
        // The decisive one: `{"disableAllHooks": true}` in a project
        // .claude/settings.json stops the confinement hook firing entirely,
        // so the hook cannot be what protects it.
        assert!(rules.contains(&"Edit(//home/me/Blink/.claude/**)".to_string()));
        assert!(rules.contains(&"Edit(//home/me/Blink/.git/**)".to_string()));
        assert!(rules.contains(&"Edit(~/.claude/**)".to_string()));
        assert!(rules.contains(&"Edit(~/.claude.json)".to_string()));
    }

    #[test]
    fn deny_rules_protect_the_sessions_own_policy_files() {
        // Defence in depth: the settings file lives outside the project, so
        // the hook already refuses it — but the hook is exactly what stops
        // working when hooks are disabled, and rewriting the `--settings`
        // file mid-session is a chained escalation (the CLI's settings
        // watcher picks up new hooks). The anchor layer must cover it.
        let rules = deny_rules("/home/me/Blink", "/tmp");
        assert!(
            rules.contains(&"Edit(//tmp/bancada-agent-*)".to_string()),
            "{rules:?}"
        );
    }

    #[test]
    fn deny_rules_tolerate_a_trailing_slash_on_the_project_dir() {
        let rules = deny_rules("/home/me/Blink/", "/tmp/");
        assert!(
            rules.contains(&"Edit(//home/me/Blink/.claude/**)".to_string()),
            "{rules:?}"
        );
    }

    /// The two layers must agree about what is off limits: a directory the
    /// deny rules protect but the hook waves through (or vice versa) is a
    /// gap that only shows up when one layer is disabled.
    #[test]
    fn the_deny_rules_and_the_hook_refuse_the_same_project_dirs() {
        let dir = sketch();
        let rules = deny_rules(dir.path().to_str().unwrap(), "/tmp");
        for name in REFUSED_DIRS {
            assert!(
                !path_is_confined(dir.path(), &format!("{name}/x")),
                "the hook must refuse {name}/"
            );
            assert!(
                rules.iter().any(|r| r.contains(&format!("/{name}/"))),
                "the deny rules must refuse {name}/: {rules:?}"
            );
        }
    }

    // ---------- A1: the disableAllHooks pre-flight ----------

    #[test]
    fn settings_that_switch_hooks_off_are_recognised() {
        assert!(settings_disables_hooks(r#"{"disableAllHooks": true}"#));
        // Generous about truthiness: this decides whether to *refuse*, so
        // anything that might mean "on" must count.
        assert!(settings_disables_hooks(r#"{"disableAllHooks": "true"}"#));
        assert!(settings_disables_hooks(r#"{"disableAllHooks": 1}"#));
        assert!(settings_disables_hooks(
            r#"{"permissions":{"deny":[]},"disableAllHooks":true}"#
        ));
    }

    #[test]
    fn ordinary_settings_do_not_trip_the_pre_flight() {
        assert!(!settings_disables_hooks(r#"{"disableAllHooks": false}"#));
        assert!(!settings_disables_hooks("{}"));
        assert!(!settings_disables_hooks(r#"{"hooks":{"PreToolUse":[]}}"#));
        // An invalid settings file is silently ignored by the CLI in -p
        // mode, so it cannot disable hooks either — refusing to start over
        // one would be a false positive.
        assert!(!settings_disables_hooks("not json"));
        assert!(!settings_disables_hooks(""));
        // The *string* appearing somewhere harmless must not trip it.
        assert!(!settings_disables_hooks(
            r#"{"env":{"NOTE":"disableAllHooks is not set"}}"#
        ));
    }

    #[test]
    fn the_pre_flight_checks_the_project_its_ancestors_and_the_user_config() {
        let repo = tempfile::tempdir().unwrap();
        let project = repo.path().join("sketches/Blink");
        std::fs::create_dir_all(&project).unwrap();
        let home = repo.path().join("home");
        std::fs::create_dir_all(&home).unwrap();

        let paths = hook_disabling_settings_paths(&project, Some(&home));
        let canonical = std::fs::canonicalize(&project).unwrap();
        assert!(paths.contains(&canonical.join(".claude/settings.json")));
        assert!(paths.contains(&canonical.join(".claude/settings.local.json")));
        // The repo root: since 2.1.211 settings.local.json loads from the
        // git root, which can sit above the sketch dir.
        let repo_root = std::fs::canonicalize(repo.path()).unwrap();
        assert!(paths.contains(&repo_root.join(".claude/settings.local.json")));
        // The user's own config disables hooks globally, ours included.
        assert!(paths.contains(&home.join(".claude/settings.json")));
    }

    #[test]
    fn the_pre_flight_path_list_has_no_duplicates() {
        let dir = sketch();
        let paths = hook_disabling_settings_paths(dir.path(), Some(dir.path()));
        let mut sorted = paths.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), paths.len(), "{paths:?}");
    }

    // ---------- A2: unexpected_tools ----------

    #[test]
    fn the_expected_tool_set_raises_nothing() {
        let tools: Vec<String> = EXPECTED_TOOLS.iter().map(|s| s.to_string()).collect();
        assert!(unexpected_tools(&tools).is_empty());
    }

    #[test]
    fn a_missing_tool_is_not_an_alarm() {
        // An `init` line emitted before the MCP server finishes connecting
        // legitimately lacks mcp__bancada__verify.
        let tools: Vec<String> = ["Read", "Edit", "Write", "Glob", "Grep"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(unexpected_tools(&tools).is_empty());
    }

    #[test]
    fn an_extra_tool_is_reported_sorted_and_deduped() {
        let tools: Vec<String> = [
            "Read", "Bash", "Edit", "Skill", "Write", "Glob", "Grep", "Bash",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(unexpected_tools(&tools), vec!["Bash", "Skill"]);
    }

    #[test]
    fn a_foreign_mcp_tool_is_reported() {
        // --strict-mcp-config should make this impossible; the assertion is
        // what catches it having failed.
        let tools = vec![
            "Read".to_string(),
            "mcp__bancada__verify".to_string(),
            "mcp__someones_plugin__deploy".to_string(),
        ];
        assert_eq!(
            unexpected_tools(&tools),
            vec!["mcp__someones_plugin__deploy"]
        );
    }

    #[test]
    fn the_expected_set_matches_the_argv_the_session_is_started_with() {
        // EXPECTED_TOOLS is the assertion; --tools/--allowedTools are what
        // produce the reality it asserts on. Drift between them would either
        // alarm on every session or stop asserting anything.
        let cfg = AgentCfg {
            mcp_config_path: "/tmp/x.json".to_string(),
            settings_path: "/tmp/s.json".to_string(),
            system_prompt_extra: String::new(),
            resume_session_id: None,
        };
        let args = agent_args(&cfg);
        let tools_idx = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[tools_idx + 1], BUILTIN_TOOLS);
        let allowed_idx = args.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(args[allowed_idx + 1], EXPECTED_TOOLS.join(","));
        for tool in BUILTIN_TOOLS.split(',') {
            assert!(EXPECTED_TOOLS.contains(&tool), "{tool}");
        }
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
