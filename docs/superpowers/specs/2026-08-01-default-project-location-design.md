# Default Project Location Design

**Date:** 2026-08-01
**Status:** Approved (default rule chosen: remembered location wins)

## Problem

The New Project dialog pre-fills its Location field with
`last_new_project_parent || sketchbookDir()`. The sketchbook is an
Arduino-IDE convention (`~/Arduino` by default) — not where most
developers keep code. And `create_project` rejects a parent directory
that does not exist, so pointing it somewhere new fails instead of
just working.

## Decision

Replace the sketchbook fallback with a developer convention:

1. **Default parent** = `$HOME/Projects` when that directory exists,
   otherwise `$HOME`. The remembered `last_new_project_parent` setting
   still wins when present (user-chosen behavior, kept).
2. **Parent creation**: `create_project` creates the parent with
   `create_dir_all` when it is missing, instead of erroring. A path
   that exists but is not a directory is still an error.

## Components

- `core/src/project.rs` — `default_project_parent(home: &Path) -> PathBuf`,
  pure and tempfile-tested: `home/Projects` if `is_dir()`, else `home`.
  (`is_dir` follows symlinks, so a symlinked `~/Projects` counts; a plain
  file named `Projects` does not.)
- `src-tauri/src/lib.rs` — `default_project_parent` Tauri command using
  `app.path().home_dir()`; `create_project` swaps its `is_dir` guard for
  create-if-missing.
- `src/api.ts` — `defaultProjectParent()` wrapper.
- `src/components/NewProject.tsx` — fallback swaps `sketchbookDir()` for
  `defaultProjectParent()`. The `sketchbook_dir` command and API wrapper
  remain (other callers may want them); only this usage changes.

## Error handling

- Home directory unresolvable → command returns the resolver error string.
- Parent uncreatable (permissions) → `create_project` returns
  `could not create <parent>: <io error>`.

## Testing

- Core: 3 tests — Projects exists → Projects; Projects missing → home;
  Projects is a file → home.
- Frontend: no new pure-logic module (the chain is a single `||`); the
  existing vitest suites must stay green.
