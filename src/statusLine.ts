// The status bar's whole vocabulary: what one line of text says at any moment,
// and what shape the progress bar above it takes. Pure — `now` is injected, so
// a running clock is testable without a timer.
//
// The old bar was a single `useState` string plus a bare `⏳` while busy. It
// answered "something is happening" and nothing else: not what, not for how
// long, not whether the last thing worked. The tiers below are ordered by what
// a person at the bench actually wants to know, most urgent first:
//
//   1. something is running now — say what, and how long it has been going;
//   2. nothing is running but something just finished — say whether it worked;
//   3. nothing has happened yet — say what is loaded and where it will flash.

export type ActivityKey =
  | "compile"
  | "upload"
  | "sync"
  | "remote"
  | "firmware"
  | "agent_compile"
  | "agent_upload";

export interface Activity {
  key: ActivityKey;
  /** Present participle, shown verbatim: "Compiling", "Uploading". */
  label: string;
  startedAt: number;
}

export interface LastResult {
  ok: boolean;
  /** Noun form, shown verbatim: "Compile", "Upload". */
  label: string;
  durationMs: number;
  at: number;
}

/** Elapsed ms → `M:SS`. Floors (a clock that rounds up reads as finished
 *  before it is), and never goes negative — a clock adjustment mid-build
 *  must not print `-1:-3`. */
export function formatElapsed(ms: number): string {
  const total = ms > 0 ? Math.floor(ms / 1000) : 0;
  const secs = total % 60;
  return `${Math.floor(total / 60)}:${String(secs).padStart(2, "0")}`;
}

/** The string the initial `useState` has always carried. Kept here so the
 *  bottom tier of the ladder is one thing, not two copies. */
export const IDLE_GREETING = "Bancada ready — open a project folder.";

export function statusLineText(i: {
  activity: Activity | null;
  now: number;
  lastResult: LastResult | null;
  project: string | null;
  portName: string | null;
  /** How long this op took last time, from `buildHistory`. */
  estimateMs?: number | null;
}): { text: string; isError: boolean } {
  if (i.activity) {
    let text = `${i.activity.label} ${formatElapsed(i.now - i.activity.startedAt)}`;
    // "usually" and not "of" or a percentage: the number is one remembered
    // run on this machine, so it is a habit, not a prediction. Saying it that
    // way costs nothing and stops the bar lying when the build is slower.
    if (typeof i.estimateMs === "number")
      text += ` (usually ~${formatElapsed(i.estimateMs)})`;
    return { text, isError: false };
  }

  if (i.lastResult) {
    const { ok, label, durationMs } = i.lastResult;
    // A failure points at the Build panel: the bar has room for the verdict,
    // never for the compiler's reason.
    return {
      text: ok
        ? `✓ ${label} in ${formatElapsed(durationMs)}`
        : `✗ ${label} after ${formatElapsed(durationMs)} — see Build`,
      isError: !ok,
    };
  }

  if (i.project)
    return {
      text: `${i.project} · ${i.portName ?? "no port"}`,
      isError: false,
    };

  return { text: IDLE_GREETING, isError: false };
}

/** How much the bar is allowed to claim it knows.
 *  - `measured`   — parsed out of the uploader's own output;
 *  - `estimate`   — elapsed against a remembered duration (drawn dashed);
 *  - `indeterminate` — busy, but nothing to go on (drawn sliding);
 *  - `none`       — at rest, drawn as nothing at all. */
export type ProgressMode = "measured" | "estimate" | "indeterminate" | "none";

const clamp01 = (n: number) => (n < 0 ? 0 : n > 1 ? 1 : n);

export function progressMode(
  busy: boolean,
  measuredFraction: number | null,
  estimateFraction: number | null,
): { mode: ProgressMode; fraction: number } {
  if (!busy) return { mode: "none", fraction: 0 };
  if (typeof measuredFraction === "number")
    return { mode: "measured", fraction: clamp01(measuredFraction) };
  if (typeof estimateFraction === "number")
    return { mode: "estimate", fraction: clamp01(estimateFraction) };
  return { mode: "indeterminate", fraction: 0 };
}
