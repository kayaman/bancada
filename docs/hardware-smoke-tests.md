# Bancada hardware smoke tests — plan

Every path in Bancada that actually touches a board is currently unverified.
Unit tests cover the pure layers (`sketch.yaml` handling, the scope frame
decoder, the DSP engine) and `arduino-cli compile` proves the firmware builds,
but nothing has confirmed that Bancada can detect a board, put code on it, talk
to it, and read it back.

This plan defines an opt-in suite that does exactly that against a real,
**disposable** ESP32, using a blink sketch as the canonical smoke test.

## Safety: these tests erase the board

The suite flashes the attached board. Anything already on it is gone.

- Use a **dedicated spare board**, never one running something you care about
  (the plant monitors, the mesh nodes, anything deployed).
- The suite refuses to run unless an explicit environment variable names the
  port, so it can never fire by accident from a bare `cargo test`.
- The board is left running the last fixture flashed, not restored to its
  previous contents — there is no backup step and there cannot be one.

## Principles

1. **Opt-in, never default.** Gated behind `#[ignore]` *and* an env var. CI and
   ordinary `cargo test` runs skip it silently.
2. **Serial, never parallel.** The port has exactly one owner; the whole suite
   runs single-threaded.
3. **Self-verifying.** No step may depend on a human watching an LED.
4. **Idempotent.** Each test flashes what it needs and asserts from a known
   state, so tests can run in any order or individually.
5. **Fail loud on setup, skip quiet on absence.** A missing board is a skip; a
   present board that misbehaves is a failure.

## The verification problem

"Flash blink and check the LED" is the obvious smoke test and the obvious
trap: a test that needs eyes is not a test. Two mechanisms replace the human.

**Serial heartbeat as the oracle.** The blink fixture toggles the LED *and*
prints a correlated line:

```
BLINK 0 on
BLINK 0 off
BLINK 1 on
...
```

The assertion is on that stream — monotonic counter, alternating state, arriving
at roughly the expected cadence. The LED is then a bonus for a human who happens
to be looking, not the thing under test.

**The board measures itself.** For the scope (tier 2), the board generates a
known signal on one GPIO and reads it back on an ADC pin through a single
jumper wire. The scope's own measurement is asserted against the frequency the
board was told to produce, which exercises firmware, transport, decoder and DSP
in one shot.

