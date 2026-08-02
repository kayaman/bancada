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
  --include-partial-messages --permission-mode acceptEdits
  --tools Read,Edit,Write,Glob,Grep
  --allowedTools Read,Edit,Write,Glob,Grep,mcp__bancada__verify
  --disallowedTools
  "Bash,WebFetch,WebSearch,Task,NotebookEdit,KillShell,BashOutput"
  --mcp-config <path to a 0600 temp file containing the loopback HTTP URL +
  bearer token as JSON — a path, not inline JSON: a post-review fix (F5)
  found the token riding argv is world-readable via /proc/<pid>/cmdline on
  Linux, which defeats the point of a bearer token. `write_mcp_config_file`
  writes and chmods the file; `agent_start`/`stop_agent_session` delete it
  on every exit path>
  --strict-mcp-config
  --settings <path to a 0600 temp file, OUTSIDE the project tree, carrying
  the write-confinement policy: `permissions.deny` rules plus the
  `PreToolUse` guard hook — see Safety model>
  --append-system-prompt <project dir, profile, "use
  mcp__bancada__verify; iterate until it passes">`, plus env
  `MCP_TOOL_TIMEOUT`/`MCP_TIMEOUT` and
  `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`. The `mcp__bancada__verify` entry in
  `--allowedTools` is load-bearing (without it a headless `tools/call`
  stalls on a permission prompt) — unit-test its presence.
  ~~**Isolation: use `--bare`**~~ **`--bare` was dropped** after
  the milestone-5 prototype: `claude --help` (2.1.220) documents that under
  `--bare` "Anthropic auth is strictly `ANTHROPIC_API_KEY` or `apiKeyHelper`
  via `--settings` (OAuth and keychain are never read)", and the prototype
  reproduced `authentication_failed` on turn one. Since decision #2 is that
  auth comes free from the user's existing login, R3's isolation is traded
  for working auth. **`--safe-mode` was evaluated as its replacement and
  also rejected** (0.8.0): it does stop the user's hooks firing, but the
  same probe showed it disables `--mcp-config` servers, so
  `mcp__bancada__verify` disappears. The confinement therefore comes from
  `--settings` + `--tools`, not from dropping the user's config.
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
  - `{type:"stderr", line}` from the stderr drain thread.
  - `{type:"unparsed", line}` for a stdout line that is not valid JSON at
    all — `parse_event`'s only `Err` case. The raw line is preserved rather
    than dropped, because a CLI that starts printing non-JSON on stdout is
    something the transcript should show, not swallow. The store tolerates
    unknown types, so this is additive.
  - `{type:"verify_started", pid}` / `{type:"verify_done", success, pid}`
    around an MCP `verify` call, so the frontend can set `busy` during agent
    builds.
  - `{type:"security_alarm", kind, detail, pid}` when the host's backstop
    refuses something and kills the session (`kind` is `path_escape` or
    `unexpected_tools`).
  - **`pid` on the three synthetic host events** is the session's child pid
    (C1). The MCP listener thread and the stdout reader can both outlive the
    session that started them, so an unstamped event from a stopped session
    could repaint a *newer* session's panel — spinner stuck on, or a stale
    "done" clearing a live build. The store drops any whose pid is not the
    session it is showing.
- **`agent://closed`** — emitted on child EOF, carries `{reason, pid}`. The
  backend's own stdout reader reaps the session at EOF; the frontend also
  calls `agent_stop(pid)` on receipt as a second path. `pid` (post-review
  fix F4) lets `agent_stop` — and, since 0.8.0, `agentStore.closed()` —
  refuse a stale close whose session has since been superseded by a newer
  `agent_start`.

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
  summary (decision 7 above: all stderr + stdout tail, ~200 lines / 50 KB),
  formatted as `"success: <bool>\nexit_code: <n>\n\n<summary>"` and parsed
  back by `src/agent/verifyResult.ts`.
