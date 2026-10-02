# Bancada 0.26.0 — Enclosure Design, Fully Integrated

## What's New

### Enclosure design lives inside Bancada now

The third pillar — Software, Hardware, **Enclosure** — is no longer a hand-off to a
separate app. Bancada's window gained a third top-level division, alongside Software
and Hardware, with its own sidebar entry.

- **Compose, then see it live.** The Enclosure division opens with a draft prompt
  generated from the project's BOM and resolved board, which you can review and edit
  before sending. Once sent, the division becomes a live embedded preview — the 3D
  render, the script's code pane, and the design chat — right there in the main
  window, not a second window or a separate process.
- **Direct model controls.** Select a part in the viewport or from a list and move or
  rotate it with on-screen handles or exact X/Y/Z values, with optional snapping,
  undo/redo, and camera presets (3D/Top/Front/Right/Fit). Transforms persist
  per-project and apply to STL/3MF exports and the CLI, independent of the script.
- **Direct dimension editing.** Numeric literals in the script — wall thickness, boss
  height, hole diameters, and more — get exact-value inputs and a reset-to-default
  button, alongside the existing `param(...)` sliders, without the assistant needing
  to touch the file.
- **A more focused embedded assistant.** The design chat now runs isolated from your
  own Claude Code plugins, skills, and MCP servers (previously it silently inherited
  all of them, inflating its first turn by over 50x on a typical machine), and its
  system prompt was extended with fixed hardware dimensions and house printing
  guidance so it stays useful even with that narrower reach.
- **Reopening a project with an enclosure already designed** goes straight to the live
  preview — no need to re-compose or resend a prompt.

### A Ctrl+K command palette

Jump anywhere without reaching for the mouse: Ctrl+K or Ctrl+Shift+P opens a
fuzzy-match palette over Save, Open, Verify, Upload, every bottom tab, and the three
top-level divisions.

### USB CDC On Boot, handled automatically

Uploading to an ESP32-S2/S3/C3/C6/H2 board over its native USB port now auto-enables
**USB CDC On Boot** when it's still on the default setting — previously `Serial`
output silently went nowhere until this was set by hand. The fix is saved into the
active profile so it only needs to happen once; other board options and dependency
pins are untouched, and the Build console reports the adjustment.

### A one-step local build-and-install

`npm run build:install` (`scripts/build-and-install.sh`) detects your distro, builds
the matching bundle, and installs it with `sudo` — for trying a local checkout on the
machine you built it on, not a substitute for the release ritual.

## Fixes & Housekeeping

- `.bancada/` project artifacts (like the enclosure hand-off file) are tracked by git
  again — the whole directory used to be gitignored, which silently excluded them from
  commits and clones. Only `.bancada/libs/` (re-fetchable vendored libraries) stays
  ignored.
- The Assistant's stdout/stderr drain uses lossy line reads, so a build tool emitting
  non-UTF-8 bytes no longer stalls or kills the stream mid-build.
- Fleet identity merges keep notes, condition, and assigned project instead of
  dropping them.
- Renaming or duplicating an ESP-IDF project now edits the real `project()` call.
- The file explorer refuses `./`-prefixed spellings that previously dodged the
  protected-file rules.
- Added a `validate_circuit` MCP tool and matching Claude Code skill for checking a
  GPIO wiring plan against board pin-safety rules.
- Added the Diagram bottom tab (auto-SVG wiring diagrams from `bom.yaml`, plus
  agent-generated `wiring.svg` display) and taught the embedded Assistant the
  `bom.yaml` schema so it can edit it and draw wiring diagrams directly.
- Added a native end-to-end smoke test driving real Tauri IPC over WebKitWebDriver.

## Known limitations

- The embedded enclosure design chat's Export STL button reaches Bancada's native
  save path through a `postMessage` bridge (its own iframe has no direct Tauri IPC
  access) — functionally equivalent to a direct call, but it's a bridge, not the
  same code path a future refactor might assume.
- Command palette coverage is navigation and the most common actions; it does not yet
  cover every toolbar control.

## Files Changed

- `enclosure-maker/` — the CAD engine, its Rhai scripting crates, and the preview
  server, merged in-process (see `enclosure-maker/README.md`).
- `src-tauri/src/enclosure_preview.rs`, `src/components/EnclosurePanel.tsx` — the
  embedded preview and compose-then-view flow.
- `src/App.tsx`, `src/bottomTabs.ts`, `src/components/BottomTabBar.tsx` — the
  three-division sidebar and division-scoped bottom tab row.
- `src/commandPalette.ts`, `src/components/CommandPalette.tsx` — the command palette.
- `core/src/usb_cdc.rs`, `core/src/cli.rs`, `src/boardOptions.ts` — USB CDC On Boot
  auto-correction.
- `core/src/git.rs`, `core/src/clone.rs` — the `.bancada/` gitignore/clone fix.
- `scripts/build-and-install.sh` — the local build-and-install script.
