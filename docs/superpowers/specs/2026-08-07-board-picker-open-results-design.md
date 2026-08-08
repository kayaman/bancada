# Board picker: search results in an open listbox

**Date:** 2026-08-07
**Status:** Approved design

## Problem

`BoardPicker`'s filter input narrows the options of a *closed* native
`<select>` — you type, then still have to click the select to see what
matched. The results should be visible as you type.

## Design

### Open ⇔ query non-empty

While the (trimmed) query is non-empty the select renders as an open
listbox: `size = listboxRows(matches, groups)` where rows =
`min(10, max(3, matches + group headers))`. Empty query → the normal
closed dropdown with the full grouped list. Picking a result fires
`onChange` **and clears the query**, collapsing back to the closed
select showing the pick.

### No layout jump

The select sits in a `.board-select-anchor` span (`position: relative`,
fixed 28 px height — the shared `.select` height). Closed, the select
fills it in-flow as today. Open, the select goes `position: absolute`
over the anchor (full width, `z-index`, border + shadow) — a combobox
dropdown that overlays whatever is below instead of shoving the New
Project fields / profile row around.

### Keyboard

- **Enter** in the filter input selects the first match and collapses.
- **Escape** clears the query.

### Purity cleanup

`visibleBoards`'s pin-the-selection trick existed so a *closed* select
filtered down still had an option matching its value. Open ⇔ non-empty
query makes that state unreachable: the open listbox shows pure
`filterBoards` results (plus a hidden `<option value={value}>` so the
control's value always has an option), the closed select always has the
full list. `visibleBoards` and its tests are deleted. New tested pure
helper: `listboxRows(matchCount, groupCount)`. Zero matches render one
disabled "— no boards match —" row.

Both `BoardPicker` users (New Project dialog, profile-init row) get the
behavior automatically.

## Testing

vitest on `boardSearch.ts`: `listboxRows` clamping (min 3, max 10,
matches + headers), `visibleBoards` tests removed. Component/CSS
verified visually per repo convention.