- **`isError` semantics (decision, previously only in code comments):**
  `isError` means *the tool could not run* — build gate contention, a
  missing `arduino-cli`, a session stopped mid-flight. A compile that ran
  and **failed** is reported with `isError: false`, because a fix-the-build
  loop is the normal path of this whole feature and flagging it as a tool
  failure invites the model to stop calling the tool. Pass/fail for a
  completed verify therefore lives in the `success:` line of the text, not
  in `isError` — which is exactly why that text format has its own parser
  module and tests.

## Safety model

> Rewritten for 0.8.0 after the final review established that the composed
> risk was **arbitrary command execution as the user**, not merely
> out-of-project edits. Everything below is stated as verified-against-2.1.220
> or not stated at all.

### The risk being managed

Three facts compose:

1. **The user's own configuration loads into the embedded session.** `--bare`
   would prevent it but breaks keychain auth (verified); `--safe-mode` would
   prevent it but disables `--mcp-config`, taking the `verify` tool with it
   (verified: `mcp_servers: []`, no `mcp__bancada__verify`). Neither is
   usable, so user hooks/skills/plugins load. A `SessionStart` hook firing in
   the embedded session was observed directly.
2. **Hooks are shell commands.**
3. **`Write` was not confined to the project.**

(1)+(2)+(3) means: the agent writes a settings file containing a hook, and
the CLI runs it as a shell command, as the user. Bancada cannot close (1) or
(2) without losing auth or the compiler, so **(3) is what gets closed**.

### What actually enforces it

Four layers, strongest first. Each is annotated with what verified it, and
**`--disallowedTools` is deliberately absent from this list** — see below.

1. **`permissions.deny` rules** (`core::agent::deny_rules`, delivered via
   `--settings`) protecting `<project>/.claude/**`, `<project>/.git/**`,
   `<project>/.mcp.json`, `~/.claude/**`, `~/.claude.json`, shell rc files
   and `/etc/**`. This is the *anchor*: deny rules are evaluated **before**
   hooks and are unaffected by `disableAllHooks`. Two syntax traps, both
   verified and both silent when got wrong:
   - only `Edit(...)` and `Read(...)` patterns are consulted for file
     permission checks since 2.1.210; **`Write(...)` rules are accepted and
     never evaluated**;
   - `//` anchors at the filesystem root, while a single `/` anchors at *the
     directory containing the settings file*.
2. **A `PreToolUse` hook** doing subtree containment — the thing deny rules
   cannot express, since a denylist has no "everything except here" form.
   Its command is the Bancada binary itself (`bancada --agent-guard <dir>`),
   so the policy is the same tested `core::agent::path_is_confined`. It
   fails closed on unparseable input, passes by printing nothing (never by
   `permissionDecision: "allow"`, which would bypass the deny rules; and
   never `"defer"`, which is broken in 2.1.220), and always exits 0 because
   a hook that exits non-zero is "log and continue", i.e. fail *open*.
3. **A pre-flight refusal.** `agent_start` reads every
   `.claude/settings.json` / `settings.local.json` from the sketch dir up to
   the filesystem root, plus `~/.claude/`, and **refuses to start** if any
   sets `disableAllHooks` — because that switches layer 2 off entirely.
4. **A detect-and-stop backstop.** The stdout reader independently re-checks
   every `Edit`/`Write` `tool_use` against `path_is_confined`, and the
   `system`/`init` `tools` array against `EXPECTED_TOOLS`; either failing
   emits `{type:"security_alarm"}` and kills the session. This is weaker
   than a refusal — the write may already have happened — and exists because
   layers 1–3 all depend on the CLI's own policy engine behaving as probed.

**Live verification of the whole production configuration** (one turn, agent
asked to make four writes):

| attempt | result | blocked by |
|---|---|---|
| out-of-project `/tmp/…` | refused, nothing on disk | layer 2 (hook) |
| `<project>/.claude/settings.json` | refused | layer 1 (deny rule) |
| `<project>/.git/hooks/pre-commit` | refused | layer 1 (deny rule) |
| `<project>/helper.h` | **created** | — (the main use case still works) |

