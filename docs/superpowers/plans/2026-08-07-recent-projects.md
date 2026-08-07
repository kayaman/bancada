# Keep a record of recent projects

## Context

Bancada remembers only `last_sketch_dir`. The user wants a recents list:
record recently opened projects, reopen them quickly. Decisions
(AskUserQuestion): surface as a **toolbar ▾ dropdown** next to 📁, plus
**Ctrl+O** for the open-folder picker (first real consumer of the
fully-tested-but-unused `src/keys.ts`).

Exploration found a load-bearing pre-existing bug that shapes the design:
the `save_settings` Tauri command is a **full overwrite**, and all six
frontend call sites send partial payloads — each silently nulls the fields
it omits (opening a file wipes `last_new_project_parent`; creating a project
wipes the sketch memory, masked only by the immediate `loadSketch`
re-write). A `recent_projects` field routed through `saveSettings` would be
erased constantly. So this feature **replaces the whole overwrite class
with narrow mutate commands** and deletes `save_settings` — fixing the
existing clobber bugs as a side effect. Design validated by a Plan agent
against the actual code (consumer census complete; `update_settings(patch)`
rejected: needs `Option<Option>` double-option serde boilerplate AND still
can't express push semantics, which must live in Rust anyway).

## Backend (TDD)

### `core/src/settings.rs`
- Add `#[serde(default)] pub recent_projects: Vec<String>` (doc: most-recent-
  first, deduped, capped) and `pub const MAX_RECENT: usize = 10;`.
- Pure mutator methods on `AppSettings`:
  `set_last_sketch(dir, open_file)`, `set_last_project_parent(dir)`,
  `push_recent(dir)` (retain != dir, insert(0), truncate MAX_RECENT),
  `remove_recent(&dir)` (retain != dir).
- Tests (plain `#[test]`, mutators are pure — no tempfile):
  `roundtrip` extended with the new field (its break-on-field-add is the
  convention working); `push_recent_dedupes_to_front` (a,b,a → [a,b]);
  `push_recent_caps_at_ten`; `remove_recent_present_and_absent`;
  `mutators_preserve_other_fields` (pins the killed bug class);
  `json_without_recent_projects_loads` (literal old-format JSON string —
  pins the `#[serde(default)]` migration guarantee).

### `src-tauri/src/lib.rs`
- Private helper `fn update_settings(app, f: impl FnOnce(&mut AppSettings))`
  → load, mutate, atomic save. Keep the four commands **non-async** (they
  then run on the main thread and serialize for free — no mutex needed).
- Four `#[tauri::command]`s wrapping the mutators: `set_last_sketch(dir,
  open_file: Option<String>)`, `set_last_project_parent(dir)`,
  `push_recent_project(dir)`, `remove_recent_project(dir)`.
- **Delete `save_settings`** (lib.rs:1365-1371) and its
  `generate_handler!` entry; register the four new ones. `load_settings`
  stays.

## Frontend

### `src/api.ts`
`AppSettings` gains `recent_projects?: string[]`. Replace `saveSettings`
with four wrappers: `setLastSketch(dir, openFile: string | null)` (Tauri
maps `openFile` → `open_file`), `setLastProjectParent(dir)`,
`pushRecentProject(dir)`, `removeRecentProject(dir)`.

### Call-site swaps (1:1, all keep `.catch(() => {})`)
- App.tsx:500 → `setLastSketch(dir, null)`; :522 → `setLastSketch(dir,
  relPath)`; :605 → `setLastSketch(sketchDir, np)`; :647 →
  `setLastSketch(sketchDir, null)`.
- NewProject.tsx:108 and CloneProject.tsx:80 → `setLastProjectParent(parent)`.

### Recording (inside `loadSketch`, covers every entry point uniformly)
- Success tail (~App.tsx:503, before `return true`):
  `api.pushRecentProject(dir).catch(() => {})`.
- Catch block (before `return false`):
  `api.removeRecentProject(dir).catch(() => {})` — failed opens prune the
  dead entry; it returns on the next successful open. (Clearing a stale
  `last_sketch_dir` on startup restore: deliberately out of scope — needs a
  fifth command for a state that self-heals.)

### `src/components/Menu.tsx` (new) — generic popover shell
Extracted verbatim from TreeContextMenu's clamp + dismissal (verified: no
store reads inside those effects; `.ctx-menu` is `position:fixed; z-index
100` and the toolbar creates no stacking context — anchoring under a
toolbar button needs zero CSS changes). Props `{ x, y, onClose, anchorRef?,
children }`; `useLayoutEffect` viewport clamp (deps [x, y]); the three
dismiss listeners (mousedown-outside via `ref.contains`, Escape, window
blur) with `anchorRef` also excluded from "outside" — without that, a
second click on the anchor closes-then-reopens and the button appears
un-toggleable. Comment: pass a stable `onClose`.

