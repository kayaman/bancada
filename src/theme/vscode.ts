// VS Code colour theme -> Bancada tokens.
//
// The shape of the problem, stated honestly: a VS Code theme has roughly 600
// possible `colors` keys and every one of them is optional. Real themes define
// somewhere between twenty and two hundred. Bancada needs fifteen, all of them
// mandatory. So this is not a translation — it is a resolution with fallbacks,
// and where the fallbacks run out, a derivation.
//
// Three rules keep that from becoming guesswork:
//
//  1. Prefer what the theme said. Each token has a chain of VS Code keys in
//     descending order of "actually means this"; the first one present wins.
//  2. Derive the rest from what the theme did say, never from a constant. A
//     theme that only defines `editor.background` and `editor.foreground`
//     should still come out looking like itself, just with less nuance.
//  3. Never emit a token that fails its floor. Marketplace themes are not
//     contrast-audited by anyone, and a theme is welcome to be pretty at the
//     cost of legibility in VS Code — it is not welcome to do that here, where
//     the thing being read is a compiler error at 3am. `auditTheme` reports
//     and `repairTheme` fixes, both in themeAudit.ts.

import {
  composite,
  contrastRatio,
  hslToRgb,
  parseHex,
  relativeLuminance,
  rgbToHsl,
  toHex,
} from "./contrast";
import type { Appearance, ColorToken, Theme, ThemeColors } from "./tokens";

/** The subset of a VS Code colour theme this reads.
 *
 *  `tokenColors` is accepted and deliberately unused for now: it carries
 *  TextMate scope selectors, and Bancada's editor highlights off Lezer tags.
 *  Reducing one onto the other is a separate piece of work with its own
 *  judgement calls; parsing it here would imply a fidelity that does not
 *  exist. */
export interface VsCodeTheme {
  name?: string;
  type?: string;
  colors?: Record<string, unknown>;
  tokenColors?: unknown[];
}

/** Descending preference. First key actually present in the theme wins. */
const KEYS: Record<ColorToken, string[]> = {
  bg: ["editor.background"],
  bgPanel: [
    "sideBar.background",
    "panel.background",
    "activityBar.background",
    "editorGroupHeader.tabsBackground",
  ],
  bgRaised: [
    "dropdown.background",
    "input.background",
    "editorWidget.background",
    "quickInput.background",
    "menu.background",
  ],
  bgHover: [
    "list.hoverBackground",
    "toolbar.hoverBackground",
    "menu.selectionBackground",
    "list.activeSelectionBackground",
  ],
  border: [
    "panel.border",
    "sideBar.border",
    "editorGroup.border",
    "tab.border",
    "contrastBorder",
  ],
  borderStrong: [
    "input.border",
    "dropdown.border",
    "checkbox.border",
    "contrastActiveBorder",
  ],
  text: ["editor.foreground", "foreground"],
  textDim: [
    "descriptionForeground",
    "disabledForeground",
    "editorLineNumber.foreground",
  ],
  // `focusBorder` is LAST on purpose, despite being the most obviously
  // accent-shaped name. Themes routinely set it to a muted separator colour —
  // One Dark Pro uses #3e4452, a grey — while their actual signature colour
  // lives in the link or the activity-bar badge. Leading with focusBorder
  // produced a dull grey accent for a theme whose whole identity is a blue.
  accent: [
    "textLink.foreground",
    "activityBarBadge.background",
    "button.background",
    "progressBar.background",
    "focusBorder",
  ],
  accentDim: ["button.background", "button.hoverBackground", "badge.background"],
  onAccent: ["button.foreground", "badge.foreground"],
  onAccentDim: ["button.foreground", "badge.foreground"],
  warn: [
    "editorWarning.foreground",
    "list.warningForeground",
    "notificationsWarningIcon.foreground",
    "editorOverviewRuler.warningForeground",
  ],
  error: [
    "editorError.foreground",
    "errorForeground",
    "list.errorForeground",
    "editorOverviewRuler.errorForeground",
  ],
  success: [
    "gitDecoration.addedResourceForeground",
    "charts.green",
    "terminal.ansiGreen",
    "debugIcon.startForeground",
  ],
};

/** Bancada's own palettes, used only as the last resort for a token no key
 *  supplied and no derivation could reach. */
const LAST_RESORT: Record<Appearance, Pick<ThemeColors, "warn" | "error" | "success" | "accent">> = {
  dark: { warn: "#fbbf24", error: "#f87373", success: "#4ade80", accent: "#2dd4bf" },
  light: { warn: "#825e02", error: "#c90a0a", success: "#157337", accent: "#177065" },
};

function lightness(hex: string): number {
  const p = parseHex(hex);
  return p ? rgbToHsl(p).l : 0;
}

/** Moves a colour's lightness by `delta`, clamped, keeping hue and saturation.
 *  Used to build a surface ladder when a theme defines only one background. */
function shift(hex: string, delta: number): string {
  const p = parseHex(hex);
  if (!p) return hex;
  const hsl = rgbToHsl(p);
  return toHex(hslToRgb({ ...hsl, l: Math.max(0, Math.min(1, hsl.l + delta)) }));
}

/** Blends two colours in linear proportion. */
function mix(a: string, b: string, t: number): string {
  const pa = parseHex(a);
  const pb = parseHex(b);
  if (!pa || !pb) return a;
  return toHex({
    r: pa.r + (pb.r - pa.r) * t,
    g: pa.g + (pb.g - pa.g) * t,
    b: pa.b + (pb.b - pa.b) * t,
    a: 1,
  });
}

