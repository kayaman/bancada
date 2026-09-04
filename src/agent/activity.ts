// What the assistant is doing *right now* — the single vocabulary behind both
// the Assistant panel's footer and the status bar's agent segment.
//
// The old version answered one question ("what is it doing?") and returned
// null for everything else, leaving the panel to fall back to a second set of
// static labels. Two half-vocabularies meant the most important question had
// no owner at all: *is it working, or has it stopped?* A turn that wedges
// looked exactly like a turn that is thinking hard — both read "thinking… 240s"
// on a line that never moves.
//
// So `agentActivity` returns a struct whose `phase` is total: every moment of
// a session lands in exactly one of offline/starting/ready/working/stalled/
// ended, and callers render the certainty (a pip) separately from the prose.
// `formatAgentActivity` turns the struct into the one line both callers show,
// so the panel and the status bar can never drift apart.
//
// Pure, with `now` injected — a running clock stays testable without timers.

import type { AgentMessage, AgentStatus } from "./agentStore";
import { formatElapsed } from "../statusLine";
import { shortToolName } from "./mcpTool";

/**
 * How long the CLI may say nothing before the line admits it. Not a verdict
 * that the session is dead — extended thinking and a long `Bash` are both
 * legitimately silent — but the difference between a clock that is counting
 * and a clock that is counting while nothing arrives. Same discipline as
 * `statusLine.ts`'s "usually ~": report the fact, don't claim the conclusion.
 */
export const STALL_AFTER_MS = 20_000;

export type AgentPhase =
  | "offline"
  | "starting"
  | "ready"
  | "working"
  | "stalled"
  | "ended";

export interface AgentActivity {
  phase: AgentPhase;
  /** The certainty word, always present: "Working", "Ready", "Not started"… */
  state: string;
  /** What it is doing, when it is doing something: "⚙ Read /x/soil.ino". */
  detail?: string;
  /** Tools run so far in this turn — the bit that shows *progress*, not just
   *  duration, across a long turn. Absent before the first tool. */
  step?: number;
  /** `M:SS` for whatever `detail` names. */
  elapsed?: string;
  /** "no output for M:SS" — only once past `STALL_AFTER_MS`. */
  stale?: string;
}

export interface ActivityInput {
  status: AgentStatus;
  verifyRunning: boolean;
  uploadRunning: boolean;
  streaming: boolean;
  /** A turn is in flight (userSent → result/close/alarm). Off between turns,
   *  so the line can say "Ready" instead of a phantom "thinking". */
  turnActive: boolean;
  messages: AgentMessage[];
  turnStartedAt?: number;
  /** When the last `agent://event` of any kind landed — the store already
   *  stamps every one of them into its raw log, so this needs no new state.
   *  Drives staleness. */
  lastEventAt?: number;
  /** Injected clock so this stays a pure, testable function. */
  now?: number;
}

/** A short human hint for a tool's target. Full path (not basename) — the
 *  CSS ellipsis handles overflow, and "which soil.ino" is exactly the
 *  question a debugger is asking. */
function toolHint(input: unknown): string {
  if (typeof input !== "object" || input === null) return "";
  const i = input as Record<string, unknown>;
  // Ordered by how much the field narrows down "what is it touching": a path
  // or a command beats a pattern beats a port. `query` sits with them because
  // for a documentation search it is the entire subject — without it the line
  // reads "⚙ search_espressif_sources" for every one of a run of them, which
  // says the assistant is busy but not what it is busy *with*.
  for (const key of ["file_path", "command", "query", "pattern", "url", "port"]) {
    const v = i[key];
    if (typeof v === "string" && v !== "") return v;
  }
  return "";
}

/** The newest still-running tool, or undefined. */
function runningTool(messages: AgentMessage[]) {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.kind === "tool" && m.status === "running") return m;
  }
  return undefined;
}

/** Tools started since the last user message — this turn's step count. */
function stepsThisTurn(messages: AgentMessage[]): number {
  let n = 0;
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.kind === "user") break;
    if (m.kind === "tool") n++;
  }
  return n;
}

function since(from: number | undefined, now: number | undefined) {
  return from === undefined || now === undefined ? undefined : now - from;
}

