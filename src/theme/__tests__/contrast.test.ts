import { describe, expect, it } from "vitest";

import {
  composite,
  contrastRatio,
  meetsFloor,
  parseHex,
  relativeLuminance,
} from "../contrast";

describe("parseHex", () => {
  it("reads the four hex forms a real theme file uses", () => {
    expect(parseHex("#fff")).toEqual({ r: 255, g: 255, b: 255, a: 1 });
    expect(parseHex("#16181d")).toEqual({ r: 22, g: 24, b: 29, a: 1 });
    // 8-digit: VS Code themes use it constantly, so dropping it would quietly
    // discard a large slice of every imported theme.
    expect(parseHex("#00000080")).toEqual({ r: 0, g: 0, b: 0, a: 128 / 255 });
    expect(parseHex("#f00f")).toEqual({ r: 255, g: 0, b: 0, a: 1 });
  });

  it("tolerates a missing hash and surrounding space", () => {
    expect(parseHex("  16181d ")).toEqual({ r: 22, g: 24, b: 29, a: 1 });
  });

  it("returns null rather than guessing at junk", () => {
    expect(parseHex("rebeccapurple")).toBeNull();
    expect(parseHex("#12345")).toBeNull();
    expect(parseHex("")).toBeNull();
    expect(parseHex("#xyzxyz")).toBeNull();
  });
});

describe("relativeLuminance", () => {
  it("anchors at the two ends of the range", () => {
    expect(relativeLuminance({ r: 0, g: 0, b: 0, a: 1 })).toBe(0);
    expect(relativeLuminance({ r: 255, g: 255, b: 255, a: 1 })).toBeCloseTo(1, 10);
  });
});

describe("composite", () => {
  it("flattens a half-alpha white onto black to mid grey", () => {
    const out = composite(
      { r: 255, g: 255, b: 255, a: 0.5 },
      { r: 0, g: 0, b: 0, a: 1 },
    );
    expect(out).toEqual({ r: 127.5, g: 127.5, b: 127.5, a: 1 });
  });
});

describe("contrastRatio", () => {
  it("spans the full 1..21 range", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 10);
    expect(contrastRatio("#2dd4bf", "#2dd4bf")).toBeCloseTo(1, 10);
  });

  it("is symmetric — order of the pair does not change the ratio", () => {
    const a = contrastRatio("#d7dce4", "#16181d");
    const b = contrastRatio("#16181d", "#d7dce4");
    expect(a).toBeCloseTo(b, 10);
  });

  it("reproduces the audit's measurements of the pre-token palette", () => {
    // These are the numbers that motivated the whole token layer. If the maths
    // ever drifts, these drift with it and the floors below stop meaning
    // anything.
    expect(contrastRatio("#d7dce4", "#16181d")).toBeCloseTo(12.89, 1);
    expect(contrastRatio("#8b93a1", "#2e3542")).toBeCloseTo(3.98, 1);
    expect(contrastRatio("#303743", "#1e222a")).toBeCloseTo(1.33, 1);
  });

  it("accounts for alpha instead of reading the hex at face value", () => {
    // Opaque white on black is 21:1; the same white at 20% alpha is not.
    const opaque = contrastRatio("#ffffff", "#000000");
    const faint = contrastRatio("#ffffff33", "#000000");
    expect(opaque).toBeCloseTo(21, 10);
    expect(faint).toBeLessThan(3);
  });

  it("reports unparseable colours as failing rather than throwing", () => {
    expect(contrastRatio("not-a-colour", "#ffffff")).toBe(1);
    expect(contrastRatio("#ffffff", "nope")).toBe(1);
    expect(meetsFloor(contrastRatio("junk", "#fff"), "ui")).toBe(false);
  });
});

describe("meetsFloor", () => {
  it("applies the WCAG threshold for each role", () => {
    expect(meetsFloor(4.5, "text")).toBe(true);
    expect(meetsFloor(4.49, "text")).toBe(false);
    expect(meetsFloor(3, "ui")).toBe(true);
    expect(meetsFloor(3, "large-text")).toBe(true);
    // 3:1 is fine for a border and not for body text — the distinction is the
    // entire point of the role.
    expect(meetsFloor(3, "text")).toBe(false);
  });

  it("does not fail a ratio on a digit nobody can see", () => {
    expect(meetsFloor(4.499, "text")).toBe(true);
  });
});
