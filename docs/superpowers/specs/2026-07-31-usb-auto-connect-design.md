# USB Auto-Connect Design

**Date:** 2026-07-31
**Status:** Approved

## Goal

Plugging a board into USB should be enough: Bancada notices the arrival,
selects the board if the selection slot is free, starts capturing serial
output (existing "capture by default" behavior), and greets known fleet
boards by nickname. Today none of this happens until the user clicks
Refresh.

## Decisions

- **Selection never steals.** A newly attached board is auto-selected only
  when no port is selected or the selected port has vanished. This extends
  the existing `nextSelectedPort` contract ("a rescan must never silently
  retarget an upload") to hotplug events.
- **Known and unknown boards behave the same.** Any attached board can take
  a free slot; fleet enrollment continues to happen on scan as it does
  today. "Known" only upgrades the notification (nickname instead of a
  bare port path).
- **Detection is a Rust-side watcher, not frontend polling.** Cheap
  enumeration every tick; the expensive arduino-cli scan runs only on
  membership change.

## Architecture

Two units, one per side of the Tauri boundary.

### 1. Port watcher (Rust, `src-tauri/src/lib.rs`)

A background task spawned during Tauri setup:

- Every **2 seconds**, call `serialport::available_ports()` (the crate is
  already a dependency) and reduce to a sorted set of port paths.
- If the set differs from the previous tick's, emit **`ports://changed`**
  with no payload — a doorbell, not a data channel.
- On enumeration error, keep the previous set and try again next tick;
  never emit on error, never terminate.
- No arduino-cli calls, no selection logic. Dies with the process; no
  cleanup protocol.

### 2. Refresh reactor (TypeScript, `src/App.tsx` + new pure module)

A listener on `ports://changed` (new `onPortsChanged` wrapper in
`src/api.ts`) that invokes the existing `refreshPorts()`, wrapped in two
guards:

- **Busy guard:** while a compile/upload is running, the event only sets a
  pending flag; one refresh runs when busy clears. Keeps arduino-cli from
  probing ports mid-flash. Because `busy` clears before a native-USB port
  has finished re-enumerating, the queued refresh is scheduled ~1500 ms
  after busy clears (mirroring the flash flow's 1200 ms monitor-restart
  wait) so a briefly absent port is not mistaken for a detach and the
  selection is not dropped.
- **Debounce:** events within ~500 ms coalesce into one refresh (USB
  enumeration can surface multiple ports a beat apart).

Everything downstream is existing code: `nextSelectedPort` fills a free
slot, the `selectedPort` effect auto-starts the serial monitor,
`fleetSync` enrolls the board.

### Arrival notification

`refreshPorts()` stops fire-and-forgetting `fleetSync` and uses its
returned `FleetSnapshot`: diff the new `online` ids against the previous
snapshot's; announce newly online boards via the status bar as
"⚡ <name> attached (<port>)", where name is nickname → board_name → id.
The diff is a pure function in a new module (`src/portWatch.ts`, styled
like `ports.ts`). The first sync after app start announces nothing —
boards already plugged in at launch are not "arrivals".

## Data flow

```
plug in board
  → watcher tick sees /dev set changed → emit ports://changed
  → frontend: busy? queue : debounce → refreshPorts()
      → listBoards (arduino-cli)  → setPorts
      → nextSelectedPort           → fills slot only if free
      → fleetSync                  → snapshot with online ids + nicknames
      → arrival diff               → status-bar notification
  → selectedPort changed → existing effect starts serial monitor
```

Unplug is the mirror image: watcher fires, refresh drops a vanished
selection (existing behavior), `serial://closed` has already stopped the
monitor, and a freed slot goes to another attached board if present.

## Edge cases

- **Flash-induced flapping:** native-USB boards re-enumerate during
  upload; the busy guard defers refreshes until the upload finishes, and
  the queued refresh waits out the re-enumeration settle window before
  reconciling, so the post-flash monitor restart runs on the unchanged
  selection first.
- **Replug while another board holds the slot:** the returning board does
  not steal the slot back; it is announced and can be selected by hand.
- **Failed arduino-cli scan after an event:** `refreshPorts()` already
  surfaces errors; the watcher re-fires on the next real change, so the
  state self-heals.
- **Active monitor/scope on the selected port:** refreshes have side
  effects only on selection change, and an occupied slot never changes.

## Testing

- **`src/__tests__/portWatch.test.ts` (vitest):** arrival diff — newly
  online boards announced with the right display name (nickname >
  board_name > id); nothing announced on first sync; simultaneous
  arrivals handled.
- **Rust:** membership comparison is plain set inequality; no dedicated
  test unless it grows logic.
- **Manual bring-up:** plug/unplug a real board; flash a native-USB ESP32
  and confirm no selection churn mid-flash; replug while another board is
  selected.

## Out of scope (YAGNI)

Per-board auto-connect opt-out, per-board baud memory, MQTT reconnection,
any settings UI. The feature is: plug it in, and if the bench is free,
you're capturing.
