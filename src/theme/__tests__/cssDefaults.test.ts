import { describe, expect, it } from "vitest";

// Read the real stylesheet as text, the same way conflicts.test.ts reads
// App.tsx's own source. `?raw` rather than node:fs because @types/node is not
// a dependency of this project and `npm run build` runs `tsc --noEmit`.
import CSS from "../../styles.css?raw";

import { tokenDeclarations } from "../apply";
import { DEFAULT_DENSITY } from "../density";
import { BANCADA_DARK } from "../themes";

// `:root` in styles.css duplicates the default theme and density. It has to:
// those values paint the first frame, before React has mounted and had a
// chance to call applyTokens(). Duplication that nothing checks is duplication
// that drifts, and the failure mode is nasty and intermittent — a wrong colour
// visible only in the instant before hydration, or (worse) permanently, on
// whatever token the runtime writer forgets to set.
//
// So this test reads the real stylesheet and holds it to the TypeScript.

function rootBlock(css: string): string {
  const m = css.match(/^:root \{([\s\S]*?)^\}/m);
  if (!m) throw new Error("no :root block found in styles.css");
  return m[1];
}

function declaredVars(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of block.split("\n")) {
    const m = line.match(/^\s*(--[a-z0-9-]+):\s*([^;]+);/i);
    if (m) out[m[1]] = m[2].trim();
  }
  return out;
}

const declared = declaredVars(rootBlock(CSS));
const expected = tokenDeclarations(BANCADA_DARK, DEFAULT_DENSITY);

describe("styles.css :root defaults", () => {
  it("declares every token the runtime writer sets", () => {
    for (const name of Object.keys(expected)) {
      expect(declared, `styles.css :root is missing ${name}`).toHaveProperty(name);
    }
  });

  it("matches the default theme and density exactly", () => {
    for (const [name, value] of Object.entries(expected)) {
      expect(declared[name], `${name} drifted from theme/`).toBe(value);
    }
  });

  it("declares color-scheme so the first frame has dark native chrome", () => {
    expect(rootBlock(CSS)).toMatch(/color-scheme:\s*dark;/);
  });
});

describe("styles.css tokenisation", () => {
  it("has no hardcoded font sizes left", () => {
    // 0.85em survives on purpose: it is relative and already scales.
    const literals = CSS.match(/font-size:\s*\d+px/g) ?? [];
    expect(literals).toEqual([]);
  });

  it("does not pin the serial row height outside :root", () => {
    // `.serial-monitor { --serial-row-h: 18px }` would win over :root for the
    // monitor's whole subtree and silently defeat density there — while
    // ROW_HEIGHT in virtualize.ts moved, putting paint and scroll maths out of
    // register.
    const all = CSS.match(/--serial-row-h:/g) ?? [];
    expect(all).toHaveLength(1);
    expect(rootBlock(CSS)).toMatch(/--serial-row-h:/);
  });

  it("keeps the device-browser frame's white ground literal", () => {
    // The one intentional hardcoded colour: an iframe showing a device's own
    // web page, which assumes a light ground and is not ours to theme.
    expect(CSS).toMatch(/\.devweb-frame\s*\{[^}]*background:\s*#fff;/);
  });

  it("routes every translucent wash through a token", () => {
    // The five hardcoded rgba() washes were the dark theme's red and green,
    // frozen. Shadows are exempt: black at low alpha is correct on both light
    // and dark grounds.
    const washes = (CSS.match(/rgba\([^)]*\)/g) ?? []).filter(
      (w: string) => !/rgba\(0,\s*0,\s*0/.test(w),
    );
    const outsideRoot = washes.filter((w: string) => !rootBlock(CSS).includes(w));
    expect(outsideRoot).toEqual([]);
  });
});
