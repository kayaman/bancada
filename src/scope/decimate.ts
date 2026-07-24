// Min/max decimation of a ring-buffer window onto display columns.

import type { RingBuffer } from "./ring";

export interface DecimateOut {
  mins: Float32Array;
  maxs: Float32Array;
}

/**
 * Map `count` samples starting at absolute index `fromAbs` onto `columns`
 * buckets, taking per-bucket min and max. `out` (with arrays of length >=
 * columns) may be supplied to avoid allocation; otherwise fresh arrays are
 * created. Buckets with no samples (count < columns) reuse the nearest
 * sample so traces stay continuous.
 */
export function minMaxDecimate(
  ring: RingBuffer,
  fromAbs: number,
  count: number,
  columns: number,
  out?: DecimateOut,
): DecimateOut {
  const mins = out && out.mins.length >= columns ? out.mins : new Float32Array(columns);
  const maxs = out && out.maxs.length >= columns ? out.maxs : new Float32Array(columns);

  if (columns <= 0) return { mins, maxs };
  if (count <= 0 || ring.writtenTotal === 0) {
    mins.fill(0, 0, columns);
    maxs.fill(0, 0, columns);
    return { mins, maxs };
  }

  const last = fromAbs + count - 1;
  for (let c = 0; c < columns; c++) {
    let start = fromAbs + Math.floor((c * count) / columns);
    let end = fromAbs + Math.floor(((c + 1) * count) / columns);
    if (end <= start) {
      // sparse window: sample the nearest point
      start = Math.min(start, last);
      end = start + 1;
    }
    let mn = Infinity;
    let mx = -Infinity;
    for (let i = start; i < end; i++) {
      const v = ring.get(i);
      if (v < mn) mn = v;
      if (v > mx) mx = v;
    }
    mins[c] = mn;
    maxs[c] = mx;
  }
  return { mins, maxs };
}