Timing assertions must stay generous. Host-side timestamps on a USB-serial
stream carry buffering jitter of tens of milliseconds, and native-USB CDC boards
batch writes. Assert on **ordering and count within a window** (e.g. "8–12
toggles in 5 s"), not on precise inter-arrival times.

## Tier 1 — bare board, no wiring

Everything here needs only a USB cable.

| # | Test | Asserts |
|---|---|---|
| 1 | **Port detection** | `arduino-cli board list` reports the configured port; if the board is auto-identified, the detected FQBN matches the configured one |
| 2 | **Compile** | The blink fixture compiles for the configured profile, producing a binary |
| 3 | **Upload** | Upload completes, exit status clean, and the streamed output reaches the expected "hard resetting" terminus |
| 4 | **Blink heartbeat** | After upload, the serial stream yields a monotonically increasing `BLINK n on/off` sequence at the expected cadence |
| 5 | **Serial TX round-trip** | A line sent to the board comes back echoed, proving the write path and `monitor_send` |
| 6 | **MAC / chip read** | esptool returns a syntactically valid MAC and a chip type consistent with the configured FQBN |
| 7 | **Monitor ↔ upload contention** | With the monitor running, an upload evicts it, succeeds, and the monitor can be restarted afterwards — the `SerialOwner` eviction path on real hardware |

Test 7 is the one most likely to expose a genuine bug: it is the only place the
single-owner rule meets a real port that can be held open by a child process.

## Tier 2 — one jumper wire

Wire one PWM-capable GPIO to one ADC1 pin. No other components.

| # | Test | Asserts |
|---|---|---|
| 8 | **Companion firmware install + flash** | `scope_install_firmware` writes the sketch, it compiles, and it uploads |
| 9 | **Banner handshake** | `scope_probe` returns `proto: 1` and capabilities consistent with the chip |
| 10 | **Known-signal capture** | With a 1 kHz square wave driven into the ADC pin, the scope's measured frequency is within a few percent |
| 11 | **Scope ↔ monitor eviction** | Starting the scope evicts a running monitor and vice versa |
| 12 | **Single-shot record** | A device-triggered `single` returns a record whose trigger sample sits at `trigger_idx` |

**Signal choice.** Use LEDC PWM, not the DAC: classic ESP32 has a DAC on
GPIO25/26 but the S3 and C3 do not, so PWM is the only portable generator. A
0–3.3 V square wave will clip against the ADC's ~3.1 V ceiling even at 11 dB
attenuation, so **assert frequency and period, not amplitude** — frequency is
robust to clipping, amplitude is not. Amplitude accuracy needs a resistor
divider and belongs in a later, deliberately-wired fixture.

## The onboard-LED trap

"Blink the board LED" is not portable across the ESP32 family:

- Classic **ESP32** devkits: a plain LED on a GPIO (commonly 2). Bare modules
  frequently have no user LED at all.
- **ESP32-S3** and **ESP32-C3** devkits: usually a single addressable WS2812
  RGB LED, not a GPIO-driven one. Pin varies by board and revision.

arduino-esp32 3.x papers over some of this (`RGB_BUILTIN`, `rgbLedWrite()`, and
a `digitalWrite` shim for the RGB pin), but coverage is per-variant and some
boards define no LED macro whatsoever. The fixture therefore:

- branches on `RGB_BUILTIN` / `LED_BUILTIN` at compile time,
- compiles and runs correctly on a board with **no** LED,
- and never lets LED availability affect pass/fail — the serial heartbeat is
  the oracle.

## Harness

Rust integration tests, because the layer under test is `bancada-core` driving
the real binaries.

```
core/tests/hardware.rs              # the suite, every test #[ignore]
core/tests/fixtures/blink/          # tier 1 sketch + sketch.yaml
core/tests/fixtures/adc_signal/     # tier 2 signal generator
```

Configuration comes from the environment so no machine-specific paths are
committed:

| variable | required | meaning |
|---|---|---|
| `BANCADA_HW_PORT` | yes | serial port, e.g. `/dev/ttyACM0`. Absent ⇒ whole suite skips |
| `BANCADA_HW_FQBN` | yes | e.g. `esp32:esp32:esp32s3` |
| `BANCADA_HW_PROFILE` | no | `sketch.yaml` profile name, defaults per FQBN |
| `BANCADA_HW_SIG_PIN` | tier 2 | PWM output GPIO |
| `BANCADA_HW_ADC_PIN` | tier 2 | ADC1 GPIO wired to `SIG_PIN` |

Invocation:

```bash
BANCADA_HW_PORT=/dev/ttyACM0 \
BANCADA_HW_FQBN=esp32:esp32:esp32s3 \
cargo test -p bancada-core --test hardware -- --ignored --test-threads=1
```

`--test-threads=1` is not optional: concurrent tests would fight over the port.
The suite additionally serialises through a process-wide mutex so that a
forgotten flag degrades into slowness rather than flaky failures.

Prerequisites the harness checks up front, failing with an actionable message
rather than a confusing timeout: `arduino-cli` on PATH, the `esp32:esp32`
platform installed, `esptool` present, and the port readable (the `dialout`
group question on Linux).

## CI

These tests do not belong in ordinary CI — no board, no run. Two realistic
options, in order of cost:

1. **Manual pre-release checklist** (near term). One documented command, run
   against a spare board before tagging a release. This is the honest answer
   for a single-maintainer project.
2. **Self-hosted runner with a board attached** (later). A scheduled job on the
   homelab Pi with a dedicated ESP32 on USB. Worth it only once the suite is
   stable enough that a failure means a real regression.

## Definition of done

- One documented command runs the full tier-1 suite against a named board and
  passes end to end.
- Tier 2 passes with the jumper wire in place and is clearly documented as
  requiring it.
- The suite skips cleanly, with an explanatory message, when no board is
  configured.
- README gains a short "hardware testing" section pointing here.
- The items in WDN-621 that these tests actually cover are closed, and the ones
  they cannot cover (eFuse calibration accuracy, sustained 2 MSa/s drain) are
  explicitly called out as still open.
