# Sketch starter templates — design

**Date:** 2026-08-07
**Status:** Approved (chat)

## Goal

New projects currently always start as Blink. Offer a small set of starter
templates — each one a bench-proven "first upload" diagnostic — chosen from
template cards in the New Project dialog.

## Templates

All follow `blink.ino.tmpl`'s voice: an opening `// {name} — …` comment
explaining what one upload proves, `{name}` substitution, serial-verbose
output at 115200, pins/tunables as named constants at the top.

| id | label | one upload proves |
|---|---|---|
| `blink` | Blink | toolchain, serial port, board (existing template, unchanged) |
| `i2c-scan` | I2C scanner | wiring of any I2C module — prints found addresses, rescans periodically |
| `wifi-scan` | Wi-Fi scanner | radio + antenna — lists networks with RSSI/channel, strongest first |
| `board-info` | Board info | chip identity — model/revision/cores, MAC, flash + PSRAM sizes, reset reason; prints once |

## Architecture

- `core/src/project.rs`: `pub struct SketchTemplate { id, label, description }`
  plus a `TEMPLATES: &[SketchTemplate]` registry (Blink first) backed by
  `include_str!` templates; `sketch_from_template(id, name) -> Option<String>`;
  `write_main_ino(dir, name, template_id)` errors on an unknown id.
- `src-tauri`: `create_project` gains `template: Option<String>` (missing/None
  → `blink`, so existing callers keep working); new `list_sketch_templates`
  command returns the registry for the UI.
- `src/api.ts`: `createProject(..., template)` + `listSketchTemplates()`.
- `NewProject.tsx`: a card grid ("Starter" field) between Board and Profile —
  label + description per card, Blink preselected, selected card highlighted.

## Testing

- Rust: every template renders a complete named program (setup/loop present,
  `{name}` fully substituted, starts with `// <name> — `); unknown template id
  is an error; registry ids are unique and Blink is first.
- Frontend: api passes `template` through to the command; template list shape.

## Out of scope

Per-board template filtering (e.g. hiding Wi-Fi scan on boards without radio),
user-defined templates, template previews in the dialog.
