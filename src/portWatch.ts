import type { FleetSnapshot } from "./api";
import { BRIDGE_TITLE, fleetDisplayName } from "./ports";

/**
 * Unidentified (bridge) ports newly attached since the previous sync.
 *
 * A CH343/CP210x board carries no board identity, so it can never appear in
 * `snap.online` — without this, plugging one in produced no feedback at all.
 * Same first-sync rule as `arrivals`: `null` means launch, announce nothing.
 */
export function bridgeArrivals(
  prevAddrs: string[] | null,
  snap: FleetSnapshot,
): Arrival[] {
  if (prevAddrs === null) return [];
  const seen = new Set(prevAddrs);
  return snap.unidentified
    .filter((p) => !seen.has(p.port.address))
    .map((p) => ({
      id: p.port.address,
      // Not `protocol_label`: it is "Serial Port (USB)" for every bridge on
      // the bench, so two arrivals announced themselves identically. The
      // shared title at least matches what the picker calls them.
      name: BRIDGE_TITLE,
      port: p.port.address,
    }));
}

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
        // The shared chain, not a fourth copy of it — chip_type was the rung
        // this one used to skip.
        name: b ? fleetDisplayName(b) : id,
        port: b?.last_port ?? null,
      };
    });
}
