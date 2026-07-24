# Bancada Scope — architecture & wire contracts

A software oscilloscope with two sources feeding one TypeScript engine:

- **Plotter** — parses numeric values from the existing serial monitor stream
  (`serial://line` events). Works with any board, runs alongside the monitor.
- **ADC** — companion firmware on an ESP32-family board streams raw 12-bit
  ADC samples over serial. Rust reads the port raw (`serialport` crate),
  validates frames, and forwards binary batches over a Tauri **Channel**.

```
ESP32 firmware ──serial──▶ Rust scope reader ──Channel (binary)──┐
                            (sync/CRC/seq only)                  ▼
arduino-cli monitor ──▶ serial://line events ──▶  TS ScopeEngine (src/scope/)
                                                  rings · trigger · measure
                                                  · decimate · FFT worker
                                                        │
                                              ScopeView canvas (60 fps rAF)
```

All multi-byte integers everywhere are **little-endian**.
CRC everywhere is **CRC-16/CCITT-FALSE**: poly `0x1021`, init `0xFFFF`,
no reflection, no xor-out.

---

## 1. Device → host serial protocol (firmware output)

### 1.1 Binary frame

| offset | size | field |
|---|---|---|
| 0 | 2 | sync `0xA5 0x5A` |
| 2 | 1 | `type`: `0x01` SAMPLES, `0x02` META, `0x03` RECORD_HDR, `0x7F` ERROR |
| 3 | 1 | `flags`: bit0 dual-channel, bit1 reserved(0), bit2 last-in-record, bit3 overflow-occurred |
| 4 | 2 | `seq` u16, increments per frame, wraps |
| 6 | 2 | `payload_len` u16, ≤ 2048 |
| 8 | 4 | `first_sample_index` u32 (SAMPLES: index of first sample since capture start; others: 0) |
| 12 | N | payload |
| 12+N | 2 | CRC-16/CCITT-FALSE over bytes `[2, 12+N)` (type through payload) |

- **SAMPLES** payload: array of u16. Bits 0–11 = raw ADC counts,
  bits 12–15 = channel index (0 or 1). Single-channel capture uses nibble 0.
- **META** payload: UTF-8 JSON
  `{"ev":"meta","sps":50000,"pins":[1],"atten":3,"width":12,"mode":"stream","overflows":0,"cal":[[0,0],[256,142],...]}`
  — `sps` is the **actual** rate granted by the driver; `cal` is a 17-point
  `[raw, millivolts]` table (raw 0..4096 step 256) for the active attenuation.
  **`sps` is per-channel, not aggregate.** With two pins the ADC runs at
  `sps * 2` and each pin is sampled `sps` times per second, so the host uses
  `dt = 1/sps` for every channel regardless of channel count. The chip windows
  quoted in §1.3 are aggregate, so the firmware clamps to
  `min(req, chipMax/nch, linkMax/nch)` before reporting.
  Sent right after `start`, on config change, and every ~1 s (counters updated).
- **RECORD_HDR** payload: UTF-8 JSON
  `{"ev":"record","trigger_idx":2048,"pre":2048,"post":6144,"sps":1000000,"pin":34}`
  — announces a single-shot record; SAMPLES frames follow, final one carries
  flag `last-in-record`. `trigger_idx` is the sample offset of the trigger
  point within the record.
- **ERROR** payload: UTF-8 JSON `{"ev":"err","msg":"..."}`.

### 1.2 Text banner (handshake)

On boot and in reply to `{"c":"id"}` the firmware prints one ASCII line:

```
!BANCADA {"proto":1,"chip":"esp32s3","fw":"0.1.0","max_sps":83333,"max_stream_sps":83333,"pins":[1,2,3,4,5,6,7,8,9,10],"atten":[0,1,2,3],"maxch":2}
```

- `proto` must be `1`; host rejects mismatches with a "reflash firmware" message.
- `pins` = usable **ADC1** GPIOs for this chip.
- `max_stream_sps` = what the transport sustains continuously
  (firmware computes: native USB CDC → `max_sps` capped at ADC limit;
  UART → `baud/10/2` bytes budget). `max_sps` = single-shot burst ceiling.
- The banner is the only unsolicited text; it never appears mid-binary-stream
  (firmware stops streaming before answering `id`).

### 1.3 Host → device control plane

Newline-delimited JSON, one command per line:

