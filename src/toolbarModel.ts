// What the toolbar's project affordance shows and offers.
//
// Extracted because `vitest` runs in the node environment and its include glob
// is `**/*.test.ts` — no component in this repo is ever rendered by a test, so
// a decision left inside `ProjectMenu.tsx` is a decision verified only by eye.
// The component renders what these functions hand it.

import { formatAccel, parseAccel } from "./keys";

/** Ctrl+O, already wired to `openSketch` by App's global key handler. */
const ACCEL_OPEN = parseAccel("Ctrl+O");

export type ProjectAction = "open" | "new" | "duplicate" | "rename";

export interface MenuItem {
  id: ProjectAction;
  label: string;
  /** Rendered right-aligned and dim. Only real, wired shortcuts appear here. */
  accel?: string;
  /**
   * Present when the item cannot be used, and shown as its tooltip. A disabled
   * control that does not say why is the thing `gitStatus.syncDisabledReason`
   * exists to avoid; the same rule applies in a menu.
   */
  disabledReason?: string;
}

/** The project button's label: the open project's name, or the invitation. */
export function projectButtonLabel(sketchDir: string | null): string {
  const name = sketchDir?.split("/").filter(Boolean).pop();
  return name || "Open project";
}

/**
 * The project menu, in order.
 *
 * Only `Rename` needs an open project — it acts on the current folder in
 * place. `Duplicate` deliberately does not: its pane takes the source as a
 * prefill and offers its own folder picker, so it has always worked from a
 * cold start, and disabling it here would quietly remove that.
 *
 * The one disabled item says why rather than vanishing. The bar already mixes
 * hide-and-disable for the same condition, and an item that disappears teaches
 * the user nothing.
 */
export function projectMenuItems(state: { sketchDir: string | null }): MenuItem[] {
  return [
    {
      id: "open",
      label: "Open project…",
      ...(ACCEL_OPEN ? { accel: formatAccel(ACCEL_OPEN) } : {}),
    },
    { id: "new", label: "New project…" },
    { id: "duplicate", label: "Duplicate project…" },
    {
      id: "rename",
      label: "Rename project…",
      disabledReason: state.sketchDir ? undefined : "open a project first",
    },
  ];
}
