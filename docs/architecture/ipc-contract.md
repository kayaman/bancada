# IPC contract

Everything that crosses the Rust ↔ webview boundary. Four mechanisms:

| Mechanism | Direction | Count | Use |
|---|---|---|---|
| `invoke` commands | frontend → Rust, request/response | **94** | everything transactional |
| Tauri events | Rust → frontend, broadcast | **7** | line streams, hotplug, agent |
| `Channel<T>` | Rust → frontend, per-session | **3** | high-rate or per-panel streams |
| Loopback MCP | agent → Rust, HTTP JSON-RPC | 4 tools | the AI Assistant's tools |

Authoritative sources: the `generate_handler![…]` list in
`src-tauri/src/lib.rs`, and `src/api.ts` — the **only** frontend file importing
`@tauri-apps/api/core` or `/event`.

---

## 1. The naming rule

Frontend argument keys are camelCase; Rust command parameters are snake_case.
Tauri maps between them, and **a mismatch fails only at runtime** — there is no
compiler on either side of this boundary.

```ts
invoke<SketchFile[]>("list_sketch_files", { sketchDir })   // → sketch_dir
```

This is why `src/__tests__/api.test.ts` exists: it mocks `@tauri-apps/api` and
asserts the exact `(command, argKeys)` pair for every wrapper. **Adding a
command means adding its contract test.**

---

## 2. Commands (94)

Grouped by domain; the order within each group follows `generate_handler!`.

### Environment and toolchain — 4
`cli_version` · `list_boards` · `sketchbook_dir` · `default_project_parent`

### Sketch files and explorer — 8
`list_sketch_files` · `read_sketch_file` · `write_sketch_file` ·
`create_sketch_file` · `create_sketch_dir` · `rename_sketch_entry` ·
`delete_sketch_entry` · `load_sketch_yaml`

`delete_sketch_entry` moves to the OS trash. All paths are validated as
relative and non-escaping by `core::files::validate_rel_path` — the frontend's
`newFile.ts` checks are a friendly pre-validation only; **the backend is the
authority**.

### Profiles (`sketch.yaml`) — 5
`init_profile` · `retarget_profile` · `add_local_library` ·
`add_registry_library_to_profile` · `add_platform_to_profile`

### Libraries — 6
`search_libraries` · `list_installed_libraries` · `install_library` ·
`uninstall_library` · `sketchbook_libraries_dir` · `create_library`

### Cores and boards — 6
`list_cores` · `search_cores` · `install_core` · `uninstall_core` ·
`update_core_index` · `list_all_boards`

The middle three stream to `build://line` — they are long-running and share the
build console with compiles.

### Projects — 4
`create_project` · `list_sketch_templates` · `clone_project` · `rename_project`

`create_project` runs `sketch new` → `write_main_ino` → `profile create` →
`profile lib add` per library → `ensure_under_git`. **Library and git failures
are non-fatal** and are reported in the returned
`CreatedProject { library_errors, under_git, git_error }` — a project with a
failed optional library is still a project.

`rename_project` moves the folder *and* its main `.ino`, which the explorer's
`rename_sketch_entry` refuses to touch (`files.rs::is_protected`). It then
carries across the state keyed to the old path: the chat directory and the
usage entry, both keyed by `chatlog::sketch_key(path)` — whose hash *and*
basename halves change on a rename — plus the recent-projects entry.

Two refusals worth knowing. It is **refused outright while an Assistant
session is live**: the session pins the old path in four places that cannot be
rewritten after the child is spawned (see [agent-safety](agent-safety.md)). And
it holds the **build gate** for the move, so it cannot race a compile or flash
reading the tree it is relocating.

The state moves run after the directory rename and are best-effort — by then
the project has moved, and failing the call would leave the frontend pointing
at a path that no longer exists. Each failure comes back in `warnings`.

### Git-hosted libraries (`bancada.yaml`) — 4
`gh_list_versions` · `gh_manifest` · `gh_add_library` · `gh_restore`

`gh_restore` re-materialises every manifest entry at its pinned commit,
collecting per-entry errors rather than aborting on the first.

### Build and flash — 2
`compile_sketch` · `upload_sketch`

Both `async`, both `spawn_blocking`, both take the **non-blocking** build gate
and stream every `OutputLine` as `build://line`. Contention returns
`"build already in progress"`. See [runtime-model](runtime-model.md).

