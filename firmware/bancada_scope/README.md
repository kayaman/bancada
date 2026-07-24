# bancada_scope — ESP32 companion firmware

Companion firmware for the Bancada software oscilloscope (ADC source).
Implements the device→host protocol from `docs/scope-architecture.md` §1
(proto 1): binary frames with CRC-16/CCITT-FALSE, the `!BANCADA` text banner,
and the newline-delimited JSON control plane (`id` / `start` / `stop` /
`single`).

## Build

From this directory (profiles pinned to `esp32:esp32@3.3.11` in `sketch.yaml`):

```sh
arduino-cli compile --profile esp32   .
arduino-cli compile --profile esp32s3 .
arduino-cli compile --profile esp32c3 .
```

No external libraries — JSON parsing, CRC and framing are hand-rolled.

## Implementation notes

- Sampling uses the ESP-IDF `adc_continuous` driver directly
  (`esp_adc/adc_continuous.h`). The Arduino `analogContinuousRead()` wrapper is
  deliberately avoided: it averages every conversion frame into one value.
- Rates: hardware span is `SOC_ADC_SAMPLE_FREQ_THRES_LOW..HIGH`
  (esp32: 20 kHz–2 MHz; esp32s3/esp32c3: 611 Hz–83.333 kHz aggregate).
  Requests below the low bound run at the lowest legal multiple and are
  integer-decimated on device; requests above the ceiling are clamped. META
  always echoes the effective per-channel rate.
- Dual channel: hardware conversion rate = 2× the per-channel rate; samples
  are emitted in DMA arrival order (interleaved), each tagged with its channel
  index in bits 12–15. `first_sample_index` counts kept samples across both
  channels combined.
- Transport: UART builds run at 921600 baud, so `max_stream_sps` is the
  `baud/10/2` byte budget (46080); native-USB builds
  (`ARDUINO_USB_CDC_ON_BOOT=1`) ignore baud and cap streaming at
  min(chip max, 83333). As pinned in `sketch.yaml`, the `esp32` profile is
  UART (that board's USB is an external bridge) while `esp32s3`/`esp32c3` use
  `CDCOnBoot=cdc` and therefore the native USB-Serial/JTAG path. Resulting
  banner values:

  | profile | branch | `max_sps` | `max_stream_sps` |
  |---|---|---|---|
  | `esp32`   | UART 921600 | 2000000 | 46080 |
  | `esp32s3` | USB-Serial/JTAG | 83333 | 83333 |
  | `esp32c3` | USB-Serial/JTAG | 83333 | 83333 |
- Backpressure: the firmware never blocks on Serial while streaming — if the
  TX buffer is full, whole SAMPLES frames are dropped (seq still advances so
  the host sees the gap), the overflow flag (bit 3) is set on the next frame,
  and the drop is counted in META `overflows` together with DMA pool
  overflows.
- Single-shot: pre-trigger ring + level/edge trigger with ±16-count
  hysteresis, evaluated on device. `pre+post` is capped at 64k samples
  (128 KB) and to free heap; RECORD_HDR echoes the clamped values.
  `trigger_idx` equals `pre` — the sample that crossed the level is the first
  post sample. The final META for a completed record carries `"done":true`.
- Calibration: curve fitting (S3/C3) or line fitting (classic ESP32) via
  `adc_cali`; if the scheme is unavailable the 17-point `cal` table falls back
  to a nominal linear ramp per attenuation (esp32 1100/1500/2200/3900 mV FS,
  s3/c3 950/1250/1750/3100 mV FS). The table's last point is raw 4096 — one
  step past full scale — so it linearly extends the 3840→4095 segment rather
  than repeating the 4095 value.

## Quirks

- The pinned profiles use UART (esp32) or USB-Serial/JTAG (s3/c3), both of
  which get an 8 KB TX buffer. If you override the S3 to `USBMode=default`
  (USB-OTG TinyUSB) the TX FIFO is only 64 bytes and writes drain through it
  with a 300 ms timeout, so sustained streaming throughput then depends on
  host polling.
- The driver may round `sample_freq_hz` slightly (integer clock dividers);
  the granted rate is not queryable, so META reports the requested effective
  rate.