### `src/components/TreeContextMenu.tsx` — shell adoption, no behavior change
Delete its local ref/pos/two layout-effects; keep store reads, `item()`
helper, all JSX; outer div becomes `<Menu x={menu.x} y={menu.y}
onClose={close}>`.

### `src/components/RecentsMenu.tsx` (new) — self-contained
Fetches on open (freshness for free; precedent: Toolbar's own `getVersion`
call, NewProject/CloneProject's `loadSettings`). `▾` button (`.btn icon`,
`title` + `aria-label` "Recent projects"); toggle: if open → close, else
`loadSettings().catch(...)` → `setRecents(s.recent_projects ?? [])`, anchor
at `btnRef.getBoundingClientRect()` → `{x: left, y: bottom + 4}`. Items:
`.ctx-item` buttons — basename as label, full path as `title`, click →
close + `onOpen(dir)`; disabled "No recent projects" item when empty.
Props: `{ onOpen: (dir: string) => void }`.

### `src/components/Toolbar.tsx` + `src/App.tsx` wiring
Toolbar: `onOpenRecent` prop; `<RecentsMenu onOpen={props.onOpenRecent} />`
right after the Open Sketch button in the project group. App:
`onOpenRecent={(dir) => void loadSketch(dir)}` — pruning already lives
inside loadSketch.

### Keyboard: Ctrl+O (+ Ctrl+S folded in) via `src/keys.ts`
Module-scope `const ACCEL_SAVE = parseAccel("Ctrl+S")!;` /
`ACCEL_OPEN = parseAccel("Ctrl+O")!;`. Replace the App.tsx:740-750 listener
body with `matchesAccel` checks: save → `saveCurrent()`, open →
`openSketch()`, both `preventDefault()`. **No `isTypingTarget` guard** —
keys.ts's own doc scopes the typing guard to unmodified keys; Ctrl+O must
preventDefault inside inputs anyway or the webview pops its own dialog.
(Delta from folding Ctrl+S in: Ctrl+Alt+S no longer triggers save —
exact-modifier matching is a correctness improvement.)

## Tests
- Core: the six settings tests above.
- `src/__tests__/api.test.ts`: delete the old `saveSettings` test
  (:491-495 — it needed an `as never` cast to send a partial, evidence for
  the refactor); add contract tests: `setLastSketch` with `("/s","a.ino")`
  and `("/s", null)` → `["set_last_sketch", { dir, openFile }]`;
  `setLastProjectParent`; `pushRecentProject` + `removeRecentProject`.
- JSX (Menu/RecentsMenu/Toolbar/App wiring) untested by repo convention;
  keys.ts already has its own suite.

## Files
Create: `src/components/Menu.tsx`, `src/components/RecentsMenu.tsx`.
Modify: `core/src/settings.rs`, `src-tauri/src/lib.rs`, `src/api.ts`,
`src/App.tsx`, `src/components/{Toolbar,TreeContextMenu,NewProject,CloneProject}.tsx`,
`src/__tests__/api.test.ts`. No new dependencies, no new CSS.

## Verification
- `cargo test --workspace`, `npx tsc --noEmit`, `npm test`.
- Bench: open a few projects → ▾ lists them newest-first, basenames with
  full-path tooltips; click one → it opens and moves to the top; delete a
  listed project's folder → clicking it errors once and it disappears from
  the list; Ctrl+O pops the folder picker (also from inside the editor);
  Ctrl+S still saves; file-tree right-click menu unchanged (shell
  adoption); create/clone a project → parent memory AND recents both
  survive (the clobber bug is dead: open a file, then check
  `settings.json` still has `last_new_project_parent`).
- Docs: spec + plan under `docs/superpowers/{specs,plans}/2026-08-07-recent-projects-*`, committed with the change.
