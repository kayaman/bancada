# Profiles: add a board, change a board

**Date:** 2026-08-09
**Status:** Approved design

## Problem

A sketch.yaml profile names the board a project builds for, and the
profile silently wins over whatever is plugged in. But the UI only
offers profile *creation* while the sketch has **zero** profiles
(`Toolbar.tsx` gates "＋ Create profile…" on `profiles.length === 0`).
Once sketch.yaml exists there is no way, from the app, to:

- add a profile for a second board ("flash the same project on the
  Nano too"), or
- change a profile's board when the original is gone for good.

The 2026-08-09 bench session hit this directly: swapping a UNO Q for a
Uno clone meant hand-editing sketch.yaml. The upload-time mismatch
warning (`flashTargetMismatch`) tells you the profile disagrees with
the port — this feature is the *fix* path for it.

## Design

### Toolbar

Two small controls next to the profile selector, visible whenever a
sketch is open:

- **＋** — add a profile. Always enabled.
- **✎** — change the selected profile's board. Disabled while the
  sketch has no profiles.

The existing zero-profile "＋ Create profile…" button stays as the
bootstrap entry point. All three open the same one-row form; the three
editor-area forms (New project / Clone / profile form) stay mutually
exclusive.

### The form: `ProfileInit` grows modes

`creatingProfile: boolean` becomes a mode:
`"bootstrap" | "add" | "retarget" | null`.

- **bootstrap** — exactly today's behavior.
- **add** — board picker (preselecting the detected board, as
  bootstrap does) + name input with the `profileNameForFqbn`
  suggestion. Creates the profile with libraries copied from the
  currently selected profile; on success the new profile becomes the
  session's selected profile. `default_profile` is never touched.
- **retarget** — the profile name is shown fixed (not editable); the
  board picker starts on the profile's current board. On confirm the
  profile's `fqbn:` and platform pin are rewritten in place; name,
  `libraries:`, and pinned `port:` are kept. Selection stays on the
  profile; if it was the default it stays the default.

### Backend

Core (`sketch.rs`), pure and unit-tested:

- `add_profile(name, fqbn, platform_entry, copy_libs_from: Option<&str>)`
  — creates the profile (yaml created when absent, existing duplicate-
  name rejection kept), cloning the source profile's `libraries:` list
  verbatim: registry pins, `dependency:` entries, and `dir:` locals.
  `copy_libs_from: None` is the bootstrap case; the existing
  `init_profile` becomes a thin wrapper or is absorbed.
- `retarget_profile(name, fqbn, platform_entry)` — swaps `fqbn:` and
  replaces the profile's platform pin, touching nothing else.

Both take the platform pin as a pre-built `platform_entry` string
(`boards::platform_dep_entry`), keeping core free of subprocess calls.

A pin is not required for a profile to build (verified 2026-08-09:
arduino-cli 1.5.0 falls back to the installed platform), but writing
one matches `create_project`'s hermetic convention — and on retarget,
replacing the old pin is mandatory: a leftover `arduino:avr` pin under
an `arduino:zephyr` fqbn is exactly the broken state this feature
exists to prevent. (Today's `init_profile` writes no pin; absorbing it
into `add_profile` quietly upgrades bootstrap to pinning too.)

Tauri commands (`lib.rs`) wrap them:

1. Resolve the new board's platform id from the FQBN and look up the
   **installed** version via the CLI. Not installed → error naming the
   core to install; nothing is written.
2. Call the core edit.
3. Inject `required_profile_libs` (UNO Q → Arduino_RouterBridge) via
   `profile lib add`, loud on failure — same contract as `init_profile`
   has since 2026-08-09.
4. Return fresh `SketchYaml`.

The `initProfile` API keeps its signature plus an optional
copy-libs-from argument; `retargetProfile` is new.

### Semantics and non-goals

- Library copying is verbatim; no cross-architecture validation. The
  compiler reports what doesn't fit, and the upload-time mismatch
  warning stays the safety net.
- Retarget to the same board is a no-op success.
- A pinned `port:` is kept on retarget — the same USB slot is the
  common case, and a stale pin already renders "(not attached)".
- No profile removal, no default_profile management (deliberately out
  of scope — not selected).

## Testing

- **Core:** add copies libraries (all three entry shapes) and rejects
  duplicate names; bootstrap path unchanged with `copy_libs_from:
  None`; retarget swaps fqbn + platform pin and keeps name, libraries,
  port; retarget of an unknown profile errors.
- **Frontend:** pure mode logic of the generalized form; existing
  suites (429 TS, 424 core, 43 tauri) stay green.
- **Bench check:** on the FlashProbe scratch project, add a `nano`
  profile from the `uno` one and confirm the yaml; retarget `uno` →
  `arduino:avr:nano` and confirm libraries/port survive.
