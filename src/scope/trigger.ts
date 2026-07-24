// Host-side trigger: hysteresis arm/fire state machine over a RingBuffer,
// scanned incrementally by absolute sample index. Pure logic — the engine
// decides window anchoring; this only detects fires.
//
// Contract (docs/scope-architecture.md §4):
//  - rising edge: arm when sample < level - h, fire when armed && sample >= level
//  - falling edge: mirrored (arm above level + h, fire at <= level)
//  - holdoff: minimum seconds between fires (converted to samples)
//  - modes: auto / normal / single (single fires once until rearm())
//  - auto timeout: max(100 ms, 2 x window) without a fire => untriggered sweep
//  - sub-sample crossing fraction via linear interpolation

import type { RingBuffer } from "./ring";
import type { TriggerConfig, TriggerStatus } from "./types";

type State = "disarmed" | "armed" | "waitRearm";

export interface TriggerFire {
  /** absolute sample index of the first sample at/past the level */
  at: number;
  /** sub-sample fraction in [0,1): crossing happened at (at - 1 + frac) */
  frac: number;
}

const clamp01 = (x: number) => (x < 0 ? 0 : x > 1 ? 1 : x);

export class TriggerEngine {
  private cfg: TriggerConfig = {
    mode: "auto",
    source: 1,
    level: 0,
    edge: "rising",
    hysteresis: 0,
    holdoff: 0,
    position: 0.5,
  };
  private sps = 0;
  private hyst = 0; // resolved hysteresis in channel units
  private holdoffSamples = 0;

  private state: State = "disarmed";
  private prevValue: number | null = null;
  private lastFireAbs: number | null = null;
  private lastFireFrac = 0;
  private scannedTo = 0; // absolute index we've consumed up to (exclusive)
  private fireCount = 0;

  /**
   * (Re)configure. `resolvedHysteresis` is used when cfg.hysteresis === 0
   * (auto = 2% of visible Vpp, computed by the engine which can see the
   * displayed window).
   */
  configure(cfg: TriggerConfig, sps: number, resolvedHysteresis?: number): void {
    const edgeChanged = cfg.edge !== this.cfg.edge;
    const levelChanged = cfg.level !== this.cfg.level;
    const modeChanged = cfg.mode !== this.cfg.mode;
    this.cfg = { ...cfg };
    this.sps = sps > 0 ? sps : 0;
    this.hyst = cfg.hysteresis > 0 ? cfg.hysteresis : Math.abs(resolvedHysteresis ?? 0);
    this.holdoffSamples = this.sps > 0 ? Math.round(cfg.holdoff * this.sps) : 0;
    if (edgeChanged || levelChanged || (modeChanged && this.state === "waitRearm")) {
      this.state = "disarmed";
    }
  }

  get config(): TriggerConfig {
    return this.cfg;
  }

  /** Scan new samples [fromAbs, toAbs). Returns the first fire in the range
   *  (state advances through the whole range; later fires update lastFire). */
  process(ring: RingBuffer, fromAbs: number, toAbs: number): { firedAt: number | null; frac: number } {
    let firedAt: number | null = null;
    let frac = 0;
    const rising = this.cfg.edge === "rising";
    const level = this.cfg.level;
    const h = this.hyst;

    for (let i = fromAbs; i < toAbs; i++) {
      const v = ring.get(i);
      const prev = this.prevValue;
      this.prevValue = v;

      if (this.state === "waitRearm") continue; // single already fired

      if (this.state === "disarmed") {
        if (rising ? v < level - h : v > level + h) this.state = "armed";
        continue;
      }

      // armed
      const crossed = rising ? v >= level : v <= level;
      if (!crossed) continue;

      if (
        this.lastFireAbs !== null &&
        this.holdoffSamples > 0 &&
        i - this.lastFireAbs < this.holdoffSamples
      ) {
        this.state = "disarmed"; // swallow this crossing entirely
        continue;
      }

      // fire
      this.fireCount++;
      this.lastFireAbs = i;
      this.lastFireFrac =
        prev !== null && v !== prev ? clamp01((level - prev) / (v - prev)) : 0;
      this.state = this.cfg.mode === "single" ? "waitRearm" : "disarmed";
      if (firedAt === null) {
        firedAt = i;
        frac = this.lastFireFrac;
      }
    }

    if (toAbs > this.scannedTo) this.scannedTo = toAbs;
    return { firedAt, frac };
  }

  /** Total fires since construction/reset (testing & stats). */
  get firesTotal(): number {
    return this.fireCount;
  }

  /** Most recent fire seen so far, or null. */
  lastFire(): TriggerFire | null {
    return this.lastFireAbs === null ? null : { at: this.lastFireAbs, frac: this.lastFireFrac };
  }

  /** Auto-mode sweep timeout in seconds for a given window length. */
  autoTimeoutSec(windowSec: number): number {
    return Math.max(0.1, 2 * windowSec);
  }

  /** True when auto mode should force an untriggered sweep: no fire within
   *  the timeout, measured in samples up to `nowAbs`. */
  shouldAutoSweep(nowAbs: number, windowSec: number): boolean {
    if (this.cfg.mode !== "auto") return false;
    if (this.sps <= 0) return this.lastFireAbs === null;
    const timeoutSamples = this.autoTimeoutSec(windowSec) * this.sps;
    const since = this.lastFireAbs === null ? nowAbs : nowAbs - this.lastFireAbs;
    return since >= timeoutSamples;
  }

  /** Display status word given run state and fire recency. */
  status(running: boolean, nowAbs: number, windowSec: number): TriggerStatus {
    if (!running) return "stop";
    if (this.lastFireAbs !== null) {
      if (this.sps <= 0) return "trigd";
      const since = (nowAbs - this.lastFireAbs) / this.sps;
      if (since <= this.autoTimeoutSec(windowSec)) return "trigd";
    }
    return this.cfg.mode === "auto" ? "auto" : "wait";
  }

  /** Re-arm after a single-mode capture (engine's run(true)). */
  rearm(): void {
    if (this.state === "waitRearm") this.state = "disarmed";
  }

  reset(): void {
    this.state = "disarmed";
    this.prevValue = null;
    this.lastFireAbs = null;
    this.lastFireFrac = 0;
    this.scannedTo = 0;
    this.fireCount = 0;
  }
}
