// What the assistant is doing *right now*, for the panel's footer strip.
// Pure over store state so the priority order is testable: an active
// verify outranks a running tool outranks streamed text outranks plain
// "thinking…". Null when the session isn't live — the panel falls back to
// its static status labels.

import type { AgentMessage, AgentStatus } from "./agentStore";

export interface ActivityInput {
  status: AgentStatus;
  verifyRunning: boolean;
  streaming: boolean;
  messages: AgentMessage[];
}

/** A short human hint for a tool's target: file basename or search pattern. */
function toolHint(name: string, input: unknown): string {
  if (typeof input !== "object" || input === null) return "";
  const i = input as Record<string, unknown>;
  if (
    (name === "Edit" || name === "Write" || name === "Read") &&
    typeof i.file_path === "string"
  ) {
    return i.file_path.split("/").pop() ?? "";
  }
  if ((name === "Grep" || name === "Glob") && typeof i.pattern === "string") {
    return i.pattern;
  }
  return "";
}

export function activityLabel(a: ActivityInput): string | null {
  if (a.status !== "running" && a.status !== "starting") return null;
  if (a.verifyRunning) return "🔨 verify (compiling)…";
  for (let i = a.messages.length - 1; i >= 0; i--) {
    const m = a.messages[i];
    if (m.kind === "tool" && m.status === "running") {
      const hint = toolHint(m.name, m.input);
      return hint ? `⚙ ${m.name} ${hint}…` : `⚙ ${m.name}…`;
    }
  }
  if (a.streaming) return "✍ writing…";
  return "thinking…";
}
