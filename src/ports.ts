import type { DetectedPort, FleetEntry, FleetSnapshot, MatchingBoard } from "./api";

/** Separator between a port's name and its device path. */
const SEP = " · ";

/** What a serial port with no board-specific identity is called. */
export const BRIDGE_TITLE = "USB serial bridge";

/**
 * Join a board's name to a device path in the standard order — name first,
 * path second. The path is dropped when there isn't one rather than rendering
 * an empty tail.
 */
export function withPath(title: string, address: string | null): string {
  return address ? `${title}${SEP}${address}` : title;
}

/** The fleet entry currently sitting on `address`, if the fleet knows one.
 *
 *  Matched on `last_port` **and** confirmed online: `last_port` is a memory of
 *  where a board was seen, and the kernel reuses `ttyACM0` for whatever is
 *  plugged in next. Without the online check a nickname would migrate onto
 *  whichever board inherited the path. */
function entryOn(address: string, fleet?: FleetSnapshot | null): FleetEntry | null {
  if (!fleet) return null;
  const online = new Set(fleet.online);
  return fleet.boards.find((b) => b.last_port === address && online.has(b.id)) ?? null;
}

/**
 * What a remembered board is called. **Mirrors
 * `core::fleet::FleetEntry::display_name`** — every rung, in the same order.
 *
 * Three copies of this chain had drifted apart: the fleet card kept all four,
 * the arrival toast dropped `chip_type`, and the "Forgot …" toast dropped
 * `board_name` too, so forgetting a named board reported a bare MAC.
 */
export function fleetDisplayName(b: FleetEntry): string {
  return (
    b.nickname?.trim() ||
    b.board_name?.trim() ||
    b.chip_type?.trim() ||
    b.id
  );
}

/**
 * A board name only when arduino-cli's match is actually about *this* board.
 *
 * **Mirrors `core::fleet::board_name`, deliberately.** A hidden sibling in the
 * list means the match was made on a family-wide USB vid/pid, so the non-hidden
 * entry arduino-cli offers alongside it is an arbitrary member of that family —
 * which is how a plain ESP32-S3 dev board gets confidently labelled "Ozobot
 * DRVKit". Several named matches mean the same thing.
 *
 * This is **not** `visibleBoard`, and the difference matters: for choosing an
 * FQBN to compile with, the arbitrary sibling is the best guess available. For
 * a *name the user reads to tell two boards apart*, being wrong is worse than
 * being silent — the Rust side has refused this for as long as the fleet has
 * existed, and the toolbar was the one place still printing it.
 */
export function confidentBoardName(p: DetectedPort): string | null {
  if (p.matching_boards.some((b) => b.is_hidden)) return null;
  const named = p.matching_boards.filter((b) => b.name !== "");
  return named.length === 1 ? named[0].name : null;
}

/**
 * What a board is called, without its device path.
 *
 * In order: the nickname the user chose, a board name arduino-cli is actually
 * sure about, the board name the fleet remembers, and finally an honest
 * description of a port with no identity at all. The nickname wins because it
 * is the only one of these the user picked, and the only thing that tells two
 * identical clones apart.
 */
export function portTitle(p: DetectedPort, fleet?: FleetSnapshot | null): string {
  const entry = entryOn(p.port.address, fleet);
  if (entry?.nickname?.trim()) return entry.nickname.trim();
  const board = confidentBoardName(p);
  if (board) return board;
  if (entry?.board_name?.trim()) return entry.board_name.trim();
  if (entry?.chip_type?.trim()) return entry.chip_type.trim();
  return p.port.protocol === "network" ? "Network port" : BRIDGE_TITLE;
}

/**
 * **The one way a port is named in this UI.** Board first, device path second:
 * the name is what a person thinks in, the path is what they occasionally need.
 *
 * Every surface that shows a port goes through this — the picker, the status
 * bar, the serial tab, notifications — so the same board reads the same way
 * everywhere. Machine-facing strings (the MCP tools, `arduino-cli` argv) keep
 * the bare address; a friendly name is for people.
 */
export function portName(p: DetectedPort, fleet?: FleetSnapshot | null): string {
  return withPath(portTitle(p, fleet), p.port.address);
}

/**
 * The same, for a port that is selected but no longer attached — a board that
 * was unplugged, or one pinned in `sketch.yaml`. There is no `DetectedPort` to
 * describe, so the fleet's memory is all there is.
 */
export function missingPortName(
  address: string,
  fleet?: FleetSnapshot | null,
): string {
  const remembered = fleet?.boards.find((b) => b.last_port === address);
  const title = remembered?.nickname?.trim() || remembered?.board_name?.trim();
  return title ? `${title}${SEP}not attached` : `${address}${SEP}not attached`;
}

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

/**
 * True when a profile would flash a different board than the one detected on
 * the selected port — the profile silently wins over the port, so this is the
 * only place the disagreement can be surfaced. Compares the vendor:arch:board
 * base (a profile may pin board options; `board list` never reports them), and
 * stays quiet when either side is unknown: a bare USB bridge reports no
 * identity, and that must not turn every upload into a warning.
 */
export function flashTargetMismatch(
  profileFqbn: string | undefined,
  detectedFqbn: string | undefined,
): boolean {
  const base = (f: string | undefined) =>
    (f ?? "").trim().split(":").slice(0, 3).join(":");
  const p = base(profileFqbn);
  const d = base(detectedFqbn);
  return p !== "" && d !== "" && p !== d;
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
  fleet?: FleetSnapshot | null,
): PortOption[] {
  const opts = ports.map((p) => ({
    address: p.port.address,
    label: portName(p, fleet),
    missing: false,
  }));
  if (selected && !ports.some((p) => p.port.address === selected)) {
    opts.push({
      address: selected,
      label: missingPortName(selected, fleet),
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
