import { describe, expect, it } from "vitest";
import { minMaxDecimate } from "../decimate";
import { RingBuffer } from "../ring";

describe("minMaxDecimate", () => {
  it("computes known min/max per bucket", () => {
    const r = new RingBuffer(7); // 128
    const vals: number[] = [];
    for (let i = 0; i < 100; i++) vals.push(i);
    r.write(vals);
    const { mins, maxs } = minMaxDecimate(r, 0, 100, 10);
    for (let c = 0; c < 10; c++) {
      expect(mins[c]).toBe(c * 10);
      expect(maxs[c]).toBe(c * 10 + 9);
    }
  });

  it("captures alternating extremes inside a bucket", () => {
    const r = new RingBuffer(6);
    const vals: number[] = [];
    for (let i = 0; i < 64; i++) vals.push(i % 2 === 0 ? 5 : -5);
    r.write(vals);
    const { mins, maxs } = minMaxDecimate(r, 0, 64, 4);
    for (let c = 0; c < 4; c++) {
      expect(mins[c]).toBe(-5);
      expect(maxs[c]).toBe(5);
    }
  });

  it("works from a non-zero absolute offset across wraparound", () => {
    const r = new RingBuffer(3); // 8
    for (let i = 0; i < 20; i++) r.push(i);
    // history holds 12..19
    const { mins, maxs } = minMaxDecimate(r, 12, 8, 4);
    expect(Array.from(mins)).toEqual([12, 14, 16, 18]);
    expect(Array.from(maxs)).toEqual([13, 15, 17, 19]);
  });

  it("sparse windows (count < columns) reuse nearest sample", () => {
    const r = new RingBuffer(4);
    r.write([1, 2, 3]);
    const { mins, maxs } = minMaxDecimate(r, 0, 3, 6);
    for (let c = 0; c < 6; c++) {
      expect(mins[c]).toBe(maxs[c]);
      expect(mins[c]).toBeGreaterThanOrEqual(1);
      expect(mins[c]).toBeLessThanOrEqual(3);
    }
    expect(mins[0]).toBe(1);
    expect(maxs[5]).toBe(3);
  });

  it("reuses caller-provided output arrays", () => {
    const r = new RingBuffer(4);
    r.write([4, 8]);
    const mins = new Float32Array(2);
    const maxs = new Float32Array(2);
    const out = minMaxDecimate(r, 0, 2, 2, { mins, maxs });
    expect(out.mins).toBe(mins);
    expect(out.maxs).toBe(maxs);
    expect(mins[0]).toBe(4);
    expect(maxs[1]).toBe(8);
  });

  it("zero-count window yields zeros", () => {
    const r = new RingBuffer(4);
    const { mins, maxs } = minMaxDecimate(r, 0, 0, 3);
    expect(Array.from(mins)).toEqual([0, 0, 0]);
    expect(Array.from(maxs)).toEqual([0, 0, 0]);
  });
});
