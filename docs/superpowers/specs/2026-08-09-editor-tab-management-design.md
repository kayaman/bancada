# Editor tabs: open/close instead of every-file

**Date:** 2026-08-09
**Status:** Approved design

## Problem

`fileTabs()` gives **every** project file a tab ("sketches are small").
The assumption died on 2026-08-09 when a project gained a `web/` Python
venv: the strip rendered two dozen `activate`/`.pyc`/`_virtualenv` tabs.
"Close" cannot exist in that model — a closed tab would reappear on the
next render.

## Design

### Model

`openTabs: string[]` (ordered rel_paths) in App, alongside the existing
`openFile` as the active tab. Project load opens the main `.ino` only;
clicking a file in the tree (and the conflict-jump path) opens a tab.
`fileTabs()` is deleted. Renames/deletes run the same fixup the tree
uses; deleted files lose their tabs.

Pure helpers in `editorTabs.ts`, unit-tested: `openTab`, `closeTab`
(returns `{tabs, nextActive}` — right neighbor, else left, else null),
`closeOthers`/`closeAll` (both skip dirty tabs), `renameTabs`,
`deleteTabs` (dir-prefix aware). `tabLabel` unchanged.

### Close affordances

✕ per tab and middle-click anywhere on the tab. Right-click opens a
small popover (RecentsMenu pattern): "Close others", "Close all".
Closing the active tab activates its right neighbor, else left, else the
editor goes honestly empty — no placeholder buffer, strip height stable.

### Unsaved guard

No modal dialogs. Closing a dirty tab arms it: tab restyles, title
becomes "unsaved — close again to discard", a status-line notice fires;
a second close while armed discards. Any other action disarms. Bulk
closes never touch dirty tabs and report how many they kept.

### Component contract

`EditorTabs` props: `tabs`, `active`, `dirty: ReadonlySet<string>`,
`armed: string | null`, `onSelect`, `onClose`, `onCloseOthers`,
`onCloseAll`. All arming/dirty state lives in App; the strip is
presentation only.

## Testing

Helpers fully unit-tested (neighbor selection, dirty-skip, dir-prefix
fixups); form of the tests follows `ports.test.ts`. Suites stay green.

## Execution note

Implemented as a parallel fan-out: helpers and component in isolated
worktrees against the fixed contract above; App wiring integrates last
(sequenced behind the board-switching fix wave, which shares App.tsx).
