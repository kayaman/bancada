import { describe, expect, it } from "vitest";
import {
  grandTotals,
  prunedCount,
  projectName,
  stemToDisplay,
} from "../usageDashboard";
import type { ProjectUsage } from "../api";

const row = (over: Partial<ProjectUsage>): ProjectUsage => ({
  sketch_dir: "/s",
  cost_usd: 0,
  input_tokens: 0,
  output_tokens: 0,
  turns: 0,
  sessions: 0,
  last_chat: null,
  ...over,
});

describe("grandTotals", () => {
  it("sums cost and tokens across projects", () => {
    const t = grandTotals([
      row({ cost_usd: 0.5, input_tokens: 100, output_tokens: 10 }),
      row({ cost_usd: 0.25, input_tokens: 50, output_tokens: 5 }),
    ]);
    expect(t.projects).toBe(2);
    expect(t.costUsd).toBeCloseTo(0.75);
    expect(t.inputTokens).toBe(150);
    expect(t.outputTokens).toBe(15);
  });

  it("is all zeros for no projects", () => {
    expect(grandTotals([])).toEqual({
      projects: 0,
      costUsd: 0,
      inputTokens: 0,
      outputTokens: 0,
    });
  });
});

describe("stemToDisplay", () => {
  it("renders a chat file stem as date + hh:mm", () => {
    expect(stemToDisplay("2026-08-05T09-30-00")).toBe("2026-08-05 09:30");
  });

  it("passes through anything that is not a stem", () => {
    expect(stemToDisplay("weird")).toBe("weird");
  });
});

describe("prunedCount", () => {
  it("is the store's excess over files on disk, floored at zero", () => {
    expect(prunedCount(row({ sessions: 60 }), 50)).toBe(10);
    expect(prunedCount(row({ sessions: 3 }), 3)).toBe(0);
    // A chat created before the store existed can leave disk ahead of the
    // record; that must not go negative.
    expect(prunedCount(row({ sessions: 1 }), 2)).toBe(0);
  });
});

describe("projectName", () => {
  it("is the basename, falling back to the full path", () => {
    expect(projectName("/home/me/Projects/Blink")).toBe("Blink");
    expect(projectName("odd")).toBe("odd");
  });
});
