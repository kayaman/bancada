// WCAG contrast math.
//
// This exists because the audit found `--border` sitting at 1.33:1 against
// `--bg-panel` — every separator in the app was technically drawn and
// practically invisible. Numbers, not eyeballs, are the only way to keep that
// from happening again, and once themes can be *imported* it stops being a
// one-off cleanup: a marketplace theme has never been contrast-audited by
// anyone, so the floors below are what stand between a pretty `.vsix` and an
// unreadable bench tool at 3am.
//
// Pure and dependency-free on purpose — the same functions guard the built-in
// themes in tests and vet imported ones at runtime.

/** A colour parsed to 0-255 channels plus alpha 0-1. */
export type Rgba = { r: number; g: number; b: number; a: number };

/** Parses `#rgb`, `#rgba`, `#rrggbb` and `#rrggbbaa`.
 *
 *  The 8-digit form matters more than it looks: VS Code themes lean on it
 *  heavily (`editorIndentGuide.background: "#404040aa"`), so a parser that
 *  only speaks 6 digits silently drops a large slice of every real theme. */
export function parseHex(input: string): Rgba | null {
  const h = input.trim().replace(/^#/, "");
  if (!/^[0-9a-fA-F]+$/.test(h)) return null;
  const dup = (c: string) => parseInt(c + c, 16);
  const pair = (i: number) => parseInt(h.slice(i, i + 2), 16);
  switch (h.length) {
    case 3:
      return { r: dup(h[0]), g: dup(h[1]), b: dup(h[2]), a: 1 };
    case 4:
      return { r: dup(h[0]), g: dup(h[1]), b: dup(h[2]), a: dup(h[3]) / 255 };
    case 6:
      return { r: pair(0), g: pair(2), b: pair(4), a: 1 };
    case 8:
      return { r: pair(0), g: pair(2), b: pair(4), a: pair(6) / 255 };
    default:
      return null;
  }
}

/** Flattens a translucent colour onto an opaque backdrop.
 *
 *  Contrast is only meaningful between things the eye actually sees, and a
 *  50%-alpha border over a dark panel is not the colour its hex claims. */
export function composite(fg: Rgba, bg: Rgba): Rgba {
  const mix = (f: number, b: number) => f * fg.a + b * (1 - fg.a);
  return { r: mix(fg.r, bg.r), g: mix(fg.g, bg.g), b: mix(fg.b, bg.b), a: 1 };
}

/** WCAG 2.x relative luminance. */
export function relativeLuminance(c: Rgba): number {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
}

/** Contrast ratio, 1..21.
 *
 *  Returns 1 — the "no contrast at all" floor, which fails every threshold —
 *  for unparseable input rather than throwing. A malformed colour in an
 *  imported theme should be *reported as failing*, not abort the import. */
export function contrastRatio(fg: string, bg: string): number {
  const b = parseHex(bg);
  const f = parseHex(fg);
  if (!f || !b) return 1;
  const opaqueBg = { ...b, a: 1 };
  const lf = relativeLuminance(f.a < 1 ? composite(f, opaqueBg) : f);
  const lb = relativeLuminance(opaqueBg);
  const hi = Math.max(lf, lb);
  const lo = Math.min(lf, lb);
  return (hi + 0.05) / (lo + 0.05);
}

// ---------- colour space ----------
//
// HSL, purely so a colour can be moved lighter or darker without moving its
// hue. Every derivation and every repair in this module works that way: an
// imported theme's teal that fails a contrast floor should come back a
// legible teal, not a legible grey. Nudging RGB channels or blending toward
// white does not preserve hue; changing L does.

export type Hsl = { h: number; s: number; l: number };

export function rgbToHsl(c: Rgba): Hsl {
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return { h: h / 6, s, l };
}

export function hslToRgb({ h, s, l }: Hsl): Rgba {
  if (s === 0) {
    const v = Math.round(l * 255);
    return { r: v, g: v, b: v, a: 1 };
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const chan = (t: number) => {
    let x = t;
    if (x < 0) x += 1;
    if (x > 1) x -= 1;
    if (x < 1 / 6) return p + (q - p) * 6 * x;
    if (x < 1 / 2) return q;
    if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
    return p;
  };
  return {
    r: Math.round(chan(h + 1 / 3) * 255),
    g: Math.round(chan(h) * 255),
    b: Math.round(chan(h - 1 / 3) * 255),
    a: 1,
  };
}

export function toHex(c: Rgba): string {
  const p = (v: number) =>
    Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0");
  return `#${p(c.r)}${p(c.g)}${p(c.b)}`;
}

/** Moves `color` along its own lightness axis until it clears `floor` against
 *  every colour in `against`, and returns it — or `null` if no lightness does.
 *
 *  Direction is chosen by where the room is: against dark backgrounds it walks
 *  up, against light ones down. A pair with backgrounds at both extremes (a
 *  foreground that must work on both black and white) genuinely has no answer
 *  at some floors, and saying so beats returning a colour that fails quietly.
 *
 *  Steps in 1/1000 of lightness, taking the FIRST value that passes — the
 *  minimum change that does the job, so an imported theme is altered as little
 *  as the floor allows. */
export function adjustToFloor(
  color: string,
  against: string[],
  floor: number,
): string | null {
  const base = parseHex(color);
  if (!base || against.length === 0) return null;
  const clears = (hex: string) =>
    against.every((bg) => contrastRatio(hex, bg) >= floor);
  if (clears(color)) return color;

  const hsl = rgbToHsl(base);
  // Which way is there more contrast to gain? Compare the mean background
  // luminance against mid-grey.
  const meanBg =
    against.reduce((sum, bg) => {
      const p = parseHex(bg);
      return sum + (p ? relativeLuminance({ ...p, a: 1 }) : 0);
    }, 0) / against.length;
  const dirs = meanBg < 0.18 ? [1, -1] : [-1, 1];

  for (const dir of dirs) {
    for (let step = 1; step <= 1000; step++) {
      const l = hsl.l + dir * step * 0.001;
      if (l < 0 || l > 1) break;
      const cand = toHex(hslToRgb({ ...hsl, l }));
      if (clears(cand)) return cand;
    }
  }
  return null;
}

/** Renders a hex colour as `rgba(...)` at the given alpha.
 *
 *  Used to derive the translucent tints from a theme's own error/success
 *  colours. `rgba()` rather than `color-mix()` on purpose: this runs inside
 *  WebKitGTK, whose version is whatever the user's distro shipped, and a
 *  `color-mix()` that fails to parse takes the whole declaration with it —
 *  the wash would simply not paint, silently, on exactly the older systems
 *  least likely to be tested against. */
export function withAlpha(hex: string, alpha: number): string {
  const c = parseHex(hex);
  if (!c) return "transparent";
  const a = Math.max(0, Math.min(1, alpha)) * c.a;
  const r = Math.round(c.r);
  const g = Math.round(c.g);
  const b = Math.round(c.b);
  return `rgba(${r}, ${g}, ${b}, ${Number(a.toFixed(4))})`;
}

/** What a colour pair is *for*, which is what decides its floor.
 *
 *  `ui` is WCAG 1.4.11 (non-text contrast): borders, focus rings, the edge of
 *  a control. It is the rule the pre-token palette broke. */
export type ContrastRole = "text" | "large-text" | "ui";

export const CONTRAST_FLOOR: Record<ContrastRole, number> = {
  text: 4.5,
  "large-text": 3,
  ui: 3,
};

export function meetsFloor(ratio: number, role: ContrastRole): boolean {
  // Round to 2dp first: a 4.499 that reports as "4.50" should not fail on a
  // digit nobody can see.
  return Math.round(ratio * 100) / 100 >= CONTRAST_FLOOR[role];
}