A successful `upload_sketch` also calls `tag_flash` before releasing the gate —
checkpoint, tag, push. So does the agent's MCP `upload`. See
[data-flows](data-flows.md) for the trace, and note that **nothing in that path
can fail the flash**: every step reports to `build://line` and returns.

### Git and GitHub — 8
`git_state` · `git_commit` · `git_init` · `git_init_here` · `git_sync` ·
`git_create_remote` · `git_set_remote` · `gh_available`

`git_create_remote`, `git_set_remote` and `git_sync` stream to `build://line`.

`git_init_here` is `git_init` without its "already under git" refusal: it
initializes a *nested* sketch as a repository in its own right, which
`ensure_under_git` deliberately declines to do. Tags are per-repository, so a
nested sketch needs this before its flashes can be tagged — at the cost of an
embedded repository the parent will report as such.

`git_create_remote` takes `visibility` and an optional `description`, and
initializes a repository first when the sketch is `no_git` — publishing an
unversioned sketch is one action. It **refuses a public repo whose
`tracked_secrets` is non-empty**: `.gitignore` does not untrack what is already
in the index, and the frontend's identical check is only a courtesy.

### Serial monitor — 4
`start_monitor` · `set_selected_target` · `stop_monitor` · `monitor_send`

All take the `serial` leaf lock. `set_selected_target` mirrors the UI's port and
baud into Rust so the agent's MCP tools can use them without a port argument.

### Scope — 6
`scope_probe` · `scope_start` · `scope_single` · `scope_send` · `scope_stop` ·
`scope_install_firmware`

`scope_probe` tries 921600 then 115200 and rejects a banner whose `proto` is not
`1`. `scope_install_firmware` materialises the firmware sketch from
`include_str!` into a temp directory (or a caller-supplied one). Full semantics:
[`docs/scope-architecture.md`](../scope-architecture.md) §3.

### File export — 2
`save_text_file` · `save_binary_file`

Paths come from the OS save dialog; `save_binary_file` takes base64.

### Settings — 5
`load_settings` · `set_last_sketch` · `set_last_project_parent` ·
`push_recent_project` · `remove_recent_project`

### Chat log — 5
`chat_append` · `chat_list` · `chat_load` · `chat_delete` · `chat_totals`

`chat_append` also drives usage accounting and prunes to 50 chats per sketch.

All five are **path-addressed**: they take the open sketch's directory and hash
it to a `sketch_key` internally. That is safe here — a live caller's path is
current by definition.

### Usage — 3
`usage_overview` · `chat_list_usage` · `chat_load_by_key`

The last two are **key-addressed**, and the split from the chat-log commands
above is load-bearing rather than stylistic. The dashboard browses *historical*
records whose `sketch_dir` was recovered by `usage::backfill` from transcripts'
`meta` lines — which record where a conversation happened, not where the
project is now. After a rename that string can name a directory that is gone,
and hashing it yields a key nothing is stored under: empty session lists and
un-openable chats.

So `usage_overview` returns each row's `key`, and these two take it unchanged.
Never re-derive a key from a `ProjectUsage.sketch_dir`.

### Fleet — 6
`read_board_mac` · `fleet_sync` · `set_board_nickname` · `note_board_fqbn` ·
`identify_board` · `forget_board`

`identify_board` **evicts the serial owner** before reading the MAC with
esptool, then migrates a serial-keyed record to the MAC. `note_board_fqbn` is
opportunistic after an upload; an unknown port is a silent no-op.

### MQTT — 7
`mqtt_connect` · `mqtt_publish` · `mqtt_subscribe` · `mqtt_unsubscribe` ·
`mqtt_disconnect` · `load_mqtt_config` · `save_mqtt_config`

### Device browser — 3
`device_browse_start` · `device_browse_set_target` · `device_browse_stop`

### Agent — 6
`agent_probe` · `agent_start` · `agent_send` · `agent_interrupt` ·
`agent_set_uploads_armed` · `agent_stop`

`agent_start` returns the child pid. Several later calls are **pid-guarded** so
a stale session cannot act on a newer one — see [agent-safety](agent-safety.md).

---

## 3. Events (7)

Rust → frontend broadcast, subscribed via `listen` in `src/api.ts`.

