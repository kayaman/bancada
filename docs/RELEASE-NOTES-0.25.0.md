# Bancada 0.25.0 — Diagram Tab & Assistant BOM/Wiring

## What's New

### Diagram Tab
A new **Diagram** bottom panel tab visualises the wiring data stored in `bom.yaml`.

- **Live auto-diagram**: rendered as pure React SVG from the BOM's wiring entries.
  Components appear on the left with their pins listed; the board's header pins appear
  on the right; bezier lines connect each pin to its GPIO or power rail. Colours follow
  a consistent scheme — orange for GPIO connections, crimson for 3V3/5V, dark grey for
  GND, steel-blue for all others.
- **Agent SVG display**: when the assistant writes `wiring.svg` to the sketch directory,
  the Diagram tab displays it automatically below the auto-diagram. The file is rendered
  as an `<img>` (not `dangerouslySetInnerHTML`) so scripts inside agent-generated SVG
  cannot execute.
- Both views reload whenever `bom.yaml` or `wiring.svg` are edited — either manually or
  by the assistant.

### Assistant BOM/Wiring Awareness
The embedded Claude agent now understands the `bom.yaml` schema and can:

- **Edit `bom.yaml` directly** — add, update, or remove components, descriptions,
  images, and wiring entries. The BOM panel reloads automatically when the agent
  writes the file.
- **Draw wiring diagrams** — when asked to draw or illustrate the circuit, the agent
  reads `bom.yaml`, calls `board_pinout` for pin layout, and writes a clean
  `wiring.svg` to the sketch directory. The Diagram tab picks it up immediately.

The agent's system prompt was extended with the full `bom.yaml` field schema and
guidance to call `board_pinout` before generating wiring data.

## Fixes & Housekeeping

- Auto-reload triggers use the existing `handleAgentFileChange` hook (no extra
  file-system watchers needed); detects file path suffixes `/bom.yaml` and `/wiring.svg`.
- Diagram tab follows the same lazy-mount pattern as the BOM tab — never unmounted
  once opened, hidden via `display:none`.

## Files Changed

- `src/bottomTabs.ts` — added `"diagram"` tab
- `src/components/DiagramPanel.tsx` — new component
- `src/App.tsx` — `diagramMounted`, `diagramVersion`, agent file-change hooks, DiagramPanel render
- `src/styles.css` — diagram panel styles
- `src-tauri/src/lib.rs` — extended `system_prompt_extra()` with bom.yaml schema and wiring.svg guidance
- `src/__tests__/bottomTabs.test.ts`, `src/__tests__/BottomTabBar.test.tsx` — updated for 9 tabs
