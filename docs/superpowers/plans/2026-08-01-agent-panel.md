# Agent Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Ship an Assistant panel where the user chats with a Claude agent
that reads/edits project files, runs Verify through bancada's existing
compile path, and iterates on compiler errors — the first slice of
bancada's AI-assisted-IDE vision.

**Architecture:** `claude` CLI spawned as a supervised child process
(stream-json over stdio), thin src-tauri plumbing, a loopback-HTTP MCP
server exposing one `verify` tool that reuses the existing compile path.
Full architecture, event contracts, and safety model are in the spec.

**Tech Stack:** Rust (`bancada-core`, `src-tauri`, `tiny_http`),
TypeScript/React 18, vitest, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-01-agent-panel-design.md`

## Global Constraints

- Milestones 1–2 (this spec and this plan) are already done by the time
  Milestone 3 starts.
- Commit trailer: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Repo workflow order: spec → plan → implement → release.

## Files

### New — core (pure, heavily tested)

- `core/src/agent.rs` — `AgentEvent` serde enum
  (System/Assistant/User/StreamEvent/Result + `Unknown(Value)` catch-all so
  unknown event types never error); `parse_event(line)`;
  `user_message_json`, `interrupt_json`; `agent_args(cfg)` (tested like
  `compile_args`, core/src/cli.rs ~396–413); `summarize_build_output(lines,
  caps)` — keeps **all** stderr lines plus the tail of stdout, capped at
  **~200 lines / 50 KB total** (decision made after the initial plan; see
  spec §Decisions #7). Register `pub mod agent;` in `core/src/lib.rs`.
- `core/src/mcp.rs` — JSON-RPC 2.0 types; `handle_request(body, tools) ->
  McpReply` (immediate reply or `CallTool{id,name,args}` for the host to
  execute + `tool_result_json`); `verify` tool schema (no-args tool named
  `verify`, server name `bancada`, full tool id `mcp__bancada__verify`).
  All protocol decisions testable without a socket.

### Modified — src-tauri (thin; follow the serial-monitor pattern at lib.rs ~762–798)

- `src-tauri/src/lib.rs`: `AgentSession { child, mcp_port, mcp_token,
  stop_mcp, sketch_dir, profile, fqbn }` as `agent: Mutex<Option<AgentSession>>`
  in `AppState` (sibling of `serial`/`mqtt`). Commands: `agent_probe`
  (`claude --version` + min-version gate), `agent_start(sketch_dir,
  profile, fqbn)`, `agent_send(text)` (writeln to stdin, like
  `monitor_send` ~811), `agent_interrupt`, `agent_stop`. Reader thread:
  stdout lines → `parse_event` → emit `agent://event`; EOF → `agent://closed
  {reason}`; stderr thread → `agent://event` `{type:"stderr"}`. MCP
  listener thread owns clones of `ArduinoCli` + `AppHandle`, dispatches via
  `core::mcp`. Kill child + stop listener in the existing `RunEvent::Exit`
  handler (~1682–1694). Document new events in the lib.rs header comment.
  Add `tiny_http` to `src-tauri/Cargo.toml`.

### New/modified — frontend

- `src/bottomTabs.ts`: tab `"agent"`, group `"assistant"` ("🤖 Assistant")
  in all four Records (`GROUP_OF`, `GROUP_TABS`, `GROUP_LABEL`,
  `TAB_LABEL`). Resize + maximize ARE free (they belong to the whole
  `.bottom` section). **Not free (review finding):** App.tsx hard-codes
  group→tab in TWO ternaries — the `bottomTab` derivation (~113–118) and
  the group-button onClick (~884–887) — where a 4th group silently falls
  through to `obsTab`; both must add `assistant → "agent"`. Add
  `agentMounted` flag + `openBottomTab` case (~180–188). Extend
  `src/__tests__/bottomTabs.test.ts` D1 with an assistant assertion.
- **Agent event subscription lives at App level** (not panel-owned): App
  subscribes `onAgentEvent`, feeds `agentStore.push(ev)`, sets the unseen
  dot when `bottomTabRef.current !== "agent"` (unseen is only ever set from
  App-level listeners today — panel-owned Channels never get dots, per
  MQTT precedent), and handles file-changed side effects. Solves dots +
  file refresh in one place.
- `src/agent/agentStore.ts` — pure TS, **modeled on `src/obs/obsStore.ts`**:
  `push(ev)`; ordered messages (user / assistant text accumulated from
  deltas / tool cards `{name, input, status, result}`); session status;
  last Result (cost, turns, error). `version` + `snapshot()`; panel polls
  **10 Hz while `active`** (chat deltas read smoother than the 4 Hz obs
  cadence; store still fills while hidden). Vitest fixtures in
  `src/agent/__tests__/agentStore.test.ts`.
