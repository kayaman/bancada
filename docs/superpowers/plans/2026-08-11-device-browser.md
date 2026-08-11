# Device Browser (Web tab + logging proxy) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Browse a device's HTTP UI inside Bancada with a complete request/response log.

**Architecture:** A loopback `tiny_http` reverse proxy in src-tauri (MCP-listener pattern) forwards to a single user-set device URL and streams per-exchange events over a `Channel`; a `WsPanel`-style bottom tab renders the page in an iframe pointed at the proxy and feeds the events into the existing `ObsStore`/`ObsLog`.

**Tech Stack:** Rust (`tiny_http` existing, `ureq` new), Tauri 2 Channel IPC, React/TS, vitest + cargo test.

**Spec:** `docs/superpowers/specs/2026-08-11-device-browser-design.md`

## Global Constraints

- Core crate stays subprocess-free for this feature: pure helpers only (URL validation, header classification, preview capping); all IO in src-tauri.
- Proxy forwards ONLY to the invoke-set target; no HTTP-steerable redirection. Residual loopback exposure documented in a code comment.
- Reuse, don't reinvent: `bind_mcp_server`-style port-0 bind, `Server::unblock` lifecycle, `mqtt_connect`'s Channel shape, `ObsStore`/`ObsLog`, `WsPanel`'s URL-bar/history idiom.
- Every commit ends with the Co-Authored-By trailer used throughout this repo; shared checkout — stage only named files.
- Suites green after every task: `cargo test -p bancada-core --lib`, `cargo check -p bancada`, `npx tsc --noEmit && npx vitest run`.

---

### Task 1: `web` tab in the bottom-tab model

**Files:**
- Modify: `src/bottomTabs.ts`, `src/__tests__/bottomTabs.test.ts`

**Interfaces:**
- Produces: `BottomTab` includes `"web"`; `GROUP_OF.web === "obs"`; `GROUP_TABS.obs` ends with `"web"`; `TAB_LABEL.web === "Web"`.

- [ ] **Step 1:** Update the D1 grouping assertion in the test to expect `obs: ["mqtt", "ws", "web"]` and add label coverage; run `npx vitest run src/__tests__/bottomTabs.test.ts` — FAIL.
- [ ] **Step 2:** Add `"web"` to the union, `GROUP_OF`, `GROUP_TABS.obs`, `TAB_LABEL` ("Web").
- [ ] **Step 3:** `npx vitest run src/__tests__/bottomTabs.test.ts` — PASS (tsc will fail in App.tsx only after Task 5 wiring begins; run the full tsc there, not here).
- [ ] **Step 4:** Commit `feat: web tab joins the observability group`.

### Task 2: core pure helpers (`core/src/devproxy.rs`)

**Files:**
- Create: `core/src/devproxy.rs`; register `pub mod devproxy;` in `core/src/lib.rs`

**Interfaces (produces, exact):**
```rust
/// Validate + normalize a device base URL: http only, host required,
/// default port 80, path collapsed to "" (target is an origin).
pub fn parse_target(raw: &str) -> Result<Target>   // Target { host: String, port: u16 }
/// Hop-by-hop headers that must not be forwarded either direction.
pub fn is_hop_by_hop(name: &str) -> bool           // connection, keep-alive, te, trailer,
                                                   // transfer-encoding, upgrade, proxy-*
/// Cap a body preview at `max` bytes on a char boundary when valid UTF-8;
/// binary flagged so the UI hex-dumps. Returns (preview, truncated).
pub fn body_preview(body: &[u8], max: usize) -> (String, bool)
```

- [ ] **Step 1 (TDD):** tests first: parse_target accepts `http://unoq.local`, `http://192.168.0.7:8080/`, strips trailing path with an error naming why (`a device target is an origin — drop the path`), rejects https (clear message: proxy speaks plain http), rejects empty/garbage; is_hop_by_hop full list + case-insensitivity; body_preview UTF-8 boundary (multi-byte char straddling max), binary detection (NUL byte ⇒ binary), exact-max no-truncate. Run — FAIL.
- [ ] **Step 2:** Implement (no deps beyond std).
- [ ] **Step 3:** `cargo test -p bancada-core --lib devproxy` then full core suite — PASS.
- [ ] **Step 4:** Commit `feat: devproxy pure helpers — target parsing, header class, previews`.

