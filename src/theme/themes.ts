// The built-in themes.
//
// Every value here was solved for, not picked: each palette is walked up (or
// down) its own hue ramp until it clears the contrast floors in
// `contrast.ts`, which `__tests__/themes.test.ts` then re-checks on every run.
// That is why `bancada-dark` is *nearly* but not exactly the palette Bancada
// shipped through 0.19.0 — three tokens were failing and had to move:
//
//   --border        #303743 -> #475263   (1.33:1 -> 2.02:1 vs --bg-panel)
//   --border-strong #3d4655 -> #66758e   (1.49:1 -> 3.04:1 vs --bg-raised)
//   --text-dim      #8b93a1 -> #959da9   (3.98:1 -> 4.50:1 vs --bg-hover)
//
// The border pair is split by *role*, which is the part worth remembering:
// `--border` is a divider between surfaces and is held to a legibility floor
// of 2:1, while `--border-strong` is a control edge and is held to WCAG
// 1.4.11's 3:1. Holding every one of the stylesheet's 54 borders to 3:1 was
// tried and produced loud grey rules through the whole window — correct by a
// misreading of the spec, and worse to look at.

import type { Theme } from "./tokens";

/** Bancada's own dark palette — the shipped look, with the three failing
 *  tokens lifted along their existing hue ramps so it still reads as itself. */
export const BANCADA_DARK: Theme = {
  id: "bancada-dark",
  name: "Bancada Dark",
  appearance: "dark",
  colors: {
    bg: "#16181d",
    bgPanel: "#1e222a",
    bgRaised: "#262b35",
    bgHover: "#2e3542",
    border: "#475263",
    borderStrong: "#66758e",
    text: "#d7dce4",
    textDim: "#959da9",
    accent: "#2dd4bf",
    // #14867a was carrying white button text at 4.45:1 — `.btn.primary`, the
    // Verify/Flash affordance, was itself under AA. It is squeezed from both
    // sides: white on it must clear 4.5, and it must clear 3:1 against
    // --bg-panel as an edge. Only a 29-step band satisfies both; this is its
    // middle (white 4.87, panel 3.28).
    accentDim: "#137f74",
    onAccent: "#10221f",
    onAccentDim: "#ffffff",
    warn: "#fbbf24",
    // #f87171 measured 4.46:1 on --bg-hover — under AA by four hundredths,
    // which matters because error text is exactly what you read on a hovered
    // console row. Lifted the minimum distance along its own hue ramp.
    error: "#f87373",
    success: "#4ade80",
  },
};

/** The light counterpart, built to the same contrast structure rather than by
 *  inverting the dark one — an inverted dark palette is how you get grey text
 *  on white. Borders here start at the surface colour and darken by the
 *  minimum that clears the floor, so a divider stays a divider. */
export const BANCADA_LIGHT: Theme = {
  id: "bancada-light",
  name: "Bancada Light",
  appearance: "light",
  colors: {
    bg: "#ffffff",
    bgPanel: "#f4f5f7",
    bgRaised: "#eaecef",
    bgHover: "#dde1e6",
    border: "#a7b0c0",
    borderStrong: "#7a88a0",
    text: "#1c1f26",
    textDim: "#5d6472",
    // The dark theme's teal is unreadable on white (1.7:1), so the accent
    // keeps its hue and loses lightness until it is legible as text.
    accent: "#177065",
    // Light inverts the hover direction: the button rests on the lighter teal
    // and darkens on hover, which is the convention on a light ground. Both
    // carry white text.
    accentDim: "#1a8175",
    onAccent: "#ffffff",
    onAccentDim: "#ffffff",
    warn: "#825e02",
    error: "#c90a0a",
    success: "#157337",
  },
};

/** For bright-bench conditions: sunlight on the desk, or eyes that have been
 *  reading 11px mono since midnight. Text goes to the maximum 21:1, and the
 *  dividers are held to the *text* floor rather than the UI one — at this
 *  setting a visible edge matters more than a quiet one. */
export const BANCADA_CONTRAST: Theme = {
  id: "bancada-contrast",
  name: "Bancada High Contrast",
  appearance: "dark",
  colors: {
    bg: "#000000",
    bgPanel: "#0b0d11",
    bgRaised: "#15181e",
    bgHover: "#1f242c",
    border: "#8a97ad",
    borderStrong: "#b6c1d2",
    text: "#ffffff",
    textDim: "#cbd2de",
    accent: "#5eead4",
    accentDim: "#2dd4bf",
    onAccent: "#000000",
    onAccentDim: "#000000",
    warn: "#fcd34d",
    error: "#fca5a5",
    success: "#86efac",
  },
};

export const BUILTIN_THEMES: readonly Theme[] = [
  BANCADA_DARK,
  BANCADA_LIGHT,
  BANCADA_CONTRAST,
];

export const DEFAULT_THEME_ID = BANCADA_DARK.id;

export function themeById(id: string | null | undefined): Theme {
  return BUILTIN_THEMES.find((t) => t.id === id) ?? BANCADA_DARK;
}