- `src/agent/diff.ts` — pure diff helper for Edit/Write tool cards, vitest-
  tested. Renders **unified diffs of `old_string` → `new_string`**: full
  strings (no elision), `-`/`+` lines, red/green backgrounds, mono font
  (decision made after the initial plan; see spec §Decisions #8).
- `src/agent/fences.ts` — small pure ```` ``` ````-aware fence splitter;
  assistant text renders as **plain text + mono code blocks** — no
  markdown dep exists in the repo and adding renderer+sanitizer is out of
  character; full markdown explicitly deferred.
- `src/components/AgentPanel.tsx` — message list; tool cards (Edit/Write →
  diff card; verify → status + exit code + "Open Console ↗" via
  `openBottomTab("build")` — `"build"` confirmed as a real tab id); input +
  Send (Enter), Stop, New session; footer with cost + turns; empty states
  (no project / `agent_probe` failed → install/login guidance, same UX as
  the arduino-cli-missing banner, App.tsx ~270); "project not under git —
  agent edits have no undo" hint. CSS: follow the `.obs-panel` flex-column
  + `min-height: 0` + scrolling-list recipe (styles.css ~843 ff.) so the
  input row survives the 120 px min height.
- `src/api.ts`: `agentProbe/agentStart/agentSend/agentInterrupt/agentStop/
  onAgentEvent/onAgentClosed` typed wrappers (invoke + listen style,
  api.ts ~589–599).
- `src/App.tsx` wiring: **call existing `saveAll()` (~429, confirmed stable
  useCallback) before every `agentSend`**; after each Edit/Write tool
  result: re-run `listSketchFiles` (~400 pattern), and if the touched file
  is open and not dirty, re-read via `readSketchFile` + `setContent`
  (verify during implementation that programmatic `setContent` does not
  fire `onChange`/re-dirty — CodeMirror annotates external updates, but
  confirm). **Use refs, not captured state**
  (`buffersRef.current.has(relPath)` for dirtiness + new `openFileRef`) —
  the repo documents this exact stale-closure hazard (App.tsx ~90–93).
  **Dirty-conflict rule:** if the agent touched a file the user has dirty,
  mark it conflicted, use `notify(msg, true)`, and refuse the next
  `agentSend` until resolved — otherwise the next send's `saveAll()`
  silently clobbers the agent's on-disk edit with the stale buffer. No FS
  watcher — tool events are the change feed.

## Milestones

3. `core/src/agent.rs` + tests (stream-json fixtures recorded from the
   real CLI via `include_str!`).
4. `core/src/mcp.rs` + tests (initialize/list/call/bad-token/bad-json).
5. src-tauri: MCP listener + `AgentSession` + commands + events + exit
   cleanup. **Prototype the HTTP MCP round-trip first** — it is the only
   genuinely unknown integration point (see spec Risk R2).
6. `src/api.ts` wrappers; `agentStore` + `diff.ts` + vitest.
7. `bottomTabs` group; `AgentPanel.tsx`; App.tsx wiring.
8. Env-gated live tests + manual verification pass.
9. Release 0.8.0 (bump `package.json`, `src-tauri/tauri.conf.json`, Cargo
   versions; `Release 0.8.0` commit).

### Learning-mode contribution points (5–10-line user-authored pieces, flagged during implementation)

- `summarize_build_output` capping strategy in `core/src/agent.rs` (what
  the agent sees of a huge build log — signal-vs-noise trade-off).
- Diff-card rendering choice in `src/agent/diff.ts` (inline vs unified;
  how much context).

## Verification

- **Headless**: `cargo test` (parser fixtures incl. unknown-event
  tolerance, argv builder, MCP dispatch, summarizer; src-tauri integration
  test with the `with_stub` stub-arduino-cli pattern from cli.rs tests).
  `npx vitest` (agentStore sequences: interleaved deltas, tool lifecycle,
  closed mid-turn; diff; bottomTabs group).
- **Live (env-gated, like docs/hardware-smoke-tests.md)**: `#[ignore]` +
  `BANCADA_AGENT_LIVE=1` (the env var Task 5's prototype test actually
  landed with; this doc originally said `BANCADA_AGENT_TEST=1`, corrected
  in Task 8) — spawn real `claude`, "reply with the word pong", assert
  init+result; second test drives `mcp__bancada__verify` end-to-end on a
  temp Blink sketch with a real (non-stubbed) `arduino-cli` compile.
- **Manual**: open a sketch with a planted `missing ;`, ask "make this
  build" → diff card → verify card → Console streams → green result. Then:
  Stop mid-turn; kill `claude` externally → closed banner; project without
  git → warning hint; `claude` renamed off PATH → install guidance.

## Risks

- **R1** The CLI's stdin stream-json protocol (user-message shape,
  `control_request` interrupt) is **undocumented** — doc-verified; the SDK
  is the documented surface and shells this same protocol. Mitigation:
  record fixtures from the real CLI 2.1.220 in milestone 3, tolerant parser
  (`Unknown` catch-all), `agent_probe` min-version gate, kill-based
  interrupt as the reliable path. Escape hatch if the protocol proves
  unstable: swap the transport to a bundled Agent SDK sidecar later —
  `core::agent` types and the whole frontend are transport-agnostic.
- **R2** `claude`'s HTTP MCP client behavior (plain JSON POST response vs
  SSE) is unspecified in docs → **prototype the HTTP MCP round-trip first
  in milestone 5**; wrap as single-event SSE if needed; worst case, fall
  back to a tiny stdio→HTTP proxy binary. Same prototype also confirms:
  `--bare` + `--mcp-config` coexistence, out-of-cwd edit denial under
  acceptEdits in `-p` mode.
- **R3** User-level settings/hooks leaking into the embedded agent →
  `--strict-mcp-config` + `--bare` (doc-verified: `--setting-sources` does
  NOT block hooks). Project-level CLAUDE.md loading is arguably a feature;
  confirm `--bare` behavior in the prototype.
- **R4** No undo without git → UI hint in slice 1; optional pre-session
  snapshot as fast-follow.
- **R5** Concurrent user/agent builds racing the arduino-cli build cache →
  `build_gate: Mutex<()>` in `AppState` shared by `compile_sketch` /
  `upload_sketch` / MCP verify (see spec Architecture).
