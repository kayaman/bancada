// Plain-JS message store for the Assistant panel. No React: `push(ev)`
// ingests one `agent://event` payload at a time, `userSent`/`closed`/`clear`
// cover the remaining state transitions, and the panel polls `version` to
// know when to re-render — modeled on `src/obs/obsStore.ts` (same
// version/snapshot discipline, same reason for existing: decouple the event
// callback from React's render cycle).
//
// Event payloads are the raw claude-CLI stream-json shapes (see
// `src/agent/types.ts`) plus bancada's synthetic stderr/verify events. The
// wire protocol is undocumented and deliberately under-modelled upstream
// (core/src/agent.rs), so `push` must never throw on an event shape it
// doesn't recognise — an unknown `type` paints nothing into the transcript.
// It does land in the raw debug log and bump `version` (the 🐛 Debug view
// must repaint); the original "no version bump" task-brief rule predates
// that view and only ever protected the transcript.

import type {
  AgentEvent,
  ContentBlockText,
  ContentBlockToolUse,
  UserContentToolResult,
} from "./types";
import {
  addUsage,
  emptySessionUsage,
  parseTurnUsage,
  type SessionUsage,
  type TurnUsage,
} from "./usage";

export type AgentMessage =
  | { kind: "user"; text: string }
  | { kind: "assistant"; text: string }
  | {
      kind: "tool";
      id: string;
      name: string;
      input: unknown;
      status: "running" | "ok" | "error";
      result?: string;
      /** Date.now() when the tool_use block arrived — for the footer's elapsed counter. */
      startedAt?: number;
    }
  | { kind: "stderr"; line: string }
  /** Pushed on every `result` event: the turn's ledger line. `usage` is
   *  absent when the event carried none we could trust. */
  | {
      kind: "turn_end";
      usage?: TurnUsage;
      summary?: string;
      tools: { name: string; status: "running" | "ok" | "error" }[];
    };

export interface AgentResult {
  isError: boolean;
  costUsd?: number;
  numTurns?: number;
  text?: string;
}

export type AgentStatus = "idle" | "starting" | "running" | "ended";

/** A refused/stopped session, surfaced prominently by the panel. */
export interface AgentAlarm {
  kind: string;
  detail: string;
}

/** One row of the 🐛 Debug view: a raw `agent://event` payload as received.
 *  Consecutive `stream_event`s coalesce into one row (count > 1, newest
 *  json wins) — text deltas arrive dozens per second. */
export interface RawLogEntry {
  ts: number;
  type: string;
  subtype?: string;
  count: number;
  json: string;
}

const RAW_LOG_CAP = 500;

export class AgentStore {
  private msgs: AgentMessage[] = [];
  private ver = 0;
  private statusFlag: AgentStatus = "idle";
  private lastResultVal?: AgentResult;
  private sessionIdVal?: string;
  private verifyRunningFlag = false;
  private closedReasonVal?: string;
  private alarmVal?: AgentAlarm;
  /** Text deltas are streaming and no tool has started since — "✍ writing". */
  private streamingFlag = false;
  /** True between userSent() and the turn's result/close/alarm — the
   *  footer's "is the agent actually working" bit, distinct from
   *  statusFlag ("is the session alive"). */
  private turnActiveFlag = false;
  private turnStartedAtVal?: number;
  private rawLog: RawLogEntry[] = [];
  private sessionUsageVal: SessionUsage = emptySessionUsage();
  /**
   * The child pid of the session this store is showing, set by
   * `sessionStarted` from `agent_start`'s return value.
   *
   * Every backend event that can outlive its session carries a `pid`
   * (`agent://closed`, and the synthetic verify/alarm events). Without
   * comparing it, a *stale* close could mark a **live newer** session
   * "ended": `newAgentSession()` fires `agentStop()` without awaiting and
   * clears the store, the user sends again and session B starts, and only
   * then does session A's stdout EOF deliver `agent://closed{pid: A}` —
   * which used to flip B to "ended", disabling Send while B's child was
   * alive and still streaming. The backend already guarded its *kill* path
   * with `should_stop_agent`; this is the same guard for the store.
   */
  private pidVal?: number;

