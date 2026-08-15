// Whether to offer the user their board's project back, and which one.
//
// Extracted because this is all judgement and no rendering: four separate
// reasons to stay quiet, and getting any of them wrong is a feature that
// nags. `vitest` runs in the node environment and renders no component, so a
// decision left in the banner would be verified by eye alone.

import type { FleetSnapshot, FlashRecord } from "./api";
import { fleetDisplayName } from "./ports";

export interface BoardOffer {
  /** Fleet id — the key for dismissing, and stable across replug. */
  boardId: string;
  /** What to call the board: the nickname, then the chain in `ports.ts`. */
  title: string;
  rec: FlashRecord;
}

/**
 * The project offer for a board that is plugged in right now, or null.
 *
 * Computed from the **snapshot** rather than from `portWatch.arrivals`, and
 * that is deliberate: `arrivals` suppresses everything present at launch, but
 * sitting down, plugging in, and *then* opening Bancada is the normal bench
 * order — keyed off arrivals this would almost never fire. The `⚡ attached`
 * toast keeps the arrivals rule; only this offer ignores it.
 *
 * Four reasons to stay quiet, in order:
 *
 * 1. the board carries no flash record — nothing to offer;
 * 2. its project is already open — the answer is "you're in it";
 * 3. it was dismissed this session — asked and answered;
 * 4. it was flashed moments ago — a flash resets the board and it
 *    re-enumerates, so without this the act of flashing would offer you the
 *    project you are already working in, every single time.
 *
 * When several boards qualify, the most recently seen wins. Two boards with
 * two projects is a real bench, but two banners is not something to build.
 */
export function boardOffer(
  fleet: FleetSnapshot | null,
  openSketchDir: string | null,
  dismissed: ReadonlySet<string>,
  suppressed: ReadonlySet<string>,
): BoardOffer | null {
  if (!fleet) return null;
  const online = new Set(fleet.online);

  const candidates = fleet.boards.filter(
    (b) =>
      online.has(b.id) &&
      b.last_flash != null &&
      b.last_flash.project_dir !== openSketchDir &&
      !dismissed.has(b.id) &&
      !suppressed.has(b.id),
  );
  if (candidates.length === 0) return null;

  const best = candidates.reduce((a, b) => (b.last_seen > a.last_seen ? b : a));
  return {
    boardId: best.id,
    title: fleetDisplayName(best),
    // Non-null by the filter above; narrowing does not survive it.
    rec: best.last_flash as FlashRecord,
  };
}

/**
 * How the project has moved since the board was flashed.
 *
 * `drift` is the commit count from `git_project_drift`; `null` means the
 * recorded commit could not be found — a deleted tag, a re-cloned repo, a
 * force-push — which is a different answer from "identical" and is worded
 * differently.
 */
export function driftLabel(drift: number | null, tag: string): string {
  if (drift === null) return `${tag} is no longer in this repository`;
  if (drift === 0) return `still at ${tag}`;
  return `${drift} commit${drift === 1 ? "" : "s"} ahead of ${tag}`;
}
