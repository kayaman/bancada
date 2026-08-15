# System context

What Bancada talks to, and the one principle that decides how.

Bancada is a single-user Linux desktop application. It has no server and no
network service of its own. Everything it does is local orchestration of five
external binaries, one optional catalog API, and whatever the bench happens to
have plugged in or reachable.

```
                          ┌─────────────────────────┐
                          │        Bancada          │
                          │   (Tauri desktop app)   │
                          └────┬───────────┬────────┘
                               │           │
          ┌────────────────────┘           └──────────────────┐
          │ subprocess (PATH)                                 │ network
          ▼                                                   ▼
 ┌────────────────────┐                          ┌──────────────────────────┐
 │ arduino-cli        │──▶ Arduino registry      │ MQTT broker    (rumqttc) │
 │ esptool            │    (index, libraries)    │ WebSocket peer (webview) │
 │ git                │──▶ GitHub / any remote   │ device HTTP    (ureq)    │
 │ gh                 │──▶ GitHub API            │ GitHub raw/ls-remote     │
 │ claude             │──▶ Anthropic API         │ Mouser Search API (TLS)  │
 └────────┬───────────┘
          │ USB serial (/dev/ttyACM*, /dev/ttyUSB*)
          ▼
 ┌────────────────────┐
 │  the board on the  │  ESP32 / AVR / …
 │       bench        │  optionally running bancada_scope firmware
 └────────────────────┘
```

---

## 1. The external-engine principle

> Bancada does not reimplement the toolchain. It drives the same engines the
> official IDE uses.

This is the single most consequential toolchain decision in the codebase.
There is no `libgit2`, Arduino SDK binding, Anthropic client library, or Node
sidecar. Capabilities provided by mature local tools are obtained by spawning
those tools and parsing their output. Mouser is the deliberate exception: it
offers an HTTPS Search API rather than a local engine, so `core::mouser` calls
that official contract directly and normalizes its JSON.

It buys correctness Bancada could not otherwise afford: platform resolution,
dependency resolution, the build cache, and upload protocols are all *exactly*
what Arduino IDE 2.x does, because it is the same binary.

It also has consequences that show up all over the code, and a contributor
should recognise them as consequences rather than quirks:

### PATH resolution, by bare name

Every engine is looked up on `PATH` by its plain name — `arduino-cli`, `git`,
`gh`, `claude` — with no bundled copy and no configured path. `esptool` is the
one exception: it probes `esptool` then `esptool.py`, taking the first whose
`version` subcommand succeeds (`core/src/esptool.rs`), because the pip package
has shipped under both names.

### Missing tools are a first-class error, not a panic

`Error::ToolMissing` (`core/src/lib.rs:47`) exists specifically so a missing
engine produces an actionable message rather than a spawn failure. `esptool`'s
carries its own install hint; `claude`'s is turned into "Claude Code is not
installed" at the Tauri layer.

### Version drift is a real risk, and it is tested for

Parsing another program's `--json` output is a contract with no compiler behind
it. `core/tests/core_list_real.rs` exists purely as a read-only drift check
against a real `arduino-cli core list`. It is `#[ignore]`d — see
[conventions](conventions.md) — because it needs the engine installed.

Where an engine refuses `--json` and prints prose, there is a separate `run_ok`
path rather than a fake parse (`core/src/cli.rs`).

### Everything long-running streams the same way

Any engine invocation that can take minutes uses one uniform shape: spawn with
piped stdio, one reader thread per pipe, both feeding a single `mpsc` channel so
stdout and stderr interleave in real order, each line delivered to a callback as
an `OutputLine { stream, line }`. That type reaches the frontend unchanged as
the `build://line` event.

The same pattern is implemented three times — `core/src/cli.rs` (`run_streaming`),
`core/src/git.rs` (`run_streaming`, and again inline for `gh`) — because the
crates differ in what they need to do with the exit status, not because anyone
forgot to factor it.

### Processes are owned, evicted, and torn down

A subprocess holding a serial port is exclusive hardware. This is why the serial
slot has a single-owner rule and an eviction protocol rather than a pool — see
[runtime-model](runtime-model.md).

### No lock-in, stated as a property

Because the sketch on disk is a plain arduino-cli sketch with a plain
`sketch.yaml` profile, a project created in Bancada builds in Arduino IDE, in
`arduino-cli` directly, or in CI, with Bancada uninstalled. Bancada-owned
`bancada.yaml` and `hardware/circuit.yaml` are additive; the generated circuit
pin header is ordinary Arduino C++ and remains buildable without Bancada.

---

## 2. The engines

