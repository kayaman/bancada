# Per-project usage scoping — design

**Date:** 2026-08-06
**Request:** "scope tokens and cost by project. A new project must have zero
when just created."

## Problem

The storage layer is already per-project (`core/src/chatlog.rs` keys chat dirs
by `sketch_key()`, an fnv1a-64 of the full sketch path; `project_totals()`
reads only that dir). The frontend is not: `loadSketch()` — the sole
open/switch/create path — resets only editor state and never touches the
module-level singletons `agentStore` and `chatRecorder`. Four leaks follow:

1. The footer Σ chip reads `agentStore.sessionUsage`, zeroed only by the
   manual "New session" button — a new project displays the previous
   project's cost/tokens.
2. The ChatRecorder's append closure is frozen over the `sketchDir` captured
   at `start()` — after a switch, ops (including the `result` usage the
   totals sum) keep landing in the old project's chat dir.
3. With the store still `"running"`, `sendToAgent` skips `agent_start` — the
   new project's prompts feed the old project's `claude` child, whose cwd is
   the old sketch.
4. The History view fetches lists/totals only on 🕘 click and never on
   `sketchDir` change — new sketch's name over old sketch's totals, and
   delete/open would mix the new dir with the old filenames.

## Design

**A `sketchDir` change is a hard agent-session boundary.** Switching to or
creating a project runs the same teardown as "New session" (stop child, close
recording with an honest `closed("project switched")` op, clear store), and
the AgentPanel remounts via a `key={sketchDir}` prop so all panel-local state
resets. A new project then shows zero by construction: empty store, no chat
dir on disk.

Supporting hardening: `AgentStore.clear()` records the cleared session's pid
in a superseded set consulted by the pid guard, so the killed child's late
`agent://closed` (or verify/alarm events) cannot flip the fresh store to
"ended" during the window where no new session pid is known. The
"undefined pid means assume ours" contract is preserved for genuinely
unknown sessions.

Same-dir reopen is not a boundary (exact string compare — the same rule
`sketch_key` uses). Teardown is idempotent, so the startup-restore path needs
no special case. No backend changes.

## Retention note

Unchanged from the chat-history spec: chat files persist tool results
verbatim under the app config dir; revisit with `src/obs/redact` if chats
ever leave the machine.
