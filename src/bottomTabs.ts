// Bottom-panel navigation: one flat row, scoped to the active Software/
// Hardware/Enclosure division (see `App.tsx`'s `sideGroup`). Pure data + one
// view model; no React.
//
// History: this row used to be the only navigation axis, and before that a
// two-level hierarchy (Console / Debugging / Observability / Assistant
// groups over per-group sub-tabs) that cost more than it bought — two of
// the four groups held a single tab, so their sub-row had to render empty,
// and the taxonomy misfiled the tab people use most: Serial Monitor lived
// under "Debugging", two clicks deep, next to the oscilloscope. Scoping by
// division instead of re-introducing bottom-tab-local groups sidesteps both
// failures: "Enclosure" never has to render a degenerate single-tab
// sub-row (it has none of its own — see `TAB_DIVISION`), and Serial stays
// exactly one click away once you're in the Hardware division, same as
// today's flat row.

export type BottomTab = "agent" | "build" | "serial" | "scope" | "mqtt" | "ws" | "web" | "bom" | "diagram";

export const BOTTOM_TABS: readonly BottomTab[] = [
  "agent",
  "build",
  "serial",
  "scope",
  "mqtt",
  "ws",
  "web",
  "bom",
  "diagram",
];

export const TAB_LABEL: Record<BottomTab, string> = {
  agent: "Assistant",
  build: "Build",
  serial: "Serial",
  scope: "Scope",
  mqtt: "MQTT",
  ws: "WS",
  web: "Web",
  bom: "BOM",
  diagram: "Diagram",
};

/** The three divisions this row can be scoped to — mirrors `App.tsx`'s own
 *  `SideGroup` (imported there from here, not duplicated) so the sidebar
 *  switcher and this row can never disagree on what the three options are. */
export type SideDivision = "software" | "hardware" | "enclosure";

/** Which division shows a tab. "global" tabs show in every division — today
 *  just Assistant: it routinely drives both code edits and hardware actions
 *  (verify, flash, watch serial) in one conversation, so it must not
 *  disappear when the division changes. Always first in `BOTTOM_TABS` so it
 *  anchors the row's left edge regardless of which division's own tabs
 *  follow it. */
export const TAB_DIVISION: Record<BottomTab, "global" | SideDivision> = {
  agent: "global",
  build: "software",
  serial: "hardware",
  scope: "hardware",
  mqtt: "hardware",
  ws: "hardware",
  web: "hardware",
  bom: "hardware",
  diagram: "hardware",
};

/** Thin separators after these tabs — the former group boundaries, still
 *  legible within the Hardware division's own row (Serial │ Scope │
 *  MQTT · WS · Web │ BOM · Diagram): */
export const SEPARATOR_AFTER: ReadonlySet<BottomTab> = new Set([
  "serial",
  "scope",
  "web",
]);

/** One rendered tab: everything the bar needs, decided here rather than in JSX. */
export interface TabRowItem {
  tab: BottomTab;
  label: string;
  active: boolean;
  /** Unseen-content dot. Never on the active tab — you are looking at it. */
  dot: boolean;
  /** Count pill (e.g. build errors); null when absent or zero. */
  badge: number | null;
  separatorAfter: boolean;
}

/**
 * The tab bar's whole view model, in render order, filtered to `division`'s
 * own tabs plus every "global" one. Unknown keys in `unseen` or `badges` are
 * ignored — only `BOTTOM_TABS` members are rendered.
 */
export function tabRow(
  active: BottomTab,
  unseen: Partial<Record<BottomTab, boolean>>,
  division: SideDivision,
  badges?: Partial<Record<BottomTab, number>>,
): TabRowItem[] {
  return BOTTOM_TABS.filter((tab) => {
    const d = TAB_DIVISION[tab];
    return d === "global" || d === division;
  }).map((tab) => {
    const n = badges?.[tab];
    return {
      tab,
      label: TAB_LABEL[tab],
      active: tab === active,
      dot: !!unseen[tab] && tab !== active,
      badge: n !== undefined && n > 0 ? n : null,
      separatorAfter: SEPARATOR_AFTER.has(tab),
    };
  });
}
