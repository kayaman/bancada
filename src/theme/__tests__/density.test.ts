import { describe, expect, it } from "vitest";

import {
  DENSITIES,
  DENSITY_LABEL,
  DEFAULT_DENSITY,
  METRICS,
  isDensity,
  metricsFor,
} from "../density";
import { FONT_TOKENS, SPACE_TOKENS } from "../tokens";

describe("density metrics", () => {
  it("reproduces the pre-token stylesheet exactly at compact", () => {
    // This is the whole contract of the default: adopting the token layer must
    // not move a single pixel for anyone who does not ask it to. These six
    // sizes are the six the stylesheet used across its 110 font-size
    // declarations; the seven spaces are the values its padding/gap/margin
    // rules overwhelmingly used.
    expect(METRICS.compact.font).toEqual({
      micro: 9,
      tiny: 10,
      small: 11,
      body: 12,
      lg: 13,
      xl: 14,
    });
    expect(METRICS.compact.space).toEqual({
      s1: 2,
      s2: 4,
      s3: 6,
      s4: 8,
      s5: 10,
      s6: 12,
      s7: 16,
    });
    // Matches ROW_HEIGHT in serial/virtualize.ts.
    expect(METRICS.compact.serialRow).toBe(18);
  });

  it("defaults to compact, so nothing changes until asked", () => {
    expect(DEFAULT_DENSITY).toBe("compact");
    expect(metricsFor(undefined)).toEqual(METRICS.compact);
  });

  it("defines every token at every density", () => {
    for (const d of DENSITIES) {
      for (const f of FONT_TOKENS) {
        expect(METRICS[d].font[f], `${d}.font.${f}`).toBeGreaterThan(0);
      }
      for (const s of SPACE_TOKENS) {
        expect(METRICS[d].space[s], `${d}.space.${s}`).toBeGreaterThanOrEqual(0);
      }
      expect(METRICS[d].serialRow).toBeGreaterThan(0);
      expect(DENSITY_LABEL[d]).toBeTruthy();
    }
  });

  it("keeps every type scale ascending", () => {
    // micro < tiny < small < body < lg < xl. A scale that folds back on itself
    // makes "smaller" and "larger" meaningless to every caller.
    for (const d of DENSITIES) {
      const sizes = FONT_TOKENS.map((t) => METRICS[d].font[t]);
      for (let i = 1; i < sizes.length; i++) {
        expect(sizes[i], `${d}: ${FONT_TOKENS[i]}`).toBeGreaterThan(sizes[i - 1]);
      }
    }
  });

  it("keeps every spacing scale non-decreasing", () => {
    for (const d of DENSITIES) {
      const steps = SPACE_TOKENS.map((t) => METRICS[d].space[t]);
      for (let i = 1; i < steps.length; i++) {
        expect(steps[i], `${d}: ${SPACE_TOKENS[i]}`).toBeGreaterThanOrEqual(steps[i - 1]);
      }
    }
  });

  it("grows monotonically from compact to comfortable", () => {
    // Every token at every step must be >= the same token one density down,
    // or "comfortable" would be smaller than "normal" somewhere and the
    // control would stop meaning what its label says.
    const order = ["compact", "normal", "comfortable"] as const;
    for (let i = 1; i < order.length; i++) {
      const lo = METRICS[order[i - 1]];
      const hi = METRICS[order[i]];
      for (const f of FONT_TOKENS) {
        expect(hi.font[f], `${order[i]}.font.${f}`).toBeGreaterThanOrEqual(lo.font[f]);
      }
      for (const s of SPACE_TOKENS) {
        expect(hi.space[s], `${order[i]}.space.${s}`).toBeGreaterThanOrEqual(lo.space[s]);
      }
      expect(hi.serialRow).toBeGreaterThanOrEqual(lo.serialRow);
    }
  });

  it("leaves room for the text inside a serial row at every density", () => {
    // The monitor paints mono text at --fs-small inside a fixed-height row it
    // also virtualises against. If the row stops clearing the glyphs, the text
    // clips — and it clips at every scroll position, so it reads as corruption
    // rather than as a layout bug.
    for (const d of DENSITIES) {
      expect(METRICS[d].serialRow, d).toBeGreaterThanOrEqual(
        METRICS[d].font.small + 4,
      );
    }
  });
});

describe("isDensity", () => {
  it("accepts the three levels and rejects anything else", () => {
    // Guards the settings file, which is user-editable JSON.
    for (const d of DENSITIES) expect(isDensity(d)).toBe(true);
    expect(isDensity("cosy")).toBe(false);
    expect(isDensity(null)).toBe(false);
    expect(isDensity(2)).toBe(false);
    expect(isDensity(undefined)).toBe(false);
  });

  it("falls back rather than returning undefined metrics", () => {
    expect(metricsFor("nonsense" as never)).toEqual(METRICS.compact);
  });
});
