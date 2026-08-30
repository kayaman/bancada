import { describe, expect, it } from "vitest";

import { contrastRatio, parseHex } from "../contrast";
import { BUILTIN_THEMES, DEFAULT_THEME_ID, themeById } from "../themes";
import { COLOR_TOKENS, type Theme } from "../tokens";

/** The surfaces any given foreground can end up sitting on. */
const surfaces = (t: Theme) => [
  ["bg", t.colors.bg],
  ["bgPanel", t.colors.bgPanel],
  ["bgRaised", t.colors.bgRaised],
  ["bgHover", t.colors.bgHover],
];

describe.each(BUILTIN_THEMES.map((t) => [t.id, t] as const))(
  "built-in theme: %s",
  (_id, theme) => {
    it("defines every token in the vocabulary", () => {
      for (const token of COLOR_TOKENS) {
        expect(theme.colors[token], `missing ${token}`).toBeTruthy();
      }
      expect(Object.keys(theme.colors).sort()).toEqual([...COLOR_TOKENS].sort());
    });

    it("uses only parseable colours", () => {
      for (const token of COLOR_TOKENS) {
        expect(parseHex(theme.colors[token]), `${token}`).not.toBeNull();
      }
    });

    // --- text floors: WCAG AA, 4.5:1 -------------------------------------

    it("keeps body text legible on every surface", () => {
      for (const [name, bg] of surfaces(theme)) {
        const r = contrastRatio(theme.colors.text, bg);
        expect(r, `text on ${name} = ${r.toFixed(2)}`).toBeGreaterThanOrEqual(4.5);
      }
    });

    it("keeps dimmed text legible on every surface", () => {
      // The pre-token palette failed exactly here: #8b93a1 on --bg-hover was
      // 3.98:1, across 79 usages of the token.
      for (const [name, bg] of surfaces(theme)) {
        const r = contrastRatio(theme.colors.textDim, bg);
        expect(r, `textDim on ${name} = ${r.toFixed(2)}`).toBeGreaterThanOrEqual(4.5);
      }
    });

    it("keeps status colours legible as text on every surface", () => {
      for (const token of ["accent", "warn", "error", "success"] as const) {
        for (const [name, bg] of surfaces(theme)) {
          const r = contrastRatio(theme.colors[token], bg);
          expect(r, `${token} on ${name} = ${r.toFixed(2)}`).toBeGreaterThanOrEqual(4.5);
        }
      }
    });

    // --- non-text floors: WCAG 1.4.11 ------------------------------------

    it("draws dividers that can actually be seen", () => {
      // Not 3:1 — a divider is not a control, and holding all 54 borders to
      // the control floor produced loud rules through the whole window. 2:1 is
      // the legibility floor this project sets for itself; the pre-token value
      // was 1.33:1.
      const r = contrastRatio(theme.colors.border, theme.colors.bgPanel);
      expect(r, `border on bgPanel = ${r.toFixed(2)}`).toBeGreaterThanOrEqual(2);
    });

    it("draws control edges to the WCAG non-text floor", () => {
      const r = contrastRatio(theme.colors.borderStrong, theme.colors.bgRaised);
      expect(r, `borderStrong on bgRaised = ${r.toFixed(2)}`).toBeGreaterThanOrEqual(3);
    });

    it("keeps button text legible on both accent fills", () => {
      // The pre-token palette failed here too, and on the most important
      // control in the window: `.btn.primary` drew #fff on #14867a at 4.45:1.
      const pairs = [
        ["onAccent", theme.colors.onAccent, theme.colors.accent],
        ["onAccentDim", theme.colors.onAccentDim, theme.colors.accentDim],
      ] as const;
      for (const [name, fg, bg] of pairs) {
        const r = contrastRatio(fg, bg);
        expect(r, `${name} on its fill = ${r.toFixed(2)}`).toBeGreaterThanOrEqual(4.5);
      }
    });

    it("keeps the accent fill distinguishable where it is used as a fill", () => {
      // accentDim is never text — only a fill or an accented edge — so it
      // answers to the 3:1 UI floor.
      const r = contrastRatio(theme.colors.accentDim, theme.colors.bgPanel);
      expect(r, `accentDim on bgPanel = ${r.toFixed(2)}`).toBeGreaterThanOrEqual(3);
    });

    it("orders its surfaces monotonically", () => {
      // bg -> bgPanel -> bgRaised -> bgHover must step consistently in one
      // direction, or "raised" stops meaning raised and hover states read as
      // depressions.
      const [, ...rest] = surfaces(theme);
      const lums = surfaces(theme).map(([, c]) => contrastRatio(c, "#000000"));
      const rising = lums.every((v, i) => i === 0 || v >= lums[i - 1]);
      const falling = lums.every((v, i) => i === 0 || v <= lums[i - 1]);
      expect(rising || falling, `surface ladder: ${lums.map((l) => l.toFixed(2))}`).toBe(true);
      expect(rest).toHaveLength(3);
    });

    it("declares an appearance matching its actual lightness", () => {
      const bgIsDark = contrastRatio(theme.colors.bg, "#ffffff") > 4.5;
      expect(theme.appearance).toBe(bgIsDark ? "dark" : "light");
    });
  },
);

describe("theme lookup", () => {
  it("has unique ids", () => {
    const ids = BUILTIN_THEMES.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("resolves a known id", () => {
    expect(themeById("bancada-light").id).toBe("bancada-light");
  });

  it("falls back to the default rather than returning nothing", () => {
    // Settings are user-editable JSON and themes can be removed; an unknown id
    // must never leave the UI with no palette at all.
    expect(themeById("deleted-theme").id).toBe(DEFAULT_THEME_ID);
    expect(themeById(null).id).toBe(DEFAULT_THEME_ID);
    expect(themeById(undefined).id).toBe(DEFAULT_THEME_ID);
  });

  it("ships both a dark and a light option", () => {
    const looks = new Set(BUILTIN_THEMES.map((t) => t.appearance));
    expect(looks).toContain("dark");
    expect(looks).toContain("light");
  });
});