The hook denial appears in the CLI's `permission_denials`; **deny-rule
denials do not** — do not use that field as an audit signal.

### Tool restriction

- **`--tools Read,Edit,Write,Glob,Grep` is a real restriction.** Verified by
  reading the `system`/`init` `tools` array: without it a session lists 25
  built-ins including `Skill`, `Task*`, `Monitor`, `Workflow`, `SendMessage`,
  `LSP` and `Cron*`. It does not filter MCP tools, so `mcp__bancada__verify`
  survives.
- **`--disallowedTools` is NOT a boundary.** With it set and `--tools`
  absent, all 25 of those tools were still offered. It is a permission-layer
  nudge, kept only because defence in depth is free. No document, comment or
  code should present it as isolation.
- `--strict-mcp-config` keeps the user's personal MCP servers out (and
  closes the `.mcp.json` vector).
- On `system/init` the tool list is **asserted** against the expected set;
  anything unexpected stops the session (closes I3).

### What remains open — stated plainly

- **Reads are not confined at all.** The agent can `Read` anything the user's
  uid can, including `~/.ssh` and `~/.claude/.credentials.json`. Only writes
  are confined.
- **This is in-process policy, in the process the model drives.** A bug in
  path normalisation, a new file-writing tool, or a tool-input schema change
  is a bypass. A *real* boundary requires wrapping the whole `claude`
  process — container, dedicated uid, or bubblewrap. That is the only thing
  that would make the guarantee independent of the CLI's own policy engine,
  and Bancada does not do it.
- **A pre-existing hostile hook in the user's own config still runs.**
  Bancada stops the agent from *installing* one; it cannot stop one already
  there.
- **`--managed-settings`** (the one tier `disableAllHooks` cannot override)
  does not carry hooks — probed clean, the hook never fired.
- **Prompt injection persists via `CLAUDE.md` and project files**, which are
  legitimately inside the writable subtree. `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`
  is set on the child to close the auto-memory variant of this.

### Unchanged from earlier slices

- `verify` is compile-only — it never runs `-u` (upload).
- Edits auto-apply with no per-edit approval gate (decision 4); the panel
  warns when the project is not under git, since there is no undo path
  without it (see Risk R4).
- The MCP listener is bearer-token protected and bound to `127.0.0.1`. The
  token is written to a 0600 temp file and referenced by path in
  `--mcp-config`, never inline in argv (F5 — argv is readable by any local
  process via `/proc/<pid>/cmdline` on Linux). The `--settings` file gets the
  same 0600 treatment and lives **outside the project tree**, so the agent
  cannot rewrite its own policy. `random_token()` fails the session rather
  than falling back to a guessable clock/ASLR mix.

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
  **still true, and now compensated rather than accepted.**
  `--strict-mcp-config` covers MCP servers, but neither `--bare` (breaks
  auth) nor `--safe-mode` (kills `--mcp-config`, so no `verify`) can be
  used, and `--setting-sources` does NOT block hooks (doc-verified). The
  user's hooks/skills/plugins do load. Since hooks are shell commands, R3
  composed with an unconfined `Write` into **arbitrary command execution**;
  0.8.0 closes the write leg (see Safety model) so the agent can no longer
  install a hook, and asserts the session's tool list so an unexpected
  plugin-provided tool stops the session. A hostile hook *already present*
  in the user's config still runs — that is the residue.
- **R6 (new, 0.8.0)** The confinement is **in-process policy inside the
  process the model drives**, and reads are not confined at all. A hard
  boundary needs the whole `claude` process wrapped (container, dedicated
  uid, bubblewrap). Out of scope for 0.8.0; the honest description of what
  ships is "the agent cannot write outside the project or escalate into
  code execution through the CLI's own config", not "the agent is
  sandboxed".
- **R4** No undo without git → UI hint in slice 1; optional pre-session
  snapshot as fast-follow.
- **R5** Concurrent user/agent builds racing the arduino-cli build cache →
  `build_gate: Mutex<()>` in `AppState` shared by `compile_sketch` /
  `upload_sketch` / MCP verify (see Architecture).
