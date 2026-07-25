// Bottom-panel two-level navigation: tabs grouped into Console / Debugging /
// Observability. Pure data + one predicate; no React.

export type BottomTab = "build" | "serial" | "scope" | "mqtt" | "ws";
export type BottomGroup = "console" | "debug" | "obs";

export const GROUP_OF: Record<BottomTab, BottomGroup> = {
  build: "console",
  serial: "debug",
  scope: "debug",
  mqtt: "obs",
  ws: "obs",
};

export const GROUP_TABS: Record<BottomGroup, BottomTab[]> = {
  console: ["build"],
  debug: ["serial", "scope"],
  obs: ["mqtt", "ws"],
};

export const GROUP_LABEL: Record<BottomGroup, string> = {
  console: "⚙ Console",
  debug: "🐞 Debugging",
  obs: "📡 Observability",
};

export const TAB_LABEL: Record<BottomTab, string> = {
  build: "⚙ Console",
  serial: "❯ Serial Monitor",
  scope: "∿ Oscilloscope",
  mqtt: "MQTT",
  ws: "WebSocket",
};

/**
 * A group button shows an unseen dot iff it is NOT the active group and any
 * of its tabs is flagged unseen (D2: the active group never rolls up).
 */
export function groupHasUnseen(
  g: BottomGroup,
  active: BottomGroup,
  unseen: Partial<Record<BottomTab, boolean>>,
): boolean {
  return g !== active && GROUP_TABS[g].some((t) => unseen[t]);
}
