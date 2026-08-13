# bancada_webscope

An oscilloscope the **board** serves over HTTP. Bancada shows it in the Web
tab: the device-browser proxy loads the board's page into the iframe and logs
every request alongside it.

This is the WiFi counterpart to `bancada_scope`, which streams samples over
serial into Bancada's own canvas. Neither replaces the other — this one wins
when you want a scope on any screen on the network and no cable; the serial
one wins when there is no WiFi, or when you want the FFT, cursors and CSV
export in the app.

## Wiring

| Signal | ESP32-S3 / C3 | classic ESP32 |
|---|---|---|
| Input | GPIO4 | GPIO32 |
| Test generator | GPIO5 | GPIO25 |

Jumper the generator pin to the input pin and you have a signal to look at
with nothing else on the bench — a 1 kHz square wave at 50 % duty by default,
adjustable from the page.

The generator pin **cannot** also be the input: once LEDC drives a pad, the
ADC input path to it is gone and the channel reads a flat zero (measured on an
S3, not assumed). Selecting the generator pin as input therefore switches the
generator off rather than showing a dead trace.

Input range is 0–3.3 V at the pin. Anything outside that needs a divider
ahead of it — set the page's probe factor to match and the readings scale.

## Getting it on the network

Credentials are not compiled in. They arrive over serial and live in NVS, so
this is a one-time step per board. Paste into Bancada's serial monitor:

```json
{"c":"wifi","ssid":"your-network","pass":"your-password"}
```

The board answers with a banner naming its address:

```
!BANCADA-WEBSCOPE {"fw":"1.0.0","chip":"esp32s3","mode":"sta","ssid":"...","ip":"192.168.15.2","url":"http://192.168.15.2","mdns":"http://bancada-scope.local"}
```

Put that `url` in the Web tab and press Go.

With no stored credentials — or when the stored ones fail — the board raises
its own access point (`bancada-scope-XXXX`, password `bancada123`) and serves
the same page at `http://192.168.4.1`, so it is never unreachable.

Other serial commands: `{"c":"id"}` reprints the banner, `{"c":"forget"}`
drops the credentials, `{"c":"ap"}` forces AP mode for this boot.

**The banner goes to both serial ports.** With `CDCOnBoot=cdc` the sketch
writes to `Serial` (native USB) *and* `Serial0` (UART0), and reads commands
from both, so provisioning works from whichever of a DevKit's two USB-C
sockets happens to be plugged in.

## HTTP surface

| route | purpose |
|---|---|
| `GET /` | the page — HTML, CSS and JS in one response, no external assets |
| `GET /info` | capabilities, current settings, and a 17-point `[raw, mV]` calibration table |
| `GET /data` | one triggered frame: 512 raw u16 counts, little-endian, with `X-Fs` and `X-Pin` headers |
| `GET /cfg?…` | `sps`, `pin`, `atten`, `trig`, `level`, `gen`, `genhz`, `genduty`; replies with `/info` |

Raw counts go over the wire and the page interpolates the calibration table,
rather than the board converting 512 samples per frame through the curve.

The DMA pool is flushed before every capture. Without that, a poll picks up
whatever the ring happened to hold since the last request, and the trace you
are looking at can be seconds old.

## Limits

Sample rate is bounded by the chip: 611 S/s – 83.333 kS/s on the S3 and C3,
20 kS/s – 2 MS/s on the classic ESP32. Ten points per cycle is the practical
floor for a recognisable wave, so 83 kS/s is about 8 kHz of usable bandwidth.

A capture at the slowest rates takes real time — 1536 samples at 611 S/s is
2.5 s — and the HTTP request is held open for it.

## Targets

`esp32`, `esp32s3`, `esp32c3` — arduino-esp32 3.x, ESP-IDF 5.x. All three
profiles are in `sketch.yaml`; `esp32s3` is the default.
