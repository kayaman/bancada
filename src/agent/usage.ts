// Token/cost figures from the claude CLI's `result` events. The host
// forwards stream-json verbatim, so the shapes here are the CLI's own —
// undocumented like the rest of the wire protocol, hence the defensive
// parsing: a malformed event yields null, never a throw (same contract as
// agentStore.push).

/** What one completed turn cost. */
export interface TurnUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  costUsd?: number;
  numTurns?: number;
  durationMs?: number;
}

/** Running totals for the whole session. */
export interface SessionUsage {
  costUsd: number;
  inputTokens: number;
  outputTokens: number;
}

export const emptySessionUsage = (): SessionUsage => ({
  costUsd: 0,
  inputTokens: 0,
  outputTokens: 0,
});

const num = (v: unknown): number | undefined =>
  typeof v === "number" && Number.isFinite(v) ? v : undefined;

/** Usage of a `result` event, or null if it carries none we can trust. */
export function parseTurnUsage(ev: unknown): TurnUsage | null {
  if (typeof ev !== "object" || ev === null) return null;
  const usage = (ev as { usage?: unknown }).usage;
  if (typeof usage !== "object" || usage === null || Array.isArray(usage)) {
    return null;
  }
  const u = usage as Record<string, unknown>;
  const inputTokens = num(u.input_tokens);
  const outputTokens = num(u.output_tokens);
  if (inputTokens === undefined || outputTokens === undefined) return null;
  const e = ev as Record<string, unknown>;
  return {
    inputTokens,
    outputTokens,
    cacheReadTokens: num(u.cache_read_input_tokens) ?? 0,
    costUsd: num(e.total_cost_usd),
    numTurns: num(e.num_turns),
    durationMs: num(e.duration_ms),
  };
}

export function addUsage(s: SessionUsage, t: TurnUsage): SessionUsage {
  return {
    costUsd: s.costUsd + (t.costUsd ?? 0),
    inputTokens: s.inputTokens + t.inputTokens,
    outputTokens: s.outputTokens + t.outputTokens,
  };
}

/** 999 → "999", 12401 → "12.4k", 1200000 → "1.2M". */
export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}
