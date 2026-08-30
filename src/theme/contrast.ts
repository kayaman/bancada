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