export function agentActivity(a: ActivityInput): AgentActivity {
  if (a.status === "idle") return { phase: "offline", state: "Not started" };
  if (a.status === "ended") return { phase: "ended", state: "Session ended" };

  // Upload and verify outrank the turn gate on purpose: an MCP-driven flash
  // can outlive the turn that asked for it, and "flashing" is the one state
  // the user must not interrupt.
  const building = a.uploadRunning || a.verifyRunning;
  if (!building && !a.turnActive) {
    return a.status === "starting"
      ? { phase: "starting", state: "Starting…" }
      : { phase: "ready", state: "Ready" };
  }

  const tool = building ? undefined : runningTool(a.messages);
  const detail = a.uploadRunning
    ? "📡 upload (flashing)"
    : a.verifyRunning
      ? "🔨 verify (compiling)"
      : tool
        ? // The short name for an MCP tool: `mcp__espressif-docs__search_…`
          // spends the line's whole width on a prefix that is the same for
          // every call, crowding out the hint that actually differs.
          `⚙ ${shortToolName(tool.name)}${toolHint(tool.input) ? ` ${toolHint(tool.input)}` : ""}`
        : a.streaming
          ? "✍ writing"
          : "thinking";

  // A build has no start time on the snapshot, and the turn's clock is not
  // its clock — a turn minutes old can be seconds into a compile. Rather than
  // print a duration that is false, print none; the status bar's own
  // "Assistant compiling… 0:12" already owns that number.
  const elapsedMs = building
    ? undefined
    : tool
      ? since(tool.startedAt, a.now)
      : since(a.turnStartedAt, a.now);

  // Quiet is measured from the LATER of the turn's start and the last event:
  // a fresh turn whose first event has not landed yet must not inherit the
  // previous turn's timestamp and read as instantly stalled. A silent
  // compiler is normal, so a build is never called stalled.
  const quietFrom = Math.max(a.turnStartedAt ?? -Infinity, a.lastEventAt ?? -Infinity);
  const quietMs = building || quietFrom === -Infinity
    ? undefined
    : since(quietFrom, a.now);
  const stalled = quietMs !== undefined && quietMs >= STALL_AFTER_MS;

  const steps = a.turnActive ? stepsThisTurn(a.messages) : 0;

  return {
    phase: stalled ? "stalled" : "working",
    state: "Working",
    detail,
    ...(steps > 0 ? { step: steps } : {}),
    ...(elapsedMs !== undefined ? { elapsed: formatElapsed(elapsedMs) } : {}),
    ...(stalled ? { stale: `no output for ${formatElapsed(quietMs)}` } : {}),
  };
}

/**
 * The line split at its one shrinkable seam.
 *
 * Both callers live on a crowded row and must ellipsis somewhere. Left to a
 * single text node the browser cuts from the right, which sacrifices exactly
 * the wrong things: the elapsed clock and the "no output for M:SS" caveat —
 * the two facts this strip exists to deliver — while a file path nobody is
 * squinting at survives in full. So the middle is the only part allowed to
 * shrink: `head` names the subject, `tail` carries the numbers, and the
 * detail (a path, a command, a URL) yields first.
 *
 * `state` overrides the struct's own word for a caller that needs a subject
 * instead of a verb — "Assistant" on the status bar, where "Working" alone
 * would not say working at *what*.
 *
 * Separators live inside the parts, so concatenating them is exactly
 * `formatAgentActivity` — the two can never drift.
 */
export function agentActivityParts(
  a: AgentActivity,
  state: string = a.state,
): { head: string; detail: string; tail: string } {
  const tail = [
    a.step !== undefined ? `step ${a.step}` : undefined,
    a.elapsed,
    a.stale,
  ].filter((p): p is string => p !== undefined);
  return {
    head: state,
    detail: a.detail === undefined ? "" : ` · ${a.detail}`,
    tail: tail.length === 0 ? "" : ` · ${tail.join(" · ")}`,
  };
}

/** The whole line as one string — for `title` tooltips and for anything that
 *  wants the text without the layout. Defined through `agentActivityParts`
 *  so the rendered spans and this can never disagree. */
export function formatAgentActivity(a: AgentActivity, state?: string): string {
  const p = agentActivityParts(a, state);
  return p.head + p.detail + p.tail;
}
