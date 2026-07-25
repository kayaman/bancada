import type { DetectedPort } from "./api";

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