| Engine | Used for | Invoked from |
|---|---|---|
| **`arduino-cli`** | boards, cores, libraries, sketch creation, profiles, compile, upload, serial monitor, port enumeration | `core/src/cli.rs` |
| **`esptool`** | ESP MAC address and chip type (board identity for the fleet) | `core/src/esptool.rs` |
| **`git`** | status, commit, sync, init, and sparse subtree fetch for pinned libraries | `core/src/git.rs`, `core/src/ghlib.rs` |
| **`gh`** | one-button private repo creation (`gh repo create --source --push`) | `core/src/git.rs` |
| **`claude`** | the AI Assistant panel, driven over stdio stream-json | `src-tauri/src/lib.rs` only |

Mouser is not an engine. It is an optional network catalog used only from the
Circuit workspace; builds and circuit validation do not depend on its
availability.

Two notes worth internalising:

**Port enumeration is delegated on purpose.** The `serialport` crate is built
with `default-features = false` so Bancada does not require `libudev` at build
time. That strips enumeration, so the *board list* comes from
`arduino-cli board list --json` — which is better anyway, since it also carries
FQBN matching. The crate is used only to open, read and write a port.

The one place raw enumeration *is* used is the hotplug watcher thread, which
polls `serialport::available_ports()` every 2s and keys ports with
`core::ports::port_key`. That is a deliberate, contained exception.

**`claude` is driven, not linked.** Bancada speaks the CLI's stream-json
protocol over stdin/stdout and forwards each event to the frontend **verbatim**.
The frontend's contract is the wire shape itself, not a re-modelled subset —
which is why `src/agent/types.ts` mirrors the CLI's objects and why every
interface there has an index signature and must not throw on an unknown `type`.

---

## 3. Network peers

Bancada makes no outbound connection except through an engine or one of these:

| Peer | Transport | Reached from |
|---|---|---|
| MQTT broker | TCP, plain (no rustls) | `rumqttc` sync client, `core/src/mqtt.rs` |
| WebSocket endpoint | ws/wss | the **webview's own** `WebSocket` — not Rust |
| Device HTTP server | plain HTTP | `ureq`, via the loopback reverse proxy |
| Arduino registry, GitHub | HTTPS | inside `arduino-cli` / `git` / `gh` |
| Anthropic API | HTTPS | inside `claude` |
| Mouser Search API | HTTPS (rustls) | `core/src/mouser.rs`, with the credential supplied only by `src-tauri` |

Four of these are worth a contributor's attention:

- **The WebSocket panel runs in the webview**, not in Rust. That is why
  `tauri.conf.json` sets `csp: null`. It is a deliberate trade.
- **The device browser is a loopback reverse proxy.** An iframe cannot load a
  bench device's page directly under the app's origin, so `tiny_http` listens on
  an ephemeral `127.0.0.1` port, `ureq` fetches from the device, and every
  exchange is logged. `core::devproxy::parse_target` **refuses `https` targets
  up front** — bench devices do not serve TLS, and pretending to support it
  would be worse than refusing.
- **MQTT never retries.** Any error emits `closed` and the thread exits.
  Reconnection policy lives in the frontend (`src/obs/backoff.ts`), where it can
  be shown to the user as a countdown.
- **Mouser is live lookup metadata, not an electrical authority.** Calls are
  user-triggered, and the native layer applies a process-local quota guard,
  holds the saved/environment key, caps response bodies, rejects non-HTTPS
  links, and normalizes untrusted JSON. Results remain transient; exact part
  declarations and datasheet verification stay user-maintained in the circuit
  manifest.

---

## 4. Constraints and quality attributes

These shape the whole design and are otherwise only implied by asides in the
code. A contributor should treat them as given, not as gaps to fill.

**Linux desktop, single user.** Bundles are deb/rpm/AppImage; prerequisites are
documented for openSUSE Tumbleweed. Paths assume XDG. `main.rs` carries a
`windows_subsystem` attribute, but no Windows/macOS support is claimed or
tested.

**Single maintainer, no CI.** There is no `.github/workflows`. The suites are
run locally, and the opt-in hardware/network suites are a pre-release ritual.
This is a deliberate, stated trade-off, not an oversight — see
`docs/hardware-smoke-tests.md`.

**A bench tool, with a bench threat model.** Bancada assumes a trusted local
machine and a physically present user. The MCP listener binds loopback only and
is bearer-authenticated; the device proxy is loopback and single-target. The one
place with a genuinely adversarial threat model is the AI Assistant, because
there a model chooses the actions — see [agent-safety](agent-safety.md).

**Hardware is exclusive and slow.** One process may own a serial port. A
platform build can take minutes and cannot be aborted. These two facts explain
the single-owner slot, the eviction protocol, and the non-blocking build gate.

**Offline-capable where it matters.** Editing, browsing a project, and reading
past chats work with no network. Anything needing an index, a registry or an API
fails with the engine's own message rather than a Bancada-invented one.

---

## See also

- [conventions](conventions.md) — the rules that follow from this principle
- [backend-modules](backend-modules.md) — where each engine wrapper lives
- [runtime-model](runtime-model.md) — process ownership and eviction
- [`docs/INSTALL.md`](../INSTALL.md) — installing the engines as an end user
