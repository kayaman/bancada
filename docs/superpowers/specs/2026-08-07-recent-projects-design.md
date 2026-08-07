# Recent projects — design

**Date:** 2026-08-07
**Request:** "keep record of recent projects"

## Scope (user-decided)

Record recently opened sketch dirs (most-recent-first, deduped, capped at
10) and surface them as a **toolbar ▾ dropdown** next to the 📁 button.
Plus **Ctrl+O** for the open-folder picker — the first real consumer of the
fully-tested-but-unused `src/keys.ts` (Ctrl+S folds into the same handler).

## The bug that shaped the design

`save_settings` was a full-overwrite command, and every frontend call site
sent a partial payload — each write silently nulled the fields it omitted
(opening a file wiped `last_new_project_parent`; creating a project wiped
the sketch memory, masked only by the immediate `loadSketch` re-write). A
recents list routed through it would have been erased constantly.

So the feature **deletes `save_settings` outright** and replaces the whole
class with narrow mutate commands — `set_last_sketch`,
`set_last_project_parent`, `push_recent_project`, `remove_recent_project` —
each doing load→mutate→atomic-save in Rust over pure, unit-tested
`AppSettings` mutators. Kept non-async deliberately: non-async Tauri
commands run on the main thread, so the read-modify-write sequences
serialize without a mutex. A patch-style `update_settings` was considered
and rejected (double-`Option` serde boilerplate, and push semantics must
live in Rust regardless).

## Recording and pruning

Both hooks live inside `loadSketch` — the single choke point for picker,
startup restore, New Project, Clone, and recents-click: push on the success
tail, remove on failure (a dead entry errors once when clicked, then
disappears; it returns on the next successful open). Old `settings.json`
files load fine — the new field rides `#[serde(default)]`, pinned by a
literal-JSON migration test.

## Surfacing

A generic `Menu` popover shell was extracted from `TreeContextMenu` (viewport
clamp + mousedown-outside/Escape/blur dismissal, unchanged behavior) with
one addition: an `anchorRef` exclusion so clicking the anchor toggles
instead of close-then-reopen. `RecentsMenu` is self-contained — fetches
`loadSettings()` on each open (fresh by construction), renders basenames
with full-path tooltips, and a disabled "No recent projects" item when
empty. No new CSS; `.ctx-menu`/`.ctx-item` are reused (fixed-position,
z-index 100 — anchoring under the toolbar needs nothing).

Ctrl+O carries no typing guard: keys.ts scopes `isTypingTarget` to
unmodified keys, and Ctrl+O must `preventDefault` inside inputs anyway.
