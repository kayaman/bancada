# bancada 0.18.0

Three things a bench asks for, and one long-standing trap closed: profiles can
pin board options, a board's project is reachable from the fleet, and the
oscilloscope can now live on the board itself.

## Board options in a profile

An FQBN can carry the board's menu choices — `esp32:esp32:esp32s3:CDCOnBoot=cdc`
— and Bancada could only ever express the bare `vendor:arch:board` form. So a
profile could not say `CDCOnBoot=cdc`, and on an ESP32-S3 the core's default
sends `Serial` to the UART0 peripheral instead of the native USB port. The
sketch compiles, the upload succeeds, the board runs, and the serial monitor
sits empty. Nothing in the build output mentions it.

A **Board options** popover next to the board picker now reads the real menu
from `arduino-cli board details` and writes the choice into the profile's
FQBN. Options left at the board's default are omitted, so the stored FQBN
names what you actually changed rather than seventeen defaults that churn on
every core update.

Alongside it, a warning fires when the selected port is native USB, the board
is a variant that *has* native USB, and the FQBN does not already enable CDC:

> Serial output will not reach /dev/ttyACM0: with CDCOnBoot left at its default
> this board sends Serial to the UART0 peripheral rather than the USB port…

It deliberately does not suggest using the other USB socket. A DevKit has two,
and on the board this was found with, the UART one prints nothing at all —
enabling CDC is the fix that works regardless of how a board is wired.

## An oscilloscope the board serves

New companion firmware, `firmware/bancada_webscope`. The board joins WiFi and
serves its own scope page; Bancada shows it in the **Web** tab, where the
device-browser proxy loads it into the iframe and logs every request beside
it. Any browser on the network is a second screen.

This does not replace `bancada_scope`, which streams samples over serial into
the app's own canvas. That one still wins with no WiFi, and it owns the FFT,
cursors and CSV export. The new one wins when you want a scope without a
cable.

- DMA continuous ADC, software edge trigger, per-chip result formats and
  eFuse calibration to millivolts
- A PWM test generator, so one jumper gives you something to look at
- **No credentials in source.** They arrive over serial
  (`{"c":"wifi","ssid":"…","pass":"…"}`), live in NVS, and a board with no
  stored network raises its own access point rather than being unreachable
- **The banner goes to both serial ports.** With `CDCOnBoot=cdc`, `Serial` is
  the native USB socket and `Serial0` is UART0 — two different USB-C sockets
  on a DevKit — so provisioning works from whichever cable is plugged in
- The DMA pool is flushed before each capture, so a poll draws a fresh frame
  rather than whatever the ring held from last time

Targets `esp32`, `esp32s3`, `esp32c3`. Input and generator pins are GPIO4 and
GPIO5 on the S3 and C3, GPIO32 and GPIO25 on the classic ESP32.

One idea died on the bench and is worth recording: pointing the input at the
generator pin would have made the scope self-demonstrating with no wiring at
all, but on an S3 a pad driven by LEDC has no ADC input path left and the
channel reads a flat zero. Selecting the generator as input now switches the
generator off instead of drawing a dead line.

## A board's project, from the fleet

Since 0.16.0 every flash writes a `flash/<stamp>` tag, and since the last cut
a board records what was flashed to it. Only the arrival banner ever read that
record, and once dismissed it had no way back into view — while the Fleet
panel, whose whole job is "what is this board", said nothing about it.

Every fleet card now carries its flash record: project name, tag, and how long
ago. Click it for the path, branch, short commit, flash time, and the drift
against the project's HEAD, with an **Open project** button.

The tag is reported, never checked out. Opening a folder is recoverable;
moving a working tree onto a tag is not. Opening reuses the arrival banner's
arm-then-confirm — the first click names what would be discarded, the second
commits — and the cost is checked at click time, so a file dirtied after the
card was expanded still counts. A record whose folder has since been deleted
disables Open with the reason rather than failing on the click.

Drift is fetched on expand and keyed to the record's own fields. Keyed on the
boards array instead — which `fleetSync` rebuilds on every 2 s port scan — it
would run `git rev-list` twice a minute for as long as a card stayed open; a
test pins the dependency list so that cannot come back.

## Notes

`cargo test --workspace` 658 passed, `vitest` 693 passed, `tsc --noEmit`
clean, and all 15 opt-in suites green against a real `arduino-cli`, the live
Claude CLI, and an attached ESP32-S3.

The web scope was verified on hardware: it joins from stored credentials on
boot, `/info` returns a real curve-fitted calibration table, `/data` returns
512 samples with the right headers, and `/cfg` clamps and echoes. Two bugs
were caught that way — an AP SSID of `bancada-scope-0000`, because
`WiFi.macAddress()` reads an STA interface that does not exist in AP mode, and
the generator-loopback idea above.

Not verified on hardware, and the honest gap in this cut: the board-options
popover, the silent-serial warning and the fleet flash card have all been
exercised by tests but never seen rendered. Nothing in this repository renders
a component.

Still open, unchanged from 0.17.1: `serial://closed` carries no session
identity, so a dying reader thread from a previous monitor can report a live
one as closed. The agent path solved the same problem by stamping a pid; the
serial path has no equivalent yet.