| Event | Payload | Emitted by |
|---|---|---|
| `build://line` | `{ stream: "stdout" \| "stderr", line: string }` | compile, upload, core install/uninstall/index, git sync/remote, and the agent's MCP `verify`/`upload` |
| `serial://line` | `{ stream, line }` | monitor stdout and stderr reader threads |
| `serial://closed` | `{}` | monitor stdout thread at EOF |
| `serial://started` | `{ port, baud }` | the agent's `serial_read` when it auto-starts the monitor |
| `ports://changed` | `()` | the hotplug watcher thread |
| `agent://event` | the `claude` CLI's stream-json object **verbatim**, plus synthetic host events | agent stdout/stderr readers, verify/upload, alarms |
| `agent://closed` | `{ reason, pid }` | agent stdout reader at EOF |

Two things a contributor will trip over:

**Most emits go through `EmitFn`, not `AppHandle::emit`.** Grepping for
`emit("serial://line"` finds nothing useful — reader threads take an injected
emitter so they are testable. Grep for the event-name string literal instead.

**`serial://started` exists to keep the frontend honest.** When the agent's
`serial_read` starts the monitor itself, the UI's monitor state would otherwise
be wrong and its auto-start effect would double-start.

### Synthetic `agent://event` variants

Alongside the CLI's own events, the host injects:

```
{ type: "stderr",         line }
{ type: "unparsed",       line }              non-JSON stdout
{ type: "verify_started", pid }
{ type: "verify_done",    success, pid }
{ type: "upload_started", port }
{ type: "upload_done",    success }
{ type: "security_alarm", kind, detail, pid } kind: "unexpected_tools" | "path_escape"
```

Each carries the session's child `pid` so a synthetic event from a session the
user already stopped cannot render into a *newer* session's panel.

`src/agent/types.ts` mirrors these. **Every interface there has an index
signature and an unknown `type` must not throw** — the frontend consumes a wire
format owned by another program.

---

## 4. Channels (3)

`tauri::ipc::Channel` is used where a broadcast event would be wrong: high rate,
or scoped to one panel's session.

| Command | Body | Contract |
|---|---|---|
| `scope_start` | **Raw (binary)** | `core::scope` envelopes: kind `0x01` samples, `0x02` JSON. Synthetic JSON events: `{"ev":"drop","frames":N}`, `{"ev":"crc","count":N}` (throttled to every 50), `{"ev":"closed"}`. Decoded by `src/scope/binary.ts`. |
| `mqtt_connect` | JSON | `core::mqtt::MqttEvent`, `#[serde(tag = "ev")]`: `stage` / `msg` / `closed`. |
| `device_browse_start` | JSON | `DeviceBrowseEvent`, `#[serde(tag = "type")]`: `stage` / `exchange` / `error` / `closed`. Returns the loopback port. |

A failed channel send means the frontend is gone; the producing thread returns
quietly rather than erroring.

---

## 5. The loopback MCP server

Not a Tauri mechanism. The AI Assistant's `claude` child reaches back into
Bancada over HTTP JSON-RPC on `127.0.0.1:<ephemeral>/mcp`, bearer-authenticated.

Four tools (`core/src/mcp.rs`):

| Tool | Does |
|---|---|
| `verify` | compile the open sketch, through the same build gate as the button |
| `upload` | flash it — **no port argument**; uses the UI-selected port and the session-frozen profile, and is refused unless "Allow uploads" is armed |
| `serial_read` | read from the rolling `SerialRing` since this session's cursor (`wait_s` clamped to 10) |
| `serial_send` | write a line to the monitor |

Listener discipline (`mcp_listener_loop`), each rule the result of an observed
failure:

- **Non-POST → 405.** The CLI opens a `GET /mcp` SSE stream this server does not
  offer; returning a JSON-RPC parse error caused a reconnect busy-loop.
- **Bad or absent bearer → 401.** The token is per-session, from
  `/dev/urandom`, with **no weak fallback**.
- **`Content-Length` over 1 MiB → 413, before reading a single byte.**
- **Notifications → 202 empty.**

The listener thread gets owned clones at spawn and **never locks
`state.agent`** — a `verify` arriving while a command held that mutex would
deadlock the compile against the UI. It may lock `serial`, because it is never
joined under it.

---

## See also

- [runtime-model](runtime-model.md) — the locks and threads behind these commands
- [agent-safety](agent-safety.md) — what the MCP surface is allowed to do
- [data-flows](data-flows.md) — these mechanisms traced end to end
- [`docs/scope-architecture.md`](../scope-architecture.md) — the scope envelope in byte detail