/**
 * Reads one key chain, flattening any alpha against `over`.
 *
 * The alpha handling is not a nicety. VS Code themes use 8-digit hex
 * constantly — `list.hoverBackground: "#ffffff0a"` is an extremely common
 * idiom — and taking those at face value yields a near-white hover row on a
 * black editor. Compositing is what makes the value mean what it looks like.
 */
function pick(
  colors: Record<string, unknown>,
  chain: string[],
  over: string,
): string | null {
  for (const key of chain) {
    const raw = colors[key];
    if (typeof raw !== "string") continue;
    const parsed = parseHex(raw);
    if (!parsed) continue;
    if (parsed.a >= 1) return toHex(parsed);
    const base = parseHex(over);
    if (!base) continue;
    return toHex(composite(parsed, { ...base, a: 1 }));
  }
  return null;
}

/** Dark or light, from the theme's own declaration where it made one and from
 *  the actual editor background where it did not — `type` is optional, and
 *  some themes lie about it. */
export function appearanceOf(theme: VsCodeTheme, bg: string): Appearance {
  const t = (theme.type ?? "").toLowerCase();
  if (t === "light" || t === "hclight") return "light";
  if (t === "dark" || t === "hc" || t === "hcdark") return "dark";
  const p = parseHex(bg);
  return p && relativeLuminance({ ...p, a: 1 }) > 0.18 ? "light" : "dark";
}

/**
 * Maps a parsed VS Code theme onto Bancada's token set.
 *
 * Produces a complete, self-consistent palette for any input, including an
 * empty one — but makes no promise that the result is *legible*. Run it
 * through `repairTheme` before use; `importTheme` does both.
 */
export function mapVsCodeTheme(theme: VsCodeTheme, id: string): Theme {
  const colors = (theme.colors ?? {}) as Record<string, unknown>;

  // Background first: everything else composites against it and several
  // tokens are derived from it.
  const bg =
    pick(colors, KEYS.bg, "#000000") ??
    (appearanceOf(theme, "#1e1e1e") === "light" ? "#ffffff" : "#1e1e1e");
  const appearance = appearanceOf(theme, bg);
  const dark = appearance === "dark";
  // Surfaces step *away* from the page on dark themes and *toward* ink on
  // light ones, which is why the ladder's sign follows appearance.
  const dir = dark ? 1 : -1;
  const fallbacks = LAST_RESORT[appearance];

  const bgPanel = pick(colors, KEYS.bgPanel, bg) ?? shift(bg, dir * 0.03);
  const bgRaised = pick(colors, KEYS.bgRaised, bg) ?? shift(bg, dir * 0.06);
  const bgHover = pick(colors, KEYS.bgHover, bgPanel) ?? shift(bg, dir * 0.09);

  const text = pick(colors, KEYS.text, bg) ?? (dark ? "#d4d4d4" : "#1f1f1f");
  // A dimmed foreground is the theme's text walked toward its background —
  // the relationship every hand-made theme encodes anyway.
  //
  // The distinguishability check is not paranoia: One Dark Pro really does set
  // `descriptionForeground` to the same value as `editor.foreground`. Mapping
  // that faithfully is correct and useless — Bancada leans on --text-dim in 79
  // places to separate secondary text from primary, and a theme where the two
  // are identical erases that everywhere at once. So a theme that declines to
  // distinguish them gets a derived value instead of its own.
  const declaredDim = pick(colors, KEYS.textDim, bg);
  const textDim =
    declaredDim && contrastRatio(declaredDim, text) >= 1.15
      ? declaredDim
      : mix(text, bg, 0.38);

  const accent = pick(colors, KEYS.accent, bg) ?? fallbacks.accent;
  const accentDim = pick(colors, KEYS.accentDim, bg) ?? shift(accent, -0.12);

  // Borders: prefer the theme's, else sit them between the surfaces they
  // separate. The floors get enforced later, by repairTheme.
  const border = pick(colors, KEYS.border, bgPanel) ?? mix(bgPanel, text, 0.22);
  const borderStrong =
    pick(colors, KEYS.borderStrong, bgRaised) ?? mix(bgRaised, text, 0.38);

  // Foreground on the accent fills. A theme that named `button.foreground`
  // has answered; otherwise pick the end of the range further from the fill.
  const onAccentKey = pick(colors, KEYS.onAccent, accent);
  const onAccent = onAccentKey ?? (lightness(accent) > 0.5 ? "#000000" : "#ffffff");
  const onAccentDim =
    pick(colors, KEYS.onAccentDim, accentDim) ??
    (lightness(accentDim) > 0.5 ? "#000000" : "#ffffff");

  return {
    id,
    name: typeof theme.name === "string" && theme.name.trim() ? theme.name.trim() : id,
    appearance,
    colors: {
      bg,
      bgPanel,
      bgRaised,
      bgHover,
      border,
      borderStrong,
      text,
      textDim,
      accent,
      accentDim,
      onAccent,
      onAccentDim,
      warn: pick(colors, KEYS.warn, bg) ?? fallbacks.warn,
      error: pick(colors, KEYS.error, bg) ?? fallbacks.error,
      success: pick(colors, KEYS.success, bg) ?? fallbacks.success,
    },
  };
}
