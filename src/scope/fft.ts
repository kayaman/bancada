// In-repo iterative radix-2 complex FFT and windowed real-input dB spectrum.
// No dependencies; runs in the worker (fftWorker.ts) or synchronously.

import type { FftWindowKind } from "./types";

// Flat-top coefficients (a0..a4); coherent gain = a0.
const FT_A0 = 0.21557895;
const FT_A1 = 0.41663158;
const FT_A2 = 0.277263158;
const FT_A3 = 0.083578947;
const FT_A4 = 0.006947368;

/** Coherent (amplitude) gain of a window kind. */
export function coherentGain(window: FftWindowKind): number {
  switch (window) {
    case "hann":
      return 0.5;
    case "flattop":
      return FT_A0;
    case "rect":
      return 1;
  }
}

function windowValue(window: FftWindowKind, i: number, n: number): number {
  const t = (2 * Math.PI * i) / n;
  switch (window) {
    case "hann":
      return 0.5 - 0.5 * Math.cos(t);
    case "flattop":
      return (
        FT_A0 -
        FT_A1 * Math.cos(t) +
        FT_A2 * Math.cos(2 * t) -
        FT_A3 * Math.cos(3 * t) +
        FT_A4 * Math.cos(4 * t)
      );
    case "rect":
      return 1;
  }
}

export function nextPow2(n: number): number {
  let p = 1;
  while (p < n) p <<= 1;
  return p;
}

/**
 * In-place iterative radix-2 complex FFT (decimation in time).
 * `re`/`im` length must be an equal power of two.
 */
export function fftComplex(re: Float64Array, im: Float64Array): void {
  const n = re.length;
  if (n !== im.length || (n & (n - 1)) !== 0) {
    throw new Error("fftComplex: length must be a power of two");
  }
  // bit reversal permutation
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      const tr = re[i];
      re[i] = re[j];
      re[j] = tr;
      const ti = im[i];
      im[i] = im[j];
      im[j] = ti;
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wr = Math.cos(ang);
    const wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cwr = 1;
      let cwi = 0;
      const half = len >> 1;
      for (let k = 0; k < half; k++) {
        const a = i + k;
        const b = a + half;
        const xr = re[b] * cwr - im[b] * cwi;
        const xi = re[b] * cwi + im[b] * cwr;
        re[b] = re[a] - xr;
        im[b] = im[a] - xi;
        re[a] += xr;
        im[a] += xi;
        const nwr = cwr * wr - cwi * wi;
        cwi = cwr * wi + cwi * wr;
        cwr = nwr;
      }
    }
  }
}

/**
 * Windowed real-input spectrum in dB.
 * Input is zero-padded (or used as-is) to a power-of-two length N; returns
 * N/2 + 1 bins: 20*log10(2*mag/(N*cg) + 1e-12), cg = window coherent gain,
 * so a full-scale sine peaks near 0 dB regardless of window.
 */
export function realFftDb(samples: Float32Array, window: FftWindowKind): Float32Array {
  const len = samples.length;
  const n = Math.max(2, nextPow2(len));
  const re = new Float64Array(n);
  const im = new Float64Array(n);
  for (let i = 0; i < len; i++) {
    re[i] = samples[i] * windowValue(window, i, len);
  }
  fftComplex(re, im);
  const cg = coherentGain(window);
  const half = n >> 1;
  const db = new Float32Array(half + 1);
  const scale = 2 / (n * cg);
  for (let k = 0; k <= half; k++) {
    const mag = Math.hypot(re[k], im[k]);
    db[k] = 20 * Math.log10(mag * scale + 1e-12);
  }
  return db;
}

/**
 * Parabolic interpolation around the strongest bin (DC excluded).
 * Returns nulls when there is no usable interior peak.
 */
export function parabolicPeak(
  db: Float32Array,
  binHz: number,
): { peakHz: number | null; peakDb: number | null } {
  if (db.length < 3 || !(binHz > 0)) return { peakHz: null, peakDb: null };
  let k = 1;
  let best = -Infinity;
  for (let i = 1; i < db.length - 1; i++) {
    if (db[i] > best) {
      best = db[i];
      k = i;
    }
  }
  if (!Number.isFinite(best)) return { peakHz: null, peakDb: null };
  const y0 = db[k - 1];
  const y1 = db[k];
  const y2 = db[k + 1];
  const denom = y0 - 2 * y1 + y2;
  let delta = 0;
  if (denom !== 0) {
    delta = (0.5 * (y0 - y2)) / denom;
    if (!(delta > -1 && delta < 1)) delta = 0;
  }
  return {
    peakHz: (k + delta) * binHz,
    peakDb: y1 - 0.25 * (y0 - y2) * delta,
  };
}