| command | fields |
|---|---|
| `{"c":"id"}` | — |
| `{"c":"start","sps":50000,"pins":[1],"atten":3}` | 1–2 pins; continuous stream |
| `{"c":"stop"}` | stop streaming, stay idle |
| `{"c":"single","sps":1000000,"pin":34,"atten":3,"pre":2048,"post":6144,"level":2048,"edge":"r"}` | device-side trigger burst; `level` raw counts 0–4095, `edge` `"r"`/`"f"` |

Rules: firmware answers `start`/`single` with a META frame (or ERROR);
requests exceeding chip/transport limits are clamped and the META echoes the
real values. `single` re-arms only on a new `single` command.
Chip capability facts (from ESP-IDF `soc_caps.h`): classic ESP32 continuous
ADC spans 20 kHz–2 MHz; S3/C3 span 611 Hz–83.333 kHz aggregate. Requests
below the low bound are served by on-device decimation of the lowest legal rate.
Firmware uses the ESP-IDF `adc_continuous` API directly (`esp_adc/adc_continuous.h`)
— **not** `analogContinuousRead()`, which averages frames. Serial is opened at
921600 on UART-bridge boards; baud is irrelevant on native USB CDC.

---

## 2. Rust → frontend Channel envelope

`scope_start` takes a `tauri::ipc::Channel<InvokeResponseBody>` and sends
`InvokeResponseBody::Raw(Vec<u8>)` messages (frontend receives `ArrayBuffer`,
or `number[]` on some platforms — decode both):

| kind byte | layout |
|---|---|
| `0x01` samples | `u8 kind, u8 flags (as §1.1), u32 first_sample_index, u16 nsamples, u16[nsamples] samples` |
| `0x02` json | `u8 kind`, rest UTF-8 JSON event |

JSON events forwarded/injected by Rust:

- `{"ev":"meta",...}` / `{"ev":"record",...}` / `{"ev":"err","msg":...}` — forwarded from device
- `{"ev":"drop","frames":N}` — injected on seq gap
- `{"ev":"crc","count":N}` — injected on CRC failures (periodic, not per-frame)
- `{"ev":"closed"}` — reader thread exited (port vanished, stop, error)

Rust batches: forward each valid device frame as one channel message
(device frames are already ≤2048 B ≈ ≤20 ms of data — no re-batching needed).

---

## 3. Tauri commands (src-tauri)

`AppState.monitor: Mutex<Option<Child>>` is **generalized**:

```rust
enum SerialOwner { Monitor(Child), Scope(ScopeSession) }
struct AppState { cli: ArduinoCli, serial: Mutex<Option<SerialOwner>> }
// ScopeSession: writer half (Box<dyn SerialPort>), stop: Arc<AtomicBool>, JoinHandle
```

Acquiring the port for either owner **evicts** the other (kill child / set stop
flag + join). `monitor_send` errors when owner is Scope and vice versa.
Existing commands `start_monitor`/`stop_monitor`/`monitor_send` keep their
exact signatures and event names.

New commands (all return `Result<_, String>`):

```rust
scope_probe(port: String) -> ScopeCaps
// Opens port (921600 then 115200), asserts DTR/RTS, tolerates reset garbage,
// sends {"c":"id"} up to 3× with 300 ms timeout, parses !BANCADA banner.
// ScopeCaps { baud: u32, proto: u32, chip: String, fw: String, max_sps: u32,
//             max_stream_sps: u32, pins: Vec<u8>, atten: Vec<u8>, maxch: u8 }

scope_start(port: String, baud: u32, cfg: ScopeStreamCfg,
            on_message: Channel<InvokeResponseBody>)
// ScopeStreamCfg { sps: u32, pins: Vec<u8>, atten: u8 }
// Evicts current owner, opens port, sends start cmd, spawns reader thread
// (sync-scan → CRC → seq-track → channel.send). Stores ScopeSession.

scope_single(cfg: ScopeSingleCfg)
// ScopeSingleCfg { sps: u32, pin: u8, atten: u8, pre: u32, post: u32,
//                  level: u16, edge: String }
// Requires active Scope session; writes the single command line.

scope_send(line: String)      // raw control line to scope port (escape hatch)
scope_stop()                  // stop + join reader, drop session, sends {"c":"stop"} first
scope_install_firmware(dest_dir: String) -> String
// Writes embedded firmware (include_str!) to dest_dir/bancada_scope/,
// returns the sketch dir path. Frontend then uses existing compile/upload.

save_text_file(path: String, contents: String)
save_binary_file(path: String, contents_b64: String)   // base64 payload (PNG export)
```

