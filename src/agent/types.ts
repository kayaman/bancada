// TS mirror of the objects carried on the `agent://event` Tauri event.
//
// Per the design spec (docs/superpowers/specs/2026-08-01-agent-panel-design.md,
// event contracts) the payload is the RAW `claude` CLI stream-json object for
// each line — types `system` (subtype `init`), `assistant`, `user`,
// `stream_event`, `result` — PLUS bancada-synthetic events the host adds:
// `stderr`, `unparsed`, `verify_started`, `verify_done` and
// `security_alarm`. This is NOT a re-serialisation
// of core/src/agent.rs's `AgentEvent` enum; it is the parsed line's own
// shape, one field of which (`type`) core's tolerant parser also switches
// on.
//
// The wire protocol is undocumented (see core/src/agent.rs's module doc) and
// real traffic already includes shapes nobody anticipated (a `status`
// subtype under `system`, a bare top-level `rate_limit_event`, hook
// lifecycle events...). Every interface below carries an index signature so
// an unrecognised *field* is preserved rather than dropped. A genuinely
// unrecognised `type` (not one of the literals below) is a value this
// file cannot type — it is still valid `AgentEvent` at the type level (the
// listener has no way to validate untrusted JSON against a union), but
// agentStore's `push` switches on `.type` with a `default` branch so at
// *runtime* it is ignored rather than thrown on. See the task brief's
// "unknown event types never throw" constraint.

export interface ContentBlockText {
  type: "text";
  text: string;
  [key: string]: unknown;
}

export interface ContentBlockToolUse {
  type: "tool_use";
  id: string;
  name: string;
  input: unknown;
  [key: string]: unknown;
}

/** A content block this file doesn't model (e.g. `thinking`). */
export interface ContentBlockOther {
  type: string;
  [key: string]: unknown;
}

export type ContentBlock =
  | ContentBlockText
  | ContentBlockToolUse
  | ContentBlockOther;

export interface UserContentText {
  type: "text";
  text: string;
  [key: string]: unknown;
}

export interface UserContentToolResult {
  type: "tool_result";
  tool_use_id: string;
  /** String or a content-block array — agentStore only ever displays it. */
  content: unknown;
  is_error?: boolean;
  [key: string]: unknown;
}

export interface UserContentOther {
  type: string;
  [key: string]: unknown;
}

export type UserContentBlock =
  | UserContentText
  | UserContentToolResult
  | UserContentOther;

/** `{"type":"system",...}` — only `subtype:"init"` carries session/model info. */
export interface AgentSystemEvent {
  type: "system";
  subtype?: string;
  session_id?: string;
  model?: string;
  tools?: string[];
  [key: string]: unknown;
}

export interface AgentAssistantEvent {
  type: "assistant";
  message?: {
    role?: string;
    content?: ContentBlock[];
    [key: string]: unknown;
  };
  [key: string]: unknown;
}

export interface AgentUserEvent {
  type: "user";
  message?: {
    role?: string;
    content?: UserContentBlock[];
    [key: string]: unknown;
  };
  [key: string]: unknown;
}

/**
 * `{"type":"stream_event","event":{...}}` — a partial-message delta wrapper
 * around the Anthropic Messages streaming format. `event` is kept loose
 * (only the `content_block_delta`/`text_delta` shape is read); everything
 * else (message_start, content_block_start/stop, message_delta/stop,
 * signature deltas...) is ignored.
 */
export interface AgentStreamEvent {
  type: "stream_event";
  event?: {
    type?: string;
    index?: number;
    delta?: { type?: string; text?: string; [key: string]: unknown };
    [key: string]: unknown;
  };
  [key: string]: unknown;
}

/** `{"type":"result",...}` — the final line of a turn. */
export interface AgentResultEvent {
  type: "result";
  is_error?: boolean;
  subtype?: string;
  result?: string;
  total_cost_usd?: number;
  num_turns?: number;
  session_id?: string;
  [key: string]: unknown;
}

/** Synthetic — from the src-tauri stderr drain thread. */
export interface AgentStderrEvent {
  type: "stderr";
  line: string;
  [key: string]: unknown;
}

/** Synthetic — brackets an MCP `verify` tool call.
 *
 * `pid` is the session's child pid (C1): the MCP listener thread can outlive
 * the session that started it, so the store drops one whose pid is not the
 * session it is showing. */
export interface AgentVerifyStartedEvent {
  type: "verify_started";
  pid?: number;
  [key: string]: unknown;
}

export interface AgentVerifyDoneEvent {
  type: "verify_done";
  success: boolean;
  pid?: number;
  [key: string]: unknown;
}

/** Synthetic — brackets an MCP `upload` tool call (agent-initiated flash).
 *  Same pid contract as the verify pair. */
export interface AgentUploadStartedEvent {
  type: "upload_started";
  port?: string;
  pid?: number;
  [key: string]: unknown;
}

export interface AgentUploadDoneEvent {
  type: "upload_done";
  success: boolean;
  pid?: number;
  [key: string]: unknown;
}

/** Synthetic — the host refused something and stopped the session.
 *
 * Emitted by src-tauri's stdout reader when the safety backstop fires: an
 * `Edit`/`Write` aimed outside the project (`kind: "path_escape"`, which
 * should already have been *refused* by the PreToolUse guard hook, so
 * seeing one means that boundary is not holding), or a session offered
 * tools Bancada did not ask for (`kind: "unexpected_tools"`). The child is
 * already dead by the time this arrives. */
export interface AgentSecurityAlarmEvent {
  type: "security_alarm";
  kind: string;
  detail: string;
  pid?: number;
  [key: string]: unknown;
}

// Deliberately NO `AgentUnknownEvent` member in the union below: giving it a
// non-literal `type: string` would defeat the other members' literal
// discriminants for switch/if-narrowing (TS can only discriminate a union
// when every member's tag is a literal). A payload whose `type` is e.g.
// `"rate_limit_event"` is still handled safely — agentStore's `push` switches
// on `.type` with a `default` branch — it just isn't a shape this file
// models, the same trust boundary every other `invoke`/`listen` call in
// api.ts already has towards its Rust-declared type.
export type AgentEvent =
  | AgentSystemEvent
  | AgentAssistantEvent
  | AgentUserEvent
  | AgentStreamEvent
  | AgentResultEvent
  | AgentStderrEvent
  | AgentVerifyStartedEvent
  | AgentVerifyDoneEvent
  | AgentUploadStartedEvent
  | AgentUploadDoneEvent
  | AgentSecurityAlarmEvent;

/** Result of `agent_probe` — whether a usable `claude` CLI is on PATH. */
export interface AgentProbe {
  ok: boolean;
  version?: string;
  error?: string;
}
