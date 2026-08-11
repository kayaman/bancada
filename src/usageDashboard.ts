// Pure math and formatting for the usage dashboard, kept out of the
// component so it is unit-testable. Ordering is core's job
// (UsageStore::overview sorts by cost); this module never reorders.

import type { ProjectUsage } from "./api";

export interface GrandTotals {
  projects: number;
  costUsd: number;
  inputTokens: number;
  outputTokens: number;
}

export function grandTotals(rows: ProjectUsage[]): GrandTotals {
  return rows.reduce(
    (t, r) => ({
      projects: t.projects + 1,
      costUsd: t.costUsd + r.cost_usd,
      inputTokens: t.inputTokens + r.input_tokens,
      outputTokens: t.outputTokens + r.output_tokens,
    }),
    { projects: 0, costUsd: 0, inputTokens: 0, outputTokens: 0 },
  );
}

/** `2026-08-05T09-30-00` (chat file stem) → `2026-08-05 09:30`. */
export function stemToDisplay(stem: string): string {
  const m = stem.match(/^(\d{4}-\d{2}-\d{2})T(\d{2})-(\d{2})-\d{2}$/);
  return m ? `${m[1]} ${m[2]}:${m[3]}` : stem;
}

/** Sessions banked in the store but no longer on disk (pruned or deleted). */
export function prunedCount(row: ProjectUsage, onDisk: number): number {
  return Math.max(0, row.sessions - onDisk);
}

export function projectName(sketchDir: string): string {
  return sketchDir.split("/").pop() || sketchDir;
}
