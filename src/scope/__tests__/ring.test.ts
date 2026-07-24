import { describe, expect, it } from "vitest";
import { RingBuffer } from "../ring";

describe("RingBuffer", () => {
  it("reports capacity and writtenTotal", () => {
    const r = new RingBuffer(3); // 8 samples
    expect(r.capacity).toBe(8);
    expect(r.writtenTotal).toBe(0);
    r.write([1, 2, 3]);
    expect(r.writtenTotal).toBe(3);
  });

  it("indexes by absolute sample index across wraparound", () => {
    const r = new RingBuffer(3); // 8
    const vals: number[] = [];
    for (let i = 0; i < 20; i++) vals.push(i * 10);
    r.write(vals);
    expect(r.writtenTotal).toBe(20);
    expect(r.firstAvailable).toBe(12);
    // newest 8 samples are 12..19
    for (let i = 12; i < 20; i++) expect(r.get(i)).toBe(i * 10);
  });

  it("accepts Float32Array input and push()", () => {
    const r = new RingBuffer(4);
    r.write(new Float32Array([1.5, -2.5]));
    r.push(7);
    expect(r.get(0)).toBe(1.5);
    expect(r.get(1)).toBe(-2.5);
    expect(r.get(2)).toBe(7);
  });

  it("readInto copies a window and clamps to available history", () => {
    const r = new RingBuffer(3); // 8
    for (let i = 0; i < 20; i++) r.push(i);
    const dst = new Float32Array(10);
    // ask for 10 samples from abs 5: history starts at 12 => clamped
    const n = r.readInto(dst, 5, 10);
    // from clamps to 12, count shrinks by 7 => 3 samples: 12,13,14
    expect(n).toBe(3);
    expect(Array.from(dst.subarray(0, 3))).toEqual([12, 13, 14]);
  });

  it("readInto clamps count to writtenTotal", () => {
    const r = new RingBuffer(4);
    r.write([1, 2, 3]);
    const dst = new Float32Array(8);
    const n = r.readInto(dst, 1, 100);
    expect(n).toBe(2);
    expect(Array.from(dst.subarray(0, 2))).toEqual([2, 3]);
  });

  it("readInto returns 0 for fully unavailable ranges", () => {
    const r = new RingBuffer(2);
    r.write([1, 2]);
    const dst = new Float32Array(4);
    expect(r.readInto(dst, 5, 3)).toBe(0);
  });

  it("clear resets state", () => {
    const r = new RingBuffer(2);
    r.write([1, 2, 3]);
    r.clear();
    expect(r.writtenTotal).toBe(0);
    expect(r.firstAvailable).toBe(0);
    expect(r.get(0)).toBe(0);
  });
});
