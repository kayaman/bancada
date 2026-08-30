// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";

import { applyTokens, serialRowHeight, tokenDeclarations } from "../apply";
import { METRICS } from "../density";
import { BANCADA_DARK, BANCADA_LIGHT } from "../themes";
import { COLOR_VAR, FONT_VAR, SPACE_VAR } from "../tokens";

describe("tokenDeclarations", () => {
  it("emits every colour, font and space custom property", () => {
    const d = tokenDeclarations(BANCADA_DARK, "compact");
    for (const cssVar of Object.values(COLOR_VAR)) {
      expect(d, cssVar).toHaveProperty(cssVar);
    }
    for (const cssVar of Object.values(FONT_VAR)) {
      expect(d, cssVar).toHaveProperty(cssVar);
    }
    for (const cssVar of Object.values(SPACE_VAR)) {
      expect(d, cssVar).toHaveProperty(cssVar);
    }
    expect(d).toHaveProperty("--serial-row-h");
  });

  it("writes colours through unchanged", () => {
    const d = tokenDeclarations(BANCADA_LIGHT, "compact");
    expect(d["--bg"]).toBe(BANCADA_LIGHT.colors.bg);
    expect(d["--text-dim"]).toBe(BANCADA_LIGHT.colors.textDim);
  });

  it("stamps metrics with px units", () => {
    // The stylesheet uses these directly in `font-size:` and `padding:`, where
    // a bare number is invalid and silently drops the declaration.
    const d = tokenDeclarations(BANCADA_DARK, "compact");
    expect(d["--fs-small"]).toBe("11px");
    expect(d["--sp-4"]).toBe("8px");
    expect(d["--serial-row-h"]).toBe("18px");
  });

  it("changes only metrics when density moves, never colours", () => {
    const compact = tokenDeclarations(BANCADA_DARK, "compact");
    const comfy = tokenDeclarations(BANCADA_DARK, "comfortable");
    for (const cssVar of Object.values(COLOR_VAR)) {
      expect(comfy[cssVar], cssVar).toBe(compact[cssVar]);
    }
    expect(comfy["--fs-small"]).not.toBe(compact["--fs-small"]);
  });

  it("changes only colours when the theme moves, never metrics", () => {
    const dark = tokenDeclarations(BANCADA_DARK, "normal");
    const light = tokenDeclarations(BANCADA_LIGHT, "normal");
    for (const cssVar of [
      ...Object.values(FONT_VAR),
      ...Object.values(SPACE_VAR),
      "--serial-row-h",
    ]) {
      expect(light[cssVar], cssVar).toBe(dark[cssVar]);
    }
    expect(light["--bg"]).not.toBe(dark["--bg"]);
  });

  it("is pure — repeated calls give equal output and touch no DOM", () => {
    const before = document.documentElement.getAttribute("style");
    expect(tokenDeclarations(BANCADA_DARK, "normal")).toEqual(
      tokenDeclarations(BANCADA_DARK, "normal"),
    );
    expect(document.documentElement.getAttribute("style")).toBe(before);
  });
});

describe("serialRowHeight", () => {
  it("agrees with the custom property for the same density", () => {
    // These two are the pair that must never drift: the virtualiser computes
    // row offsets from this number while CSS paints rows at the property.
    for (const d of ["compact", "normal", "comfortable"] as const) {
      const css = tokenDeclarations(BANCADA_DARK, d)["--serial-row-h"];
      expect(`${serialRowHeight(d)}px`, d).toBe(css);
      expect(serialRowHeight(d)).toBe(METRICS[d].serialRow);
    }
  });
});

describe("applyTokens", () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement("div");
  });

  it("sets each declaration on the element", () => {
    applyTokens(BANCADA_DARK, "compact", root);
    expect(root.style.getPropertyValue("--bg")).toBe(BANCADA_DARK.colors.bg);
    expect(root.style.getPropertyValue("--fs-small")).toBe("11px");
  });

  it("sets color-scheme so native chrome follows the theme", () => {
    // Without this a light theme keeps dark scrollbars and dark <select>
    // popups, and the illusion breaks at the first dropdown.
    applyTokens(BANCADA_LIGHT, "compact", root);
    expect(root.style.getPropertyValue("color-scheme")).toBe("light");
    applyTokens(BANCADA_DARK, "compact", root);
    expect(root.style.getPropertyValue("color-scheme")).toBe("dark");
  });

  it("mirrors the selection onto data attributes for CSS to branch on", () => {
    applyTokens(BANCADA_LIGHT, "comfortable", root);
    expect(root.getAttribute("data-theme")).toBe("bancada-light");
    expect(root.getAttribute("data-appearance")).toBe("light");
    expect(root.getAttribute("data-density")).toBe("comfortable");
  });

  it("fully replaces the previous theme, leaving nothing behind", () => {
    // Switching themes must not leave one stale token from the old palette —
    // that is how you get light text on a light background in one corner.
    applyTokens(BANCADA_DARK, "compact", root);
    applyTokens(BANCADA_LIGHT, "compact", root);
    for (const [, cssVar] of Object.entries(COLOR_VAR)) {
      const v = root.style.getPropertyValue(cssVar);
      const stale = Object.values(BANCADA_DARK.colors).includes(v);
      const shared = Object.values(BANCADA_LIGHT.colors).includes(v);
      expect(stale && !shared, `${cssVar} still dark: ${v}`).toBe(false);
    }
  });

  it("defaults to the document root when no element is given", () => {
    applyTokens(BANCADA_DARK, "compact");
    expect(document.documentElement.style.getPropertyValue("--bg")).toBe(
      BANCADA_DARK.colors.bg,
    );
  });
});
