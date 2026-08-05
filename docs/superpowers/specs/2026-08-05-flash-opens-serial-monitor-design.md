# Flash success opens the Serial Monitor

**Date:** 2026-08-05
**Status:** Approved design

## Problem

A successful flash arms a 15-second window and pulls the Serial Monitor
tab forward only when the first serial line arrives. A sketch that prints
nothing (wrong CDC routing, crashed boot, quiet firmware) leaves the user
on the Build console with the monitor running invisibly — the moment they
most want to be watching serial output.

## Design

On flash success in `upload()` (`src/App.tsx`):

- Open the Serial Monitor tab immediately, when the "✓ Flashed" status
  lands (`openBottomTab("serial")`).
- Keep the existing 1200 ms re-enumeration settle before
  `startMonitorQuiet()` starts capturing. If the monitor cannot start
  (port busy or gone), the tab is still open with its Start button — the
  user is in debugging posture either way.
- Delete the wait-for-first-line machinery: `autoOpenSerialUntilRef` and
  its branch in the `onSerialLine` handler.

Unchanged: failed flashes stay on the Build console (errors stay in
front of the user); Verify never touches the serial tab; the
scope-firmware flash keeps its own flow (ScopeView owns the port);
unseen-dot behavior.

## Testing

No new logic function — App wiring only. Existing vitest suites must stay
green; behavior verified in the running app (flash → tab switches, output
streams after re-enumeration).