  /**
   * Pids of sessions this store has already been cleared away from. After
   * `clear()` and before the next `sessionStarted`, `pidVal` is `undefined`
   * — and "undefined means assume ours" would let the cleared child's late
   * EOF flip the *fresh* store to "ended". The set (one entry per session
   * per app run; a rapid double-switch can have two EOFs in flight) survives
   * `clear()` so those events die here regardless of timing. Ordinary
   * events (`assistant`, `stderr`, …) carry no pid on the wire, so a
   * handful already queued at teardown can still paint into the fresh
   * store; fixing that needs backend pid-stamping on every event, and the
   * window is milliseconds.
   */
  private supersededPids = new Set<number>();

  /**
   * Index into `msgs` of the assistant message currently accumulating text,
   * or `undefined` if the next delta/assistant text should start a fresh
   * one. Reset on any boundary between model inferences: a `result` event,
   * a new user message (`userSent`), or a `tool_use` content block (the CLI
   * emits a separate non-delta `assistant` line per inference, and a
   * tool-using turn has one inference before the call and another after the
   * tool_result — without the tool_use reset the second inference's text
   * would overwrite the first bubble instead of following it).
   */
  private currentAssistantIdx?: number;

  /** tool_use id → index into `msgs`, so a later tool_result can find its card. */
  private toolIndexById = new Map<string, number>();

  /** Bumps on any visible change (push that mutates state, userSent, closed, clear). */
  get version(): number {
    return this.ver;
  }

  /**
   * Is `pid` the session this store is showing?
   *
   * `undefined` on either side means "cannot tell, assume it is ours": a
   * store that never learned its pid (an event arriving between
   * `agent_start` being invoked and its promise resolving) must still show
   * events, and an event without a pid predates the stamping.
   */
  private isOurs(pid: number | undefined): boolean {
    if (pid !== undefined && this.supersededPids.has(pid)) return false;
    return pid === undefined || this.pidVal === undefined || pid === this.pidVal;
  }

  /** Ingest one `agent://event` payload. Unknown `type`s are ignored. */
  push(ev: AgentEvent): void {
    // Synthetic host events carry the pid of the session that produced
    // them; one from a session the user already stopped must not repaint
    // the new session's panel (verify spinner, alarm banner).
    if (
      (ev.type === "verify_started" ||
        ev.type === "verify_done" ||
        ev.type === "security_alarm") &&
      !this.isOurs(typeof ev.pid === "number" ? ev.pid : undefined)
    ) {
      return;
    }
    this.recordRaw(ev);
    // Every surviving event repaints: an open 🐛 Debug view must show it
    // even when no handler below changes transcript state.
    this.ver++;
    switch (ev.type) {
      case "system":
        this.handleSystem(ev.subtype, ev.session_id);
        break;
      case "assistant":
        this.handleAssistant(ev.message?.content);
        break;
      case "user":
        this.handleUser(ev.message?.content);
        break;
      case "stream_event":
        this.handleStreamEvent(ev.event);
        break;
      case "result":
        this.handleResult(ev);
        break;
      case "stderr":
        this.handleStderr(typeof ev.line === "string" ? ev.line : "");
        break;
      case "verify_started":
        this.verifyRunningFlag = true;
        this.ver++;
        break;
      case "verify_done":
        this.verifyRunningFlag = false;
        this.ver++;
        break;
      case "security_alarm":
        this.handleAlarm(ev);
        break;
      default:
        // Unrecognised type (or missing `type`) — no transcript message;
        // it was already recorded into the raw log above.
        return;
    }
  }

