# Brand the app: finish icon wiring, in-app lockup, top-panel polish

## Context

Commit `060ca13` (concurrent session) added the new brand mark — SVG masters
and PNG renders in `src-tauri/icons/brand/` — and already updated the three
Tauri bundle icons (`32x32.png`, `128x128.png`, `icon.png` are byte-identical
to the brand renders), so the window/taskbar/launcher icons are done. What's
missing: the frontend has no favicon (404 in browser dev; no `public/` dir),
the UI shows zero brand identity (no logo, wordmark, or version anywhere),
and the toolbar has mechanical defects found in review. User decisions
(AskUserQuestion): **mark + wordmark + version** lockup; **polish + grouping**
for the toolbar; **favicon yes**.

No backend/Rust changes. No new dependencies. All changes are JSX/CSS wiring
plus one asset copy — untested by repo convention (no pure logic extracted;
nothing here warrants it).

## Changes

### 1. `src/components/BrandMark.tsx` (new)

Inline JSX transcription of `src-tauri/icons/brand/bancada-small.svg` (the
≤32px simplified variant — the master's glow ring and bevel mud out below
~40px). Five shapes, hardcoded brand palette (`#1a1f2e` tile, `#2dd4bf`
bench = `--accent`, `#fbbf24` lamp = `--warn`). Component takes
`size` (default 20), renders `<svg viewBox="0 0 512 512" width={size}
height={size} aria-hidden="true">…`. Inlining avoids a new asset pipeline;
`src-tauri/icons/brand/` is outside the Vite root and must not be imported
from the frontend. The explicit width/height matters: the source SVG
hardcodes 512×512 and would blow out the flex toolbar.

### 2. `src/components/Toolbar.tsx` — lockup + grouping + mechanical fixes

New leading lockup, then the existing controls wrapped into groups with thin
separators:

```
[BrandMark 20px] bancada 0.10.0 │ 📁 Open… ＋ New Project… │ [profile ▾] [port ▾]⟳ │ …spacer… ✓ Verify → Flash
```

- **Lockup**: `<div className="brand" title="Bancada — Arduino Workbench">`
  containing `<BrandMark size={20} />`, `<span className="brand-name">bancada</span>`,
  and `{version && <span className="brand-version">{version}</span>}`.
- **Version source**: `getVersion()` from `@tauri-apps/api/app` (existing
  dependency) in a small `useEffect` + `useState` in Toolbar;
  `.catch(() => {})` leaves it empty so the span simply doesn't render
  outside a Tauri shell. Single source of truth stays `tauri.conf.json`
  (0.10.0) — no vite `define` plumbing, no duplicated version constant.
- **Grouping**: wrap controls in `.toolbar-group` divs — project (Open
  Sketch, New Project), board (profile select / Create-profile button, port
  select + ⟳), build (Verify, Flash) — with `.toolbar-sep` rules between
  lockup/project/board. Port select + ⟳ become a `.toolbar-pair` (2px gap)
  so the rescan button reads as attached to the port picker. The
  conditional Create-profile button stays inside the board group.
- **Mechanical fixes**:
  - ⟳ button gains `aria-label="Rescan ports"` (matches the app's other
    glyph-only buttons, e.g. App.tsx:1222).
  - Open Sketch gains a tooltip: `title={props.sketchDir ?? "Open a sketch
    folder"}` — full path when open, plain hint otherwise.
  - Drop the dead `action` class from Verify/Flash (no CSS rule anywhere
    matches it).

### 3. `src/styles.css` — new rules

```css
.brand          { display:flex; align-items:center; gap:7px; padding-right:2px; user-select:none; }
.brand-name     { font-size:12px; letter-spacing:.04em; color:var(--text); }
.brand-version  { font-size:11px; color:var(--text-dim); }
.toolbar-sep    { width:1px; height:18px; background:var(--border); margin:0 4px; flex:none; }
.toolbar-group  { display:flex; align-items:center; gap:6px; }
.toolbar-pair   { display:flex; align-items:center; gap:2px; }
```

Toolbar height is untouched (20px mark < 28px controls in the ~41px bar), so
nothing anchored below it (ProfileInit, conflict banner) moves. The lockup +
two separators add ~110px of row width; window `minWidth` 900 still fits.

### 4. Favicon — `public/favicon.svg` (new) + `index.html`

- Create `public/` (Vite's default `publicDir`, currently absent) with
  `favicon.svg` = a copy of `src-tauri/icons/brand/bancada-small.svg`.
- Add `<link rel="icon" type="image/svg+xml" href="/favicon.svg" />` to
  `index.html`'s head. Fixes the 404 and blank favicon when opening
  `localhost:5173` in a browser during dev; invisible inside WebKitGTK.

### 5. "Change icons to brand" — verification only

The Tauri side is already fully branded by `060ca13` (verified byte-identical
sizes; no stale `.ico`/`.icns`/`Square*` art exists — bundle targets are
Linux-only). Nothing to change; the plan records this so the request isn't
half-remembered as unfinished.

### 6. Docs

Spec + plan copies under `docs/superpowers/{specs,plans}/2026-08-06-brand-in-app-…`,
committed with the change (repo convention).

## Verification

- `npx tsc --noEmit` and `npm test` (407 currently; expect no change — no
  pure logic touched), `cargo test` regression only.
- Visual, via chrome-devtools MCP against the running `npm run tauri dev`
  (same technique as the earlier two-row-panel check): screenshot the
  toolbar; assert lockup renders at 20px, separators 18px, toolbar height
  unchanged (~41px) before vs after; check narrow-ish window (~900px) for
  clipping.
- Browser dev: `http://localhost:5173` shows the brand favicon, no 404.
- Bench: hover ⟳ (tooltip + screen-reader name), hover Open Sketch with a
  sketch open (full path tooltip), version reads 0.10.0 next to the wordmark.