### Task 3: proxy listener + commands (src-tauri)

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `ureq = { version = "2", default-features = false }` — no TLS needed), `src-tauri/src/lib.rs`

**Interfaces (produces):**
- `AppState` gains `device_browse: Mutex<Option<DeviceBrowse>>` (`DeviceBrowse { server: Arc<tiny_http::Server>, port: u16, target: Arc<Mutex<bancada_core::devproxy::Target>>, thread: JoinHandle<()> }`).
- Commands (register all in `generate_handler!`):
```rust
#[tauri::command] async fn device_browse_start(state, url: String, on_event: Channel<InvokeResponseBody>) -> Result<u16, String>  // returns proxy port; stops any previous instance first
#[tauri::command] fn device_browse_set_target(state, url: String) -> Result<(), String>
#[tauri::command] fn device_browse_stop(state) -> Result<(), String>
```
- Event envelope (serde tag "type", mirrors mqtt's): `stage {stage: "listening", port}`, `exchange {method, path, status, duration_ms, content_type, req_bytes, resp_bytes, preview, truncated, binary}`, `error {path, message}`, `closed {}`.

- [ ] **Step 1:** Read `mcp_listener_loop`/`bind_mcp_server`/`stop_agent_session` and the listener-thread contract comment; mirror them: bind port 0, spawn one listener thread; per request — read method/path/headers/body (cap request body at 1 MiB → 413), forward via `ureq` to `http://{target.host}:{target.port}{path}` with non-hop-by-hop headers, stream response back with non-hop-by-hop headers preserved (status included), emit one `exchange` event with `body_preview(resp, 2048)`; device unreachable → 502 with a plain-text body naming the target + `error` event.
- [ ] **Step 2:** `device_browse_start`: validate via `parse_target` BEFORE binding; replace any existing instance (unblock + join). `set_target`: validate, swap the Mutex. `stop`: unblock, join, clear state; also hook the existing window-close cleanup path where the MCP server is stopped.
- [ ] **Step 3:** Rust tests where pure (the envelope serde shapes; a `forward_headers` helper if extracted). `cargo check -p bancada` + core suite green.
- [ ] **Step 4:** Commit `feat: loopback device proxy — single-target forwarder with exchange events`.

### Task 4: typed API wrappers

**Files:**
- Modify: `src/api.ts`; Test: `src/__tests__/api.test.ts`

**Interfaces (produces):**
```ts
export type DeviceBrowseEvent =
  | { type: "stage"; stage: "listening"; port: number }
  | { type: "exchange"; method: string; path: string; status: number; duration_ms: number;
      content_type: string | null; req_bytes: number; resp_bytes: number;
      preview: string; truncated: boolean; binary: boolean }
  | { type: "error"; path: string; message: string }
  | { type: "closed" };
export const deviceBrowseStart = (url: string, onEvent: (ev: DeviceBrowseEvent) => void): Promise<number>  // Channel plumbing like mqttConnect
export const deviceBrowseSetTarget = (url: string): Promise<void>
export const deviceBrowseStop = (): Promise<void>
```

- [ ] **Step 1 (TDD):** contract tests (command names `device_browse_start`/`device_browse_set_target`/`device_browse_stop`, keys `url`, channel arg key matching the Rust param `onEvent` — copy `mqttConnect`'s existing test for the Channel pattern). Run — FAIL.
- [ ] **Step 2:** Implement following `mqttConnect` (`api.ts` ~714-739).
- [ ] **Step 3:** `npx tsc --noEmit && npx vitest run src/__tests__/api.test.ts` — PASS.
- [ ] **Step 4:** Commit `feat: deviceBrowse api wrappers and event union`.

### Task 5: `DeviceBrowserPanel` component

**Files:**
- Create: `src/components/DeviceBrowserPanel.tsx`
- Modify: `src/styles.css` (only if an existing class truly doesn't fit)

**Interfaces:**
- Consumes: Task 4 wrappers; `ObsStore` (`src/obs/obsStore.ts`), `ObsLog` (`src/components/ObsLog.tsx`); `WsPanel.tsx` as the structural template (URL bar + localStorage history + datalist + chip + 4 Hz active poll).
- Produces: `export default function DeviceBrowserPanel({ active, notify }: { active: boolean; notify: (m: string, e?: boolean) => void })`.

- [ ] **Step 1:** Clone WsPanel's skeleton: header row = URL input (placeholder `http://unoq.local`), Go/Stop button, status chip (idle/listening/error), history datalist under localStorage key `deviceBrowser.history` (cap 12, most-recent first, dedup).
- [ ] **Step 2:** Go: `deviceBrowseStart(url, ev => store.push(...))` → on `stage` set iframe `src = http://127.0.0.1:${port}/` and chip listening; map `exchange` events into ObsStore entries (`label: `${method} ${path} → ${status} (${duration_ms}ms)``, payload = preview/meta for the expander; `error` entries flagged error-styled; `closed` → chip idle. Changing the URL while running calls `deviceBrowseSetTarget` + reloads the iframe.
- [ ] **Step 3:** Layout: iframe fills remaining height above a collapsible ObsLog strip (reuse the split the Scope/obs panels use — read how WsPanel sizes ObsLog; if none fits, one flex container in styles.css). Iframe `sandbox` attribute OFF (device UIs need scripts) but `referrerpolicy="no-referrer"`.
- [ ] **Step 4:** Pure helpers extracted and tested if any nontrivial mapping logic grew (e.g. exchange→ObsStore entry formatting) in `src/obs/` with a vitest file.
- [ ] **Step 5:** `npx tsc --noEmit` (App.tsx still un-wired: expect ONLY unused-export absence — the component compiles standalone). Commit `feat: device browser panel — iframe + exchange log`.

### Task 6: App wiring

**Files:**
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: Task 1 tab model, Task 5 component. The exploration's six-edit checklist:

- [ ] **Step 1:** Widen `obsTab` state to `"mqtt" | "ws" | "web"`; `openBottomTab`'s obs branch casts accordingly and sets a new `webMounted` flag alongside `wsMounted`.
- [ ] **Step 2:** Render `{webMounted && <DeviceBrowserPanel active={bottomTab === "web"} notify={notify} />}` beside the WsPanel render, hidden-not-unmounted like its siblings.
- [ ] **Step 3:** `npx tsc --noEmit && npx vitest run` fully green; `cargo check -p bancada`.
- [ ] **Step 4:** Commit `feat: web tab renders the device browser`.

### Task 7: end-to-end verification

**Files:** none (scratchpad only)

- [ ] **Step 1:** Fixture device: `python3 -m http.server <port>` in a scratch dir with an `index.html` + `data.json`; start the proxy via a small Rust integration test OR drive the built commands with a scratch tauri-driven test if impractical — minimum bar: `curl http://127.0.0.1:<proxyport>/data.json` through a manually started proxy instance (a `#[test]`-gated harness in lib.rs tests may spawn the listener directly with a fake Channel collector, following the MCP listener's existing test style — READ how mcp listener tests fake the channel).
- [ ] **Step 2:** Assert: response body forwarded byte-identical; `exchange` event captured with correct method/path/status; unreachable target yields 502 + `error` event; hop-by-hop headers absent from the forwarded request.
- [ ] **Step 3:** Full suites one final time. No commit unless fixes were needed.
- [ ] **Step 4:** Report bench steps for Marco: open a project, Web tab, `http://unoq.local:<port>` (or an ESP32 IP), watch the page render and the poll traffic stream into the log.
