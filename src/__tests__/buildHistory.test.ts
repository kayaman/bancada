import { describe, expect, it } from "vitest";
import {
  BUILD_HISTORY_KEY,
  type KVStorage,
  estimateFraction,
  loadDurations,
  recordDuration,
} from "../buildHistory";

const T0 = 1_700_000_000_000;

const storage = (seed?: string) => {
  const map = new Map<string, string>();
  if (seed !== undefined) map.set(BUILD_HISTORY_KEY, seed);
  const kv: KVStorage & { map: Map<string, string> } = {
    map,
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => {
      map.set(k, v);
    },
  };
  return kv;
};

describe("loadDurations / recordDuration", () => {
  it("round-trips a duration under the documented key", () => {
    const kv = storage();
    recordDuration(kv, "/home/m/Projects/blink", "compile", 4_500, T0);
    expect(kv.map.has(BUILD_HISTORY_KEY)).toBe(true);
    expect(loadDurations(kv, "/home/m/Projects/blink")).toEqual({
      compileMs: 4_500,
      updatedAt: T0,
    });
  });

  it("keeps the two ops apart, and the last value of each", () => {
    const kv = storage();
    recordDuration(kv, "/p/blink", "compile", 4_500, T0);
    recordDuration(kv, "/p/blink", "upload", 12_000, T0 + 1);
    recordDuration(kv, "/p/blink", "compile", 3_100, T0 + 2);
    expect(loadDurations(kv, "/p/blink")).toEqual({
      compileMs: 3_100,
      uploadMs: 12_000,
      updatedAt: T0 + 2,
    });
  });

  it("does not leak one project's timing into another", () => {
    const kv = storage();
    recordDuration(kv, "/p/blink", "compile", 4_500, T0);
    recordDuration(kv, "/p/sonar", "compile", 61_000, T0);
    expect(loadDurations(kv, "/p/blink")!.compileMs).toBe(4_500);
    expect(loadDurations(kv, "/p/sonar")!.compileMs).toBe(61_000);
    expect(loadDurations(kv, "/p/never-built")).toBe(null);
  });

  it("is null when nothing has ever been stored", () => {
    expect(loadDurations(storage(), "/p/blink")).toBe(null);
  });

  it("prunes to 50 projects, dropping the least recently built", () => {
    const kv = storage();
    for (let i = 0; i < 51; i++)
      recordDuration(kv, `/p/proj${i}`, "compile", 1_000 + i, T0 + i);

    const kept = Object.keys(JSON.parse(kv.map.get(BUILD_HISTORY_KEY)!));
    expect(kept.length).toBe(50);
    expect(loadDurations(kv, "/p/proj0")).toBe(null);
    expect(loadDurations(kv, "/p/proj1")!.compileMs).toBe(1_001);
    expect(loadDurations(kv, "/p/proj50")!.compileMs).toBe(1_050);
  });

  it("prunes by when a project was last built, not by insertion order", () => {
    const kv = storage();
    for (let i = 0; i < 50; i++)
      recordDuration(kv, `/p/proj${i}`, "compile", 1_000, T0 + i);
    // proj0 is the oldest by insertion — building it again makes it newest
    recordDuration(kv, "/p/proj0", "compile", 2_000, T0 + 100);
    recordDuration(kv, "/p/fresh", "compile", 3_000, T0 + 101);

    expect(loadDurations(kv, "/p/proj0")!.compileMs).toBe(2_000);
    expect(loadDurations(kv, "/p/proj1")).toBe(null);
    expect(loadDurations(kv, "/p/fresh")!.compileMs).toBe(3_000);
  });
});

describe("malformed storage", () => {
  it("reads as absent rather than throwing", () => {
    expect(loadDurations(storage("}{ not json"), "/p/blink")).toBe(null);
    expect(loadDurations(storage("[1,2,3]"), "/p/blink")).toBe(null);
    expect(loadDurations(storage("null"), "/p/blink")).toBe(null);
    expect(loadDurations(storage('{"/p/blink": 7}'), "/p/blink")).toBe(null);
  });

  it("is overwritten by the next successful build", () => {
    const kv = storage("}{ not json");
    recordDuration(kv, "/p/blink", "compile", 4_500, T0);
    expect(loadDurations(kv, "/p/blink")).toEqual({
      compileMs: 4_500,
      updatedAt: T0,
    });
  });
});

describe("estimateFraction", () => {
  it("is elapsed over the remembered duration", () => {
    expect(estimateFraction(2_000, 10_000)).toBeCloseTo(0.2, 6);
    expect(estimateFraction(0, 10_000)).toBe(0);
  });

  it("never reaches the end on a guess", () => {
    expect(estimateFraction(10_000, 10_000)).toBe(0.95);
    expect(estimateFraction(600_000, 10_000)).toBe(0.95);
  });

  it("is null without a usable estimate", () => {
    expect(estimateFraction(2_000, null)).toBe(null);
    expect(estimateFraction(2_000, undefined)).toBe(null);
    expect(estimateFraction(2_000, 0)).toBe(null);
    expect(estimateFraction(2_000, -5)).toBe(null);
  });

  it("does not go negative if the clock jumps", () => {
    expect(estimateFraction(-1_000, 10_000)).toBe(0);
  });
});
