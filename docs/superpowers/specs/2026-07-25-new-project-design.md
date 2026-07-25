# Design: New Project

## Problem

Bancada can open a sketch but cannot create one. Starting a project means
leaving the app: `mkdir`, write an `.ino`, hand-write `sketch.yaml` with the
right platform and version. Since `sketch.yaml` profiles are Bancada's whole
thesis, that is the wrong first experience.

Goal: name a project, pick a board, tick the libraries it needs, get a
profile-backed sketch that compiles immediately.

## The engine already does this

Bancada's principle is to drive `arduino-cli`, not reimplement it. The full
pipeline exists and was verified end to end:

```
arduino-cli sketch new Demo
  → Demo/Demo.ino

arduino-cli profile create -m esp32s3 -b esp32:esp32:esp32s3 --set-default Demo
  → Demo/sketch.yaml, with the installed platform version resolved:
      profiles:
        esp32s3:
          fqbn: esp32:esp32:esp32s3
          platforms:
            - platform: esp32:esp32 (3.3.11)
      default_profile: esp32s3

arduino-cli profile lib add ArduinoJson@7.4.2 -m esp32s3 --sketch-path Demo
  → libraries:
      - ArduinoJson (7.4.2)
```

Two things this buys that hand-written YAML does not: the **platform version is
resolved from what is installed** rather than guessed, and `profile lib add`
**resolves dependencies** (`--no-deps` opts out).

Note the argument shape: `profile lib add` takes the library positionally and the
sketch via `--sketch-path`. Passing the sketch positionally makes it look for
`<cwd>.ino` and fail confusingly.

## Consequence: unify the existing pin path

Bancada currently hand-writes registry pins in `sketch.rs::add_registry_library`,
without dependency resolution. Adding the same library from the Libraries tab and
from New Project would therefore produce different `sketch.yaml` content.

The `add_registry_library_to_profile` command is reimplemented on top of
`profile lib add`, keeping its name and signature so `api.ts` and the UI are
untouched. `sketch.rs::add_registry_library` then has no callers and is removed
along with its test. `add_local_library` stays exactly as it is — `profile lib
add` has no `dir:` form, so the local-library differentiator remains Bancada's
own.

A profile build downloads whatever the profile declares, so pinning no longer
*requires* a global install. The Libraries tab keeps installing globally anyway:
it populates the Installed tab and serves non-profile builds.

## Where projects are created

Default is the sketchbook (`directories.user`, i.e. `~/Arduino`) — the
conventional home and where arduino-cli looks. But this user keeps real work in
`~/Projects` (the `~/Arduino/libraries` entries are symlinks back there), so
sketchbook-only would fight their habits.

Resolution: the parent directory is a field, defaulted to the sketchbook on first
use and to **the last parent used** afterwards, persisted as
`last_new_project_parent` in the existing `AppSettings` (which already writes
atomically). A "Choose…" button opens the folder picker. One field to fill on the
common path, no ceremony, and it learns.

## Where the form lives

The app still has no modal primitive and this does not add the first one. The
sidebar is 280 px — too narrow for a board selector plus library search.

The form replaces the **editor area** while active, the way a full-width view
would, reached from a `＋ New Project…` button in the toolbar next to
`📁 Open Sketch…`. `App` gains `creatingProject: boolean`; the sidebar and bottom
panel stay put, so the build console remains visible.

## Form fields

| Field | Control | Default |
|---|---|---|
| Name | text | empty (required) |
| Location | text + Choose… | sketchbook, then last used |
| Board | select, `<optgroup>` per platform | the connected board if one is detected, else none |
| Profile name | text | derived from the FQBN's last segment (`esp32:esp32:esp32s3` → `esp32s3`) |
| Libraries | registry search + tick list | none |

464 boards come back from `board listall`, so they are grouped by platform in
`<optgroup>`s; a native select handles type-to-search without extra interaction
code. Only **installed** platforms are offered, because `profile create` needs
one.

Preselecting the currently connected board is a real convenience: the common case
is starting a project for the board already plugged in.

Library search reuses the existing `searchLibraries`; each ticked entry is added
at its latest version, pinned explicitly rather than left floating.

## Order of operations

1. Validate name and location; refuse if `<parent>/<Name>` already exists —
   never pass `sketch new --overwrite`.
2. `sketch new` → `.ino`
3. `profile create --set-default` → `sketch.yaml`
4. `profile lib add <name>@<version>` per ticked library
5. Persist `last_new_project_parent`
6. Open the project: set the sketch dir, load the yaml, select the profile, open
   the main `.ino`

Steps 2–4 each shell out, so the command streams nothing and simply reports. A
library that fails to add is **non-fatal**: the project exists and is usable, so
the failures are collected and reported rather than aborting a created project —
the same boundary `create_library` and `gh_add_library` already draw.

Creation is *not* atomic in the `create_library` sense. Once `sketch new`
succeeds the directory exists, and rolling it back on a later failure would
delete a directory the user can see. Reporting what did and did not happen is
more honest than a silent cleanup.

## Structure

`core/src/project.rs` — new, pure and unit-tested:

- `validate_project_name(&str) -> Result<String>` — non-empty, no path
  separators, no leading `.`, no characters illegal in a sketch folder name.
  Arduino additionally requires the main file to match the folder, which
  `sketch new` handles.
- `profile_name_for_fqbn(&str) -> String` — third segment of the FQBN, options
  stripped (`esp32:esp32:esp32s3:PSRAM=opi` → `esp32s3`), falling back to the
  whole FQBN sanitised.

`core/src/cli.rs` — three thin wrappers: `sketch_new`, `profile_create`,
`profile_lib_add`, plus `board_listall` for the picker.

`core/src/settings.rs` — one new optional field.

`src-tauri` — `create_project`, `list_all_boards`, and the reimplemented
`add_registry_library_to_profile`.

`src/components/NewProject.tsx` — the form. `App.tsx` gains the view switch and
the toolbar button.

## Error handling

| Failure | Behaviour |
|---|---|
| empty or malformed name | refuse, naming the rule |
| destination exists | refuse; never overwrite |
| no installed platforms | refuse, pointing at the core manager |
| `sketch new` fails | refuse, surfacing arduino-cli's stderr |
| `profile create` fails | report; the `.ino` remains, so say so |
| a library fails to add | collect, continue, report per library |
| settings write fails | ignored; a forgotten default is not worth failing on |

## Testing

Pure unit tests in `core/src/project.rs`: name validation (empty, `/`, `\`,
leading dot, valid names with spaces/dashes), and FQBN → profile name (three
segments, with board options, malformed input).

One `#[ignore]`d integration test driving the real pipeline into a temp dir:
`sketch new` → `profile create` → `profile lib add` → assert the `sketch.yaml`
contains the resolved platform version and the pinned library, then
`arduino-cli compile --profile` the result. That last step is the real proof: a
created project must build without further input.

## Deliberately out of scope

Templates beyond arduino-cli's bare `setup()`/`loop()`; multiple profiles at
creation (add more later with `profile create` on an existing sketch); git init
for the new project.
