# Assistant chat history

**Date:** 2026-08-05
**Status:** Approved design

## Problem

Assistant transcripts live only in the in-memory `AgentStore`; "New
session" and app close destroy them. The user wants every chat persisted
automatically and browsable, per sketch, from the Assistant panel.

## Design

### Format: a store-operation log

One NDJSON file per chat. Each line is one mutating `AgentStore` call:

    {"op":"meta","sketchDir":"/home/…/soil","startedAt":"2026-08-05T14:02:31"}
    {"op":"sessionStarted","pid":901395}
    {"op":"userSent","text":"fix the wifi reconnect loop"}
    {"op":"push","ev":{…verbatim agent://event…}}
    {"op":"closed","reason":"…","pid":901395}

Replaying the lines through a fresh `AgentStore` reproduces the exact
live rendering — no second message schema to version. Raw events alone
would not be enough: user bubbles enter via `userSent`, a local call,
never as an `agent://event`.

### Storage

`<app_config_dir>/chats/<sketch-key>/<timestamp>.ndjson`, where
`sketch-key` is `<fnv1a-hex(path)>-<sanitized basename>` (readable and
collision-safe without new dependencies) and `<timestamp>` is
`YYYY-MM-DDTHH-MM-SS` supplied by the frontend at session start. The
file is created lazily on the first `userSent` of a session — empty
sessions never write. Appends are per-op, fire-and-forget. Creating a
new chat file prunes the sketch's directory to the newest 50 files.

### Backend: `core/src/chatlog.rs` + thin commands

Pure module in the repo's convention (explicit paths in, no clock —
filenames come from the caller; unit-tested like `settings.rs`):

- `sketch_key(sketch_dir) -> String`
- `append_line(chats_root, sketch_key, file, line) -> Result<()>` —
  creates parents, appends `line` + newline. Rejects a `file` that is
  not a plain `*.ndjson` basename (no separators, no `..`).
- `list_chats(chats_root, sketch_key) -> Vec<ChatEntry>` — newest
  first; `ChatEntry { file, title }` where title is the first
  `userSent` line's text (truncated to 80 chars), else the filename.
  Unreadable/corrupt files are skipped, never fatal.
- `load_chat(chats_root, sketch_key, file) -> Vec<String>`
- `delete_chat(chats_root, sketch_key, file) -> Result<()>`
- `prune(chats_root, sketch_key, keep) -> ()` — best-effort.

Tauri commands `chat_append` (calls prune when it creates the file),
`chat_list`, `chat_load`, `chat_delete` wrap these with
`app_config_dir()/chats` as the root, mirroring `settings` plumbing.

### Frontend capture: `src/agent/chatLog.ts`

A small recorder owning the current chat's filename:
`startChat(sketchDir, now: Date)` computes the filename;
`record(op)` serializes one op line and forwards it via
`api.chatAppend` **fire-and-forget** — a failed append must never break
a live chat (the `fleetSync` philosophy). `App.tsx` calls it at the
four existing mutation sites: `sessionStarted`, `userSent` (which also
triggers `startChat` on the first send of a session), the
`onAgentEvent` push, and `closed` / `newAgentSession` (which ends the
recording). Also exported: `replayChat(lines) -> AgentStore` — parses
each line, applies the op, skips corrupt lines.

### History UI (AgentPanel)

A `🕘 History` button in the footer loads `chat_list` and shows an
in-panel list (title + file date, newest first, trash button per row
calling `chat_delete`). Clicking a chat loads its lines, replays them
into a separate read-only `AgentStore`, and renders with the existing
`MessageView` components; the input row is replaced by a
`← Back to current chat` bar. The live store, session, and unseen-dot
plumbing are untouched while browsing. No search, no resume, no export.

## Testing

- `chatlog.rs`: append creates dirs; list orders newest-first and
  derives titles; corrupt/foreign files skipped; filename validation
  rejects traversal; prune keeps N; delete removes exactly one.
- `chatLog.ts` (vitest): recorded ops replay to the same snapshot as
  the equivalent direct store calls; corrupt lines skipped; filename
  format from a fixed Date.
- Panel/JSX: visual, per repo convention.
