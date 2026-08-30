// The token vocabulary: the complete set of names the UI is allowed to render
// through, and the CSS custom properties they land on.
//
// Everything downstream is a consumer of this file. A built-in theme is a
// `ThemeColors`. A density level is a `Metrics`. An imported VS Code theme is
// ~600 arbitrary keys mapped down onto `ThemeColors`. Keeping the vocabulary
// small and closed is what makes that last one tractable at all — the mapping
// problem is "fill 13 slots", not "translate a foreign design system".
//
// The CSS names are spelled out rather than derived from the TS names. They
// are load-bearing (3200 lines of stylesheet reference them) and a clever
// camelCase→kebab transform would turn a rename into a silent no-paint.

/** Semantic colour slots. Adding one means adding it to every built-in theme
 *  and to the VS Code mapping — that friction is deliberate. */
export type ColorToken =
  | "bg"
  | "bgPanel"
  | "bgRaised"
  | "bgHover"
  | "border"
  | "borderStrong"
  | "text"
  | "textDim"
  | "accent"
  | "accentDim"
  // Text drawn *on* an accent fill. Two of them because the accent has two
  // fills at different lightnesses (a primary button and its hover), and the
  // legible foreground flips between them: Bancada Dark puts near-black on its
  // bright teal and white on the dim one. Deriving this instead of authoring
  // it was tempting and wrong — "pick black or white by luminance" produces
  // 4.4:1 on mid-tone accents, which is exactly where it matters.
  | "onAccent"
  | "onAccentDim"
  | "warn"
  | "error"
  | "success";

export type ThemeColors = Record<ColorToken, string>;

export const COLOR_VAR: Record<ColorToken, string> = {
  bg: "--bg",
  bgPanel: "--bg-panel",
  bgRaised: "--bg-raised",
  bgHover: "--bg-hover",
  border: "--border",
  borderStrong: "--border-strong",
  text: "--text",
  textDim: "--text-dim",
  accent: "--accent",
  accentDim: "--accent-dim",
  onAccent: "--on-accent",
  onAccentDim: "--on-accent-dim",
  warn: "--warn",
  error: "--error",
  success: "--success",
};

export const COLOR_TOKENS = Object.keys(COLOR_VAR) as ColorToken[];

/** Tokens computed from a theme rather than authored in one.
 *
 *  The stylesheet needs translucent washes behind failed rows, danger buttons
 *  and diff lines. Those were hardcoded as `rgba(248, 113, 113, 0.08)` — the
 *  dark theme's red, frozen, in five places. On a light theme that is a pink
 *  smear under dark-red text. Deriving them from `error`/`success` means a
 *  theme author (or an imported `.vsix`) never has to think about tints, and
 *  they can never disagree with the colour they are supposed to be tinting. */
export const DERIVED_VAR = {
  errorTint: "--error-tint",
  errorTintStrong: "--error-tint-strong",
  successTint: "--success-tint",
} as const;

/** Alphas the tints are mixed at, matching what the stylesheet already used.
 *
 *  `errorTintStrong` is 0.15 rather than the 0.14 one of its two call sites
 *  had. The other site — the deleted line in an agent diff — was already 0.15,
 *  and its companion `successTint` (the added line) still is. Collapsing the
 *  pair onto 0.14 would have left added and deleted rows at visibly different
 *  weights; the 0.01 the danger-tab hover gains instead is beneath perception. */
export const TINT_ALPHA = {
  errorTint: 0.08,
  errorTintStrong: 0.15,
  successTint: 0.15,
} as const;

/** Type scale steps, named by role rather than by size — the whole point is
 *  that `small` is 11px at one density and 13px at another.
 *
 *  Six steps because the stylesheet independently converged on six sizes
 *  (9/10/11/12/13/14px across 110 declarations). This is not a fresh scale
 *  imposed on the app; it is the scale the app already had, given names. */
export type FontToken = "micro" | "tiny" | "small" | "body" | "lg" | "xl";

export const FONT_VAR: Record<FontToken, string> = {
  micro: "--fs-micro",
  tiny: "--fs-tiny",
  small: "--fs-small",
  body: "--fs-body",
  lg: "--fs-lg",
  xl: "--fs-xl",
};

export const FONT_TOKENS = Object.keys(FONT_VAR) as FontToken[];

/** Spacing steps. Seven, matching the values the stylesheet actually leans on
 *  (2/4/6/8/10/12/16px account for the overwhelming majority of its padding,
 *  gap and margin declarations). */
export type SpaceToken = "s1" | "s2" | "s3" | "s4" | "s5" | "s6" | "s7";

export const SPACE_VAR: Record<SpaceToken, string> = {
  s1: "--sp-1",
  s2: "--sp-2",
  s3: "--sp-3",
  s4: "--sp-4",
  s5: "--sp-5",
  s6: "--sp-6",
  s7: "--sp-7",
};

export const SPACE_TOKENS = Object.keys(SPACE_VAR) as SpaceToken[];

/** The metric half of a render: sizes, not colours. Swapped by density. */
export type Metrics = {
  font: Record<FontToken, number>;
  space: Record<SpaceToken, number>;
  /** Serial-monitor row height. It is a token because the monitor virtualises
   *  its rows off a fixed row height — if type grows and this does not, the
   *  text clips inside its own row. */
  serialRow: number;
};

export type Appearance = "dark" | "light";

export type Theme = {
  id: string;
  name: string;
  appearance: Appearance;
  colors: ThemeColors;
};
