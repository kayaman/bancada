# Assistant panel: detailed debug views + turn-aware footer

**Date:** 2026-08-07
**Status:** Approved design

## Problem

Two related gaps in the Assistant panel:

1. **The footer lies between turns.** `activityLabel` falls back to
   `thinking…` whenever the session status is `running` and no tool or
   stream is active. The persistent CLI session stays `running` *between*
   turns, so after the final `result` event the footer keeps showing
   `thinking…` while the agent is actually idle, waiting for input. The
   store conflates two lifetimes — "session alive" and "turn in flight" —
   into one `status` field.

2. **Debug detail is thin.** Generic tool cards show only two truncated
   args and never their results; events the store doesn't model
   (`system:status`, `rate_limit_event`, hook lifecycle) vanish without a
   trace; the footer activity strip shows a basename and no sense of how
   long the current activity has been running.

## Decisions made with the user

1. All three debug improvements: raw event log, richer generic tool
   cards, richer footer activity.
2. Between turns (session live, no turn in flight) the footer shows
   **`Ready`** — distinct from the pre-session `Not started`.

## Design

### Turn-in-flight tracking (the `thinking…` fix)

`AgentStore` gains a private `turnActiveFlag`, exposed as `turnActive` in
`snapshot()`:

- set `true` in `userSent()`;
- cleared in `handleResult()`, `closed()`, `handleAlarm()`, and `clear()`.

`activityLabel` gates on it: when no turn is in flight it returns `null`
(verify still outranks the gate — a verify spinner must never be hidden).
`statusLabel("running")` changes from `Running` to `Ready`; that branch is
currently unreachable while running (activityLabel always won), so after
this change it becomes precisely the between-turns label.

### Raw event log

The store records **every** `agent://event` payload into a capped ring
buffer (500 entries), including types `push` currently ignores. Entry:
`{ ts, type, subtype?, count, json }` where `json` is the pretty-printed
payload. Consecutive `stream_event` entries coalesce into one row —
`count` increments and `json` is replaced by the newest — otherwise text
deltas flood the log at dozens per second. `closed()` appends a synthetic
`closed` entry (reason + pid) so session teardown is visible too.
`clear()` resets the log with the rest of the store.

Unknown event types therefore now bump `version` (they must repaint an
open debug view). The old "no version bump" rule was about not painting
junk into the *transcript*, which still holds — unknown types still
produce no transcript message.

UI: a `🐛 Debug` toggle button in the footer swaps the scroll area to the
log view (same mount pattern as History), one `<details>` per entry —
summary is `HH:MM:SS type/subtype ×count`, body is the pretty JSON in a
`<pre>`. Autoscroll keeps following the tail via the existing
stick-to-bottom logic.

### Richer generic tool cards

The one-liner `🔍 Name(args)` becomes the `<summary>` of a `<details>`
(the existing stderr pattern) with a status icon (⟳/✓/✗). Expanding shows
the full input as pretty-printed JSON plus the tool result text, capped
at 4000 chars. Edit/Write/Verify cards are unchanged.

### Richer footer activity

- Tool hints show the **full `file_path`** instead of the basename
  (CSS ellipsis handles overflow).
- The label gains elapsed seconds: `⚙ Edit /x/soil/soil.ino… 12s`.
  Tool messages get a `startedAt` stamp when pushed; `userSent` stamps
  `turnStartedAt` (used for `✍ writing…`/`thinking…` elapsed).
  `activityLabel` stays pure — it takes `now` as an input.
- The panel's 100 ms poll currently repaints only on `version` change;
  while `turnActive` it now repaints every poll so the elapsed counter
  ticks. (During a turn the store is churning versions anyway; the extra
  repaints between events are cheap and bounded to live turns.)

## Unchanged

Edit/Write diff cards, Verify card, alarm banner, turn_end dividers and
Turn Summary view, history/replay, session lifecycle, pid guards, and the
whole Rust backend (events already arrive verbatim).

## Testing

vitest, test-first: store tests for `turnActive` transitions, raw-log
recording/coalescing/cap/clear and the synthetic `closed` entry;
activity tests for the `turnActive` gate, full-path hints, and elapsed
formatting. Panel JSX and CSS verified visually per repo convention.
