// Remembered build durations, per project. The one thing that turns an
// indeterminate spinner into a bar with a number on it for boards whose
// uploader says nothing useful (avrdude) and for the compile phase, which has
// no progress output at all.
//
// Storage is injected rather than reaching for `localStorage`, and so is
// `now`, for the same reason the Rust side does it (conventions §1, "the
// injection corollary"): a module that reads a clock or a global cannot be
// tested for the boundary it actually gets wrong.
//
// The value is a habit, not a prediction — one previous run on this machine.
// That is why `estimateFraction` refuses to reach 1: a bar that sits at 100%
// while the build is still going is worse than one that sits at 95%.

export interface KVStorage {
  getItem(k: string): string | null;
  setItem(k: string, v: string): void;
}

export const BUILD_HISTORY_KEY = "bancada.buildDurations";

export interface ProjectDurations {
  compileMs?: number;
  uploadMs?: number;
  /** Epoch-ms of the last build; the pruning key. */
  updatedAt: number;
}

/** Beyond this the store is someone's whole project history, and none of it
 *  is worth a byte of quota — the oldest entries go. */
const MAX_PROJECTS = 50;

const isDurations = (v: unknown): v is ProjectDurations =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v) &&
  typeof (v as ProjectDurations).updatedAt === "number";

/** Every project's record, or an empty map when the blob is absent or
 *  unreadable. Malformed storage is never fatal: it is someone else's key
 *  collision or a half-written value, and the next build overwrites it. */
function readAll(storage: KVStorage): Record<string, ProjectDurations> {
  let raw: string | null;
  try {
    raw = storage.getItem(BUILD_HISTORY_KEY);
  } catch {
    return {};
  }
  if (raw === null) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed))
    return {};

  const out: Record<string, ProjectDurations> = {};
  for (const [k, v] of Object.entries(parsed as Record<string, unknown>))
    if (isDurations(v)) out[k] = v;
  return out;
}

export function loadDurations(
  storage: KVStorage,
  sketchDir: string,
): ProjectDurations | null {
  return readAll(storage)[sketchDir] ?? null;
}

/** Record how long an op took, keeping only the latest value per op. */
export function recordDuration(
  storage: KVStorage,
  sketchDir: string,
  op: "compile" | "upload",
  ms: number,
  now: number,
): void {
  const all = readAll(storage);
  const prev = all[sketchDir];
  all[sketchDir] = {
    ...prev,
    [op === "compile" ? "compileMs" : "uploadMs"]: ms,
    updatedAt: now,
  };

  // Prune by last build, not by insertion order: a project you return to
  // after a month is not stale, it is the one you are working on.
  const keys = Object.keys(all);
  if (keys.length > MAX_PROJECTS) {
    keys
      .sort((a, b) => all[a].updatedAt - all[b].updatedAt)
      .slice(0, keys.length - MAX_PROJECTS)
      .forEach((k) => delete all[k]);
  }

  try {
    storage.setItem(BUILD_HISTORY_KEY, JSON.stringify(all));
  } catch {
    // Quota, or a private-mode storage that refuses writes. A forgotten
    // duration costs a progress bar, never a build.
  }
}

/** Elapsed against a remembered duration, capped at 0.95 so the bar cannot
 *  claim to be finished on the strength of a guess. Null when there is no
 *  usable estimate — which is the signal to draw an indeterminate bar. */
export function estimateFraction(
  elapsedMs: number,
  estimateMs: number | null | undefined,
): number | null {
  if (typeof estimateMs !== "number" || !(estimateMs > 0)) return null;
  return Math.max(0, Math.min(elapsedMs / estimateMs, 0.95));
}
