import { describe, expect, it } from "vitest";
import {
  addUsage,
  emptySessionUsage,
  formatTokens,
  parseTurnUsage,
} from "../usage";

// The claude CLI's real result-event shape (forwarded verbatim by the host).
const resultEvent = {
  type: "result",
  subtype: "success",
  is_error: false,
  duration_ms: 42137,
  num_turns: 3,
  result: "Done — the backoff now caps at 30s.",
  total_cost_usd: 0.0213,
  usage: {
    input_tokens: 12401,
    cache_creation_input_tokens: 2100,
    cache_read_input_tokens: 9800,
    output_tokens: 1203,
  },
};

describe("parseTurnUsage", () => {
  it("reads tokens, cost, turns and duration from a result event", () => {
    expect(parseTurnUsage(resultEvent)).toEqual({
      inputTokens: 12401,
      outputTokens: 1203,
      cacheReadTokens: 9800,
      costUsd: 0.0213,
      numTurns: 3,
      durationMs: 42137,
    });
  });

  it("returns null when the usage object is missing", () => {
    const { usage: _usage, ...noUsage } = resultEvent;
    expect(parseTurnUsage(noUsage)).toBeNull();
  });

  it("returns null when usage token counts are not numbers", () => {
    expect(
      parseTurnUsage({ usage: { input_tokens: "12k", output_tokens: 3 } }),
    ).toBeNull();
  });

  it("never throws on junk", () => {
    for (const junk of [null, undefined, 7, "x", [], { usage: [] }]) {
      expect(parseTurnUsage(junk)).toBeNull();
    }
  });

  it("tolerates absent optional fields", () => {
    const t = parseTurnUsage({ usage: { input_tokens: 5, output_tokens: 2 } });
    expect(t).toEqual({
      inputTokens: 5,
      outputTokens: 2,
      cacheReadTokens: 0,
      costUsd: undefined,
      numTurns: undefined,
      durationMs: undefined,
    });
  });
});

describe("addUsage", () => {
  it("accumulates cost and tokens without mutating the input", () => {
    const s0 = emptySessionUsage();
    const t = parseTurnUsage(resultEvent)!;
    const s1 = addUsage(s0, t);
    const s2 = addUsage(s1, t);
    expect(s2).toEqual({
      costUsd: 0.0426,
      inputTokens: 24802,
      outputTokens: 2406,
    });
    expect(s0).toEqual({ costUsd: 0, inputTokens: 0, outputTokens: 0 });
  });

  it("treats a turn without cost as zero cost", () => {
    const t = parseTurnUsage({ usage: { input_tokens: 5, output_tokens: 2 } })!;
    expect(addUsage(emptySessionUsage(), t).costUsd).toBe(0);
  });
});

describe("formatTokens", () => {
  it("keeps small counts verbatim", () => {
    expect(formatTokens(999)).toBe("999");
  });
  it("abbreviates thousands to one decimal", () => {
    expect(formatTokens(12401)).toBe("12.4k");
  });
  it("abbreviates millions", () => {
    expect(formatTokens(1200000)).toBe("1.2M");
  });
});
