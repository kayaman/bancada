import { describe, expect, it } from "vitest";
import { measure } from "../measure";

const DT = 1e-5; // 100 kS/s

function sine(freq: number, seconds: number, amp = 1, offset = 0): Float32Array {
  const n = Math.round(seconds / DT);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    out[i] = offset + amp * Math.sin(2 * Math.PI * freq * i * DT);
  }
  return out;
}

describe("measure", () => {
  it("1 kHz sine: freq within 0.5%, vpp/mean/rms correct", () => {
    const s = sine(1000, 0.01); // 10 periods, 1000 samples
    const m = measure(s, DT);
    expect(m.min).toBeCloseTo(-1, 3);
    expect(m.max).toBeCloseTo(1, 3);
    expect(m.vpp).toBeCloseTo(2, 3);
    expect(Math.abs(m.mean)).toBeLessThan(1e-3);
    expect(m.rms).toBeCloseTo(Math.SQRT1_2, 3);
    expect(m.freq).not.toBeNull();
    expect(Math.abs(m.freq! - 1000) / 1000).toBeLessThan(0.005);
    expect(m.period).toBeCloseTo(1e-3, 6);
    expect(m.dutyPos).not.toBeNull();
    expect(m.dutyPos!).toBeGreaterThan(0.45);
    expect(m.dutyPos!).toBeLessThan(0.55);
  });

  it("offset sine keeps mean and duty", () => {
    const m = measure(sine(1000, 0.01, 0.5, 1.65), DT);
    expect(m.mean).toBeCloseTo(1.65, 3);
    expect(m.vpp).toBeCloseTo(1, 3);
    expect(m.freq).not.toBeNull();
    expect(Math.abs(m.freq! - 1000) / 1000).toBeLessThan(0.005);
  });

  it("returns null freq with fewer than 2 full periods", () => {
    const m = measure(sine(1000, 0.0015), DT); // 1.5 periods
    expect(m.freq).toBeNull();
    expect(m.period).toBeNull();
    expect(m.dutyPos).toBeNull();
  });

  it("returns null freq for unstable periods", () => {
    // chirp: period changes >5% between cycles
    const n = 2000;
    const s = new Float32Array(n);
    let phase = 0;
    for (let i = 0; i < n; i++) {
      const f = 500 + 1500 * (i / n); // 500 -> 2000 Hz sweep
      phase += 2 * Math.PI * f * DT;
      s[i] = Math.sin(phase);
    }
    const m = measure(s, DT);
    expect(m.freq).toBeNull();
  });

  it("handles DC and empty input", () => {
    const dc = measure(new Float32Array(100).fill(2.5), DT);
    expect(dc.min).toBe(2.5);
    expect(dc.max).toBe(2.5);
    expect(dc.vpp).toBe(0);
    expect(dc.mean).toBeCloseTo(2.5, 6);
    expect(dc.rms).toBeCloseTo(2.5, 6);
    expect(dc.freq).toBeNull();

    const empty = measure(new Float32Array(0), DT);
    expect(empty.freq).toBeNull();
    expect(Number.isNaN(empty.min)).toBe(true);
  });

  it("measures duty of an asymmetric square wave", () => {
    // 25% duty 1 kHz square
    const n = 1000;
    const s = new Float32Array(n);
    for (let i = 0; i < n; i++) s[i] = i % 100 < 25 ? 1 : -1;
    const m = measure(s, DT);
    expect(m.freq).not.toBeNull();
    expect(Math.abs(m.freq! - 1000) / 1000).toBeLessThan(0.005);
    expect(m.dutyPos).not.toBeNull();
    expect(m.dutyPos!).toBeGreaterThan(0.2);
    expect(m.dutyPos!).toBeLessThan(0.3);
  });
});
