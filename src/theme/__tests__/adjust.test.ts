import { describe, expect, it } from "vitest";

import {
  adjustToFloor,
  contrastRatio,
  hslToRgb,
  parseHex,
  rgbToHsl,
  toHex,
} from "../contrast";

describe("hsl round trip", () => {
  it("survives a round trip within a rounding step", () => {
    for (const hex of [
      "#2dd4bf",
      "#16181d",
      "#ffffff",
      "#000000",
      "#f87373",
      "#825e02",
      "#7f7f7f",
    ]) {
      const back = toHex(hslToRgb(rgbToHsl(parseHex(hex)!)));
      const a = parseHex(hex)!;
      const b = parseHex(back)!;
      for (const ch of ["r", "g", "b"] as const) {
        expect(Math.abs(a[ch] - b[ch]), `${hex} ${ch}`).toBeLessThanOrEqual(1);
      }
    }
  });

  it("reports greys as having no saturation", () => {
    expect(rgbToHsl(parseHex("#808080")!).s).toBe(0);
  });
});

describe("adjustToFloor", () => {
  it("returns the colour untouched when it already clears", () => {
    // An imported theme should be altered as little as the floor demands, and
    // not at all when it demands nothing.
    expect(adjustToFloor("#ffffff", ["#000000"], 4.5)).toBe("#ffffff");
  });

  it("lifts a failing colour until it just clears", () => {
    const out = adjustToFloor("#303743", ["#1e222a"], 3)!;
    expect(out).not.toBeNull();
    expect(contrastRatio(out, "#1e222a")).toBeGreaterThanOrEqual(3);
  });

  it("takes the minimum change that works", () => {
    // One step darker must still fail, or we moved further than needed.
    const bg = "#1e222a";
    const out = adjustToFloor("#303743", [bg], 3)!;
    const hsl = rgbToHsl(parseHex(out)!);
    const oneLess = toHex(hslToRgb({ ...hsl, l: hsl.l - 0.002 }));
    expect(contrastRatio(oneLess, bg)).toBeLessThan(3);
  });

  it("preserves hue — a failing teal comes back a teal, not a grey", () => {
    // This is the whole reason the adjustment goes through HSL. A theme
    // author's accent surviving as a legible version of itself is the
    // difference between repairing a theme and flattening it.
    const before = rgbToHsl(parseHex("#14867a")!);
    const out = adjustToFloor("#14867a", ["#1e222a"], 6)!;
    const after = rgbToHsl(parseHex(out)!);
    expect(Math.abs(after.h - before.h)).toBeLessThan(0.02);
    expect(after.s).toBeGreaterThan(0.3);
  });

  it("darkens instead of lightening against light backgrounds", () => {
    const out = adjustToFloor("#dddddd", ["#ffffff"], 4.5)!;
    expect(rgbToHsl(parseHex(out)!).l).toBeLessThan(rgbToHsl(parseHex("#dddddd")!).l);
    expect(contrastRatio(out, "#ffffff")).toBeGreaterThanOrEqual(4.5);
  });

  it("satisfies every background at once, not just the easiest", () => {
    const bgs = ["#16181d", "#1e222a", "#262b35", "#2e3542"];
    const out = adjustToFloor("#8b93a1", bgs, 4.5)!;
    for (const bg of bgs) {
      expect(contrastRatio(out, bg), bg).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("returns null when no lightness can satisfy the demand", () => {
    // Balancing a foreground between black and white caps out around 4.58:1
    // — the lightness where (L+0.05)/0.05 and 1.05/(L+0.05) meet. So 4.5 is
    // satisfiable (and the function finds it), and 7 is arithmetically not.
    // Saying so beats handing back something that fails quietly.
    expect(adjustToFloor("#888888", ["#000000", "#ffffff"], 4.5)).not.toBeNull();
    expect(adjustToFloor("#888888", ["#000000", "#ffffff"], 7)).toBeNull();
  });

  it("returns null rather than throwing on unparseable input", () => {
    expect(adjustToFloor("not-a-colour", ["#000000"], 4.5)).toBeNull();
    expect(adjustToFloor("#ffffff", [], 4.5)).toBeNull();
  });
});