`bancada-core` gets `core/src/scope.rs`: pure, unit-tested frame
scanner/decoder (`FrameScanner::push(&[u8]) -> Vec<Frame>`), CRC16, banner
parser, control-command serializers. The Tauri layer owns threads/IO only.
New Cargo deps: `serialport` (core), `base64` (src-tauri).

---

## 4. TS engine contract

`src/scope/types.ts` (written, authoritative) defines the shared interfaces:
`ScopeEngine` public API, `RenderFrame`, `TriggerConfig`, `Measurements`,
`ScopeEvent`, etc. The DSP implementation (`src/scope/*.ts`) implements them;
the UI (`src/components/ScopeView.tsx`) consumes only `types.ts` + `engine.ts`
+ `api.ts`.

Key behaviors:

- **Rings**: power-of-two `Float32Array` per channel (2^20 samples ADC,
  2^14 plotter), values stored in **volts** (ADC counts converted via META cal
  table interpolation) or raw plotter units.
- **Plotter timing**: sample rate estimated by EMA of arrival intervals;
  each parsed line = one sample per named channel.
  Text format: Arduino Serial Plotter conventions — `label:value` pairs or
  bare numbers, separated by space/comma/tab; labels persist as channels.
- **Trigger** (host side, both sources): arm/fire hysteresis state machine,
  modes auto/normal/single, holdoff, position 0–100 % pre-trigger, sub-sample
  linear interpolation offset in the RenderFrame. Auto-timeout
  `max(100 ms, 2× window)` forces an untriggered sweep.
  ADC single-shot records (device-triggered) bypass the host trigger: the
  record is displayed as a frozen capture with `trigger_idx` centered per
  position setting.
- **RenderFrame**: per visible channel, min/max pairs decimated to
  ≤ 2 × canvas width, plus trigger state (`auto|trigd|wait|stop`), t0, dt.
- **Measurements**: min, max, vpp, mean, rms, freq (zero-crossing at 50 %
  level with hysteresis + interpolation, averaged over all periods; `null`
  when < 2 periods or unstable), period, +duty. Computed over the displayed
  window.
- **FFT**: in-repo radix-2, sizes 1k–8k, Hann/Rect/Flat-top, dB magnitudes,
  run in a Web Worker with transferable buffers.
- **Export**: `exportCsv()` → header rows (channel names, dt, t0) + rows,
  Rigol-style; PNG handled by the UI via `canvas.toBlob` + `save_binary_file`.

## 5. UI integration

- `App.tsx`: `mainView: "editor" | "scope"`; Toolbar gains a **Scope** toggle
  button. Scope view replaces `.editor-area` (sidebar/bottom stay).
- Plotter source: App forwards `serial://line` payloads to
  `engine.feedText()` while the scope view is on and source = plotter
  (monitor keeps running — shared stream).
- ADC source: Probe button → `scope_probe`; if no firmware detected, offer
  "Install companion firmware" (→ `scope_install_firmware` into scratch dir →
  existing compile/upload with chip profile chosen by user). Start/stop
  streaming → `scope_start`/`scope_stop`. Single → `scope_single`.
- Scope canvas: single `<canvas>`, rAF loop, draws grid/divisions, traces
  from `RenderFrame`, trigger level line (draggable), cursors (X1/X2, Y1/Y2,
  draggable, ΔX + 1/ΔX + ΔY readout), channel colors CH1 `#e8c34a` (yellow),
  CH2 `#4ac3e8` (cyan). Status word Auto/Trig'd/Wait/Stop, color-coded.
- Space = Run/Stop when scope view focused.

## 6. Verification

- `cargo test` — scope.rs decoder against hand-built byte vectors (incl. CRC
  corruption, split frames, resync mid-garbage, seq wrap).
- `npx vitest run` — trigger state machine, ring, text parser, measure
  (synthetic sine), FFT (peak bin of known tone).
- `arduino-cli compile` firmware profiles `esp32`, `esp32s3`, `esp32c3`.
- `npm run build` (tsc) and `cargo build` clean.
