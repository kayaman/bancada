// When to re-take the serial port after capture is lost.
//
// A native-USB board (ESP32-S3/C3/C6, Leonardo-class) drops off the USB bus
// and re-enumerates on **every reset** — the flash, the reset button, a
// watchdog reboot, ESP.restart(). The monitor child dies with the port and
// correctly reports `serial://closed`.
//
// Nothing then brought it back. The auto-start effects are keyed on
// `selectedPort` and `bottomTab` changing *value*, and the port returns at
// the same address — `/dev/ttyACM0` → `/dev/ttyACM0`. Worse, the 2 s port
// poll compares port *sets*: a 2 s dropout is routinely never observed at
// all, so no `ports://changed` fires and no effect re-runs. The monitor
// stayed dead until a manual Start, which is the whole "sometimes it stops
// working" report.
//
// Boards behind a UART bridge (FTDI, CP2102, CH340) never show this: the
// bridge stays enumerated across a board reset. That is why it looked
// intermittent.

import { nextBackoff } from "./obs/backoff";

/**
 * How many times to chase a lost port before giving up.
 *
 * With `nextBackoff`'s 1/2/4/8/16 s ladder this spans ~31 s — comfortably
 * longer than the ~2 s a reset takes to re-enumerate, and short enough that
 * a board genuinely unplugged and carried off stops being chased.
 */
export const MAX_RECAPTURE_ATTEMPTS = 5;

export interface RecaptureState {
  /** The standing request for capture. False after an *explicit* stop — the
   *  user's Stop button, the scope taking the port, the pre-flash handoff. */
  wanted: boolean;
  /** A build, flash or agent upload owns the port; it restarts the monitor
   *  itself when it is done. */
  busy: boolean;
  /** Recapture attempts since the last successful start. */
  attempt: number;
}

/** Whether to try again, and after how long. */
export function recapturePlan(s: RecaptureState): {
  retry: boolean;
  delayMs: number;
} {
  if (!s.wanted || s.busy || s.attempt >= MAX_RECAPTURE_ATTEMPTS)
    return { retry: false, delayMs: 0 };
  // `attempt` is how many have already been made, so the first retry is
  // attempt 0 → nextBackoff(1) → 1 s. Immediate would race the kernel: the
  // device node outlives the disconnect by a moment.
  return { retry: true, delayMs: nextBackoff(s.attempt + 1) };
}
