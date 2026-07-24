import { describe, expect, it } from "vitest";
import { coherentGain, fftComplex, nextPow2, parabolicPeak, realFftDb } from "../fft";

const SPS = 100_000;

function tone(freq: number, n: number, amp = 1): Float32Array {
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = amp * Math.sin((2 * Math.PI * freq * i) / SPS);
  return out;
}

describe("fft", () => {
  it("nextPow2 / coherentGain basics", () => {
    expect(nextPow2(1000)).toBe(1024);
    expect(nextPow2(1024)).toBe(1024);
    expect(coherentGain("rect")).toBe(1);
    expect(coherentGain("hann")).toBe(0.5);
    expect(coherentGain("flattop")).toBeCloseTo(0.21557895, 8);
  });

  it("fftComplex matches DFT of an impulse", () => {
    const re = new Float64Array(8);
    const im = new Float64Array(8);
    re[0] = 1;
    fftComplex(re, im);
    for (let k = 0; k < 8; k++) {
      expect(re[k]).toBeCloseTo(1, 9);
      expect(im[k]).toBeCloseTo(0, 9);
    }
  });

  it("4096-sample 1 kHz tone at 100 kS/s: Hann peak bin within ±1", () => {
    const db = realFftDb(tone(1000, 4096), "hann");
    expect(db.length).toBe(4096 / 2 + 1);
    const binHz = SPS / 4096;
    let peak = 1;
    for (let i = 1; i < db.length - 1; i++) if (db[i] > db[peak]) peak = i;
    const expected = Math.round(1000 / binHz); // 41
    expect(Math.abs(peak - expected)).toBeLessThanOrEqual(1);
    // full-scale sine should sit near 0 dB (within Hann scalloping loss)
    expect(db[peak]).toBeGreaterThan(-2);
    expect(db[peak]).toBeLessThan(0.5);
  });

  it("parabolic interpolation recovers the tone within 5 Hz", () => {
    const db = realFftDb(tone(1000, 4096), "hann");
    const { peakHz, peakDb } = parabolicPeak(db, SPS / 4096);
    expect(peakHz).not.toBeNull();
    expect(Math.abs(peakHz! - 1000)).toBeLessThan(5);
    expect(peakDb).not.toBeNull();
    expect(peakDb!).toBeGreaterThan(-1.5);
  });

  it("rect window with an exact-bin tone peaks at ~0 dB", () => {
    const binHz = SPS / 4096;
    const f = 40 * binHz; // exactly on bin 40
    const db = realFftDb(tone(f, 4096), "rect");
    const { peakHz, peakDb } = parabolicPeak(db, binHz);
    expect(Math.abs(peakHz! - f)).toBeLessThan(binHz / 10);
    expect(peakDb!).toBeCloseTo(0, 1);
  });

  it("flattop window reports amplitude accurately off-bin", () => {
    const binHz = SPS / 4096;
    const f = 40.5 * binHz; // worst case: exactly between bins
    const db = realFftDb(tone(f, 4096), "flattop");
    const { peakDb } = parabolicPeak(db, binHz);
    // flat-top scalloping loss is < 0.02 dB; the parabolic fit in dB adds a
    // small bias on the flat mainlobe top, so allow 0.5 dB total
    expect(Math.abs(peakDb!)).toBeLessThan(0.5);
  });

  it("pads non-power-of-two input", () => {
    const db = realFftDb(tone(1000, 3000), "hann"); // padded to 4096
    expect(db.length).toBe(4096 / 2 + 1);
  });

  it("amplitude scales in dB", () => {
    const binHz = SPS / 4096;
    const f = 40 * binHz;
    const full = realFftDb(tone(f, 4096, 1), "rect");
    const half = realFftDb(tone(f, 4096, 0.5), "rect");
    const { peakDb: dbFull } = parabolicPeak(full, binHz);
    const { peakDb: dbHalf } = parabolicPeak(half, binHz);
    expect(dbFull! - dbHalf!).toBeCloseTo(6.02, 1);
  });
});
