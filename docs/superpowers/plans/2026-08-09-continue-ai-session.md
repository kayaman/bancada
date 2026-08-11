# Continue an AI Session (Assistant panel)

## Context

Bancada's Assistant panel starts every claude-CLI child fresh: History can
*replay* a past chat read-only (spec 2026-08-05 explicitly deferred resume),
but there is no way to pick up where a session left off with its facts in
context. Everything needed already exists in pieces: each saved ndjson chat
records the CLI `session_id` verbatim inside its `push` ops, `replayChat()`
rebuilds a store from disk, and the CLI supports native `--resume`.

**Approach (user-selected): native `--resume <session_id>`, with automatic
fallback** to a fresh session whose system prompt carries a bounded facts
block distilled from the saved chat, for when the CLI transcript is gone.

## Implementation

### Backend

1. **`core/src/agent.rs`** — `AgentCfg` gains `resume_session_id:
   Option<String>`; `agent_args()` (:741) appends `--resume <id>` as the
   *final* flag pair (pinned `args[0..9]` prefix untouched). Update the two
   pinned argv tests + all `AgentCfg` literals; add
   `agent_args_appends_resume_id_as_the_final_pair` (Some → last two args;
   None → no `--resume` anywhere).
2. **`src-tauri/src/lib.rs` `agent_start` (:3192)** — two new optional
   params: `resume_session_id` (validated `^[0-9a-fA-F-]{8,64}$` so a value
   can never parse as a flag) and `context_facts` (clamped 4 KiB, appended to
   `system_prompt_extra` with a blank line). Tests: facts land in extra;
   malformed id → Err.

### Frontend plumbing

3. **`src/api.ts`** — `agentStart(sketchDir, profile?, fqbn?, uploadsArmed =
   false, resumeSessionId?, contextFacts?)`; existing callers unchanged.
   Contract test in api.test.ts.
4. **`src/agent/chatLog.ts`** — factor `replayChat` into
   `applyChatOps(store, lines)` + thin wrapper. Add
   `ChatRecorder.resume(fileName, send)`: like `start` but writes **no meta
   line** (a second meta would corrupt core's `title_from_head`,
   core/src/chatlog.rs:126). Continued chats append to the SAME ndjson.
5. **`src/agent/agentStore.ts`** — `prepareContinuation()`: supersede the
   dead pid, status → "idle" (re-enables `sendToAgent`'s lazy start), clear
   closed/alarm/turn flags and assistant cursor; KEEP messages, sessionId,
   usage, rawLog.
6. **New `src/agent/continueChat.ts`** — pure `distillFacts(snap)`: last 3
   user texts (300 ch), last assistant text (600 ch), unique Edit/Write file
   paths (cap 15), last verify/upload outcome; hard cap 2048 chars.
7. **New `src/agent/resumeWatch.ts`** — resume-failure state machine
   (testable, fake timers): WATCHING buffers events; `system/init` →
   CONFIRMED (flush); pid-matched `closed` before init → FAILED (drop buffer,
   trigger fallback — a failed child's stderr never paints, no flapping);
   20 s timeout → CONFIRMED. Rationale: `--resume` with an unknown id (or a
   CLI too old for the flag) exits fast without ever emitting `system/init`.

### App wiring & UI

8. **`src/App.tsx`** — `continueChat(file)`: re-entry gate →
   `teardownAgentSession("continued another chat")` (kills any live session;
   uploadsArmed stays false — deliberate invariant; teardown also cancels
   any pending watch) → `chatLoad` → `applyChatOps` into the LIVE singleton
   + `prepareContinuation()` (never `clear()` afterward) → stash
   `{file, sessionId, facts}` → `chatRecorder.resume(file, …)`.
   `sendToAgent`'s idle branch passes `sessionId` (native) or `facts`
   (fallback-from-the-start when the chat has no session_id) and arms the
   ResumeWatch. `fallbackRespawn`: reap → `agentStart(…, facts)` →
   `sessionStarted(pid2)` + record → `agentSend(text)` (no duplicate
   `userSent` op). Listeners offer events to the watch first.
9. **`src/components/AgentPanel.tsx`** — new `onContinueChat(file)` prop;
   "▶ Continue this chat" in the replay back-bar (:357) and a ▶ beside 🗑 on
   History rows (:728); both reset panel-local views first. "New session"
   semantics untouched.

## Edge cases covered

- Chat with no recorded session_id → facts-only spawn, no resume attempt.
- CLI without `--resume` support → same no-init fallback path; no version gate.
- Continue pressed mid-session → teardown-first (old chat gets its `closed`
  op), arming reset.
- Double-continue race → gate + teardown cancels the pending watch; a stale
  fallback can never respawn over a newer continuation.
- Resume forks a new CLI session id → store's existing init/result handlers
  capture it; subsequent continues use the newest id.

## Verification

- `cargo test -p bancada-core --lib` (argv tests), `cargo check -p bancada`,
  `npx tsc --noEmit && npx vitest run` (chatLog resume-mode, applyChatOps,
  prepareContinuation, distillFacts bounds, resumeWatch transitions, api
  contract).
- Manual: in a project with saved chats — History → open chat → Continue →
  send a message referencing something only the old session knew (native
  path proves context); then delete `~/.claude`'s transcript for that id and
  continue again (fallback path: fresh session, facts block visible in
  behavior); confirm the continued chat appends to the same ndjson and
  History shows one entry.
