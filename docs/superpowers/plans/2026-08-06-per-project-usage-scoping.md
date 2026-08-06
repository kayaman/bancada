# Scope tokens and cost by project — a new project starts at zero

## Context

The Assistant's usage accounting leaks across projects. The storage side is
already correctly scoped — `core/src/chatlog.rs` keys each sketch's chat dir by
`sketch_key()` (fnv1a-64 of the full path) and `project_totals()` reads only
that dir — but the frontend treats a `sketchDir` change as a non-event.
`loadSketch()` (`src/App.tsx:456`, the sole open/switch/create path) resets only
editor state and never touches the two module-level singletons `agentStore`
(App.tsx:78) and `chatRecorder` (App.tsx:82). Consequences, all verified:

1. **Footer Σ chip** reads `agentStore.snapshot().sessionUsage`, zeroed only by
   `AgentStore.clear()` — reachable solely from the manual "New session" button.
   A freshly created project displays the previous project's cost/tokens.
2. **ChatRecorder** holds a `(f, l) => api.chatAppend(sketchDir, f, l)` closure
   frozen at `start()` (App.tsx:1070; chatLog.ts never re-reads). After a
   switch, every op — including the `result` usage that `project_totals` sums —
   keeps landing in the **old** project's chat dir.
3. Because the store still says `"running"`, `sendToAgent` (App.tsx:1073) skips
   `agent_start` and pipes the new project's prompt into the old project's
   `claude` child (whose cwd is the old sketch).
4. **History view** fetches `chatList`/`chatTotals` only on 🕘 click
   (AgentPanel.tsx:108-122) and never refetches on `sketchDir` change — new
   sketch's name rendered over old sketch's totals; `openChat`/`deleteChat`
   would mix B's dir with A's filenames.

