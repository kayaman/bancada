# Board Search Design

**Date:** 2026-08-01
**Status:** Implementing (user request named the shape: a search input for the board list)

## Problem

`board listall` returns hundreds of boards. Both pickers — the New
Project dialog and the Create-profile row — render them as one giant
grouped `<select>`, findable only by scrolling. Both also duplicate the
group-by-platform logic.

## Decision

A shared `BoardPicker` component: a search input that filters the
options of the familiar grouped native `<select>`. No custom dropdown —
the native select keeps keyboard navigation and optgroups for free.

## Components

- `src/boardSearch.ts` — pure, vitest-tested:
  - `filterBoards(boards, query)` — trimmed, case-insensitive,
    whitespace-tokenized AND-match over `name`, `fqbn`, `platform_name`.
    Empty query returns everything.
  - `visibleBoards(boards, query, selectedFqbn)` — `filterBoards`, plus
    the selected board pinned (prepended) when the filter would drop it,
    so the select's value always has a matching option. No duplicate
    when it already matches.
  - `groupByPlatform(boards)` — the Map grouping currently duplicated in
    NewProject and ProfileInit, extracted; returns
    `[platform_name, BoardOption[]][]` in first-seen order.
- `src/components/BoardPicker.tsx` — props `{ boards, value, onChange,
  title }`; renders search input + grouped select; local query state.
- `src/components/NewProject.tsx`, `src/components/ProfileInit.tsx` —
  replace inline select + grouping with `BoardPicker`.
- `src/styles.css` — `.board-picker` flex row, search input compact.

## Error handling

None new — the picker is presentation over already-loaded data.

## Testing

vitest on boardSearch.ts: empty query; name match (case-insensitive);
fqbn match; platform match; multi-token AND; pinned selection; no
duplicate pin; grouping order. Existing suites stay green.
