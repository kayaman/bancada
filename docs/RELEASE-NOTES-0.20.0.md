# bancada 0.20.0

Bancada has driven two toolchains since 0.19.0 — `arduino-cli` for sketches,
`idf.py` for ESP-IDF projects. What it did not have was a notion of the
**board**. It knew the chip, and it knew the serial port, and between those two
facts sits the thing actually on your bench: which pin the LED is on, whether
GPIO12 is safe to use, where UART0 comes out. Neither engine can answer any of
that. `arduino-cli` exposes nothing about pins, and `idf.py` has no board
concept at all.

This cut gives Bancada its own answer, once, for both paradigms.

## One table, two toolchains

The board model is a Rust const table — three devkits to start, the ones on the
bench — carrying headers and silkscreen labels, the onboard LED, the BOOT
button, the USB ports, and a closed vocabulary of caveats: strapping pin,
input-only, used by flash or PSRAM, shared with the USB-Serial/JTAG or the
UART0 console, ADC2-while-Wi-Fi. The three pages under `docs/boards/` are
rendered from that same table, and a test asserts the checked-in pages match
byte for byte, so the documentation cannot drift from the data.

The table is keyed by **chip target**, because that is the only key `idf.py`
could ever have supplied. An Arduino project reaches it through a fold of its
FQBN — and the fold is the interesting part, because the esp32 core spells its
board segment two ways. `esp32:esp32:esp32s3` names the bare chip;
`esp32:esp32:esp32doit-devkit-v1` names a board that merely *starts* with one.
So the answer is the longest known target id the segment begins with. `esp32s3`
beating its own prefix `esp32` is the difference between the right pin table
and a plausible wrong one.

## Recorded, inferred, unchosen, unknown

These are four different answers and Bancada renders four different things.

A project **records** its board in the project itself — a `# bancada.board =`
comment in `sdkconfig.defaults` for ESP-IDF, a `board:` key in `bancada.yaml`
for Arduino. A comment rather than a `CONFIG_` key because ESP-IDF's `kconfgen`
prints a note for every unknown symbol on every reconfigure, and a marker that
nags on each build is a marker you learn to ignore. It survives `fullclean`,
which regenerates `sdkconfig` and not `sdkconfig.defaults`, and it travels with
the repo.

When nothing is recorded but the chip has exactly one modelled devkit, Bancada
**infers** it and says so. The pinout still renders — it is very probably
right — but it is labelled, in a different colour, with a button to record it.
Where several devkits fit the chip, it asks rather than guessing. Where there is
no data at all, it says the pinout is unknown, which is not the same as saying
the pin is safe.

That distinction is the whole design. A deterministic answer and a plausible one
must never be rendered alike, so `recorded` and `inferred` are separate states
in the wire format rather than a board plus a boolean nobody displays.

## Where you meet it

**New Project** gains a Devkit select under the board picker — only when the
chip has candidates, because a select with nothing in it reads as a bug. A lone
candidate is preselected, since a question with one answer is not a question,
and "Not listed" is always there as an explicit choice.

**Blink** stops guessing. It took `LED_BUILTIN 2` with a comment apologising for
it; now it takes the pin the board actually uses. And on a board whose onboard
LED is an addressable WS2812 — the S3-DevKitC-1 and the C6-DevKitC-1 both — it
is a *different sketch*, driving `rgbLedWrite()`. A plain HIGH/LOW does nothing
whatsoever to a WS2812, which is how a first upload can succeed completely and
leave you with a dark board and an evening spent on the wiring.

**Boards ▸ Devkit** shows the pinout: every header row with its silkscreen
label, its GPIO, its alternate functions, and its caveats as badges that
explain themselves on hover.

**The Assistant** gains `board_pinout`, its first read-only tool and the only
one that takes an argument — it touches no hardware, no subprocess and no file.
The board is still not a parameter: it resolves from the session's project the
same way the GUI resolves it, so the assistant cannot reason about a board you
are not holding. Asked about a project with no board data, it answers that the
pinout is unknown, in words, as a success — because an agent that reads an empty
result as "no caveats" produces precisely the confident wrong advice this whole
subsystem exists to prevent.

## Also in this cut

`bancada-core`'s tests could not compile from a clean checkout. The ESP-IDF
backend in 0.19.0 added a fixture at `core/src/testdata/idf_build_failure.log`,
and `.gitignore`'s `*.log` silently swallowed it — and because the file is
`include_str!`d, its absence was a build error rather than a skipped test.
Anyone cloning the repo since then hit it immediately. The fixture is committed
and the ignore rule now exempts the checked-in toolchain transcripts.

Three counts in the documentation had drifted and are corrected: invoke commands
(claimed 96, actually 105), core modules (claimed 22 and 26, actually 30), and
MCP tools (4, now 5).

## Tests

```
cargo test --workspace   722 core · 84 src-tauri, 0 failed
npm test                 1234 passed, 75 files
tsc --noEmit             clean
vite build               clean
```

Run the Rust suite with `--test-threads=1`: `cli::tests` has a known parallel
flake that is not a real failure.

## Not yet seen on hardware

**No part of this cut has been run against a board.** The board facts for the
S3 and C6 come from Espressif's user guides; the DOIT board's rows were
transcribed from its silkscreen. The WS2812 Blink compiles against the
installed esp32 core 3.3.11 — where `rgbLedWrite()` is current and
`neopixelWrite()` is deprecated — but has not been flashed.

The bench pass owed here is specific:

1. New Project on an S3-DevKitC-1, Blink, flash — the RGB LED should blink
   green rather than staying dark.
2. The same on the DOIT board, confirming the plain-LED path still lights
   GPIO2.
3. Boards ▸ Devkit against a real board, checking a handful of header rows
   against the silkscreen.
4. `board_pinout` from an Assistant session, on both an Arduino and an ESP-IDF
   project, confirming the inferred-board note appears.

The bench pass owed by 0.19.0 is still owed as well.
