# bancada 0.17.0

Boards remember which project was flashed to them and offer it back when you
plug them in. Three new starter sketches, one of which makes the oscilloscope
draw with no wiring at all. And the serial monitor stops dying.

## Your board knows where it came from

0.16.0 gave every flash a `flash/<stamp>` git tag, so a *repository* recorded
what went to a board. The board recorded nothing. Pick one out of a drawer,
plug it in, and Bancada would name it and then leave you to remember which of
thirty folders that firmware came from.

Each remembered board now carries its last flash — project, tag, branch,
commit and when. Plug it in and a banner offers that project back, with how
far it has moved since: *"3 commits ahead of flash/2026-08-10T0915"*.

**Offered, never taken.** Opening a project discards unsaved buffers and stops
a running Assistant session, and this offer appears unbidden — possibly
mid-keystroke. So the first Open click explains what would be lost and the
second commits, the same way closing a dirty tab already works. It never
touches git: drift is reported, never checked out. Your working tree is yours,
and a board being plugged in should not move it.

The offer stays quiet when the project is already open, when you have waved it
away, and for a minute after flashing that board — a flash resets the board
and it re-enumerates, so without that last rule the act of flashing would
offer you the project you are already in, every single time.

Boards behind a plain USB-serial bridge cannot carry a record: they report the
bridge's identity, not their own, so there is nothing to attach it to. That
limit is the same one the Fleet panel already explains.

## Three starters that work anywhere

New projects begin as one of seven sketches rather than four:

- **Waveforms** — sine, triangle and sawtooth generated in software and
  printed in plotter format. No wiring, no ADC. It is the fastest way to see
  the Scope draw, and it cleanly separates "the scope is broken" from "there
  is no signal on the pin".
- **Analog plot** — one ADC pin, raw against a smoothed copy, so the filter's
  effect sits next to its input. A potentiometer is enough.
- **Serial echo** — numbers back whatever you type, with an idle heartbeat.
  Blink proves the board can talk; this proves it listens.

All three compile unchanged on `arduino:avr:uno`, `esp32:esp32:esp32s3`,
`esp8266:esp8266:nodemcuv2` and `arduino:zephyr:unoq` — verified against the
real toolchain, with Blink as the control.

## The serial monitor stops dying

Two independent causes, both of which produced the same "it just stopped"
with no error.

**One bad byte ended capture for good.** The reader threads dropped out of
their loop on the first line that was not valid UTF-8 — and a serial device
emits arbitrary bytes. The monitor child stayed alive still holding the port
while the console sat silent. This arrives on ordinary hardware: an ESP32's
ROM bootloader prints at 74880 baud and reads as garbage at 115200, a reset
mid-`print` truncates a sequence, a long cable picks up noise, and any wrong
baud turns the whole stream invalid. Lines are decoded lossily now.

**A lost port was never chased.** A native-USB board re-enumerates on every
reset — the device drops off the bus and returns about two seconds later — and
the monitor child dies with it. Nothing restarted it, because the restart only
ran when the selected port *changed*, and the port comes back at the same
address. Capture is now retried on a backoff ladder. Stopping the monitor
yourself, or letting the scope or a flash take the port, is never fought.

That is also why it looked random: a board behind an FTDI or CP2102 bridge
never shows either fault, because the bridge stays enumerated across a board
reset.

## Fixes

- The serial commands' lock now recovers from poisoning, which the
  architecture docs already claimed for every session mutex and which was true
  of every one except this.

## Notes

Verified on this cut: `cargo test --workspace` 650 passed, `vitest` 646
passed, `tsc --noEmit` clean, and **every** opt-in suite green against a real
`arduino-cli` and an attached ESP32 — `core_list_real`, `gh_fetch`,
`fleet_real`, `scaffold_compiles`, `new_project_builds`.

**Not verified on hardware, and worth knowing before you rely on it:** the
serial-monitor recovery and the board-offer banner are proven by unit tests
and by reading, not by watching a board disconnect and come back. The two
checks that matter most are unplugging and replugging a native-USB board to
see capture return, and flashing twice in a row to confirm the offer stays
quiet the second time.

Also still open, found while fixing the above: `serial://closed` carries no
session identity, so a dying reader thread from a previous monitor can report
a live one as closed. The visible symptom is not a dead console but a
mysterious upload failure, because freeing the port before a flash is decided
on that flag. The agent path solved the same problem by stamping its events
with a pid; the serial path has no equivalent yet.
