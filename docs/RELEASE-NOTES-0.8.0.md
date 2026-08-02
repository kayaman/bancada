# bancada 0.8.0

## AI Assistant panel

A Claude agent joins the workbench: it reads and edits your sketch's files,
runs Verify, reads the compiler errors, and iterates until the build passes.

- New **Assistant** tab in the bottom panel — chat with a `claude` CLI
  session spawned per project, scoped to the open sketch.
- Edits show up as diff cards (unified diff, red/green) and **auto-apply
  immediately** — no per-edit approval step.
- A **Verify** tool card mirrors the same compile path as the Verify button;
  output also streams live to the existing Build console.
- The embedded session's built-in tools are narrowed to `Read`, `Edit`,
  `Write`, `Glob`, `Grep`, plus one MCP tool, `verify`. It cannot run shell
  commands, fetch URLs, search the web, or touch upload/serial.
- Per-turn **cost and turn count** shown in the panel footer.
- **Stop** interrupts a turn; **New session** ends the child process and
  clears the transcript.

### Prerequisites

The `claude` CLI must be installed and signed in (resolved from PATH, same
as arduino-cli and esptool). If it isn't found, the panel shows install/login
guidance instead of a raw error.

### Safety

Edits apply automatically with no approval prompt, and the panel warns when
the open project has no `.git` since there is then no undo path.

Writes outside the sketch directory, and writes to the project's `.claude/`,
`.git/`, `.mcp.json` (plus your own `~/.claude/`, shell rc files, `/etc/`,
and similar), are refused by `permissions.deny` rules in a `--settings`
policy file Bancada writes **outside the project tree** — a policy the agent
cannot edit — anchoring a `PreToolUse` guard hook that adds the
subtree-containment check a denylist alone can't express. Bancada refuses to
*start* a session if the project's own settings already have hooks disabled
(`disableAllHooks`), since that would leave the guard hook silently not
firing. An independent detect-and-stop check re-inspects every edit the
agent reports and the session's tool list, and kills the session if either
drifted from what's expected.

This is **in-process policy enforced by the `claude` CLI's own permission
engine, not an OS-level sandbox or container**. Reads are not confined at
all — the agent can read anything your account can, including SSH keys and
credential files; only writes are policed. The embedded session also still
loads your own Claude Code configuration (hooks, plugins, skills), so a
hostile hook already present there before the session starts is out of
scope for this to catch — Bancada only stops the agent from installing a
new one.

### Also in this release

- Fixed: a session ended by "New session," or superseded by starting a new
  one, could have its stale close/verify events repaint a *newer*, still-live
  session as "ended" or stuck mid-build. Events are now tagged with the
  session's process id and dropped if they don't match the panel's current
  session.
- Fixed: clicking Verify or Upload after a save conflict (the agent and you
  editing the same file) could silently overwrite the agent's on-disk fix
  with the stale editor buffer. Conflicted files now stay out of auto-save
  until resolved with Ctrl+S.
- Fixed: `verify` tool results were rendered as raw JSON ("Verify finished"
  with a garbled summary) instead of the pass/fail card — every verify card
  was affected.

### Known limitations

- No upload or serial tools yet — the agent is compile-only in this slice;
  hardware actions are planned for a later release.
- Assistant messages render as plain text plus fenced code blocks, not full
  Markdown (no headings, lists, tables, links).
- One agent session per project; switching projects or hitting **New
  session** clears the transcript.
- A pre-existing hostile hook in your own Claude Code configuration is not
  something this release can detect or stop.
