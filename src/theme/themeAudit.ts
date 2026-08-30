// Holds any theme — built-in or imported — to the same contrast floors, and
// repairs the ones that fail.
//
// The floors are not invented here. They are exactly the assertions in
// __tests__/themes.test.ts, lifted into runtime code so that a `.vsix` off the
// marketplace has to clear the same bar the built-ins do. That symmetry is the
// point: it would be strange to fail our own build over a 4.46:1 error colour
// and then happily render an imported theme at 2:1.
//
// Repair moves a colour along its own lightness axis by the smallest step that
// clears the floor (see `adjustToFloor`), so a repaired theme still looks like
// the theme somebody designed — just legible. Where no lightness can satisfy
// the constraint, the token falls back rather than shipping something that
// fails silently.

import { adjustToFloor, contrastRatio } from "./contrast";
import type { ColorToken, Theme, ThemeColors } from "./tokens";

/** One token, the colours it must work against, and the floor it must clear. */
type Rule = {
  token: ColorToken;
  against: (c: ThemeColors) => string[];
  floor: number;
  /** Why this rule exists, shown to the user when it fails. */
  because: string;
};

const surfaces = (c: ThemeColors) => [c.bg, c.bgPanel, c.bgRaised, c.bgHover];

export const RULES: readonly Rule[] = [
  {
    token: "text",
    against: surfaces,
    floor: 4.5,
    because: "body text must be readable on every surface",
  },
  {
    token: "textDim",
    against: surfaces,
    floor: 4.5,
    because: "dimmed text is still text",
  },
  {
    token: "accent",
    against: surfaces,
    floor: 4.5,
    because: "the accent is used for text and for focus rings",
  },
  { token: "warn", against: surfaces, floor: 4.5, because: "warnings are read" },
  { token: "error", against: surfaces, floor: 4.5, because: "errors are read" },
  {
    token: "success",
    against: surfaces,
    floor: 4.5,
    because: "success text is read",
  },
  {
    token: "border",
    against: (c) => [c.bgPanel],
    floor: 2,
    because: "a divider nobody can see is not a divider",
  },
  {
    token: "borderStrong",
    against: (c) => [c.bgRaised],
    floor: 3,
    because: "a control's edge is where the control begins (WCAG 1.4.11)",
  },
  {
    token: "accentDim",
    against: (c) => [c.bgPanel],
    floor: 3,
    because: "the accent fill must be distinguishable from the panel",
  },
  {
    token: "onAccent",
    against: (c) => [c.accent],
    floor: 4.5,
    because: "button text on the accent fill is read",
  },
  {
    token: "onAccentDim",
    against: (c) => [c.accentDim],
    floor: 4.5,
    because: "button text on the dim accent fill is read",
  },
];

export type Violation = {
  token: ColorToken;
  ratio: number;
  floor: number;
  because: string;
  /** The value repair would use, or null when no lightness satisfies it. */
  repaired: string | null;
};

/** Every rule this theme breaks, worst first. Empty means it is fine. */
export function auditTheme(theme: Theme): Violation[] {
  const out: Violation[] = [];
  for (const rule of RULES) {
    const against = rule.against(theme.colors);
    const value = theme.colors[rule.token];
    const ratio = Math.min(...against.map((bg) => contrastRatio(value, bg)));
    if (Math.round(ratio * 100) / 100 >= rule.floor) continue;
    out.push({
      token: rule.token,
      ratio,
      floor: rule.floor,
      because: rule.because,
      repaired: adjustToFloor(value, against, rule.floor),
    });
  }
  return out.sort((a, b) => a.ratio / a.floor - b.ratio / b.floor);
}

export type RepairResult = {
  theme: Theme;
  /** What was changed, and what could not be. */
  violations: Violation[];
  /** Tokens no lightness could rescue — these fell back instead. */
  unrepairable: ColorToken[];
};

/**
 * Returns a legible version of `theme`.
 *
 * Rules are applied in order and each reads the colours the previous ones
 * already fixed, which matters: repairing `bgPanel` would change what
 * `border` has to clear. Backgrounds are never touched — they are the theme's
 * identity, and moving them would repaint the whole window to rescue one
 * foreground. Foregrounds move instead.
 */
export function repairTheme(theme: Theme): RepairResult {
  const violations = auditTheme(theme);
  if (violations.length === 0) {
    return { theme, violations, unrepairable: [] };
  }

  const colors: ThemeColors = { ...theme.colors };
  const unrepairable: ColorToken[] = [];

  for (const rule of RULES) {
    const against = rule.against(colors);
    const fixed = adjustToFloor(colors[rule.token], against, rule.floor);
    if (fixed) {
      colors[rule.token] = fixed;
      continue;
    }
    const ratio = Math.min(...against.map((bg) => contrastRatio(colors[rule.token], bg)));
    if (Math.round(ratio * 100) / 100 >= rule.floor) continue;
    // Nothing on this hue works. The two on-accent tokens always have an
    // answer at one end of the range, so this is reachable only for a
    // foreground constrained against surfaces at both extremes.
    unrepairable.push(rule.token);
    colors[rule.token] =
      theme.appearance === "dark" ? "#ffffff" : "#000000";
  }

  return {
    theme: { ...theme, colors },
    violations,
    unrepairable,
  };
}
