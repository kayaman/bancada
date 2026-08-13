# OTA on every Wi-Fi board, by default

**Date:** 2026-08-13
**Status:** Approved design

## Problem

Every board Bancada flashes is flashed over a cable. That is fine on the
bench and wrong everywhere else: a node behind a plant pot, on a ceiling, or
inside an enclosure has to be retrieved to receive a one-line fix. Nothing in
the repo mentions OTA — no template, no upload path, no fleet field.

The standing rule this spec establishes: **every sketch scaffolded for a
Wi-Fi-capable board is OTA-reachable from its first serial flash, without
anyone remembering to ask for it.** Radio-less targets (`arduino:avr`,
`arduino:zephyr`) are unaffected and scaffold byte-identically to today.

## Design

### Mechanism: ArduinoOTA push over mDNS

The sketch runs `ArduinoOTA` and advertises `_arduino._tcp`; arduino-cli
discovers it as a `protocol: "network"` port and its platform OTA tool pushes
the binary. This is chosen over an HTTP-pull or MQTT-triggered scheme because
it needs no server and no new wire format: `cli.rs:425 upload_args()` already
builds `compile -u -p <port>`, and an IP is a port string. The upload path is
unchanged.

Consequence accepted: the board must be on a segment where mDNS reaches it.

### `core/src/ota.rs` (new, pure)

Four concerns, none of which need a window, a runtime, or hardware — the
layering test from `docs/architecture/conventions.md`:

- **MAC ⇄ hostname codec.** `bancada-<12 hex>` in both directions.
- **Injection policy.** Whether an FQBN gets OTA, and which dialect
  (`esp32:esp32` and `esp8266:esp8266` differ in header and password API).
- **Transport resolution.** The port list → the address to flash.
- **The pre-OTA guard.** See "The one-way door" below.

### Firmware: a second `.ino` tab, not seven edited templates

Arduino concatenates every `.ino` in a sketch folder, so the OTA
implementation lands as its own `ota.ino` and `main.ino` gains two lines.
Templates carry `// {ota:setup}` and `// {ota:loop}` markers, rewritten as
`otaSetup();` / `otaLoop();` for a Wi-Fi FQBN and stripped otherwise.

The markers are comments, so the templates stay complete, core-agnostic
programs and the existing assertions at `project.rs:512` and `project.rs:557`
hold without modification.

`otaSetup()` associates with a bounded timeout and **returns regardless**;
`otaLoop()` services OTA and retries association on a slow interval. A sketch
whose AP is down still runs. OTA is a convention, not a tax on every sketch.

### Credentials

`secrets.h` — `WIFI_SSID`, `WIFI_PASS`, `OTA_PASSWORD` — written per project
at scaffold from bench values held in `settings.rs` (path-agnostic and
clock-free, like the other injected modules). Already covered by the
`.gitignore` `git.rs` writes and by the `tracked_secrets()` alarm, so it
cannot reach a commit by accident. This follows the `mqtt.rs:237` precedent
of bench credentials living in the config dir.

`ArduinoOTA` ships bundled with both the esp32 and esp8266 cores, so no
registry dependency is expected. If a profile build turns out not to see a
core-bundled library, the entry hangs off `project.rs:147
required_profile_libs()` — the same per-FQBN switch that pins RouterBridge for
the Uno Q. Which of those two it is goes on the verified-unknowns list.

### Identity: one record per board, both transports

`fleet::identify()` gains one arm; no schema change, no `FLEET_VERSION` bump.

```
serial  port → properties.serialNumber ─┐
                                        ├→ BoardId { kind: Mac } → one record
network port → hostname "bancada-<hex>" ─┘
```

Both go through the existing `normalize_mac()`. A network port whose hostname
does not parse yields `None` — unknown stays unknown, matching the strictness
that already makes `looks_like_mac()` reject a vendor serial. The flash record
gains an additive `transport` field on `#[serde(default)]`.

### Transport selection

Serial wins when cabled. USB is faster, cannot be cut mid-flash by a flaky
link, and is the recovery path for a board OTA put out of reach.

```
cabled + on network → /dev/ttyACM0   (serial)
on network only     → 192.168.1.42   (ota)
cabled only         → /dev/ttyACM0   (serial)
neither             → disabled, with a reason
```

Flashing runs `board list` with a lengthened `--discovery-timeout` so mDNS has
time to answer.

### The one-way door

OTA-flashing a sketch that lacks the OTA block removes the only route to the
board: it runs correctly and is never reachable again without physically
retrieving it. Serial flashing has no equivalent failure — a bad sketch over
USB is undone by the next one.

So an OTA upload is gated on a `core` policy check over the sketch text, which
refuses when nothing in it keeps the board reachable. The check is explicitly
**not sound** — any source-text rule has false negatives (OTA arriving via a
library it cannot see) and false positives (the call present but unreachable).
Its job is to catch the mistake that actually gets made, not to prove
reachability. Recovery when it happens anyway is a cable and a serial flash.

## Hardware-verified unknowns

Assumptions this design rests on that only the bench can settle. Each is
verified before the code that depends on it is trusted:

1. **`WiFi.macAddress()` equals the MAC arduino-cli reports as USB
   `serialNumber`**, per chip family. Both should be the base efuse MAC, but a
   family that offsets them would fork a board's fleet record silently — the
   same board arriving as two. The codec must know if so.
2. **Network ports appear in `board list --json` within the chosen discovery
   timeout**, and which field carries the hostname (`address`, `label`, or a
   property key). A bench run today returned serial ports only.
3. **esp8266 OTA dialect** — header and password API differences against
   `esp8266:esp8266 3.1.2`.
4. **Whether a `sketch.yaml` profile build resolves the core-bundled
   `ArduinoOTA`** without an explicit profile library entry.

## Testing

- `ota.rs` unit tests: codec round-trip, per-FQBN injection decisions (esp32,
  esp8266, `arduino:avr:uno`, `arduino:zephyr`), the transport table above
  including the `neither` reason string, and the guard's accept/refuse cases.
- `fleet.rs`: a network port resolving to the same record as its serial twin;
  an unparseable hostname yielding `None`.
- `project.rs`: scaffolds for a Wi-Fi FQBN write `ota.ino` + `secrets.h` and
  substitute both markers; a radio-less FQBN writes neither and strips both.
  Existing template assertions stay green unmodified.
- `cli.rs` stub-script suite: the discovery timeout reaches argv, and an IP
  passes through `upload_args()` unchanged.
- Opt-in hardware suite (`#[ignore]`d with a reason, per conventions): a real
  board flashed serially, discovered over mDNS, then flashed over OTA — the
  only test that proves the loop closes.

## Out of scope (YAGNI)

Persisted last-known IP on the fleet record; rollback / A-B partitions; staged
fleet-wide rollout; OTA for `arduino:avr` and `arduino:zephyr`; retrofitting
OTA into already-scaffolded projects.
