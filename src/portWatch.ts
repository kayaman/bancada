import type { FleetSnapshot } from "./api";

/** A board that came online since the previous fleet sync. */
export interface Arrival {
  id: string;
  /** Nickname when set, else the board model, else the raw id. */
  name: string;
  port: string | null;
}

/**
 * Boards newly online in `snap` relative to the previous sync.
 *
 * `prevOnline === null` marks the first sync after launch: boards already
 * plugged in when the app started are not "arrivals", so none are announced.
 */
export function arrivals(
  prevOnline: string[] | null,
  snap: FleetSnapshot,
): Arrival[] {
  if (prevOnline === null) return [];
  const seen = new Set(prevOnline);
  return snap.online
    .filter((id) => !seen.has(id))
    .map((id) => {
      const b = snap.boards.find((e) => e.id === id);
      return {
        id,
        name: b?.nickname ?? b?.board_name ?? id,
        port: b?.last_port ?? null,
      };
    });
}
