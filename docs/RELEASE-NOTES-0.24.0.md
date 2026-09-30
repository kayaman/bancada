# Bancada 0.24.0

**Hardware visibility: board registry, BOM, and wiring.**

## Board registry

The Fleet tab now tracks per-board metadata that persists in `fleet.json` alongside
the existing identity and flash history:

- **Condition** — Working / Broken / Reserved / Retired, colour-coded on each card.
- **Assigned project** — which sketch this board belongs to; a "Set to current" button
  wires it to the open project in one click.
- **Notes** — inline textarea, click to edit.
- **Last flash** — project name, short commit hash, branch, and time; highlighted when
  the flashed project matches the currently open one.

## Bill of materials

A new **BOM** tab in the bottom panel hosts a table editor for `bom.yaml` in the
sketch directory. The file travels with the project in git and the embedded Agent can
read and edit it.

Columns: Qty · Ref · Value · Package · Supplier · Part No · Notes. Tab navigates
between cells; Enter on the last cell of a row appends a new row.

## Wiring

Each BOM component now has an expandable detail section (▸/▾):

- **Description** — plain-text purpose for the component.
- **Images** — URLs to product photos or datasheets, stored as a list.
- **Wiring** — pin-to-GPIO / power-rail connections. Each entry records a pin label,
  an optional GPIO number, an optional rail name (3V3, GND, …), and optional notes.
  Old `bom.yaml` files without these fields parse without error.

## Fixes

- **Serial monitor autoscroll** no longer snaps back to the bottom when autoscroll is
  on and the user scrolls up. A `followingRef` latch gives the scroll effect
  synchronous access to intent, bypassing the React render-cycle lag that caused the
  race.
- **Flash no longer auto-targets `/dev/ttyS0`** (legacy UART placeholder) when a real
  USB board is connected. Port selection now weights USB ports over non-USB ones so
  the right board is always pre-selected.
- **After the assistant flashes a board**, focus returns to whichever tab the user was
  on rather than always jumping to the Assistant tab.

## Tests

```
cargo test -p bancada-core --lib   777 passed
cargo check -p bancada             clean
npx tsc --noEmit && npx vitest run 1317 passed, 82 files
npm run build                      clean
```
