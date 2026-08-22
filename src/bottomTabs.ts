// Bottom-panel navigation: one flat row of seven tabs. Pure data + one view
// model; no React.
//
// This replaced a two-level hierarchy (Console / Debugging / Observability /
// Assistant groups over per-group sub-tabs). It cost more than it bought:
// two of the four groups held a single tab, so their sub-row had to render
// empty — a sub-tab there would have read as a duplicate of the group button
// right above it — and the empty row needed a min-height hack to stop the
// content edge jumping between groups. The taxonomy also misfiled the tab
// people use most: Serial Monitor lived under "Debugging", two clicks deep,
// next to the oscilloscope. Flat, the whole bench is one click away and the
// old group boundaries survive as thin separators.

export type BottomTab = "build" | "serial" | "scope" | "mqtt" | "ws" | "web" | "agent";

export const BOTTOM_TABS: readonly BottomTab[] = [
  "build",
  "serial",
  "scope",
  "mqtt",
  "ws",
  "web",
  "agent",
];

export const TAB_LABEL: Record<BottomTab, string> = {
  build: "Build",
  serial: "Serial",
  scope: "Scope",
  mqtt: "MQTT",
  ws: "WS",
  web: "Web",
  agent: "Assistant",
};

/** Thin separators after these tabs — the former group boundaries:
 *  Build · Serial │ Scope │ MQTT · WS · Web │ Assistant */
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
 * The tab bar's whole view model, in render order. Unknown keys in `unseen`
 * or `badges` are ignored — only BOTTOM_TABS members are rendered.
 */
export function tabRow(
  active: BottomTab,
  unseen: Partial<Record<BottomTab, boolean>>,
  badges?: Partial<Record<BottomTab, number>>,
): TabRowItem[] {
  return BOTTOM_TABS.map((tab) => {
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