**Design decision:** a `sketchDir` change is a **hard agent-session boundary** —
switching or creating a project ends any running session (same teardown as "New
session"), so Σ, transcript, recorder, and History all start fresh. A new
project then shows zero for free: empty store, no chats on disk. No backend /
Rust changes.

## Changes

### 1. `src/agent/agentStore.ts` — superseded-pid set (pure, TDD first)

Teardown fire-and-forgets `api.agentStop()` then `clear()`. The killed child's
stdout EOF later emits `agent://closed{pid}`; after `clear()`, `pidVal` is
`undefined` and `isOurs()` (agentStore.ts:121-123) treats that as "assume ours"
— the **fresh** store would flip to `"ended"`, disabling Send in the new
project. Today reachable only via the "New session" button; project switching
makes it routine.

- Add `private supersededPids = new Set<number>();`
- In `clear()`: before resetting, `if (this.pidVal !== undefined)
  this.supersededPids.add(this.pidVal);`. The set survives `clear()` (a `Set`,
  not one slot, covers rapid A→B→C double-switches with two EOFs in flight;
  growth is one entry per session per app run).
- In `isOurs(pid)`: first check `if (pid !== undefined &&
  this.supersededPids.has(pid)) return false;`, then existing logic. This also
  drops superseded `verify_started`/`verify_done`/`security_alarm` (they route
  through the same guard, agentStore.ts:130-137).

Known residual (document, don't fix): plain events (`assistant`, `stderr`)
carry no pid, so a handful already queued at teardown could paint into the
fresh store — fixing needs backend pid-stamping on every event; window is
milliseconds.

### 2. `src/App.tsx` — shared teardown, called from `loadSketch`

Extract `newAgentSession()`'s body (App.tsx:1101-1111) into:

```ts
const teardownAgentSession = (reason: string) => {
  // Replays stay honest: a switched-away chat ends with a closed line, not a
  // silent truncation. Status guard: an armed-but-never-started recorder
  // (agentStart threw before userSent → status "idle") must not flush a
  // meta+closed-only file; after a natural end ("ended") onAgentClosed
  // already wrote the real closed op.
  const status = agentStore.snapshot().status;
  if (chatRecorder.active && status !== "idle" && status !== "ended") {
    chatRecorder.record({ op: "closed", reason });
  }
  api.agentStop().catch(() => {});   // not awaited — same as today
  chatRecorder.stop();               // idempotent (chatLog.ts:134-138)
  agentStore.clear();
  agentConflictsRef.current.clear();
  setConflicts([]);
  setAgentBuilding(false);
};

const newAgentSession = () => teardownAgentSession("new session");
```

`ChatOp`'s closed `reason` is a free string (chatLog.ts:29) — no enum to
extend; `replayChat` renders any reason as "Session ended — {reason}".

Call site in `loadSketch`, after the `Promise.all` succeeds and **before**
`setSketchDir(dir)` (App.tsx:461/462):

```ts
// A sketchDir change is a hard agent boundary: Σ chip, transcript and chat
// recording are per-project. Same-dir reopen keeps the session.
if (sketchDirRef.current !== dir) teardownAgentSession("project switched");
setSketchDir(dir);
```

- **After the awaits**: if `listSketchFiles` rejects (dir vanished), we stay on
  the old project with its session intact.
- **`sketchDirRef.current`** (the established live mirror, App.tsx:107-111),
  not the render-scoped const — this code runs after an `await`.
- **Same-dir guard**: re-picking the open folder must not nuke a live session.
  Exact string compare is right because `sketch_key` hashes the exact path
  string too. Startup restore (`sketchDir` null → teardown runs) is a harmless
  no-op — idempotence beats a special-case flag.
- No `shouldResetAgent()` extraction — it would be `prev !== next`; YAGNI.

This alone also fixes leaks 2 and 3: the next `sendToAgent` in the new project
re-arms the recorder with a fresh closure over the new `sketchDir`
(App.tsx:1065-1072) and re-runs `agent_start` because the cleared store is
`"idle"`.

### 3. `src/App.tsx` JSX — remount AgentPanel per project

Add `key={sketchDir ?? ""}` to `<AgentPanel>` (~App.tsx:1468). Full remount
resets all panel-local state (`viewTurn`, `histList`, `histStore`,
`histTotals`, draft) in one attribute — impossible to forget a state added
later, and it matches the hard-boundary semantics. **No AgentPanel.tsx
edits.** Cost: the one-shot CLI probe re-fires per switch (one cheap IPC) and
an in-progress draft is dropped (it addressed the old project). Fixes leak 4
including "History open during switch".

## Edge cases (traced)

- **Mid-turn switch**: `closed("project switched")` lands in A's file; child
  EOF → `agent://closed{pid A}` → `agentStore.closed()` dropped by the
  superseded set; App listener's `status === "ended"` branch (App.tsx:358)
  stays false so no foreign closed op; `api.agentStop(pid)` re-fire is a
  backend no-op.
- **Rapid A→B→C**: second teardown fully idempotent; the Set absorbs both late
  EOFs.
- **NewProject while agent running**: `onCreated` → `loadSketch` → same path;
  fresh dir has no chat dir → `project_totals` yields zeros.
- **Send in B faster than the un-awaited `agentStop` round-trip**: backend
  refuses `agent_start` ("already running", lib.rs:2671-2677) → user gets a
  notify and retries. Accepted; not worth blocking `loadSketch` on IPC.

## Tests (TDD order)

1. `src/agent/__tests__/agentStore.test.ts` — write first, watch fail:
   - `sessionStarted(41); clear(); closed("eof", 41)` → status stays `"idle"`,
     no `closedReason`, version unchanged (matches existing not-ours
     convention: `closed()` returns before `ver++`).
   - After `clear()`, `closed("eof", 99)` (never-superseded pid) still ends
     the store — the `undefined`-means-ours contract for genuinely unknown
     sessions is preserved.
   - Two sessions superseded across two clears → both old closes ignored, a
     third live session's close still lands.
   - Superseded-pid `security_alarm`/`verify_started` after `clear()` → no
     alarm, no verify flag, no version bump.
2. `src/agent/__tests__/chatLog.test.ts` — round-trip: record
   `{op:"closed", reason:"project switched"}`, `replayChat` →
   `closedReason === "project switched"`, `status === "ended"`.
3. App.tsx wiring (helper, guard, `key`) — untested by repo convention.

## Verification

- `npm test` (401 currently) and `cargo test` (382; regression only, no Rust
  edits) from the repo root.
- Manual bench:
  1. Chat in project A until Σ is nonzero → New Project → B. Expect: Σ chip
     gone, empty transcript, History says "No saved chats", no ProjectCard.
  2. Reopen A: chats and totals intact; the switched-away chat replays ending
     "Session ended — project switched".
  3. Start a long turn in A, switch to B mid-turn: no "Session ended" banner in
     B, Send enabled; sending in B starts a fresh session; the new chat file
     lands under B's chat dir, not A's.
  4. Reopen the same dir A mid-session: session and Σ survive.
  5. Restart the app (startup restore): loads clean, no-op teardown is silent.

Per repo convention, copy this into `docs/superpowers/plans/2026-08-06-per-project-usage-scoping.md` (plus a short spec note) at implementation time, and commit docs with the change.
