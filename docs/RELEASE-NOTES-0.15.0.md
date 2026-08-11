# bancada 0.15.0

The workbench learns to browse. Bench devices serve HTTP — the UNO Q's web
UI, ESP32 endpoints — and debugging them meant leaving Bancada. Now there's
a **Web** tab: the device's page rendered inside the workbench, with every
HTTP request it makes logged underneath.

## Device browser

- **Web tab** in the observability group, beside MQTT and WS: type
  `http://unoq.local` (or an ESP32's IP), hit Go, and the device's page
  renders in the panel. URL history, reload, and a stop button; the page
  survives tab switches.
- **Complete request log**: every request the page makes — the polling
  calls, the JSON they return, their timing — streams into the same
  pausable, filterable log the MQTT/WS tabs use. Rows read
  `GET /data.json → 200 (12 ms, 42 B)` and expand to the response body
  (JSON pretty-printed, binary as hex). Toggle the log away when you just
  want the page.
- **How it works, and why it's trustworthy**: the page is served through a
  loopback proxy inside Bancada, so the log is complete by construction —
  every byte passes through it. The proxy forwards only to the device you
  chose (it cannot be steered elsewhere by page content), a device's own
  4xx/5xx pass through honestly, and an unreachable device shows a clear
  502 with the target named.
- **Known limits** (by design): WebSocket-based pages degrade — raw WS
  debugging lives in the WS tab; links to other hosts don't resolve
  through the proxy. Bench device pages are self-contained.