  /** Append `payload` to the raw debug log (ring buffer, coalescing
   *  consecutive stream_events). Never throws — this runs on every event. */
  private recordRaw(payload: unknown): void {
    const p = (payload ?? {}) as Record<string, unknown>;
    const type = typeof p.type === "string" ? p.type : "unknown";
    let json: string;
    try {
      json = JSON.stringify(payload, null, 2);
    } catch {
      json = String(payload);
    }
    const last = this.rawLog[this.rawLog.length - 1];
    if (last && type === "stream_event" && last.type === "stream_event") {
      last.count++;
      last.json = json;
      last.ts = Date.now();
      return;
    }
    this.rawLog.push({
      ts: Date.now(),
      type,
      subtype: typeof p.subtype === "string" ? p.subtype : undefined,
      count: 1,
      json,
    });
    if (this.rawLog.length > RAW_LOG_CAP) this.rawLog.shift();
  }

  /** Record a user-authored message. User messages never come from events. */
  userSent(text: string): void {
    if (this.statusFlag === "idle") this.statusFlag = "starting";
    this.msgs.push({ kind: "user", text });
    this.currentAssistantIdx = undefined; // turn boundary
    this.streamingFlag = false;
    this.turnActiveFlag = true;
    this.turnStartedAtVal = Date.now();
    this.ver++;
  }

  /** Bind this store to the session `agent_start` just returned. A pid the
   *  OS recycled from a superseded session is ours again — evict it, or the
   *  set would eat the live session's close and alarms. */
  sessionStarted(pid: number): void {
    this.supersededPids.delete(pid);
    this.pidVal = pid;
  }

  /**
   * The child process exited (`agent://closed`).
   *
   * `pid` is the exited child's. A close for a session this store is no
   * longer showing is dropped — see `pidVal`'s doc for the sequence that
   * otherwise marks a live session "ended".
   */
  closed(reason: string, pid?: number): void {
    if (!this.isOurs(pid)) return;
    // Synthetic log row — session teardown must be visible in the 🐛 view.
    this.recordRaw({ type: "closed", reason, pid });
    this.statusFlag = "ended";
    this.closedReasonVal = reason;
    this.streamingFlag = false;
    this.turnActiveFlag = false;
    this.ver++;
  }

  /** Drop all messages and session state back to a fresh store. The cleared
   *  session's pid joins `supersededPids` so its in-flight events cannot
   *  touch what comes after. */
  clear(): void {
    if (this.pidVal !== undefined) this.supersededPids.add(this.pidVal);
    this.msgs = [];
    this.statusFlag = "idle";
    this.lastResultVal = undefined;
    this.sessionIdVal = undefined;
    this.verifyRunningFlag = false;
    this.closedReasonVal = undefined;
    this.alarmVal = undefined;
    this.pidVal = undefined;
    this.currentAssistantIdx = undefined;
    this.toolIndexById.clear();
    this.streamingFlag = false;
    this.turnActiveFlag = false;
    this.turnStartedAtVal = undefined;
    this.rawLog = [];
    this.sessionUsageVal = emptySessionUsage();
    this.ver++;
  }

  /** Read-time immutable-ish view (a fresh array; message objects are not cloned). */
  snapshot(): {
    messages: AgentMessage[];
    status: AgentStatus;
    lastResult?: AgentResult;
    sessionId?: string;
    verifyRunning: boolean;
    closedReason?: string;
    alarm?: AgentAlarm;
    pid?: number;
    streaming: boolean;
    turnActive: boolean;
    turnStartedAt?: number;
    rawLog: RawLogEntry[];
    sessionUsage: SessionUsage;
  } {
    return {
      messages: this.msgs.slice(),
      status: this.statusFlag,
      lastResult: this.lastResultVal,
      sessionId: this.sessionIdVal,
      verifyRunning: this.verifyRunningFlag,
      closedReason: this.closedReasonVal,
      alarm: this.alarmVal,
      pid: this.pidVal,
      streaming: this.streamingFlag,
      turnActive: this.turnActiveFlag,
      turnStartedAt: this.turnStartedAtVal,
      rawLog: this.rawLog.slice(),
      sessionUsage: this.sessionUsageVal,
    };
  }

  // ---------- event handlers ----------

