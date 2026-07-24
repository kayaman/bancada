// Waveform measurements over a displayed window.
//
// min/max/vpp/mean/rms in a single pass. Frequency by 50%-level crossing
// detection with hysteresis (5% of Vpp), linearly interpolated crossing
// times, averaged over all full periods; null when < 2 periods or the
// period standard deviation exceeds 5% of the mean.

import type { Measurements } from "./types";

export function measure(samples: Float32Array, dt: number): Measurements {
  const n = samples.length;
  const empty: Measurements = {
    min: NaN,
    max: NaN,
    vpp: NaN,
    mean: NaN,
    rms: NaN,
    freq: null,
    period: null,
    dutyPos: null,
  };
  if (n === 0 || !(dt > 0)) return empty;

  let min = Infinity;
  let max = -Infinity;
  let sum = 0;
  let sumSq = 0;
  for (let i = 0; i < n; i++) {
    const v = samples[i];
    if (v < min) min = v;
    if (v > max) max = v;
    sum += v;
    sumSq += v * v;
  }
  const vpp = max - min;
  const mean = sum / n;
  const rms = Math.sqrt(sumSq / n);

  const result: Measurements = { min, max, vpp, mean, rms, freq: null, period: null, dutyPos: null };
  if (vpp <= 0) return result;

  const mid = (min + max) / 2; // 50% level
  const h = 0.05 * vpp; // hysteresis band

  // Hysteresis crossing detection, both directions, interpolated times.
  const rises: number[] = [];
  const falls: number[] = [];
  // armLow: we've been below mid - h (armed for a rising crossing)
  // armHigh: we've been above mid + h (armed for a falling crossing)
  let armLow = samples[0] < mid - h;
  let armHigh = samples[0] > mid + h;
  let prev = samples[0];
  for (let i = 1; i < n; i++) {
    const v = samples[i];
    if (armLow && v >= mid) {
      const fr = v !== prev ? (mid - prev) / (v - prev) : 0;
      rises.push((i - 1 + fr) * dt);
      armLow = false;
    }
    if (armHigh && v <= mid) {
      const fr = v !== prev ? (mid - prev) / (v - prev) : 0;
      falls.push((i - 1 + fr) * dt);
      armHigh = false;
    }
    if (v < mid - h) armLow = true;
    if (v > mid + h) armHigh = true;
    prev = v;
  }

  // Periods from consecutive rising crossings.
  const nPeriods = rises.length - 1;
  if (nPeriods < 2) return result;

  let pSum = 0;
  for (let i = 0; i < nPeriods; i++) pSum += rises[i + 1] - rises[i];
  const pMean = pSum / nPeriods;
  if (!(pMean > 0)) return result;

  let pVar = 0;
  for (let i = 0; i < nPeriods; i++) {
    const d = rises[i + 1] - rises[i] - pMean;
    pVar += d * d;
  }
  const pStd = Math.sqrt(pVar / nPeriods);
  if (pStd > 0.05 * pMean) return result;

  result.period = pMean;
  result.freq = 1 / pMean;

  // Positive duty: time above mid between same-direction (rising) crossings.
  let dutySum = 0;
  let dutyCount = 0;
  for (let i = 0; i < nPeriods; i++) {
    const r0 = rises[i];
    const r1 = rises[i + 1];
    // first falling crossing inside (r0, r1)
    let f = -1;
    for (let j = 0; j < falls.length; j++) {
      if (falls[j] > r0 && falls[j] < r1) {
        f = falls[j];
        break;
      }
    }
    if (f >= 0) {
      dutySum += (f - r0) / (r1 - r0);
      dutyCount++;
    }
  }
  if (dutyCount > 0) result.dutyPos = dutySum / dutyCount;

  return result;
}
