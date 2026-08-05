import type { DetectedPort, MatchingBoard } from "./api";

/**
 * The board identity a user (or a compile) should see for a port.
 *
 * arduino-cli lists umbrella entries like `esp32:esp32:esp32_family` first
 * and flags them hidden; taking `[0]` hands that pseudo-FQBN to the
 * compiler. A bridge port (CH343/CP210x) has no matching boards at all.
 */
export function visibleBoard(p: DetectedPort): MatchingBoard | null {
  return p.matching_boards.find((b) => !b.is_hidden) ?? null;
}

export interface PortOption {
  address: string;
  label: string;
  /** True for a selected port that is no longer attached (e.g. pinned in
   *  sketch.yaml) — rendered so the select is never blank. */
  missing: boolean;
}

/** Options for the toolbar port picker, covering an absent selection. */
export function portOptions(
  ports: DetectedPort[],
  selected: string | null,
): PortOption[] {
  const opts = ports.map((p) => {
    const b = visibleBoard(p);
    return {
      address: p.port.address,
      label: b ? `${p.port.address} (${b.name})` : p.port.address,
      missing: false,
    };
  });
  if (selected && !ports.some((p) => p.port.address === selected)) {
    opts.push({
      address: selected,
      label: `${selected} (not attached)`,
      missing: true,
    });
  }
  return opts;
}

/**
 * Which port should be selected after a rescan.
 *
 * Keeps the user's choice while that port is still attached — a rescan must
 * never silently retarget an upload at a different board. But a choice that has
 * *disappeared* is dropped rather than kept: leaving a vanished port selected
 * makes a refresh look like it did nothing, and the next Upload would aim at a
 * device that is no longer there.
 *
 * The fallback prefers a serial port because that is what can be flashed and
 * monitored; a network port is only ever chosen when it is the only thing
 * attached or the user picked it deliberately.
 */
export function nextSelectedPort(
  ports: DetectedPort[],
  current: string | null,
): string | null {
  if (current && ports.some((p) => p.port.address === current)) return current;
  return (
    ports.find((p) => p.port.protocol === "serial")?.port.address ??
    ports[0]?.port.address ??
    null
  );
}