  private handleSystem(subtype: string | undefined, sessionId: string | undefined): void {
    if (subtype !== "init") return; // other subtypes (status, hooks...) not modelled yet
    this.statusFlag = "running";
    if (sessionId) this.sessionIdVal = sessionId;
    this.ver++;
  }

  private handleAssistant(content: unknown): void {
    if (!Array.isArray(content)) return;
    let changed = false;

    const textBlocks = content.filter(isContentBlockText);
    if (textBlocks.length > 0) {
      // Authoritative: replaces (not appends to) whatever deltas built up
      // *for this same inference*.
      this.setAssistantText(textBlocks.map((b) => b.text).join(""));
      changed = true;
    }

    for (const block of content) {
      if (isContentBlockToolUse(block) && !this.toolIndexById.has(block.id)) {
        this.toolIndexById.set(block.id, this.msgs.length);
        this.msgs.push({
          kind: "tool",
          id: block.id,
          name: block.name,
          input: block.input,
          status: "running",
          startedAt: Date.now(),
        });
        changed = true;
        // A tool call is itself a turn boundary for assistant *text*: the
        // CLI emits one non-delta `assistant` line per model inference, and
        // a tool-using turn has one inference before the call and another
        // after the tool_result comes back. Without this reset, the next
        // inference's "authoritative" text would silently overwrite (not
        // follow) the pre-call bubble in place — see the task-6 fix report
        // for the exact repro this pins.
        this.currentAssistantIdx = undefined;
        this.streamingFlag = false; // now running a tool, not writing
      }
    }

    if (changed) this.ver++;
  }

  private handleUser(content: unknown): void {
    if (!Array.isArray(content)) return;
    let changed = false;

    for (const block of content) {
      if (!isUserContentToolResult(block)) continue;
      const idx = this.toolIndexById.get(block.tool_use_id);
      if (idx === undefined) continue;
      const msg = this.msgs[idx];
      if (msg.kind !== "tool") continue;
      msg.status = block.is_error ? "error" : "ok";
      msg.result = toResultText(block.content);
      changed = true;
    }

    if (changed) this.ver++;
  }

  private handleStreamEvent(event: { type?: string; delta?: { type?: string; text?: string } } | undefined): void {
    if (!event || event.type !== "content_block_delta") return;
    const delta = event.delta;
    if (!delta || delta.type !== "text_delta" || typeof delta.text !== "string") return;
    this.appendAssistantDelta(delta.text);
  }

  private handleResult(ev: {
    is_error?: boolean;
    total_cost_usd?: number;
    num_turns?: number;
    result?: string;
    session_id?: string;
  }): void {
    this.lastResultVal = {
      isError: !!ev.is_error,
      costUsd: ev.total_cost_usd,
      numTurns: ev.num_turns,
      text: ev.result,
    };
    if (ev.session_id) this.sessionIdVal = ev.session_id;
    // The turn's ledger line: usage when the event carried a sane one
    // (parseTurnUsage never throws), plus the tools this turn ran — the
    // tool cards between the previous user message and here, with their
    // statuses as they stand now.
    const usage = parseTurnUsage(ev) ?? undefined;
    if (usage) this.sessionUsageVal = addUsage(this.sessionUsageVal, usage);
    const tools: { name: string; status: "running" | "ok" | "error" }[] = [];
    for (let i = this.msgs.length - 1; i >= 0; i--) {
      const m = this.msgs[i];
      if (m.kind === "user") break;
      if (m.kind === "tool") tools.unshift({ name: m.name, status: m.status });
    }
    this.msgs.push({
      kind: "turn_end",
      usage,
      summary: typeof ev.result === "string" ? ev.result : undefined,
      tools,
    });
    this.currentAssistantIdx = undefined; // turn boundary
    this.streamingFlag = false;
    this.turnActiveFlag = false;
    this.ver++;
  }

