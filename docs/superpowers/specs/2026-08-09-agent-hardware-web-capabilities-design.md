# Agent hardware + web capabilities — design spec

**Date:** 2026-08-09
**Status:** approved (brainstormed with the operator; concurrency review folded in)

## Goal

Extend the Assistant panel's embedded `claude` session beyond edit→verify: let it flash
the board, watch and talk to the serial monitor, and reach the web — **without raw
Bash**, and without weakening the write-confinement model. Ship as Bancada 0.12.0.

## Principle

Every hardware capability follows the `verify` design move: a bancada-owned MCP tool
that routes through the app's own code path, scoped by what the tool can *express*
rather than what a prompt forbids. Concretely: the `upload` tool takes **no port
argument** — it flashes the UI-selected board through the same build-gated path as the
Upload button. A confused or manipulated agent can only flash the board the user is
looking at, with the profile/FQBN the session was started with.

## New capabilities

### `mcp__bancada__upload`
- No arguments. Target = session-frozen `profile`/`fqbn` (what `verify` built) + the
  live UI-selected `{port, baud}` mirrored into Rust by a new `set_selected_target`
  command.
- **Arm-gated:** a per-session "Allow uploads" toggle in the panel footer, off by
  default. Unarmed, the tool returns an error telling the agent to ask the user.
  Armed, uploads run unprompted so the edit→verify→flash→watch-serial loop stays
  autonomous.
- Flow: cancel check → armed check → target check → build gate (`try_lock`) → serial
  slot: scope owner → error; monitor owner → evict, then **drop the lock** → `compile
  -u` streaming to the Build console and a collected summary (same
  `success:/exit_code:` text contract as `verify`). No automatic monitor restart — the
  agent's next `serial_read` auto-starts it.

### `mcp__bancada__serial_read`
- Optional `wait_s` (0–10): poll for fresh output, "call again to keep waiting".
- Reads from a new process-lifetime **serial ring buffer** (≈500 lines, 4 KB/line cap,
  monotonic sequence numbers) fed by the monitor reader threads; each agent session
  keeps a cursor, initialized at session start so old backlog is not replayed; lines
  that fell off the window are reported as `[N lines dropped]`.
- If no one owns the port: auto-starts the monitor at the UI-selected target and emits
  `serial://started` so the UI stays in sync. If the scope owns the port: error.

### `mcp__bancada__serial_send`
- `data` (string, required): one line to the monitor's stdin, newline appended — same
  path as the Monitor tab's send box. Error if the monitor is not running.

### Web
- `WebFetch`/`WebSearch` join `--tools`/`--allowedTools` and leave `--disallowedTools`.
- Documented trade-off: reads were never confined; web access adds an **egress** path.
  Accepted deliberately; recorded in the README safety section.

## Concurrency contract

- Lock order: `build_gate` (try-only, never waited on) → `serial` → nothing.
- `serial` is a **leaf lock**, held only across bounded-short operations (child spawn,
  port open, pipe write, evict) — never across a compile/upload or a wait loop.
- The MCP listener thread may take `serial` (it is never joined under it) but still
  never locks `state.agent`. Monitor/scope reader threads never touch `serial`; reader
  threads may lock the ring, and nothing joins a thread while holding the ring.
- `wait_s` polls the ring at ~100 ms checking the session cancel flag; tiny_http's
  `unblock()` is sticky, so shutdown is delayed at most one handler, never lost.

## Explicitly out

Raw `Bash` (the guard hook cannot police arbitrary shell — rejected), agent-chosen
ports/baud, scope access for the agent, per-flash confirmation prompts (replaced by the
arm toggle), automatic monitor restart after upload.

## Safety regression assertions

`Bash` stays out of `--tools`/`--allowedTools` (argv tests); a session whose init
advertises unexpected tools is still killed (drift alarm); the PreToolUse guard hook and
`permissions.deny` anchors are untouched; `EXPECTED_TOOLS`/`BUILTIN_TOOLS`/
`--allowedTools` change in the same commit so the drift alarm never fires spuriously.
