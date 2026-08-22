# bancada 0.19.0

The bench loop — flash, watch, read the error, fix it — was the part of Bancada
that still made you hunt. This cut is that loop: nothing is two clicks deep, the
app says what it is doing and how it went, a compiler error is a link to the
line, and the Serial Monitor is a monitor rather than a text box with a baud
select.

## One row at the bottom

The bottom panel had a group row above a tab row — `Console` / `Debugging` /
`Observability` / `Assistant`, and then the tabs inside each. Two of those four
groups held a single tab, so their sub-row was empty and pinned open with a
min-height hack; and the serial monitor, the thing you reach for most on a
bench, was filed two clicks deep under "Debugging".

It is one flat row now:

```
Build · Serial │ Scope │ MQTT · WS · Web │ Assistant
```

Thin separators stand where the group boundaries were, so the grouping is still
legible without costing a click. Order, labels, separators and the per-tab
dot/badge are data in one module (`src/bottomTabs.ts`); the row itself is a
component with a test. The sidebar keeps its two-level layout, because there the
groups are real — each remembers its own last-used tab.

## What is running, and how it went

Bancada had one status line with one text slot, so an outcome and an
in-progress message had to share it. The workaround was to concatenate them,
and whichever arrived second won: a "✓ Compiled" could be wiped by the next
"Uploading…" before you read it.

Two channels now, and they do not mix.

**Outcomes are toasts.** They stack in the corner, dedupe when the same message
repeats, and dismiss themselves — after 3 s for information, 4 s for a success.
**An error toast never expires**; it waits to be dismissed, because the one
thing a bench tool must not do is time out the sentence explaining why the
flash failed.

**The status bar is the activity line.** One activity at a time — label,
elapsed clock, and a hairline progress bar above it — with the project and port
on it at rest. It also remembers how long the last compile and the last flash of
this project took, and says so: `Compiling… 0:07 (usually ~1:05)`.

The progress bar is deliberately honest about which of three things it is
showing:

- **Measured** — a real fraction, weighted across the flash regions esptool
  announced. `aria-valuenow` is set; this is the only case where a number is
  claimed.
- **Estimate** — a dashed bar driven by the last run's duration, capped at 95%
  so it can never claim a build has finished. It reports itself as an estimate
  to assistive technology and to you.
- **Indeterminate** — a sliding bar with no number, which is what a plain
  compile gets (there is nothing to count) and what avrdude gets (through a
  pipe it prints phases, not percentages).

A guessed percentage rendered as a fact is worse than no number, so it is not
drawn.

## Click the error, land on the line

The build console printed compiler output as text. Now it parses it.

- **Every diagnostic with a location is a button.** Click it and the file opens
  at the line and column, cursor placed, editor focused. arduino-cli injects
  `#line` directives into the merged `.ino.cpp`, so gcc reports the *original*
  `.ino` path — which is why this maps back onto your buffer at all.
- **A summary strip** above the log: `✗ 2 errors · 1 warning`, or the flash and
  RAM figures on a build that got that far. A build that failed says so first,
  ahead of any warning count.
- **An errors-only filter**, which releases itself when the next build has no
  errors left rather than leaving you behind a toggle you cannot un-press.
- **The Build tab carries the error count** as a badge while you are on another
  tab.
- Warnings, notes, the `In file included from` chains and the caret lines are
  kept and attached to the diagnostic they belong to, not scattered.

Three kinds of row are deliberately **not** clickable, and it is worth knowing
why: a diagnostic in a core header or an installed library (it is not a file in
your sketch, and opening it would invite editing it); one reported against the
generated prologue `/tmp/arduino/sketches/<hash>/sketch/X.ino.cpp` (same
reason); and linker errors, which are reported without a location the parser
takes. The real `undefined reference` line from avr-gcc does carry a usable
one — making those jumpable is the obvious next step and is not in this cut.

The fixtures the parser was written against are real captures from
arduino-cli 1.5.0 with avr-gcc 7.3.0 and xtensa gcc 14.2.0, not handwritten:
the two toolchains print their caret blocks differently, and a parser that knew
only one would drop half the detail.

## The Serial Monitor

The serial tab was a console with a baud dropdown. It is a monitor now.

**It reads the baud out of your sketch.** `Serial.begin(115200)` in the code is
the project's answer to "what rate", it is in git, and it travels with the
project — so the picker follows it, resolving one level of `#define`, `const`
and `constexpr`. A per-sketch override is stored when you pick something else,
and a **Use sketch's N** button appears to hand it back. The rate list grew:
74880 (the ESP8266 boot-ROM rate — without it a reset banner is mojibake,
which is exactly when you need to read it), 250000, 500000, 1000000, 2000000.

**Line endings are yours to pick** — none, NL, CR, or both — and they go on the
wire exactly as chosen. `monitor_send` no longer appends a newline behind your
back, which is what made talking to a bare-CR firmware impossible before. The
Assistant's `serial_send` is a different write path and still always appends
`\n`; its tool description says so on the wire, so the agent is not guessing.

