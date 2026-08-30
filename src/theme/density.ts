// Density: the metric half of a theme.
//
// The audit that produced this file counted 110 `font-size` declarations in
// `styles.css` — 57 of them 11px, 12 of them 10px, one 9px — all hardcoded,
// on a 13px body. Most text in a bench tool you read at arm's length was
// smaller than the body copy, and nothing could change it.
//
// `compact` reproduces those numbers EXACTLY. That is deliberate and it is the
// default: the toolbar has documented overflow behaviour (it has no scroll of
// its own under `body { overflow: hidden }`, so anything past the right edge
// is simply unreachable), and growing every font in the window is precisely
// the way to push Verify and Flash off it. Shipping a larger default would be
// changing the UI on someone's bench without asking; shipping the control lets
// them choose and find out. `normal` and `comfortable` want a bench pass.

import type { Metrics } from "./tokens";

export type Density = "compact" | "normal" | "comfortable";

export const DENSITIES: readonly Density[] = [
  "compact",
  "normal",
  "comfortable",
];

export const DENSITY_LABEL: Record<Density, string> = {
  compact: "Compact",
  normal: "Normal",
  comfortable: "Comfortable",
};

export const DENSITY_HINT: Record<Density, string> = {
  compact: "Bancada's original sizing — the most on screen at once",
  normal: "One step up; easier at arm's length from the bench",
  comfortable: "Largest; best for a shared screen or tired eyes",
};

export const METRICS: Record<Density, Metrics> = {
  // Exactly the pre-token stylesheet. Do not "tidy" these numbers — the point
  // of this row is that switching to it is a no-op.
  compact: {
    font: { micro: 9, tiny: 10, small: 11, body: 12, lg: 13, xl: 14 },
    space: { s1: 2, s2: 4, s3: 6, s4: 8, s5: 10, s6: 12, s7: 16 },
    serialRow: 18,
  },
  normal: {
    font: { micro: 10, tiny: 11, small: 12, body: 13, lg: 15, xl: 16 },
    space: { s1: 2, s2: 5, s3: 7, s4: 9, s5: 11, s6: 14, s7: 18 },
    serialRow: 20,
  },
  comfortable: {
    font: { micro: 11, tiny: 12, small: 13, body: 15, lg: 16, xl: 18 },
    space: { s1: 3, s2: 6, s3: 8, s4: 11, s5: 14, s6: 17, s7: 22 },
    serialRow: 23,
  },
};

export const DEFAULT_DENSITY: Density = "compact";

export function metricsFor(d: Density | null | undefined): Metrics {
  return METRICS[d ?? DEFAULT_DENSITY] ?? METRICS[DEFAULT_DENSITY];
}

export function isDensity(v: unknown): v is Density {
  return typeof v === "string" && (DENSITIES as readonly string[]).includes(v);
}
