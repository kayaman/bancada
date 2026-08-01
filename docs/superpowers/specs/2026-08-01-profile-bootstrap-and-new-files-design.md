# Profile bootstrap and new-file creation

*2026-08-01 — approved design*

Two gaps when working on an existing sketch:

1. A project opened without `sketch.yaml` is stuck: the profile dropdown shows
   dead text, and nothing in the interface can create the file. Profile builds
   — the hermetic kind Bancada prefers — are unavailable until the user writes
   yaml by hand.
2. There is no way to create a file inside the open project. The editor can
   only open what already exists.

## 1. Creating sketch.yaml — "Create profile…" in the toolbar

**Entry point.** When the loaded sketch has zero profiles, the toolbar slot
that renders the profile `<select>` renders a `＋ Create profile…` button
instead. This keys off *no profiles*, not *no file*, so a yaml that exists but
is empty gets the same affordance. Non-blocking: opening a yaml-less sketch
never interrupts.

**Form.** Clicking the button opens a compact panel anchored under the
toolbar:

- **Board** — a picker fed by the existing `listAllBoards` (the same source
  New Project uses), pre-selected with the FQBN detected on the selected port
  (`detectedFqbn`) when there is one.
- **Profile name** — auto-derived from the FQBN's board segment, mirroring
  core's `profile_name_for_fqbn`; editable.
- **Create / Cancel.**

**Backend.** New `Sketch::init_profile(name, fqbn)` in `core/src/sketch.rs`:

- Creates `sketch.yaml` when absent.
- Adds the profile with the given FQBN.
- Sets `default_profile` when none is set.
- Errors when a profile with that name already exists — never overwrites.
- Returns the resulting `SketchYaml`.

Exposed as Tauri command `init_profile(sketch_dir, profile, fqbn)`, wrapped in
`api.ts`. On success the frontend stores the returned yaml (`setSketchYaml`)
and selects the new profile (`setProfile`), so the dropdown, Verify and Flash
gain it immediately.

Yaml is written by core's `save_yaml` — one source of truth for the format.
Not `arduino-cli profile init` (subprocess + version variance for a pure file
edit), and not TS-side yaml serialization (would duplicate the format).

## 2. Creating files — ＋ in the file tree and the tab strip

`write_sketch_file` already creates missing files and parent directories,
traversal-safe (`safe_join`). No backend change.

**Affordances.** A small `＋` button in two places: the Files panel header and
the end of the editor tab strip — the strip is always visible, so file
creation stays reachable when the sidebar shows another panel (the same
reachability rule that motivated the tab strip itself).

**Flow.** `＋` turns into an inline name input in place. Enter → create the
file empty, refresh the file list, open it in the editor. Escape or blur
cancels. Subpaths like `data/config.h` are allowed; parents are created by the
backend.

**Validation** — pure module `src/newFile.ts` (vitest-covered, the
`editorTabs.ts` pattern):

- Non-empty after trimming.
- No absolute paths, no `..` components (backend enforces too; the frontend
  check gives an immediate, friendly message).
- Refuses a `rel_path` that already exists in the current file list —
  `write_sketch_file` would silently overwrite it.

Errors surface through the existing `notify()` toast.

## Testing

- Rust: `init_profile` creates yaml when absent, sets `default_profile` only
  when unset, rejects a duplicate profile name, preserves existing profiles
  and unrelated yaml fields.
- Vitest: `newFile.ts` validation cases; any touched tab/tree logic.

## Out of scope

Deleting or renaming files, pinning a port into the new profile,
multi-profile management UI, directory creation as its own action (folders
appear implicitly via subpaths).
