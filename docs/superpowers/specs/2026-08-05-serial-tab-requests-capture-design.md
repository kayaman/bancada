# Serial Monitor tab is a standing request for capture

**Date:** 2026-08-05
**Status:** Approved design

## Problem

Capture-by-default is a one-shot: `startMonitorQuiet()` fires when a port
is (auto-)selected, and its failure path is deliberately silent (port
busy or gone). If that single attempt loses — e.g. the port still held by
a dying monitor child from a previous app instance after a dev-rebuild
restart — capture stays off forever with no retry, and opening the
Serial Monitor tab does nothing about it. The user expects the Debugging
Serial Monitor to be reading by default.

## Design

An invariant instead of an event: **the Serial Monitor tab being visible
(re)requests capture.** One effect in `App.tsx`, edge-triggered on
`[bottomTab, selectedPort]` (plus the `startMonitorQuiet` callback, which
also rotates on baud change): when the visible bottom tab is `serial`
and no user build/flash is in flight (`busyRef`), call
`startMonitorQuiet()` — which already no-ops when the monitor is on or
no port is selected, and stays silent on failure.

Preserved behavior:

- **Stop stays stopped.** Monitor state is read via the existing ref and
  is not a dependency, so pressing Stop while on the tab does not fight
  the user. Leaving the tab and returning re-requests capture.
- **No port contention.** The `busy` guard skips capture during a
  build/flash; the post-flash path already restarts capture itself.
- The existing port-selection auto-start remains — it covers capture
  while the user is on another tab.

## Testing

App wiring only (repo convention: untested); all suites stay green.
Verified live: open Debugging → Serial Monitor with the monitor off —
`fuser /dev/ttyACM0` shows the monitor child holding the port.
