# Assistant panel: live activity, per-turn usage, turn summaries, markdown

**Date:** 2026-08-05
**Status:** Approved design

## Problem

The Assistant panel gives almost no feedback while the agent works — the
only "running" signal is a subtle status on a tool card — and no
token/usage visibility beyond a tiny post-hoc `$cost · N turns` footer
line. Assistant replies render as plain text with bare code fences.

## Design

### Usage capture

The backend forwards CLI stream-json events verbatim, so `result` events
already carry `usage` (`input_tokens`, `output_tokens`,
`cache_read_input_tokens`, `cache_creation_input_tokens`),
`total_cost_usd`, `num_turns`, `duration_ms`, and the final summary text
in `result`. A new pure module `src/agent/usage.ts` parses that
defensively into a `TurnUsage` (null on missing/malformed usage — never
throws) and accumulates a `SessionUsage` (cost + tokens).

On each `result`, the store pushes a `turn_end` transcript message:
`{ kind: "turn_end", usage?, summary?, tools: {name, status}[] }` where
`tools` are the tool messages since the last user message, with their
statuses at that moment. The store also keeps the running session total.

### Live activity strip

While the session runs, the footer status shows what is happening now,
via a tested pure function (`src/agent/activity.ts`) over the snapshot:

1. `🔨 verify (compiling)…` when verifyRunning;
2. else the newest running tool with a human hint —
   `⚙ Edit soil.ino…` (basename of `file_path` for Edit/Write/Read,
   the pattern for Grep/Glob, bare tool name otherwise);
3. else `✍ writing…` while text deltas are streaming
   (store tracks a `streaming` flag: set on delta, cleared on
   result/tool_use/userSent/closed);
4. else `thinking…`.

Returns null when not running; the panel then falls back to today's
status labels. The footer also gains a session chip: `Σ $0.043 · 31k`.

### Per-turn divider and Turn Summary view

Each `turn_end` renders as a thin divider in the transcript:
`3 turns · 12.4k in / 1.2k out · $0.021 · details ▸`. Clicking it opens
an in-panel turn summary view (transcript hidden, `← back to chat` bar):

- the turn's final summary (`result` text) rendered as markdown,
- a usage block (tokens in/out/cache-read, cost, turns, duration),
- the tools the turn ran, with ✓/✗ status.

### Markdown rendering

Assistant bubbles and the turn summary render markdown via
`react-markdown` + `remark-gfm` (new dependencies; raw HTML stays
escaped — this is model output). The hand-rolled `splitFences` rendering
is removed from the assistant bubble; the `fences` module and its tests
are deleted if nothing else imports them. New `agent-markdown` CSS
matches the existing dark theme.

## Unchanged

Alarm banner semantics, conflict guards, session lifecycle, unseen-dot
plumbing, and the whole Rust backend (events already arrive verbatim).

## Testing

vitest, test-first: `usage.ts` (parse/accumulate/format),
`activity.ts` (priority order, hints, null when idle), and store tests
for `turn_end` emission + streaming-flag transitions. Panel JSX and CSS
verified visually per repo convention.
