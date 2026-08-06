# Project-level library pinning — design

**Date:** 2026-08-06
**Status:** approved direction (Approach 2: pin visibility/management + bare-sketch adoption)

## Problem

Library resolution feels registry-dependent: teammates cloning a sketch get
whatever versions their sketchbook happens to hold, and registry updates change
builds silently. In reality Bancada already pins registry libraries into the
`sketch.yaml` profile at install time and compiles hermetically with
`--profile` — but the pins are invisible, unmanageable, and skipped on several
paths, so the lock cannot be seen or trusted.

`sketch.yaml` stays the single source of truth. No `bancada.toml`: the profile
file is arduino-cli's native mechanism, so a teammate without Bancada still
gets the exact pinned versions from plain `arduino-cli compile --profile`.

## Current state (verified in code)

- `core/src/sketch.rs` models `sketch.yaml` (`SketchYaml`/`Profile`/
  `LibraryDep::{Registry, Local}`); registry pins look like
  `"ArduinoJson (7.4.2)"`.
- Registry pins are written only by `arduino-cli profile lib add`
  (`Cli::profile_lib_add`) so dependencies get resolved; local `dir:` entries
  are edited by hand in `sketch.rs`. There is **no remove/edit** counterpart.
- `LibraryManager.doInstall` pins after install **only when** a sketch with a
  profile is open; otherwise the install is global and silent.
- `uninstallLibrary` never touches pins.
- The UI's "pinned" list shows only git-vendored libraries (`bancada.yaml`);
  registry pins in `sketch.yaml` are never displayed.
- Compile/upload prefer `--profile` over `--fqbn` (hermetic when a profile
  exists).

## Design

### Slice 1 — see and manage pins

**Core (`core/src/sketch.rs`):**

- `remove_library(profile, dep)` — delete a `LibraryDep` from a profile.
  Registry entries match by library name (the part before ` (`), so a rename
  of the version still removes the old pin; local entries match by resolved
  path (same normalization as `add_local_library_with`).
- `registry_pins(profile) -> Vec<(name, version)>` — parse helper for display.
- Version *changes* go through `Cli::profile_lib_add` with the new
  `Name@version` spec after removing the old pin — arduino-cli resolves
  dependencies; we never hand-write registry pin strings.

**Tauri commands (`src-tauri/src/lib.rs`):**

- `remove_library_from_profile(sketch_dir, profile, dep) -> SketchYaml`
- `change_pinned_library_version(sketch_dir, profile, name, version) ->
  SketchYaml` (remove + `profile_lib_add`, in that order; if the add fails the
  original yaml is restored from the in-memory copy and the error surfaced).

**UI (`LibraryManager.tsx` + project sidebar):**

- New **Project libraries** section (shown whenever a sketch with a profile is
  open) listing the active profile's `libraries:`:
  - registry pins as `Name @ version` with a version-change control (choices
    from the existing registry search data) and an unpin control;
  - `dir:` entries labeled *local* or *vendored* (vendored = under the
    project's vendor dir recorded in `bancada.yaml`), with unpin.
- **Installed list** gains a "Pin to project" action per library when a
  profiled sketch is open and that library is not yet pinned (calls the
  existing `add_registry_library_to_profile` with the installed version).
- **Install flow**:
  - project open, no profile → offer to create one first (existing
    `init_profile` / `profile_create` path), then install + pin;
  - no project open → result notice reads "installed globally — not pinned to
    any sketch".
- **Uninstall flow**: when the library is pinned in the open project, warn and
  offer to unpin too (default: keep the pin — profile builds auto-fetch from
  the registry, so the pin still works without the global install).

### Slice 2 — bare-sketch adoption ("Pin current setup")

For a sketch with no `sketch.yaml` profile:

- Action **Pin current setup** in the Project libraries section (shown in its
  empty state).
- Steps: create a profile from the currently selected board FQBN
  (`init_profile`), then pin each library the sketch actually uses at its
  currently installed version via `profile_lib_add`.
- Used-library detection: arduino-cli prints a `Used library / Version / Path`
  table on successful compiles; the compile streaming path captures these
  lines and core parses them (`parse_used_libraries(lines)`). The action
  therefore requires one successful build first; if none has run, the button
  triggers a compile and pins on success. Libraries resolved from the platform
  (bundled) are listed under a platform path and are skipped — they come with
  the pinned core.
- Libraries used from the sketchbook get registry pins when they exist in the
  registry index (matched by `library.properties` name); otherwise they get an
  absolute `dir:` entry (existing `PathStyle::Absolute` path), with a notice
  that they are machine-local and better vendored via the git-library flow.

## Error handling

- All yaml-writing commands return the rewritten `SketchYaml` for immediate UI
  state, matching the existing `onYamlChanged` pattern.
- `profile lib add` failures during install keep today's contract: the install
  succeeded, the pin failed — surfaced via the existing `profile_error` field
  pattern, never rolled into a fake install failure.
- Unpin of a nonexistent entry is a no-op returning current yaml (idempotent —
  double-click safe).

## Testing

- Core: unit tests beside the existing `sketch.rs` suite — remove semantics
  (name-match for registry, path-match for local), idempotent unpin,
  used-library table parsing (fixtures from real arduino-cli output, incl.
  platform-bundled and sketchbook rows).
- Frontend: vitest component tests for the Project libraries section states
  (no project / project without profile / pins present) and the
  uninstall-warns-when-pinned flow, following existing `LibraryManager`
  test patterns.
- One integration test in `core/tests/` exercising create profile → pin →
  change version → unpin against a real arduino-cli when available (same
  gating as `gh_fetch.rs` / `new_project_builds.rs`).

## Out of scope

- Drift badges / sync between global sketchbook and pins (profile builds are
  hermetic; drift does not affect them).
- A separate manifest file (`bancada.toml`).
- Pinning UI for git libraries (already exists via `bancada.yaml`).
