// What the toolbar's project affordance shows and offers.
//
// Extracted because `vitest` runs in the node environment and its include glob
// is `**/*.test.ts` — no component in this repo is ever rendered by a test, so
// a decision left inside `ProjectMenu.tsx` is a decision verified only by eye.
// The component renders what these functions hand it.

import { formatAccel, parseAccel } from "./keys";
import type { ProjectKind } from "./api";

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

/**
 * Why Verify or Flash cannot run right now, or null when it can.
 *
 * Both buttons were disabled with a static tooltip describing the action, so
 * a greyed-out Flash never said whether it wanted a project, a port, or just
 * patience. `gitStatus.syncDisabledReason` exists because of exactly that
 * lesson — a disabled control must say why.
 *
 * Ordered most-fundamental first: with nothing open, the missing port is not
 * the thing to tell someone about.
 */
export function buildBlockedReason(
  action: "verify" | "flash",
  s: { sketchDir: string | null; selectedPort: string | null; busy: boolean },
): string | null {
  if (!s.sketchDir) return "open a project first";
  if (s.busy) return "a build is already running";
  if (action === "flash" && !s.selectedPort) return "select a serial port";
  return null;
}

/** What a build is aimed at. Empty for ESP-IDF: `idf.py` has no board
 *  concept, and the backend reads the chip target from sdkconfig itself. */
export interface BuildTarget {
  profile?: string;
  fqbn?: string;
}

/**
 * What Verify, Flash and an Assistant session build against, or why nothing
 * can be built yet.
 *
 * Arduino: the sketch.yaml profile first, the board detected on the port as
 * the fallback. An unrecognised folder goes the same way, because it has
 * always gone down the arduino-cli path and gets arduino-cli's own error.
 *
 * ESP-IDF: neither applies. This used to be Arduino-only and told the owner
 * of an ESP-IDF project to "create a profile" — advice that made no sense,
 * since sketch.yaml never enters an `idf.py` build and a devkit behind a bare
 * USB bridge reporting no board identity is the normal case. The one thing an
 * IDF build needs is the chip target, which the toolbar's target picker sets.
 */
export function resolveBuildTarget(s: {
  kind: ProjectKind;
  profile: string | null;
  detectedFqbn: string | null | undefined;
  idfTarget: string | null;
}): { target: BuildTarget } | { error: string } {
  if (s.kind === "idf") {
    if (s.idfTarget) return { target: {} };
    return {
      error:
        "This ESP-IDF project has no target set — choose a chip in the toolbar before building.",
    };
  }
  if (s.profile) return { target: { profile: s.profile } };
  if (s.detectedFqbn) return { target: { fqbn: s.detectedFqbn } };
  return {
    error:
      "No sketch.yaml profile, and this port reports no board identity (USB bridge) — create a profile to set the board.",
  };
}

/** Why the profile's board cannot be changed right now, or null. */
export function retargetBlockedReason(
  profiles: string[],
  profile: string | null,
): string | null {
  if (profiles.length === 0) return "this project has no sketch.yaml profile yet";
  if (!profile) return "select a profile first";
  return null;
}

/**
 * Why the ESP-IDF target cannot be changed right now, or null.
 *
 * Same "disable and say why" rule as [`buildBlockedReason`], and ordered the
 * same way — most fundamental first, because with no project open the missing
 * toolchain is not the thing to mention.
 */
export function setTargetBlockedReason(s: {
  sketchDir: string | null;
  busy: boolean;
  idfAvailable: boolean;
}): string | null {
  if (!s.sketchDir) return "open a project first";
  if (!s.idfAvailable) return "ESP-IDF is not available on this machine";
  if (s.busy) return "a build is already running";
  return null;
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
