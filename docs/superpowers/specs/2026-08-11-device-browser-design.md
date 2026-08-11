# Device browser: browse a board's HTTP UI with a request log

**Date:** 2026-08-11
**Status:** Approved design

## Problem

Bench nodes serve HTTP — the UNO Q's web status page, ESP32 endpoints — and
debugging them means leaving Bancada for a browser with devtools. Nothing in
the workbench can render a device page, and nothing can show the HTTP
traffic (the polling calls, their timing, the JSON they return) that
debugging actually needs.

## Design

### A "Web" tab in the observability group

New bottom tab `web` beside MQTT and WS (`bottomTabs.ts` model + D1 test).
The panel is a `WsPanel` sibling: URL bar with localStorage history and
`<datalist>`, connection chip, and the page itself in an `<iframe>` filling
the panel (`bottomMax` gives it the whole main area). Lazy-mounted once,
hidden with `display:none` — the device page survives tab switches.

### Loopback logging reverse-proxy

The iframe never points at the device. `device_browse_start(url, channel)`
binds a `tiny_http` server on `127.0.0.1:0` (the MCP listener's pattern:
port-0 bind, `Server::unblock()` shutdown, the documented listener-thread
contract) and returns the port; the iframe loads `http://127.0.0.1:<port>/`.
The proxy forwards each request to the device base URL and streams one
event per exchange over the invocation `Channel` (the `mqtt_connect`
shape): method, path, status, duration, content-type, request/response
sizes, capped body preview. Every byte passes through Rust, so the log is
complete — and loopback origin sidesteps CORS and the production
custom-protocol/mixed-content problem entirely.

Outbound client: `ureq` (new src-tauri dependency; blocking, matches the
per-request thread model — `tiny_http` is server-only and the workspace has
no HTTP client).

### Security posture

Single-target: the proxy forwards only to the base URL the user set via
invoke — the loopback port cannot be steered by HTTP to arbitrary hosts, so
it is not a general LAN pivot. Residual exposure (another local process
browsing the *currently chosen* device through the port while the panel is
open) is accepted for a bench tool and documented in the code. Target
changes and stop go through commands; `device_browse_stop` unblocks and
drops the listener.

### Log UI

`ObsStore(500)` + `ObsLog` verbatim — pause, filter, clear, autoscroll,
click-to-expand with JSON pretty-print and hex for binary. 4 Hz poll only
while the tab is active; the ring keeps filling while hidden.

### Known limitations (stated, not hidden)

- WebSocket upgrades do not pass through `tiny_http`: a device page using
  WS degrades; raw WS debugging stays in the WS tab.
- Absolute links to *other* hosts inside a device page escape the proxy
  (render as broken); bench device pages are self-contained.

## Testing

- Core-pure helpers unit-tested: target-URL validation/normalization,
  hop-by-hop header filtering, body-preview capping (char-boundary safe).
- Frontend: bottomTabs D1 grouping, api contract tests, event-union
  mapping into ObsStore entries.
- E2E: a scratch `python3 -m http.server`-style fixture as the "device";
  start proxy, fetch through it, assert forwarded response and log events.