Also new:

- **Timestamps** per line, toggleable.
- **A filter box** — a substring, applied to the view, not to what is kept.
- **Pause** freezes what is on screen and tells you how many lines arrived
  behind it. It pins a copy rather than a watermark, so a chatty board can no
  longer empty the very screen you paused to read.
- **Export** writes what you can see to a text file.
- **↑/↓ recall** in the send box, shell-style, with your half-typed draft
  restored on the way back down.
- **Starting the monitor no longer clears the scrollback.** An info line marks
  each start, stop and baud change instead, so a flash → reconnect cycle reads
  as one session.
- Changing the baud restarts the monitor in place, disarming the recapture
  ladder first so the two do not race for the port — and if the re-open fails,
  the standing request is put back rather than leaving the panel dead with
  **Start** as the only thing that works.
- **The picker never shows a rate the port is not open at.** The displayed baud
  is derived — switching project, or saving a sketch whose `Serial.begin`
  changed, moves it under a child still reading at the old rate — so while a
  monitor is running the toolbar shows what the child was opened with, and the
  drift re-opens the port instead of leaving the two to disagree. A monitor
  quietly decoding at the wrong rate is this panel's most confusing failure: it
  looks like a broken board.
- **The recapture ladder says when it gives up.** After it exhausts its
  attempts on a port that is not coming back, it writes
  `— gave up re-opening the port —` and stands down, rather than showing
  `↻ 5/5` forever as if it were still trying.

Under all of it the log is a plain polled store rather than React state, and
only the rows in the viewport are rendered — a board at 921600 baud emits
faster than React can commit, and the old console tried anyway.

**And the stale-reader gap is closed.** 0.17.1 and 0.18.0 both shipped with the
same line under "still open": `serial://closed` carried no session identity, so
a dying reader thread from a previous monitor could report a live one as closed.
`start_monitor` now returns a session id, both `serial://started` and
`serial://closed` carry it, and the frontend ignores a close naming any other.
The agent's events had solved the same problem by stamping a pid; the serial
path now does the same.

## Components can be rendered in a test

The structural item. `vitest` ran in the node environment only, so nothing in
this repository had ever rendered a component — every UI claim in the last two
releases was "exercised by tests but never seen rendered".

A `.tsx` test can now opt into jsdom per file (`// @vitest-environment jsdom`
plus `afterEach(cleanup)`; there is no per-glob switch since vitest 3). It
renders a **leaf component** — props in, DOM out — and asserts on roles, labels
and text. `App.tsx` is still never rendered; the `App.tsx?raw` source-text tests
remain the harness for its wiring, and that tension is still named in the
architecture index.

Five components use it: the tab bar, the toast stack, the status bar, the build
console and the serial monitor. That is what made the accessibility work in this
cut assertable rather than aspirational — the error/status split on toasts, the
always-mounted live regions, `aria-valuenow` appearing only when the number is
real, and every Serial Monitor control having a name.

## The honest gaps

**Nothing here has been verified on hardware in this cut.** Everything above is
covered by tests and none of it has been seen driving a real board; the bench
pass runs after this merges. Given that 0.17.0 shipped unable to flash for
exactly this reason, treat that as the caveat it is.

**Compiles are indeterminate-only.** A compile has no percentage to report, so
`Verify` shows a sliding bar and an elapsed clock, and — after the first build
of a project — an estimate. Only an esptool flash that announces its erase
regions produces a measured bar; a board recipe that suppresses those lines
gets an indeterminate one for the whole upload rather than a fabricated
percentage.

**The esptool fixture is written from the documented 4.x format**, not captured
from a flash on this bench. The parser accepts esptool 5's tighter spacing too,
but the fixture should be replaced with a real transcript when there is one.

**`detectBaud` is not string-literal aware.** It strips comments, but a `//`
inside a string literal reads as a comment start. The cost of being wrong is one
click on the baud picker, which is why it does not carry a C tokenizer.

**A baud override is keyed by absolute sketch directory.** Move or rename a
project and the override is orphaned — the sketch's own rate takes over, which
is the benign direction — leaving a dead key in `localStorage`. Nothing collects
them.

**A CRLF sketch and the jump.** The jump compares the editor's document against
the buffer before moving the cursor, and CodeMirror normalises `\r\n` to `\n`;
the comparison accounts for that, so CRLF files are jumpable. What it will not
do is jump into a document it cannot confirm is the right one — it drops the
jump rather than landing on a wrong line.

## Notes

`cargo test --workspace` 674 passed, `vitest` 997 passed, `tsc --noEmit` clean,
`npm run build` (vite build) clean — the fourth gate exists because vitest
never parses `styles.css`.
The opt-in suites and the hardware pass have not been run for this cut.

One flake to know about: `cargo test --workspace` in parallel can fail
`cli::tests::unparseable_output_reports_a_json_error_naming_the_command`. It
passes with `-- --test-threads=1`, which is how the count above was taken.
