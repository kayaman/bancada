// What the assistant is doing *right now*, for the panel's footer strip.
// Pure over store state so the priority order is testable: an active
// verify outranks a running tool outranks streamed text outranks plain
// "thinking…". Null when the session isn't live OR no turn is in flight —
// the panel falls back to its static status labels ("Ready" between turns).

import type { AgentMessage, AgentStatus } from "./agentStore";

export interface ActivityInput {
  status: AgentStatus;
  verifyRunning: boolean;
  uploadRunning: boolean;
  streaming: boolean;
  /** A turn is in flight (userSent → result/close/alarm). Off between
   *  turns, so the footer can say "Ready" instead of a phantom
   *  "thinking…" while the CLI merely sits alive awaiting input. */
  turnActive: boolean;
  messages: AgentMessage[];
  turnStartedAt?: number;
  /** Injected clock so this stays a pure, testable function. */
  now?: number;
}

/** A short human hint for a tool's target: full file path or search pattern.
 *  Full path (not basename) — the footer's CSS ellipsis handles overflow,
 *  and "which soil.ino" is exactly the question a debugger is asking. */
function toolHint(name: string, input: unknown): string {
  if (typeof input !== "object" || input === null) return "";
  const i = input as Record<string, unknown>;
  if (
    (name === "Edit" || name === "Write" || name === "Read") &&
    typeof i.file_path === "string"
  ) {
    return i.file_path;
  }
  if ((name === "Grep" || name === "Glob") && typeof i.pattern === "string") {
    return i.pattern;
  }
  return "";
}

/** " 12s" once an activity is at least a second old; "" otherwise. */
function elapsed(since: number | undefined, now: number | undefined): string {
  if (since === undefined || now === undefined) return "";
  const s = Math.floor((now - since) / 1000);
  return s >= 1 ? ` ${s}s` : "";
}

export function activityLabel(a: ActivityInput): string | null {
  if (a.status !== "running" && a.status !== "starting") return null;
  // A flash outranks a compile: upload's build phase can raise both flags,
  // and "flashing" is the one the user must not interrupt.
  if (a.uploadRunning) return "📡 upload (flashing)…";
  if (a.verifyRunning) return "🔨 verify (compiling)…";
  if (!a.turnActive) return null;
  for (let i = a.messages.length - 1; i >= 0; i--) {
    const m = a.messages[i];
    if (m.kind === "tool" && m.status === "running") {
      const hint = toolHint(m.name, m.input);
      const label = hint ? `⚙ ${m.name} ${hint}…` : `⚙ ${m.name}…`;
      return label + elapsed(m.startedAt, a.now);
    }
  }
  if (a.streaming) return `✍ writing…${elapsed(a.turnStartedAt, a.now)}`;
  return `thinking…${elapsed(a.turnStartedAt, a.now)}`;
}
