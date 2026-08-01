# Agent Panel — manual verification checklist

The Assistant panel (spec: `docs/superpowers/specs/2026-08-01-agent-panel-design.md`,
plan: `docs/superpowers/plans/2026-08-01-agent-panel.md`) spawns a real GUI —
`cargo test` and `npx vitest` cannot drive it. This is the human-run
checklist from the plan's Verification section's "Manual" bullet, expanded
into concrete steps. Run it in the real app (`npm run tauri dev` or a built
binary) before a release that touches the Assistant panel.

Prerequisites: a logged-in `claude` CLI on PATH, a sketch project open in
Bancada, and the `arduino:avr` (or another installed) core so Verify has
something to compile against.

## 1. Happy path: plant a bug, ask the agent to fix it, watch it verify

1. Open a sketch that currently compiles cleanly.
2. Edit a line to remove a trailing `;` (a `missing ;` compile error) and
   save.
3. Open the Assistant panel (bottom-panel "🤖 Assistant" group).
4. Send: **"make this build"**.
5. **Expect, in order:**
   - The message appears in the transcript as a user bubble.
   - Assistant text streams in (not a single dumped block — deltas arrive
     incrementally).
   - An **Edit tool card** appears showing a **diff** of the fix: unified
     diff, `-`/`+` lines, red/green backgrounds, mono font, full
     `old_string`/`new_string` (no elision).
   - The edit **auto-applies** — no approval prompt — and the file on disk
     changes immediately (check via an external `cat`/editor, or that an
     open, non-dirty buffer for that file refreshes in Bancada itself).
   - A **verify tool card** appears (status while running, then pass/fail +
     exit code once done).
   - The **Console** (bottom-panel "Build" tab) streams the same
     `arduino-cli compile` output live — open it during the run and confirm
     lines are appearing, not just present after the fact.
   - The verify card ends **green** (success) once the missing `;` is
     fixed; if the agent's first attempt is wrong, confirm it iterates
     (edits again, re-verifies) rather than stopping on a red result.
   - The panel footer shows **cost + turn count** for the completed turn.

## 2. Stop mid-turn

1. Send a prompt likely to take a few tool calls (e.g. "explain this
   project's structure and then run verify").
2. While the agent is mid-turn (before the final result line), click
   **Stop**.
3. **Expect:** the turn ends promptly (kill-based interrupt is the
   documented-reliable path per the spec's Risk R1, not the best-effort
   `control_request` line), the input box becomes usable again, and no
   stray tool card is left permanently "running".

## 3. External kill → closed banner

1. Start a session (send at least one message so a `claude` child exists).
2. From a terminal, find the child's pid (`pgrep -fl claude` or similar —
   the panel's "New session"/stop UI does not need to be touched) and
   `kill` it directly.
3. **Expect:** the panel shows a "Session ended" / closed banner (not a
   silent hang), and starting a new session afterward works cleanly (no
   leaked MCP listener port or zombie process — check `ps` for a lingering
   `claude` process after this step).

## 4. Project without git — no-undo warning

1. Open (or create) a sketch project **not** under git version control.
2. Open the Assistant panel.
3. **Expect:** a visible hint that edits auto-apply with no undo path
   without git (spec decision 4 / Risk R4) — confirm it is legible and
   present before the first message is sent, not only after an edit lands.

## 5. `claude` off PATH → install/login guidance

1. Temporarily rename or shadow the `claude` binary so it is not resolvable
   on PATH (e.g. `PATH=/usr/bin:/bin bancada` from a shell where `claude`
   normally lives in `~/.local/bin`, or rename the binary and restore it
   afterward).
2. Launch Bancada (or open the Assistant panel if the probe is lazy) and
   observe `agent_probe`'s result.
3. **Expect:** the same empty-state UX as the arduino-cli-missing banner
   (per the plan's `AgentPanel.tsx` note) — clear install/login guidance,
   not a raw error or a silently disabled panel.
4. Restore `claude` on PATH and confirm the panel recovers (probe succeeds,
   normal flow resumes) without restarting the app, if the probe re-runs on
   panel focus/open.

## 6. Dirty-conflict guard (bonus — exercises a Task 7 fix-round item)

1. Open a file the agent is likely to touch (e.g. the file with the planted
   bug from §1) and make an unsaved edit in Bancada's own editor — do not
   save.
2. Ask the agent to edit that same file.
3. **Expect:** the conflict is detected (the agent's on-disk write must not
   be silently clobbered by the next `saveAll()`), the affected buffer is
   marked conflicted with a visible `notify(...)` message, and the next
   `agentSend` is refused until the conflict is resolved (per the plan's
   App.tsx wiring note and Task 7's fix-round item on this exact race).

## Notes for whoever runs this

- Record pass/fail per section plus any screenshots/transcript excerpts in
  the task report or PR description — this checklist has no automated
  record-keeping.
- If any step fails, treat it as a real finding (file it, do not silently
  work around it) rather than adjusting the checklist to match the bug.
