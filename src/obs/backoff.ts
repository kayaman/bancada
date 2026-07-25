// Deterministic exponential backoff for observability reconnects (D4):
// 1s → 2s → 4s → … capped at 30s. No jitter — the values are asserted
// exactly in tests and shown verbatim in the UI countdown.

/**
 * Delay in ms before reconnect attempt `attempt` (1-based).
 * `attempt <= 0` means "connect now" and returns 0.
 */
export function nextBackoff(attempt: number): number {
  if (attempt <= 0) return 0;
  return Math.min(1000 * 2 ** (attempt - 1), 30_000);
}