  /**
   * The host refused something and stopped the session (A1 backstop / A2).
   *
   * Kept as its own field rather than pushed as a message: it must render
   * as a banner the user cannot scroll past, not as one more card in a
   * transcript. The backend has already killed the child, so the session is
   * over whether or not `agent://closed` has landed yet — say so now.
   */
  private handleAlarm(ev: { kind?: unknown; detail?: unknown }): void {
    this.alarmVal = {
      kind: typeof ev.kind === "string" ? ev.kind : "unknown",
      detail:
        typeof ev.detail === "string"
          ? ev.detail
          : "Bancada stopped this session for a safety reason it could not describe.",
    };
    this.statusFlag = "ended";
    this.verifyRunningFlag = false;
    this.turnActiveFlag = false;
    this.ver++;
  }

  private handleStderr(line: string): void {
    const last = this.msgs[this.msgs.length - 1];
    if (last && last.kind === "stderr") {
      last.line += "\n" + line;
    } else {
      this.msgs.push({ kind: "stderr", line });
    }
    this.ver++;
  }

  // ---------- assistant text accumulation ----------

  private setAssistantText(text: string): void {
    if (this.currentAssistantIdx === undefined) {
      this.currentAssistantIdx = this.msgs.length;
      this.msgs.push({ kind: "assistant", text });
      return;
    }
    const msg = this.msgs[this.currentAssistantIdx];
    if (msg.kind === "assistant") msg.text = text;
  }

  private appendAssistantDelta(delta: string): void {
    this.streamingFlag = true;
    if (this.currentAssistantIdx === undefined) {
      this.currentAssistantIdx = this.msgs.length;
      this.msgs.push({ kind: "assistant", text: delta });
    } else {
      const msg = this.msgs[this.currentAssistantIdx];
      if (msg.kind === "assistant") msg.text += delta;
    }
    this.ver++;
  }
}

// ---------- content-block type guards ----------

function isContentBlockText(b: unknown): b is ContentBlockText {
  return (
    typeof b === "object" &&
    b !== null &&
    (b as { type?: unknown }).type === "text" &&
    typeof (b as { text?: unknown }).text === "string"
  );
}

function isContentBlockToolUse(b: unknown): b is ContentBlockToolUse {
  return (
    typeof b === "object" &&
    b !== null &&
    (b as { type?: unknown }).type === "tool_use" &&
    typeof (b as { id?: unknown }).id === "string" &&
    typeof (b as { name?: unknown }).name === "string"
  );
}

function isUserContentToolResult(b: unknown): b is UserContentToolResult {
  return (
    typeof b === "object" &&
    b !== null &&
    (b as { type?: unknown }).type === "tool_result" &&
    typeof (b as { tool_use_id?: unknown }).tool_use_id === "string"
  );
}

/**
 * Flatten a tool_result `content` field to the text a card should show.
 *
 * Anthropic's format allows either a plain string or an array of content
 * blocks, and **both shapes occur here**: the CLI's own file tools relay a
 * string, while an MCP tool result arrives as the block array the server
 * sent — and bancada's own `mcp::tool_result_json` sends exactly
 * `[{"type":"text","text":"success: true\nexit_code: 0\n\n..."}]`.
 *
 * `JSON.stringify`ing the array case (what this used to do unconditionally)
 * therefore handed `parseVerifyResult` a JSON blob whose first line is
 * `[{"type":"text",...` — no `success:` line, so every Verify card in the
 * panel degraded to a bare "Verify finished" with a garbled summary. That is
 * the centrepiece of the whole feature, so the array case is unwrapped
 * first; `JSON.stringify` remains only as the last-resort rendering for a
 * shape that is neither.
 */
function toResultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    const texts = content
      .filter(
        (b): b is { type: string; text: string } =>
          typeof b === "object" &&
          b !== null &&
          typeof (b as { text?: unknown }).text === "string",
      )
      .map((b) => b.text);
    // Only when *every* block carried text: a mixed array (an image block
    // among them, say) would otherwise silently drop content.
    if (texts.length > 0 && texts.length === content.length) {
      return texts.join("\n");
    }
  }
  try {
    return JSON.stringify(content);
  } catch {
    return String(content);
  }
}
