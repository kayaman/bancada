import { describe, expect, it } from "vitest";
import { nextBackoff } from "../backoff";

describe("nextBackoff", () => {
  it("doubles from 1s and caps at 30s (D4, no jitter)", () => {
    expect([1, 2, 3, 4, 5, 6, 7].map(nextBackoff)).toEqual([
      1000, 2000, 4000, 8000, 16000, 30000, 30000,
    ]);
  });

  it("stays capped for large attempt counts", () => {
    expect(nextBackoff(20)).toBe(30000);
    expect(nextBackoff(1000)).toBe(30000);
  });

  it("attempt 0 → 0 (connect now)", () => {
    expect(nextBackoff(0)).toBe(0);
  });

  it("negative attempts → 0", () => {
    expect(nextBackoff(-1)).toBe(0);
    expect(nextBackoff(-100)).toBe(0);
  });

  it("is deterministic", () => {
    expect(nextBackoff(3)).toBe(nextBackoff(3));
  });
});
