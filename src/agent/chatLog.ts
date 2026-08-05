// Chat-history recorder/replayer for the Assistant panel (see
// docs/superpowers/specs/2026-08-05-assistant-chat-history-design.md). A
// chat is persisted as a store-operation log: one NDJSON line per mutating
// `AgentStore` call, so replaying the lines through a fresh store reproduces
// the exact live rendering — no second message schema to version. Raw events
// alone would not be enough: user bubbles enter via `userSent`, a local
// call, never as an `agent://event`.
//
// Contracts this module keeps:
// - Recording is fire-and-forget (the `fleetSync` philosophy): a failed or
//   even synchronously-throwing `send` must never break a live chat, so
//   `record` swallows everything.
// - Empty sessions never write: `start` only arms the recorder; the meta
//   line is flushed lazily right before the first recorded op.
// - No clock reads in here — `chatFileName` takes the Date, `start` takes
//   the timestamps, per the plan's global constraint. App.tsx owns "now".
// - `replayChat` skips corrupt/unknown lines silently: a history file is
//   untrusted input from disk, and one bad line must not hide a whole chat.

import { AgentStore } from "./agentStore";
import type { AgentEvent } from "./types";

/** One recorded store operation — exactly one NDJSON line. */
export type ChatOp =
  | { op: "meta"; sketchDir: string; startedAt: string }
  | { op: "sessionStarted"; pid: number }
  | { op: "userSent"; text: string }
  | { op: "push"; ev: unknown }
  | { op: "closed"; reason: string; pid?: number };

/**
 * The chat's on-disk filename for a session starting at `now` — LOCAL time,
 * `"2026-08-05T14-02-31.ndjson"`. Dashes, not colons, in the time part:
 * these are validated basenames on the Rust side (no separators), and
 * lexicographic order must equal chronological order for newest-first
 * listing.
 */
export function chatFileName(now: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return (
    `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}` +
    `T${p(now.getHours())}-${p(now.getMinutes())}-${p(now.getSeconds())}.ndjson`
  );
}

/**
 * Replay a chat's NDJSON lines into a fresh `AgentStore`, which the panel
 * renders read-only with the same MessageView components as a live chat.
 * Corrupt JSON, non-object lines, and unknown ops are skipped — never
 * thrown on (`push` itself already ignores unknown event types).
 */
export function replayChat(lines: string[]): AgentStore {
  const store = new AgentStore();
  for (const line of lines) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch {
      continue; // corrupt line — skip, keep the rest of the chat
    }
    if (typeof parsed !== "object" || parsed === null) continue;
    const op = parsed as Record<string, unknown>;
    switch (op.op) {
      case "sessionStarted":
        if (typeof op.pid === "number") store.sessionStarted(op.pid);
        break;
      case "userSent":
        if (typeof op.text === "string") store.userSent(op.text);
        break;
      case "push":
        store.push(op.ev as AgentEvent);
        break;
      case "closed":
        store.closed(
          typeof op.reason === "string" ? op.reason : "",
          typeof op.pid === "number" ? op.pid : undefined,
        );
        break;
      default:
        // "meta" and anything a future version might write — ignore.
        break;
    }
  }
  return store;
}

/**
 * Owns the current chat's filename and forwards each recorded op as one
 * serialized line to an injected `send` (App.tsx passes
 * `(file, line) => api.chatAppend(sketchDir, file, line)` — injection keeps
 * this module free of Tauri and testable with a spy).
 */
export class ChatRecorder {
  private fileName?: string;
  private send?: (file: string, line: string) => Promise<void>;
  /** The meta line, held back until the first op so empty sessions never write. */
  private pendingMeta?: ChatOp;

  /** Is a chat being recorded (started and not yet stopped)? */
  get active(): boolean {
    return this.fileName !== undefined;
  }

  /**
   * Arm the recorder for a new chat file. Writes nothing yet — the meta
   * line is emitted just before the first `record`ed op.
   */
  start(
    fileName: string,
    meta: { sketchDir: string; startedAt: string },
    send: (file: string, line: string) => Promise<void>,
  ): void {
    this.fileName = fileName;
    this.send = send;
    this.pendingMeta = { op: "meta", ...meta };
  }

  /**
   * Record one op. No-op unless started. Never throws and never rejects:
   * persistence failing is strictly worse-is-nothing — the live chat flow
   * must not notice.
   */
  record(op: ChatOp): void {
    if (this.fileName === undefined || this.send === undefined) return;
    if (this.pendingMeta !== undefined) {
      const meta = this.pendingMeta;
      this.pendingMeta = undefined;
      this.emit(meta);
    }
    this.emit(op);
  }

  /** Forget the current file; subsequent `record`s are no-ops until `start`. */
  stop(): void {
    this.fileName = undefined;
    this.send = undefined;
    this.pendingMeta = undefined;
  }

  private emit(op: ChatOp): void {
    if (this.fileName === undefined || this.send === undefined) return;
    try {
      // JSON.stringify can throw (circular `ev`), and a hostile `send` can
      // throw synchronously — both are this module's problem, not the chat's.
      this.send(this.fileName, JSON.stringify(op)).catch(() => {});
    } catch {
      // fire-and-forget: swallowed by design
    }
  }
}
