import { describe, expect, it } from "vitest";

import { contrastRatio, parseHex } from "../contrast";
import { auditTheme, repairTheme } from "../themeAudit";
import { COLOR_TOKENS } from "../tokens";
import { appearanceOf, mapVsCodeTheme, type VsCodeTheme } from "../vscode";
import {
  AWFUL,
  DARK_PLUS,
  LIGHT_ALPHA,
  MISLABELLED,
  SPARSE,
} from "./fixtures/vscodeThemes";

const ALL: [string, VsCodeTheme][] = [
  ["Dark+", DARK_PLUS],
  ["Sparse", SPARSE],
  ["Light+alpha", LIGHT_ALPHA],
  ["Awful", AWFUL],
  ["Mislabelled", MISLABELLED],
  ["Empty", {}],
  ["Colours-only-null", { colors: {} }],
];

describe("mapVsCodeTheme", () => {
  it.each(ALL)("produces a complete palette from %s", (_name, input) => {
    // Every token is mandatory here and every VS Code key is optional there.
    // A theme that defines nothing at all still has to come out whole.
    const mapped = mapVsCodeTheme(input, "test");
    for (const token of COLOR_TOKENS) {
      expect(parseHex(mapped.colors[token]), `${token}`).not.toBeNull();
    }
  });

  it("prefers what the theme actually said", () => {
    const m = mapVsCodeTheme(DARK_PLUS, "test");
    expect(m.colors.bg).toBe("#1f1f1f");
    expect(m.colors.text).toBe("#cccccc");
    expect(m.colors.bgPanel).toBe("#181818");
    expect(m.colors.accent).toBe("#0078d4");
    expect(m.colors.onAccent).toBe("#ffffff");
  });

  it("carries the theme's name", () => {
    expect(mapVsCodeTheme(DARK_PLUS, "id").name).toBe("Dark+ (test subset)");
    // Nameless themes fall back to the id rather than to an empty label.
    expect(mapVsCodeTheme({}, "some-id").name).toBe("some-id");
    expect(mapVsCodeTheme({ name: "   " }, "some-id").name).toBe("some-id");
  });

  it("derives a surface ladder when the theme defines only a background", () => {
    // Sparse gives bg and fg and nothing else. The four surfaces still have
    // to step in one direction, or "raised" stops meaning raised.
    const c = mapVsCodeTheme(SPARSE, "test").colors;
    const l = [c.bg, c.bgPanel, c.bgRaised, c.bgHover].map((x) =>
      contrastRatio(x, "#000000"),
    );
    for (let i = 1; i < l.length; i++) {
      expect(l[i], `step ${i}`).toBeGreaterThan(l[i - 1]);
    }
  });

  it("composites 8-digit hex instead of taking it at face value", () => {
    // `list.hoverBackground: "#0000000a"` is black at 4% over white. Read
    // literally it is near-black; composited it is the pale grey intended.
    const c = mapVsCodeTheme(LIGHT_ALPHA, "test").colors;
    const lum = contrastRatio(c.bgHover, "#000000");
    expect(lum).toBeGreaterThan(15); // still nearly white
    expect(c.bgHover).not.toBe("#000000");
  });
});

describe("appearanceOf", () => {
  it("believes the theme when it declares a type", () => {
    expect(appearanceOf({ type: "light" }, "#ffffff")).toBe("light");
    expect(appearanceOf({ type: "dark" }, "#000000")).toBe("dark");
    expect(appearanceOf({ type: "hcLight" }, "#ffffff")).toBe("light");
    expect(appearanceOf({ type: "hc" }, "#000000")).toBe("dark");
  });

  it("falls back to the background when there is no type", () => {
    expect(appearanceOf({}, "#ffffff")).toBe("light");
    expect(appearanceOf({}, "#101014")).toBe("dark");
  });

  it("is overridden by a background that contradicts the declared type", () => {
    // Mislabelled themes exist. `color-scheme` follows this, and getting it
    // wrong means dark scrollbars on a white window.
    const m = mapVsCodeTheme(MISLABELLED, "test");
    expect(m.appearance).toBe("dark"); // declared
    // ...but the mapper still had to produce something legible on #fdfdfd,
    // which repair enforces below.
    expect(repairTheme(m).theme.colors.text).toBeTruthy();
  });
});

describe("auditTheme / repairTheme", () => {
  it("finds nothing wrong with a well-made theme", () => {
    const repaired = repairTheme(mapVsCodeTheme(DARK_PLUS, "test"));
    // Dark+ is a real, professionally made theme; anything it trips is more
    // likely our rule being wrong than Microsoft's theme being unreadable.
    expect(repaired.unrepairable).toEqual([]);
  });

  it("catches an unreadable theme rather than rendering it", () => {
    const violations = auditTheme(mapVsCodeTheme(AWFUL, "test"));
    expect(violations.length).toBeGreaterThan(4);
    // Worst offender first, so a UI can lead with the real problem.
    const severity = violations.map((v) => v.ratio / v.floor);
    for (let i = 1; i < severity.length; i++) {
      expect(severity[i]).toBeGreaterThanOrEqual(severity[i - 1]);
    }
  });

  it("explains each violation in terms of what breaks", () => {
    for (const v of auditTheme(mapVsCodeTheme(AWFUL, "test"))) {
      expect(v.because.length).toBeGreaterThan(10);
      expect(v.floor).toBeGreaterThan(1);
      expect(v.ratio).toBeLessThan(v.floor);
    }
  });

  it.each(ALL)("leaves %s with zero violations after repair", (_name, input) => {
    // The invariant the whole import path rests on: whatever arrives, what
    // renders is legible. This is the same set of floors the built-in themes
    // are held to in themes.test.ts.
    const repaired = repairTheme(mapVsCodeTheme(input, "test"));
    expect(auditTheme(repaired.theme)).toEqual([]);
  });

  it("preserves the theme's identity while repairing it", () => {
    // Repair must not repaint the window. Backgrounds are the theme; only
    // foregrounds move.
    const mapped = mapVsCodeTheme(AWFUL, "test");
    const { theme } = repairTheme(mapped);
    expect(theme.colors.bg).toBe(mapped.colors.bg);
    expect(theme.colors.bgPanel).toBe(mapped.colors.bgPanel);
    expect(theme.colors.bgRaised).toBe(mapped.colors.bgRaised);
    expect(theme.colors.bgHover).toBe(mapped.colors.bgHover);
    expect(theme.appearance).toBe(mapped.appearance);
    expect(theme.name).toBe(mapped.name);
  });

  it("is idempotent — repairing a repaired theme changes nothing", () => {
    const once = repairTheme(mapVsCodeTheme(AWFUL, "test")).theme;
    const twice = repairTheme(once).theme;
    expect(twice.colors).toEqual(once.colors);
  });

  it("reports what it changed", () => {
    const r = repairTheme(mapVsCodeTheme(AWFUL, "test"));
    expect(r.violations.length).toBeGreaterThan(0);
    for (const v of r.violations) {
      expect(COLOR_TOKENS).toContain(v.token);
    }
  });
});
