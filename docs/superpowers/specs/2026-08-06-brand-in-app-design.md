# In-app branding and top-panel polish — design

**Date:** 2026-08-06
**Request:** "change icons to brand. Use an icon inside the application.
Review top panel."

## Problem

Commit `060ca13` added the new brand mark (SVG masters + PNG renders in
`src-tauri/icons/brand/`) and already wired all three Tauri bundle icons, so
the window/taskbar/launcher icons are branded. But the frontend had no
favicon (404 in browser dev, no `public/` dir), the UI showed zero brand
identity (no logo, no wordmark, version shown nowhere), and a toolbar review
found mechanical defects: no control grouping in a flat 7-control row, the
glyph-only ⟳ button missing an `aria-label`, Open Sketch the only control
without a tooltip, and a dead `.action` class on Verify/Flash with no CSS
rule anywhere.

## Design (user-selected)

- **Lockup: mark + wordmark + version** at the toolbar's leading edge —
  20px `BrandMark` (inline JSX of `bancada-small.svg`, the ≤32px simplified
  variant), "bancada" at 12px, version dim at 11px. The version comes from
  Tauri's `getVersion()` (source of truth stays `tauri.conf.json`); outside
  a Tauri shell the call rejects and the span never renders.
- **Polish + grouping**: controls wrapped in `.toolbar-group`s — project
  (Open, New Project) | board (profile, port+⟳ as a 2px `.toolbar-pair`) |
  spacer | build (Verify, Flash) — with 1×18px `.toolbar-sep` rules between
  the left clusters. Mechanical fixes: `aria-label` on ⟳, path tooltip on
  Open Sketch, dead `.action` class dropped.
- **Favicon**: `public/favicon.svg` (copy of `bancada-small.svg`) +
  `<link rel="icon">` in `index.html`.

The mark is inlined as JSX rather than imported: `src-tauri/icons/brand/`
belongs to the Rust crate and sits outside the Vite root; the frontend
bundle must not reach into it. Toolbar height is unchanged (41px); at the
900px window minimum nothing overflows (verified in-browser).

"Change icons to brand" needed no Tauri-side work — the three `bundle.icon`
entries were already byte-identical to the brand renders, and no stale
`.ico`/`.icns` art exists (bundle targets are Linux-only).
