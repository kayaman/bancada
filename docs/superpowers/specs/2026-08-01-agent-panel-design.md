# Agent Panel Design

**Date:** 2026-08-01
**Status:** Implementing (first slice of an AI-assisted IDE vision)

## Context

The long-term vision is to evolve **bancada** (Tauri 2 + Rust + React Arduino
workbench, v0.7.0) into an AI-assisted IDE for software and hardware. Bancada
already owns the hardware half — arduino-cli build/upload, esptool, serial
monitor, scope, fleet, MQTT/WS observability — behind a headless, unit-tested
`bancada-core` crate deliberately designed to be driven by something other
than the GUI. It has **zero AI code today**.

This is the **first slice** of that vision: an Assistant panel where the user
chats with a Claude agent that can read/edit project files, run Verify
through bancada's existing compile path, read compiler errors, and iterate
until the build passes. No upload/serial/hardware tools yet (v0.9.0+).

## Decisions made with the user

1. Evolve bancada (not greenfield).
2. Runtime: spawn the **`claude` CLI directly** as a supervised child
   (`--input-format stream-json --output-format stream-json`) — no Node
   sidecar. Matches the repo philosophy ("wrap external binaries, parse
   their JSON"); auth comes free from the existing Claude Code login
   (verified: `claude` 2.1.220 on PATH, credentials present).
3. First slice = chat + build loop only.
4. Edits **auto-apply**, shown as diff cards (no per-edit approval gate).
   Warn when the project is not under git.
5. Panel lives in a new bottom-panel **"Assistant"** group (reuses
   `bottomTabs.ts` machinery). Right dock deferred.
6. Show per-turn **cost + turn count** in the panel footer.
7. `summarize_build_output` strategy (decided after the initial plan): keep
   **all** stderr lines plus the tail of stdout, capped at **~200 lines /
   50 KB total** — the agent needs the errors, not a scrollback of a
   multi-minute platform build.
8. Diff card rendering (decided after the initial plan): unified diffs of
   `old_string` → `new_string`, **full strings** (no elision), `-`/`+`
   lines, red/green backgrounds, mono font.

## Architecture

```
React AgentPanel ── invoke/events ── src-tauri (thin) ── stdio stream-json ── claude CLI (child)
                                        │    ▲                                    │
                                        │    └── agent://event, agent://closed    │ MCP over loopback HTTP
                                        ▼                                         ▼
                                  bancada-core: agent.rs (protocol), mcp.rs (JSON-RPC), cli.rs compile
```

- One `claude` process per session, spawned on first message, cwd = sketch
  dir (scopes built-in file tools). Multi-turn via NDJSON user messages on
  stdin. **Interrupt:** attempt the SDK's `control_request` line
  best-effort, but `kill` after ~2 s is the documented-reliable path (the
  stdin control protocol is undocumented — verified against docs). No
  auto-restart on crash (same philosophy as the MQTT thread) — panel shows
  "Session ended".
- Argv (built by pure, tested `agent_args()` in core): `-p --verbose
  --include-partial-messages --permission-mode acceptEdits --allowedTools
  Read,Edit,Write,Glob,Grep,mcp__bancada__verify --disallowedTools
  "Bash,WebFetch,WebSearch,Task,NotebookEdit,KillShell,BashOutput"
  --mcp-config <path to a 0600 temp file containing the loopback HTTP URL +
  bearer token as JSON — a path, not inline JSON: a post-review fix (F5)
  found the token riding argv is world-readable via /proc/<pid>/cmdline on
  Linux, which defeats the point of a bearer token. `write_mcp_config_file`
  writes and chmods the file; `agent_start`/`stop_agent_session` delete it
  on every exit path>
  --strict-mcp-config --append-system-prompt <project dir, profile, "use
  mcp__bancada__verify; iterate until it passes">`. The
  `mcp__bancada__verify` entry in `--allowedTools` is load-bearing (without
  it a headless `tools/call` stalls on a permission prompt) — unit-test its
  presence. ~~**Isolation: use `--bare`**~~ **`--bare` was dropped** after
  the milestone-5 prototype: `claude --help` (2.1.220) documents that under
  `--bare` "Anthropic auth is strictly `ANTHROPIC_API_KEY` or `apiKeyHelper`
  via `--settings` (OAuth and keychain are never read)", and the prototype
  reproduced `authentication_failed` on turn one. Since decision #2 is that
  auth comes free from the user's existing login, R3's isolation is traded
  for working auth; `--strict-mcp-config` plus the explicit
  `--allowedTools`/`--disallowedTools` pair remain.
- **Process plumbing** (from adversarial review of the real code):
  - **Stdin writer thread** fed by an mpsc channel — never write to child
    stdin while holding the `agent` mutex (large pasted messages > 64 KB
    pipe buffer + a claude blocked awaiting an MCP reply = deadlock cycle).
    `agent_send` is an `async` command that just enqueues.
  - **Stderr drain thread** (emit as `agent://event` `{type:"stderr"}`) —
    an un-drained pipe wedges the child, same reason `start_monitor` runs
    two reader threads.
  - Set generous `MCP_TOOL_TIMEOUT`/`MCP_TIMEOUT` env on the child — a cold
    ESP32 platform build is multi-minute, longer than the default MCP tool
    timeout.
  - Frontend calls `agent_stop(pid)` on `agent://closed` so the exited child
    is reaped (no zombies) — `pid` is a post-review fix (F4): a stale close
    from a session already superseded by a newer `agent_start` must not
    kill the new one, so `agent_stop` refuses unless `pid` matches the
    *live* session (`should_stop_agent`). The user's explicit "stop"/"new
    session" action omits `pid` and always proceeds.
- **`verify` tool**: minimal MCP streamable-HTTP server thread inside the
  Tauri process (`tiny_http` — no reusable server dep exists in the
  workspace; hyper/reqwest are tauri client-side transitives). **Bind
  `127.0.0.1:0`** and read the port back (no collision roulette).
  Bearer-token; methods: `initialize` / `tools/list` / `tools/call`;
  JSON-RPC notifications (e.g. `notifications/initialized`, no `id`) get a
  distinct `McpReply::NoContent` → **HTTP 202 empty body** (spec-required).
  Shutdown: keep the `tiny_http::Server` handle and call `unblock()` (or use
  `recv_timeout`) before joining — a plain `AtomicBool` can't break the
  blocking `recv()`. The listener gets **owned clones at spawn time**
  (`ArduinoCli`, `AppHandle`, sketch_dir, profile, fqbn) and never locks
  `state.agent` (documented trade-off: agent verify keeps the profile from
  `agent_start` even if the user switches boards mid-session).
  - On `tools/call verify`: runs the **same** `cli.compile(...)` path
    `compile_sketch` uses (src-tauri/src/lib.rs ~705–730; signature
    confirmed at core/src/cli.rs:333–343; `ArduinoCli` is `Clone` holding
    only `bin`) with the `build://line` emit — output streams to the
    existing Console for free (Console subscribes unconditionally, App.tsx
    ~239–243); agent receives `success`, `exit_code`, capped summary via
    `summarize_build_output` (decision 7 above).
  - **Build gate (new, fixes a real race):** nothing serializes compiles
    today — user-Verify mutual exclusion is only the frontend `busy` flag,
    which agent builds bypass. Add `build_gate: Mutex<()>` in `AppState`,
    taken by `compile_sketch`, `upload_sketch`, and the MCP verify path
    (try_lock + "build already in progress" error to the agent). Emit
    `agent://event {type:"verify_started"/"verify_done"}` so App can set
    `busy` during agent builds (also restores the port-rescan deferral,
    App.tsx ~256–263).

## Event contracts

### Tauri events

- **`agent://event`** — emitted for every parsed line from the child's
  stdout (`parse_event`), plus synthetic events the host adds:
  - `{type:"stderr"}` from the stderr drain thread.
  - `{type:"verify_started"}` / `{type:"verify_done"}` around an MCP
    `verify` call, so the frontend can set `busy` during agent builds.
- **`agent://closed`** — emitted on child EOF, carries `{reason, pid}`. The
  frontend calls `agent_stop(pid)` on receipt so the exited child is reaped;
  `pid` (post-review fix F4) lets `agent_stop` refuse a stale close whose
  session has since been superseded by a newer `agent_start`.

### stream-json `AgentEvent` (core/src/agent.rs)

A serde enum over the CLI's stream-json output, with a tolerant catch-all
since the wire protocol is undocumented (see Risk R1):

- `System`
- `Assistant`
- `User`
- `StreamEvent`
- `Result`
- `Unknown(Value)` — catch-all so unknown event types never error the
  parser.

`parse_event(line)` decodes one line into this enum. `user_message_json` and
`interrupt_json` build the corresponding stdin NDJSON messages.

### MCP `verify` tool (core/src/mcp.rs)

- **Server name:** `bancada`.
- **Tool name:** `verify` — no arguments; the sketch dir, profile, and fqbn
  are bound at MCP-listener spawn time from the session that started the
  agent, not supplied by the caller.
- **Full tool id as seen by the agent:** `mcp__bancada__verify` (this is
  the exact string that must appear in `agent_args()`'s `--allowedTools`).
- JSON-RPC 2.0 over the loopback HTTP listener; `handle_request(body,
  tools) -> McpReply` either replies immediately or returns
  `CallTool{id, name, args}` for the host to execute, plus
  `tool_result_json` to format the result back.
- Methods: `initialize`, `tools/list`, `tools/call`.
- Notifications (no `id`, e.g. `notifications/initialized`) map to
  `McpReply::NoContent` → HTTP 202 with an empty body (spec-required).
- `tools/call verify` result: `success`, `exit_code`, and a capped output
  summary (decision 7 above: all stderr + stdout tail, ~200 lines / 50 KB).

## Safety model

- The agent's cwd is the sketch dir. **Prototype finding (milestone 5):
  `--permission-mode acceptEdits` does NOT confine built-in file tools to
  the cwd** — a `-p` session asked to `Write /tmp/bancada-probe-outofcwd.txt`
  created it with `"permission_denials":[]`. The sandbox is therefore
  weaker than this spec assumed; tightening it (a `PreToolUse` hook or a
  path-scoped deny rule that still permits absolute paths *inside* the
  project) is an open follow-up, not something slice 1 provides.
- `Bash`, `WebFetch`, `WebSearch`, `Task`, `NotebookEdit`, `KillShell`, and
  `BashOutput` are explicitly disallowed via `--disallowedTools`.
- `--strict-mcp-config` keeps the user's personal MCP servers out of the
  embedded session.
- The user's own hooks/skills/plugins **do** load (see the `--bare` note in
  Architecture): `--setting-sources` alone does not block user hooks
  (doc-verified) and `--bare`, which would, breaks auth.
- `verify` is compile-only — it never runs `-u` (upload).
- Edits auto-apply with no per-edit approval gate (decision 4); the panel
  warns when the project is not under git, since there is no undo path
  without it (see Risk R4).
- The MCP listener is bearer-token protected and bound to `127.0.0.1`. The
  token itself is written to a 0600 temp file and referenced by path in
  `--mcp-config`, never placed inline in argv (post-review fix F5 — argv is
  readable by any local process via `/proc/<pid>/cmdline` on Linux, which
  would have handed the token to exactly the "other local processes" the
  token exists to keep out).

## Risks

- **R1** The CLI's stdin stream-json protocol (user-message shape,
  `control_request` interrupt) is **undocumented** — doc-verified; the SDK
  is the documented surface and shells this same protocol. Mitigation:
  record fixtures from the real CLI 2.1.220 in milestone 3, tolerant parser
  (`Unknown` catch-all), `agent_probe` min-version gate, kill-based
  interrupt as the reliable path. Escape hatch if the protocol proves
  unstable: swap the transport to a bundled Agent SDK sidecar later —
  `core::agent` types and the whole frontend are transport-agnostic.
- **R2** ~~unspecified~~ **RESOLVED by the milestone-5 prototype**: the CLI
  accepts plain `Content-Type: application/json` POST responses — no SSE
  wrapping needed (it advertises `Accept: application/json,
  text/event-stream` and is happy with either). It *also* opens a
  `GET /mcp` server→client SSE stream, which a server that offers no such
  stream must answer **405**; answering it with a JSON-RPC body instead put
  the client into a tight reconnect busy-loop.
- **R3** User-level settings/hooks leaking into the embedded agent →
  **partly unmitigated**: `--strict-mcp-config` covers MCP servers, but
  `--bare` (the only thing that blocks user hooks/skills/plugins) breaks
  auth and was dropped, and `--setting-sources` does NOT block hooks
  (doc-verified). Accepted for slice 1.
- **R4** No undo without git → UI hint in slice 1; optional pre-session
  snapshot as fast-follow.
- **R5** Concurrent user/agent builds racing the arduino-cli build cache →
  `build_gate: Mutex<()>` in `AppState` shared by `compile_sketch` /
  `upload_sketch` / MCP verify (see Architecture).
