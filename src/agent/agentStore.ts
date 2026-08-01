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
// doesn't recognise — an unknown `type` is ignored outright (no version
// bump, per the task brief).

import type {
  AgentEvent,
  ContentBlockText,
  ContentBlockToolUse,
  UserContentToolResult,
} from "./types";

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
    }
  | { kind: "stderr"; line: string };

export interface AgentResult {
  isError: boolean;
  costUsd?: number;
  numTurns?: number;
  text?: string;
}

export type AgentStatus = "idle" | "starting" | "running" | "ended";

export class AgentStore {
  private msgs: AgentMessage[] = [];
  private ver = 0;
  private statusFlag: AgentStatus = "idle";
  private lastResultVal?: AgentResult;
  private sessionIdVal?: string;
  private verifyRunningFlag = false;
  private closedReasonVal?: string;

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

  /** Ingest one `agent://event` payload. Unknown `type`s are ignored. */
  push(ev: AgentEvent): void {
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
      default:
        // Unrecognised type (or missing `type`) — ignore, bump nothing.
        return;
    }
  }

  /** Record a user-authored message. User messages never come from events. */
  userSent(text: string): void {
    if (this.statusFlag === "idle") this.statusFlag = "starting";
    this.msgs.push({ kind: "user", text });
    this.currentAssistantIdx = undefined; // turn boundary
    this.ver++;
  }

  /** The child process exited (`agent://closed`). */
  closed(reason: string): void {
    this.statusFlag = "ended";
    this.closedReasonVal = reason;
    this.ver++;
  }

  /** Drop all messages and session state back to a fresh store. */
  clear(): void {
    this.msgs = [];
    this.statusFlag = "idle";
    this.lastResultVal = undefined;
    this.sessionIdVal = undefined;
    this.verifyRunningFlag = false;
    this.closedReasonVal = undefined;
    this.currentAssistantIdx = undefined;
    this.toolIndexById.clear();
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
  } {
    return {
      messages: this.msgs.slice(),
      status: this.statusFlag,
      lastResult: this.lastResultVal,
      sessionId: this.sessionIdVal,
      verifyRunning: this.verifyRunningFlag,
      closedReason: this.closedReasonVal,
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
    this.currentAssistantIdx = undefined; // turn boundary
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

/** The tool_result `content` field is either a plain string or a content-block
 * array (Anthropic's format) — agentStore only ever displays it, never
 * inspects it further. */
function toResultText(content: unknown): string {
  if (typeof content === "string") return content;
  try {
    return JSON.stringify(content);
  } catch {
    return String(content);
  }
}
