import { describe, expect, it } from "vitest";
import { RingBuffer } from "../ring";
import { TriggerEngine } from "../trigger";
import type { TriggerConfig } from "../types";

const SPS = 100_000;
const PERIOD = 100; // samples per period => 1 kHz

function sineRing(periods: number, noise = 0, seed = 1): RingBuffer {
  const r = new RingBuffer(16); // 65536
  let s = seed;
  const rand = () => {
    // deterministic LCG in [-1, 1)
    s = (s * 1664525 + 1013904223) >>> 0;
    return (s / 0x80000000) - 1;
  };
  const vals = new Float32Array(periods * PERIOD);
  for (let i = 0; i < vals.length; i++) {
    vals[i] = Math.sin((2 * Math.PI * i) / PERIOD) + noise * rand();
  }
  r.write(vals);
  return r;
}

function cfg(patch: Partial<TriggerConfig> = {}): TriggerConfig {
  return {
    mode: "auto",
    source: 1,
    level: 0,
    edge: "rising",
    hysteresis: 0.1,
    holdoff: 0,
    position: 0.5,
    ...patch,
  };
}

describe("TriggerEngine", () => {
  it("fires once per period on a clean sine, rising at level 0", () => {
    const ring = sineRing(10);
    const t = new TriggerEngine();
    t.configure(cfg(), SPS);
    const res = t.process(ring, 0, ring.writtenTotal);
    expect(res.firedAt).not.toBeNull();
    // 10 periods, first rising crossing at i~0 is missed (not yet armed) => 9
    expect(t.firesTotal).toBe(9);
  });

  it("hysteresis prevents double-fires on a noisy signal", () => {
    const ring = sineRing(10, 0.05); // noise well inside h = 0.2
    const t = new TriggerEngine();
    t.configure(cfg({ hysteresis: 0.2 }), SPS);
    t.process(ring, 0, ring.writtenTotal);
    expect(t.firesTotal).toBe(9);
  });

  it("tiny hysteresis on a noisy signal fires more often (sanity contrast)", () => {
    const ring = sineRing(10, 0.3, 7);
    const loose = new TriggerEngine();
    loose.configure(cfg({ hysteresis: 1e-6 }), SPS);
    loose.process(ring, 0, ring.writtenTotal);
    const tight = new TriggerEngine();
    tight.configure(cfg({ hysteresis: 0.6 }), SPS);
    tight.process(ring, 0, ring.writtenTotal);
    expect(tight.firesTotal).toBeLessThanOrEqual(10);
    expect(loose.firesTotal).toBeGreaterThan(tight.firesTotal);
  });

  it("holdoff suppresses fires within the holdoff window", () => {
    const ring = sineRing(10);
    const t = new TriggerEngine();
    // 2.5 periods of holdoff => fire every 3rd period
    t.configure(cfg({ holdoff: 2.5 * (PERIOD / SPS) }), SPS);
    t.process(ring, 0, ring.writtenTotal);
    expect(t.firesTotal).toBe(3); // ~samples 100, 400, 700
  });

  it("incremental process across chunks keeps state", () => {
    const ring = sineRing(10);
    const t = new TriggerEngine();
    t.configure(cfg(), SPS);
    for (let from = 0; from < ring.writtenTotal; from += 37) {
      t.process(ring, from, Math.min(from + 37, ring.writtenTotal));
    }
    expect(t.firesTotal).toBe(9);
  });

  it("falling edge fires on downward crossings", () => {
    const ring = sineRing(10);
    const t = new TriggerEngine();
    t.configure(cfg({ edge: "falling" }), SPS);
    t.process(ring, 0, ring.writtenTotal);
    expect(t.firesTotal).toBe(10);
    // falling crossing of level 0 happens mid-period (sample ~50)
    const fire = t.lastFire();
    expect(fire).not.toBeNull();
    expect((fire!.at % PERIOD + PERIOD) % PERIOD).toBeGreaterThan(45);
    expect((fire!.at % PERIOD + PERIOD) % PERIOD).toBeLessThan(56);
  });

  it("computes sub-sample crossing fraction by linear interpolation", () => {
    const ring = new RingBuffer(4);
    ring.write([-1, -1, 1, 1]); // crossing exactly halfway between idx 1 and 2
    const t = new TriggerEngine();
    t.configure(cfg({ hysteresis: 0.5 }), SPS);
    const res = t.process(ring, 0, 4);
    expect(res.firedAt).toBe(2);
    expect(res.frac).toBeCloseTo(0.5, 6);
  });

  it("single mode fires once until rearm()", () => {
    const ring = sineRing(10);
    const t = new TriggerEngine();
    t.configure(cfg({ mode: "single" }), SPS);
    t.process(ring, 0, ring.writtenTotal);
    expect(t.firesTotal).toBe(1);
    t.rearm();
    const ring2 = sineRing(10);
    t.process(ring2, ring.writtenTotal, ring.writtenTotal); // no-op range
    t.configure(cfg({ mode: "single" }), SPS);
    t.process(ring, 0, ring.writtenTotal);
    expect(t.firesTotal).toBe(2);
  });

  it("auto timeout forces an untriggered sweep on a flat signal", () => {
    const ring = new RingBuffer(16);
    const flat = new Float32Array(30_000).fill(1); // no crossings of level 0
    ring.write(flat);
    const t = new TriggerEngine();
    t.configure(cfg({ mode: "auto" }), SPS);
    t.process(ring, 0, ring.writtenTotal);
    expect(t.firesTotal).toBe(0);
    // timeout = max(100 ms, 2 x 10 ms) = 100 ms = 10k samples; 30k elapsed
    expect(t.shouldAutoSweep(ring.writtenTotal, 0.01)).toBe(true);
    expect(t.status(true, ring.writtenTotal, 0.01)).toBe("auto");
    expect(t.status(false, ring.writtenTotal, 0.01)).toBe("stop");
  });

  it("normal mode without fires reports wait; recent fire reports trigd", () => {
    const t = new TriggerEngine();
    t.configure(cfg({ mode: "normal" }), SPS);
    expect(t.status(true, 1000, 0.01)).toBe("wait");
    const ring = sineRing(2);
    t.process(ring, 0, ring.writtenTotal);
    expect(t.firesTotal).toBeGreaterThan(0);
    expect(t.status(true, ring.writtenTotal, 0.01)).toBe("trigd");
  });
});
