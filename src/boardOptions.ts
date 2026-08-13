// Board options — the part of an FQBN after the board id.
//
// An FQBN can carry the board's menu choices as a trailing, comma-separated
// list: `esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M`. Bancada could only
// ever express the bare `vendor:arch:board` form, which is why a profile could
// not say `CDCOnBoot=cdc` — and on an ESP32-S3 the core's default routes
// `Serial` to UART0 instead of the native USB port, so the serial monitor sits
// silent while the upload, the board and the wiring are all fine.
//
// This module is the string handling for that, kept away from the pickers:
// composing, parsing and the one judgement call (`silentSerialWarning`) are
// all decisions that must be right rather than merely rendered.

// The Rust-facing shapes live in `api.ts` with every other mirror of a core
// type; re-exported here so a caller of this module needs only one import.
import type { ConfigOption, ConfigValue } from "./api";

export type { ConfigOption, ConfigValue };

/**
 * The board's own defaults, as a selection map.
 *
 * An option arduino-cli reports with *nothing* selected is left out rather
 * than defaulted to its first value: claiming a default we cannot see means
 * `composeFqbn` would drop the user's matching choice from the FQBN, and a
 * choice that silently does not reach the compiler is the exact failure this
 * module exists to end. A missing key only costs an option spelled out in
 * full.
 */
export function defaultSelection(options: ConfigOption[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const o of options) {
    const chosen = o.values.find((v) => v.selected);
    if (chosen) out[o.option] = chosen.value;
  }
  return out;
}

/**
 * Compose an FQBN from a bare base and a selection.
 *
 * **Options left at the board's default are omitted.** The ESP32-S3 has 17 of
 * them; spelling all 17 out makes an FQBN nobody can read and churns the
 * `sketch.yaml` diff on every core update that shifts a default. What the user
 * actually changed is the only thing worth pinning.
 *
 * Emission follows the order of `options` — the order arduino-cli lists them —
 * so the string is stable across runs instead of riding on the key order of
 * whatever object the UI happened to build. A key `options` does not know is
 * dropped: it is a stale menu from a core that has since been updated, and
 * inventing an option for it would only produce an FQBN the compiler rejects.
 *
 * A value the board does not offer *is* emitted, deliberately: it is the
 * user's pinned choice, and arduino-cli refusing it by name is better than
 * Bancada quietly flashing something else.
 *
 * `base` is normalised through `parseFqbn`, so composing over an FQBN that
 * already carries options replaces them rather than appending a second list.
 */
export function composeFqbn(
  base: string,
  options: ConfigOption[],
  selection: Record<string, string>,
): string {
  const defaults = defaultSelection(options);
  const bare = parseFqbn(base).base;
  const parts: string[] = [];
  for (const o of options) {
    const chosen = selection[o.option];
    if (chosen === undefined) continue;
    if (chosen === defaults[o.option]) continue;
    parts.push(`${o.option}=${chosen}`);
  }
  return parts.length > 0 ? `${bare}:${parts.join(",")}` : bare;
}

/**
 * Split an FQBN into its bare base and whatever options it carries — the
 * inverse of `composeFqbn`, and it must round-trip.
 *
 * The base contains colons of its own (`vendor:arch:board`), so the options
 * are whatever follows the **fourth** colon; splitting on `:` and taking the
 * tail hands you `esp32` as an option name.
 *
 * A string that is not `vendor:arch:board[:opts]` is handed back whole as the
 * base with an empty selection — including a tail with no `key=value` in it at
 * all. Guessing at a malformed FQBN loses the only thing the user wrote down;
 * returning it intact means it still round-trips and still shows up in the UI
 * as what it is.
 */
export function parseFqbn(fqbn: string): {
  base: string;
  selection: Record<string, string>;
} {
  const parts = fqbn.split(":");
  if (parts.length < 4) return { base: fqbn, selection: {} };

  // Rejoin the tail: the split above is only ever about finding the base.
  const selection: Record<string, string> = {};
  for (const item of parts.slice(3).join(":").split(",")) {
    const eq = item.indexOf("=");
    if (eq <= 0) continue;
    selection[item.slice(0, eq)] = item.slice(eq + 1);
  }
  if (Object.keys(selection).length === 0) return { base: fqbn, selection: {} };
  return { base: parts.slice(0, 3).join(":"), selection };
}

/** ESP32 variants whose USB port is the chip's own, not a bridge chip's.
 *  Matched exactly: an unlisted variant means no warning, which is the side to
 *  be wrong on. */
const NATIVE_USB_BOARDS = new Set([
  "esp32s2",
  "esp32s3",
  "esp32c3",
  "esp32c6",
  "esp32h2",
]);

/** True for a port that *is* the chip — Linux `ttyACM`, macOS `usbmodem`.
 *  A `ttyUSB`/`cu.usbserial` port is a CH340 or CP210x wired to the UART pins,
 *  where `Serial` arrives no matter how CDCOnBoot is set. Windows `COM3` says
 *  nothing either way, so it says nothing here. */
function isNativeUsbPort(address: string): boolean {
  return (
    /^\/dev\/ttyACM\d+$/.test(address) ||
    /^\/dev\/(cu|tty)\.usbmodem/.test(address)
  );
}

/**
 * Why the selected port will show nothing, or null.
 *
 * The bench failure this exists for: an ESP32-C6 on `/dev/ttyACM0`, compiling
 * clean, flashing clean, and a serial monitor that never prints a byte. The
 * core's default for `CDCOnBoot` is Disabled, which sends `Serial` to UART0 —
 * so the one port you can see is the one port the sketch is not talking on.
 * Nothing in the build output mentions it; the fix is in the FQBN, and the
 * hours go into re-checking the wiring.
 *
 * Warned only when all three hold: the port is native USB, the board is a
 * variant that *has* native USB, and the FQBN does not already turn CDC on.
 * Everything else is null — a warning on a working setup teaches people to
 * ignore warnings, which costs more than the silence it saves.
 */
export function silentSerialWarning(
  portAddress: string | null,
  fqbn: string | undefined,
): string | null {
  if (!portAddress || !fqbn) return null;
  if (!isNativeUsbPort(portAddress)) return null;

  const { base, selection } = parseFqbn(fqbn);
  const segments = base.split(":");
  if (segments.length !== 3) return null;
  if (!NATIVE_USB_BOARDS.has(segments[2])) return null;

  const cdc = selection.CDCOnBoot;
  if (cdc !== undefined && cdc !== "default") return null;

  return (
    `Serial output will not reach ${portAddress}: with CDCOnBoot left at its ` +
    `default this board sends Serial to the UART pins, so the monitor stays ` +
    `empty while the upload itself looks fine. Set USB CDC On Boot to Enabled ` +
    `to read Serial over USB.`
  );
}
