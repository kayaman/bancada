# Bottom panel: two-row group/tab hierarchy

**Date:** 2026-08-05
**Status:** Approved design, pending implementation

## Problem

The bottom panel's navigation is hierarchical in data but flat on screen.
`src/bottomTabs.ts` groups the six tabs into Console → build, Debugging →
serial/scope, Observability → mqtt/ws, Assistant → agent, yet everything
renders in one shared row: group buttons, a thin `.tab-sep` divider, then the
active group's sub-tabs. Two problems follow:

1. Sub-tabs look like siblings of the group buttons, so the hierarchy does
   not read visually.
2. Sub-tabs appear and disappear mid-row when switching groups, so the
   buttons to their right shift position (moving targets).

The sidebar already solves the identical problem with a two-row pattern
(`.side-groups` row over a `.panel-tabs` row). The bottom panel adopts the
same idiom.

## Design

The bottom panel header becomes two rows.

### Row 1 — groups (`.bottom-groups`)

- The four group buttons: ⚙ Console, 🐞 Debugging, 📡 Observability,
  🤖 Assistant — existing `.bottom-group-btn` styling and click behavior
  (route to the group's remembered tab via `openBottomTab`).
- A spacer, then the maximize/restore button. It is a panel-level control,
  so it lives on the panel-level row.
- Group buttons keep their unseen roll-up dot (`groupHasUnseen`).

### Row 2 — sub-tabs (`.panel-tabs`)

- Always rendered. Multi-tab groups (Debugging, Observability) show
  their sub-tabs; single-tab groups (Console, Assistant) render an
  **empty** row — showing their one tab duplicated the group button
  directly above it ("Console" under "⚙ Console"), which read as a
  rendering bug (user feedback, 2026-08-05). The row's height is pinned
  in CSS (`min-height`), so the header still never jumps when switching
  groups.
- Tabs keep the existing `.tab` styling, per-tab unseen dots, and the
  `openBottomTab` click handler. Serial Monitor and Oscilloscope remain
  under Debugging; MQTT and WebSocket under Observability.
- Tab-specific controls stay on this row after a spacer: the baud-rate
  select and Start/Stop button, shown only while the Serial Monitor tab is
  active.
- Keeps the `border-bottom` that currently separates the header from the
  panel content.

### Removed

- The `.tab-sep` divider between group buttons and sub-tabs — it existed
  only to fake hierarchy within one row. Its CSS rule is deleted if
  nothing else uses it.

## Unchanged

- All of `src/bottomTabs.ts`: `GROUP_OF`, `GROUP_TABS`, labels, and
  `groupHasUnseen` — the change is render/CSS only.
- `openBottomTab` routing, per-group tab memory (`debugTab`, `obsTab`),
  lazy panel mounting, auto-open-serial behavior.
- The resize handle above the header, `BOTTOM_MIN` (120px still leaves
  roughly 68px of content under the taller header; the panel is
  user-resizable), and the maximize behavior itself.

## Files touched

- `src/App.tsx` — the header render block only (currently the single
  `.panel-tabs` div around lines 1187–1256).
- `src/styles.css` — one new `.bottom-groups` rule modeled on
  `.side-groups` (flex row, small gap/padding, `--bg-panel` background,
  buttons keep `flex: none`); delete `.tab-sep` if unused.

## Testing

The grouping logic is untouched, so the existing `bottomTabs` unit tests
stay green. The layout change is verified visually: switch across all four
groups (header height constant, no horizontal shifting), confirm unseen
dots on both rows, confirm baud/Start controls appear only on the Serial
Monitor tab, and confirm maximize/restore still works from row 1.
