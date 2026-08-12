# Backend modules

Two crates, one workspace (`Cargo.toml`, members `["core", "src-tauri"]`).

```
┌──────────────────────────────────────────────────────────────┐
│ src-tauri  (crate `bancada`, lib `bancada_lib`)              │
│   lib.rs — 91 commands, AppState, threads, events, MCP       │
│   Owns: processes · threads · mutexes · files · the window   │
└───────────────────────────┬──────────────────────────────────┘
                            │ calls
┌───────────────────────────┴──────────────────────────────────┐
│ core  (crate `bancada-core`) — 22 modules, ~14.9k lines      │
│   Pure of Tauri. Parsers, validators, policy, wire formats.  │
│   Owns: no long-lived state, no knowledge that a UI exists   │
└──────────────────────────────────────────────────────────────┘
```

The rule and its rationale are in [conventions §1](conventions.md#1-the-layering-rule).

---

## 1. `bancada-core` — the 22 modules

`core/src/lib.rs` is 61 lines: the module list, one `Error` enum, one `Result`
alias. Every error in the crate is one of six variants — `Io`, `ToolFailed`,
`ToolMissing`, `Json`, `Yaml`, `Other` — which is why error handling at the
Tauri layer is uniformly `.map_err(err_str)`.

### Toolchain wrappers

| Module | LoC | Responsibility |
|---|---|---|
| `cli.rs` | 1021 | The `arduino-cli` wrapper. `ArduinoCli { bin }` defaults to `"arduino-cli"` on PATH. Three invocation modes: `run_json` (appends `--json`, deserialises stdout), `run_ok` (side-effect commands that reject `--json`), `run_streaming` (two reader threads → one `mpsc` → `OutputLine` callback). `monitor()` returns a live `Child` with piped stdio. |
| `esptool.rs` | 316 | MAC address and chip type. Probes `esptool` then `esptool.py`. Keeps raw output for the UI's "details" view. |
| `types.rs` | 583 | The serde structs for everything `arduino-cli --json` returns — `DetectedPort`, `Port`, `IndexedLibrary`, `InstalledLibrary`, `Platform`, `BoardOption` — plus the streaming shapes `OutputLine { stream, line }` and `RunResult`. |
| `boards.rs` | 383 | Core/platform identity: `parse_core_id`, `fqbn_platform_id`, install-status derivation, version sorting, and the `sketch.yaml` platform-dependency strings. |

### Sketch and project model

| Module | LoC | Responsibility |
|---|---|---|
| `sketch.rs` | 1019 | `SketchYaml` / `Profile` / `PlatformDep` / `LibraryDep`, and `SketchProject` — the sketch on disk. File walk (`SKIP_DIRS`), profile create/add/retarget, library and platform pinning. **The only file schema Bancada itself owns.** |
| `project.rs` | 406 | New-project policy: name validation, starter templates (`TEMPLATES`), profile naming from an FQBN, board-required libraries, default parent directory. |
| `clone.rs` | 1158 | Clone a sketch under a new name. Staging directory + atomic rename, merged `.gitignore` written *into the staging dir* so credential ignore rules exist before any possible commit, best-effort `git init`. Skips `.bancada` and `.claude`. |
| `files.rs` | 389 | Explorer mutations: `validate_rel_path`, protected-path checks, collision and descendant guards. **Delete goes to the OS trash** (`trash::delete`), never `fs::remove`. |

### Libraries

| Module | LoC | Responsibility |
|---|---|---|
| `library.rs` | 774 | Scaffolding a new Arduino library: name/category/version validation, staged generation with an RAII `Staging` guard so a failure leaves nothing behind. |
| `ghlib.rs` | 646 | Git-hosted libraries. `@owner/repo/path` aliases, the `bancada.yaml` manifest, and `fetch_subtree` — a `--depth 1 --filter=blob:none` sparse clone into a dot-prefixed sibling directory so the final rename is same-filesystem and atomic. The fetched commit is **verified against the manifest pin**: a moved tag is a refusal, not a silent rebuild. |

### Git

| Module | LoC | Responsibility |
|---|---|---|
| `git.rs` | 1437 | `git` and `gh`. Gitignore policy (`GITIGNORE_REQUIRED`, `merged_gitignore`), `parse_status_v2`, `suggested_message`, `tracked_secrets`, `repo_state`, commit/sync, and `gh repo create`. The `gh` path deliberately retains the stderr tail, because that is where `gh` explains auth problems ("run: gh auth login"). |

### Hardware

| Module | LoC | Responsibility |
|---|---|---|
| `fleet.rs` | 928 | The board registry. Identity from a MAC address or USB descriptors (`BoardIdKind`), nicknames, last-seen tracking, the `fleet.json` model. Writes are skipped unless a board is new or `last_seen` is stale, to avoid churn. |
| `ports.rs` | 119 | Hotplug identity: `port_key` (name + vid:pid:serial) and `ports_changed`. The only consumer of raw `serialport` enumeration. |
| `serialring.rs` | 243 | Rolling serial scrollback with monotonic sequence numbers, capped at 500 lines / 4096 bytes per line. Feeds the agent's `serial_read` tool; sequence numbers survive monitor restarts. |

### Wire protocols

| Module | LoC | Responsibility |
|---|---|---|
| `scope.rs` | 467 | The oscilloscope protocol: `crc16_ccitt`, `FrameScanner`, banner parsing, control-line command builders, and the host→frontend envelope encoders. Contract: [`docs/scope-architecture.md`](../scope-architecture.md) §1–2. |
| `mqtt.rs` | 518 | Broker URL parsing and password redaction, topic matching, the `MqttEvent` channel contract (`#[serde(tag = "ev")]`), and the `mqtt.json` config model. |
| `devproxy.rs` | 215 | Device-browser proxy policy: `parse_target` (**refuses `https`**), the hop-by-hop header set, body previews. |
| `mcp.rs` | 762 | The MCP JSON-RPC server logic and the four tool schemas (`verify`, `upload`, `serial_read`, `serial_send`), plus the bearer check. Pure request→reply; the socket lives in `src-tauri`. |

### Agent

| Module | LoC | Responsibility |
|---|---|---|
| `agent.rs` | 2134 | The largest core module, and the one to read most carefully. Two halves: (a) the `claude` CLI stream-json model — `AgentEvent`, `parse_event`, `user_message_json`, `interrupt_json`; (b) **the entire confinement policy** — `agent_args`, `BUILTIN_TOOLS`/`EXPECTED_TOOLS`, `path_is_confined`, `deny_rules`, `guard_decision`, `settings_disables_hooks`. Policy lives here, as pure functions, so it is unit-testable and so the `--agent-guard` hook runs the *same* code. See [agent-safety](agent-safety.md). |

### Persistence

| Module | LoC | Responsibility |
|---|---|---|
| `chatlog.rs` | 722 | Assistant transcripts as NDJSON under `<chats_root>/<sketch_key>/`. `sketch_key` is an fnv1a-64 hex digest plus a sanitised basename. Listing, loading, per-project totals, pruning. |
| `usage.rs` | 372 | Cumulative per-project cost/token/turn accounting (`usage.json`, versioned). Survives chat pruning, which is why it is separate from `chatlog`. |
| `settings.rs` | 189 | `AppSettings` — last sketch and open file, last project parent, recent projects (`MAX_RECENT = 10`). |

All four take their paths and their clock from the caller — see
[conventions §1](conventions.md#the-injection-corollary).

---

## 2. `src-tauri` — the Tauri layer

One crate, one module: `src-tauri/src/lib.rs`, 6,312 lines. `main.rs` is six
lines and calls `bancada_lib::run()`.

Its first 142 lines are rustdoc, and they are the canonical prose spec for the
event taxonomy, the agent safety model, the build gate and the MQTT contract.
**Read them before changing anything in this file.**

### What it owns

- **The 91 commands** — see [ipc-contract](ipc-contract.md).
- **`AppState`** — five independent session slots plus the build gate. See
  [runtime-model](runtime-model.md).
- **Every thread** — hotplug watcher, monitor readers, scope reader, MQTT
  connection, device-proxy accept loop, and the agent's four.
- **Every file path** — it resolves the config directory and the temp directory
  and hands them to `core`.
- **The clock** — `core` never reads one.

### Two structural details worth knowing

**`run()` handles `--agent-guard` before Tauri starts.** Bancada re-invokes its
own binary as the agent's `PreToolUse` hook. In that role it is a plain
stdin→stdout JSON filter and returns before any window is built. This is why the
confinement policy cannot drift from the tested code — it *is* the tested code.

**`EmitFn` is the testability seam.** Reader threads and the MCP listener take
a `dyn Fn(&str, serde_json::Value) + Send + Sync` rather than an `AppHandle`.
Production passes a wrapper around `AppHandle::emit`; tests pass a collector.
This is why ~69 unit tests can live inside a file that otherwise needs a running
Tauri app. Preserve it.

### Dependencies beyond `bancada-core`

| Crate | Why |
|---|---|
| `tauri` 2 + `tauri-plugin-dialog` | the window, the IPC, native file dialogs |
| `tiny_http` | the loopback MCP listener **and** the device-proxy server half |
| `ureq` | the device-proxy client half; plain HTTP on purpose |
| `base64` | binary file save, non-UTF-8 MQTT payloads |

The window is granted only `core:default`, `core:event:default` and
`dialog:default` (`src-tauri/capabilities/default.json`). There is no filesystem
or shell plugin — all file access goes through a typed command.

---

## See also

- [ipc-contract](ipc-contract.md) — the command, event and channel surface
- [runtime-model](runtime-model.md) — `AppState`, threads, locks, shutdown
- [persistence](persistence.md) — every file these modules read and write
- [`docs/scope-architecture.md`](../scope-architecture.md) — the scope wire contract in full
