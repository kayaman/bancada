# File Explorer Design

**Date:** 2026-08-01
**Status:** Approved

## Problem

Bancada's file panel is a flat list: directories are inert text, the only
mutation is a single new-file input, and the backend exposes no rename,
delete, mkdir, or move operation. A serious IDE needs full file and folder
management from the explorer.

## Scope

Explorer only. Project management (recent projects, switcher, close
project) and a real opened-files tab model are explicitly deferred to
later effort. The existing tab strip stays as-is.

## Decisions

- **Real tree.** Collapsible directories, dirs-first case-insensitive
  sort, expansion state survives refresh/rename and follows the open file.
- **Operations.** Create file, create folder (empty folders become
  first-class — this reverses the earlier "folders appear when a file
  needs them" policy from the 2026-08-01 profile-bootstrap spec), inline
  rename, delete, move.
- **Delete → OS trash** via the `trash` crate in core. Confirmation
  dialog (native `ask()`) only for non-empty folders; everything else is
  recoverable from the trash without friction.
- **Move both ways.** Drag-and-drop onto folders *and* rename-as-move
  (the inline rename edits the full rel path). DnD resolves to the same
  rename code path, so guards exist once.
- **Protected files.** The main `<sketch>.ino` and top-level
  `sketch.yaml` cannot be renamed, moved, or deleted: greyed out with an
  explanatory tooltip in the UI, hard-rejected in core.
- **State.** Zustand is introduced, scoped to explorer state only
  (`files`, expansion, selection, rename/create/drag/context-menu state).
  Buffers, dirty set, and the open file stay in `App.tsx`; the store never
  initiates fs operations.

## Architecture

Backend follows repo policy — safety logic in `core`, thin Tauri
commands:

- `core/src/files.rs` (new): `validate_rel_path`, and on
  `SketchProject`: `is_protected`, `abs_path`, `create_file`,
  `create_dir`, `rename_entry`, `delete_entry`. `rename_entry` guards:
  missing source, protected source, target collision (case-only rename
  exempt via dev+ino on unix), dir-into-own-descendant, from == to.
- `src-tauri`: `create_sketch_file`, `create_sketch_dir`,
  `rename_sketch_entry`, `delete_sketch_entry` — each mutates then
  returns the refreshed `Vec<SketchFile>` (the repo's mutate-then-return
  pattern), keeping the tree disk-coherent in one round trip.
- Window config gains `"dragDropEnabled": false` so wry's native
  drag-drop handler doesn't swallow HTML5 drag events.

Frontend puts every decision in pure, vitest-covered `.ts` modules:

- `src/fileTreeModel.ts`: `buildTree`, `visibleNodes`, `ancestorsOf`,
  `pruneExpanded`, `remapSet`.
- `src/explorerOps.ts`: `pathAfterRename`, `affectedByDelete`,
  `protectedPaths`, `isNonEmptyDir`, `checkRename`, `isDescendant`.
- `src/explorerStore.ts`: the Zustand store.
- `src/newFile.ts` generalizes to `checkNewEntry` (folder-friendly);
  `checkNewFile` remains as a wrapper.

`App.tsx` keeps orchestration: `handleRename` / `handleDelete` /
`handleCreateFile` / `handleCreateDir` call the api, feed the returned
file list to the store, and remap `buffersRef` / `dirtyFiles` /
`openFile` so renaming an open dirty file keeps its content, and deleting
the open file falls back to the main `.ino`.

UI: `FileTree.tsx` rewritten as a flat map over `visibleNodes` (depth
padding, chevrons, DnD handlers, inline rename/create rows);
`TreeContextMenu.tsx` is a hand-rolled fixed-position menu (no component
library), viewport-clamped, closed on outside click / Escape / blur.

## Error handling

Core returns `Err` strings surfaced through the existing `err_str` →
`notify(..., true)` toast path. Frontend pre-validates (friendlier
messages, no round trip) but core is the authority.

## Testing

- Core: inline `#[cfg(test)]` with `tempfile::tempdir()` per repo
  convention. Tests that physically trash files are
  `#[ignore = "needs a trash-capable mount"]` (XDG trash can't span
  mounts; `/tmp` often fails) and run via the existing `--include-ignored`
  coverage path. All guard tests run everywhere.
- Frontend: pure-module tests under `src/__tests__/` (node env; `.tsx`
  stays untested per convention, which is why no logic lives in
  components). `api.test.ts` pins command names and camelCase arg keys.
- Manual: DnD inside the webview cannot be auto-tested; verified in
  `npm run tauri dev`.

## Known limitations

- `isNonEmptyDir` cannot see SKIP_DIRS contents (`build/`, `.git/`…), so
  a folder containing only those deletes without confirmation. Accepted:
  trash is recoverable.
- Case-only rename detection is exact on unix (dev+ino); other platforms
  fall back to a lowercase comparison. Linux is the bundle target.
