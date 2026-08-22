import { describe, expect, it } from "vitest";
import { OVERSCAN, ROW_HEIGHT, isNearBottom, visibleRange } from "../virtualize";

describe("visibleRange", () => {
  it("sizes the spacer to every row, shown or not", () => {
    expect(visibleRange(0, 180, 1000).totalHeight).toBe(1000 * ROW_HEIGHT);
  });

  it("renders only the viewport plus overscan", () => {
    const r = visibleRange(0, 180, 1000);
    // 180/18 = 10 rows on screen, + overscan on each side (clamped at 0).
    expect(r.start).toBe(0);
    expect(r.end).toBe(10 + OVERSCAN);
    expect(r.offsetY).toBe(0);
    expect(r.end - r.start).toBeLessThan(40);
  });

  it("offsets the window to the first rendered row", () => {
    const r = visibleRange(1000, 180, 1000);
    // First visible row is floor(1000/18) = 55; overscan pulls back to 47.
    expect(r.start).toBe(47);
    expect(r.offsetY).toBe(47 * ROW_HEIGHT);
    expect(r.end).toBe(55 + 10 + OVERSCAN);
  });

  it("clamps to the ends", () => {
    const r = visibleRange(1e9, 180, 20);
    expect(r.end).toBe(20);
    expect(r.start).toBeGreaterThanOrEqual(0);
    expect(r.start).toBeLessThanOrEqual(20);
    const neg = visibleRange(-50, 180, 20);
    expect(neg.start).toBe(0);
    expect(neg.offsetY).toBe(0);
  });

  it("handles an empty list", () => {
    expect(visibleRange(0, 180, 0)).toEqual({
      start: 0,
      end: 0,
      offsetY: 0,
      totalHeight: 0,
    });
  });

  it("renders a token window before the first measurement", () => {
    // jsdom (and the very first paint) report clientHeight 0; rendering
    // nothing there would leave the panel blank until a resize.
    const r = visibleRange(0, 0, 1000);
    expect(r.start).toBe(0);
    expect(r.end).toBe(2 * OVERSCAN);
    const few = visibleRange(0, 0, 3);
    expect(few.end).toBe(3);
  });

  it("takes a custom row height and overscan", () => {
    const r = visibleRange(100, 100, 100, 10, 0);
    expect(r.start).toBe(10);
    expect(r.end).toBe(20);
    expect(r.offsetY).toBe(100);
    expect(r.totalHeight).toBe(1000);
  });
});

describe("isNearBottom", () => {
  it("is true at the bottom and within the slack", () => {
    expect(isNearBottom(820, 180, 1000)).toBe(true);
    expect(isNearBottom(790, 180, 1000)).toBe(true);
  });

  it("is false once the user has scrolled up past the slack", () => {
    expect(isNearBottom(500, 180, 1000)).toBe(false);
  });

  it("is true when the content does not fill the viewport", () => {
    expect(isNearBottom(0, 180, 40)).toBe(true);
  });

  it("honours a custom slack", () => {
    expect(isNearBottom(700, 180, 1000, 200)).toBe(true);
    expect(isNearBottom(700, 180, 1000, 10)).toBe(false);
  });
});
