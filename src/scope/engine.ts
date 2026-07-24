// ScopeEngine: rings + trigger + measurements + decimation + FFT, wired per
// docs/scope-architecture.md §4. UI consumes only types.ts + this file.

import {
  decodeEnvelope,
  FLAG_LAST_IN_RECORD,
  FLAG_OVERFLOW,
  sampleChannel,
  sampleValue,
} from "./binary";
import { minMaxDecimate } from "./decimate";
import { parabolicPeak, realFftDb } from "./fft";
import type { FftWorkRequest, FftWorkResponse } from "./fftWorker";
import { measure as measureSamples } from "./measure";
import { RingBuffer } from "./ring";
import { parseLine } from "./textParser";
import { TriggerEngine } from "./trigger";
import type {
  ChannelInfo,
  EngineStats,
  FftResult,
  FftWindowKind,
  IScopeEngine,
  Measurements,
  RenderFrame,
  ScopeEvent,
  ScopeSourceKind,
  TracePlot,
  TriggerConfig,
  TriggerStatus,
} from "./types";

const PALETTE = ["#e8c34a", "#4ac3e8", "#7ce84a", "#e84a7c", "#4a7ce8", "#e8874a"];
const PLOTTER_RING_POW2 = 14; // 2^14
const ADC_RING_POW2 = 20; // 2^20
const DIVISIONS = 10;
const SPS_EMA_ALPHA = 0.2;
const FFT_MIN_SIZE = 1024; // §4: FFT sizes 1k..8k
const FFT_MAX_SIZE = 8192;

const now = (): number =>
  typeof performance !== "undefined" && typeof performance.now === "function"
    ? performance.now()
    : Date.now();

interface Channel {
  info: ChannelInfo;
  ring: RingBuffer;
  /** reused across frames: reallocated only when the column count changes */
  plot: TracePlot;
}

interface FrozenRecord {
  /** absolute (ring) index of the trigger point */
  triggerAbs: number;
  sps: number;
  channelId: number;
}

interface DisplayWindow {
  fromAbs: number;
  count: number;
  sps: number;
  status: TriggerStatus;
  /** absolute trigger anchor, or null for untriggered sweeps */
  anchorAbs: number | null;
  anchorFrac: number;
}

/** Piecewise-linear raw-counts -> volts conversion from a META cal table. */
class CalTable {
  private raws: number[] | null = null;
  private volts: number[] | null = null;

  set(cal: [number, number][] | undefined): void {
    if (!cal || cal.length < 2) {
      this.raws = null;
      this.volts = null;
      return;
    }
    const sorted = [...cal].sort((a, b) => a[0] - b[0]);
    this.raws = sorted.map((p) => p[0]);
    this.volts = sorted.map((p) => p[1] / 1000); // millivolts -> volts
  }

  reset(): void {
    this.raws = null;
    this.volts = null;
  }

  toVolts(raw: number): number {
    const raws = this.raws;
    const volts = this.volts;
    if (!raws || !volts) return (3.3 * raw) / 4095; // default before first META
    if (raw <= raws[0]) return volts[0];
    const last = raws.length - 1;
    if (raw >= raws[last]) return volts[last];
    // raws are few (17 points): linear search is fine
    let i = 1;
    while (raws[i] < raw) i++;
    const t = (raw - raws[i - 1]) / (raws[i] - raws[i - 1]);
    return volts[i - 1] + t * (volts[i] - volts[i - 1]);
  }
}

export class ScopeEngine implements IScopeEngine {
  private sourceKind: ScopeSourceKind = "plotter";
  private chans = new Map<number, Channel>();
  private byName = new Map<string, number>();
  private nextId = 1;

  private trigger = new TriggerEngine();
  private triggerCfg: TriggerConfig = {
    mode: "auto",
    source: 1,
    level: 0,
    edge: "rising",
    hysteresis: 0,
    holdoff: 0,
    position: 0.5,
  };
  private triggerScannedTo = 0;
  private running = true;
  private secPerDiv = 0.01;

  /** displayed anchor: latest fire with enough post-trigger samples */
  private displayAnchor: { abs: number; frac: number } | null = null;
  /** newest fire still waiting for its post-trigger samples */
  private pendingFire: { abs: number; frac: number } | null = null;
  private lastVisibleVpp = 0; // for auto hysteresis (2% of visible Vpp)

  // plotter sps estimation
  private lastLineAt: number | null = null;
  private emaIntervalMs: number | null = null;

  // adc state
  private cal = new CalTable();
  private deviceSps: number | null = null;
  private record: {
    triggerIdx: number;
    sps: number;
    startAbs: Map<number, number>;
  } | null = null;
  private frozen: FrozenRecord | null = null;

  private counters = { droppedFrames: 0, crcErrors: 0, overflows: 0 };

  // fft worker
  private worker: Worker | null = null;
  private workerFailed = false;
  private fftSeq = 0;
  private fftPending = new Map<
    number,
    { resolve: (r: FftWorkResponse) => void; reject: (e: unknown) => void }
  >();

  private lastWindow: DisplayWindow | null = null;

  // reusable per-column scratch (grown on demand, reused across frames)
  private scratchMins = new Float32Array(0);
  private scratchMaxs = new Float32Array(0);
  // reusable sample window for measure() (rings reach 2^20 samples)
  private scratchWindow = new Float32Array(0);

  /**
   * The single RenderFrame instance handed back by renderFrame(), mutated in
   * place. Consumers (WaveformCanvas) read it synchronously inside the rAF
   * tick; keeping one object avoids megabytes/second of garbage at 60 fps.
   * Callers that need to retain a frame must copy it.
   */
  private readonly frame: RenderFrame = {
    status: "stop",
    windowSec: 0,
    t0: 0,
    triggerFrac: 0,
    traces: [],
  };

  get source(): ScopeSourceKind {
    return this.sourceKind;
  }

  setSource(kind: ScopeSourceKind): void {
    if (kind === this.sourceKind) return;
    this.sourceKind = kind;
    this.reset();
  }

  // ---------- channel management ----------

  private ensureChannel(name: string, ringPow2: number, unit: string, sps: number): Channel {
    const existing = this.byName.get(name);
    if (existing !== undefined) {
      const ch = this.chans.get(existing);
      if (ch) return ch;
    }
    const id = this.nextId++;
    const color = PALETTE[(id - 1) % PALETTE.length];
    const ch: Channel = {
      info: {
        id,
        name,
        color,
        unit,
        visible: true,
        voltsPerDiv: unit === "V" ? 0.5 : 1,
        offset: 0,
        sps,
        written: 0,
      },
      ring: new RingBuffer(ringPow2),
      plot: {
        channelId: id,
        color,
        columns: 0,
        mins: new Float32Array(0),
        maxs: new Float32Array(0),
      },
    };
    this.chans.set(id, ch);
    this.byName.set(name, id);
    return ch;
  }

  channels(): ChannelInfo[] {
    const out: ChannelInfo[] = [];
    for (const ch of this.chans.values()) {
      out.push({ ...ch.info, written: ch.ring.writtenTotal });
    }
    return out.sort((a, b) => a.id - b.id);
  }

  setChannel(
    id: number,
    patch: Partial<Pick<ChannelInfo, "visible" | "voltsPerDiv" | "offset" | "name">>,
  ): void {
    const ch = this.chans.get(id);
    if (!ch) return;
    if (patch.name !== undefined && patch.name !== ch.info.name) {
      this.byName.delete(ch.info.name);
      this.byName.set(patch.name, id);
    }
    Object.assign(ch.info, patch);
  }

  // ---------- feeds ----------

  feedText(line: string): void {
    // App keeps the serial monitor running in both modes; ignore its lines
    // while the ADC source owns the display so they cannot invent channels.
    if (this.sourceKind !== "plotter") return;
    const values = parseLine(line);
    if (values.length === 0) return;

    // sample-rate EMA from line arrival intervals
    const t = now();
    if (this.lastLineAt !== null) {
      const dtMs = t - this.lastLineAt;
      if (dtMs > 0) {
        this.emaIntervalMs =
          this.emaIntervalMs === null
            ? dtMs
            : SPS_EMA_ALPHA * dtMs + (1 - SPS_EMA_ALPHA) * this.emaIntervalMs;
      }
    }
    this.lastLineAt = t;
    const sps = this.emaIntervalMs && this.emaIntervalMs > 0 ? 1000 / this.emaIntervalMs : 0;

    for (const { name, value } of values) {
      const ch = this.ensureChannel(name, PLOTTER_RING_POW2, "", sps);
      if (sps > 0) {
        for (const c of this.chans.values()) c.info.sps = sps; // shared line rate
      }
      if (this.running) ch.ring.push(value);
    }
  }

  feedBinary(data: ArrayBuffer | number[]): ScopeEvent[] {
    let msg;
    try {
      msg = decodeEnvelope(data);
    } catch {
      this.counters.crcErrors++; // malformed envelope: count and drop
      return [];
    }

    if (msg.kind === "event") {
      this.handleEvent(msg.event);
      return [msg.event];
    }

    // samples
    if (msg.flags & FLAG_OVERFLOW) this.counters.overflows++;
    const chSps = this.channelSps();
    const recording = this.record !== null;
    const samples = msg.samples;
    for (let i = 0; i < samples.length; i++) {
      const s = samples[i];
      const nib = sampleChannel(s);
      const name = nib === 0 ? "CH1" : "CH2";
      const ch = this.ensureChannel(name, ADC_RING_POW2, "V", chSps);
      if (recording && this.record && !this.record.startAbs.has(ch.info.id)) {
        this.record.startAbs.set(ch.info.id, ch.ring.writtenTotal);
      }
      if (this.running || recording) {
        ch.ring.push(this.cal.toVolts(sampleValue(s)));
      }
    }

    if (recording && this.record && msg.flags & FLAG_LAST_IN_RECORD) {
      // freeze the single-shot record around the device trigger point
      const rec = this.record;
      // single-shot records use one pin; anchor on the first recorded channel
      const first = rec.startAbs.entries().next();
      if (!first.done) {
        const [channelId, startAbs] = first.value;
        this.frozen = { triggerAbs: startAbs + rec.triggerIdx, sps: rec.sps, channelId };
        const ch = this.chans.get(channelId);
        if (ch) ch.info.sps = rec.sps;
      }
      this.record = null;
      this.running = false;
    }
    return [];
  }

  private handleEvent(ev: ScopeEvent): void {
    switch (ev.ev) {
      case "meta": {
        this.deviceSps = ev.sps;
        this.cal.reset();
        this.cal.set(ev.cal);
        if (typeof ev.overflows === "number" && ev.overflows > this.counters.overflows) {
          this.counters.overflows = ev.overflows;
        }
        const chSps = this.channelSps();
        for (const ch of this.chans.values()) {
          if (ch.info.unit === "V") ch.info.sps = chSps;
        }
        break;
      }
      case "record":
        this.record = { triggerIdx: ev.trigger_idx, sps: ev.sps, startAbs: new Map() };
        this.frozen = null;
        break;
      case "drop":
        this.counters.droppedFrames += ev.frames;
        break;
      case "crc":
        this.counters.crcErrors += ev.count;
        break;
      case "err":
        break;
      case "closed":
        this.running = false;
        break;
    }
  }

  /**
   * Per-channel sample rate for ADC streams.
   *
   * META `sps` is already per-channel: the firmware clamps the requested rate
   * to `min(req, chipMax/nch, linkMax/nch)` and runs the hardware at
   * `sps * nch`, so a dual-channel stream reports the rate each pin actually
   * sees. Dividing by the active pin count here would halve the time base.
   */
  private channelSps(): number {
    if (this.deviceSps === null) return 0;
    return this.deviceSps;
  }

  // ---------- trigger ----------

  getTrigger(): TriggerConfig {
    return { ...this.triggerCfg };
  }

  setTrigger(patch: Partial<TriggerConfig>): void {
    const prevSource = this.triggerCfg.source;
    this.triggerCfg = { ...this.triggerCfg, ...patch };
    if (this.triggerCfg.source !== prevSource) {
      this.trigger.reset();
      this.displayAnchor = null;
      this.pendingFire = null;
      const src = this.chans.get(this.triggerCfg.source);
      this.triggerScannedTo = src ? src.ring.writtenTotal : 0;
    }
  }

  run(on: boolean): void {
    if (on) {
      this.frozen = null;
      this.trigger.rearm();
      if (this.triggerCfg.mode === "single") this.displayAnchor = null;
    }
    this.running = on;
  }

  isRunning(): boolean {
    return this.running;
  }

  setSecPerDiv(s: number): void {
    if (s > 0) this.secPerDiv = s;
  }

  getSecPerDiv(): number {
    return this.secPerDiv;
  }

  private triggerSourceChannel(): Channel | null {
    return this.chans.get(this.triggerCfg.source) ?? this.chans.values().next().value ?? null;
  }

  /** Scan any new samples on the trigger source and update the display anchor. */
  private scanTrigger(windowCount: number): void {
    const src = this.triggerSourceChannel();
    if (!src) return;
    const sps = src.info.sps;
    const autoH = this.lastVisibleVpp > 0 ? 0.02 * this.lastVisibleVpp : 0;
    this.trigger.configure(this.triggerCfg, sps, autoH);

    const written = src.ring.writtenTotal;
    let from = this.triggerScannedTo;
    if (from < src.ring.firstAvailable) from = src.ring.firstAvailable;
    if (from < written) {
      this.trigger.process(src.ring, from, written);
      this.triggerScannedTo = written;
    }

    // promote fires once enough post-trigger samples exist; keep the newest
    // not-yet-mature fire pending so it can anchor a later frame
    const post = windowCount - Math.floor(this.triggerCfg.position * windowCount);
    if (this.pendingFire && this.pendingFire.abs + post <= written) {
      this.displayAnchor = this.pendingFire;
      this.pendingFire = null;
    }
    const fire = this.trigger.lastFire();
    if (fire && (this.displayAnchor === null || fire.at > this.displayAnchor.abs)) {
      if (fire.at + post <= written) {
        this.displayAnchor = { abs: fire.at, frac: fire.frac };
        this.pendingFire = null;
      } else if (this.pendingFire === null) {
        // keep the oldest immature fire: it matures first
        this.pendingFire = { abs: fire.at, frac: fire.frac };
      }
    }
  }

  // ---------- window / render ----------

  private currentWindow(): DisplayWindow | null {
    const windowSec = DIVISIONS * this.secPerDiv;

    if (this.frozen) {
      const ch = this.chans.get(this.frozen.channelId);
      if (!ch) return null;
      const sps = this.frozen.sps > 0 ? this.frozen.sps : ch.info.sps;
      if (!(sps > 0)) return null;
      const count = Math.max(2, Math.round(windowSec * sps));
      const pre = Math.floor(this.triggerCfg.position * count);
      return {
        fromAbs: this.frozen.triggerAbs - pre,
        count,
        sps,
        status: "stop",
        anchorAbs: this.frozen.triggerAbs,
        anchorFrac: 0,
      };
    }

    const src = this.triggerSourceChannel();
    if (!src) return null;
    const sps = src.info.sps;
    if (!(sps > 0)) return null;
    const count = Math.max(2, Math.round(windowSec * sps));
    const written = src.ring.writtenTotal;

    this.scanTrigger(count);

    const pre = Math.floor(this.triggerCfg.position * count);
    const status = this.trigger.status(this.running, written, windowSec);
    const autoSweep =
      this.triggerCfg.mode === "auto" && this.trigger.shouldAutoSweep(written, windowSec);

    if (this.displayAnchor && !(autoSweep && status !== "trigd")) {
      return {
        fromAbs: this.displayAnchor.abs - pre,
        count,
        sps,
        status,
        anchorAbs: this.displayAnchor.abs,
        anchorFrac: this.displayAnchor.frac,
      };
    }

    // untriggered sweep (auto timeout, or nothing fired yet): newest data
    return {
      fromAbs: Math.max(0, written - count),
      count,
      sps,
      status,
      anchorAbs: null,
      anchorFrac: 0,
    };
  }

  renderFrame(columns: number): RenderFrame {
    const cols = Math.floor(columns);
    const windowSec = DIVISIONS * this.secPerDiv;
    const win = this.currentWindow();
    this.lastWindow = win;

    const frame = this.frame;
    frame.windowSec = windowSec;
    if (!win || cols <= 0) {
      frame.status = this.running ? (this.triggerCfg.mode === "auto" ? "auto" : "wait") : "stop";
      frame.t0 = 0;
      frame.triggerFrac = 0;
      frame.traces.length = 0;
      return frame;
    }

    if (this.scratchMins.length < cols) {
      this.scratchMins = new Float32Array(cols);
      this.scratchMaxs = new Float32Array(cols);
    }

    const traces = frame.traces;
    let nTraces = 0;
    let visMin = Infinity;
    let visMax = -Infinity;

    for (const ch of this.chans.values()) {
      if (!ch.info.visible) continue;
      if (ch.ring.writtenTotal === 0) continue;

      // Clamp the window to this channel's history *without* rescaling: data
      // must keep the column position implied by its absolute index, or a
      // half-filled ring would stretch the trace across the whole screen.
      const first = ch.ring.firstAvailable;
      const written = ch.ring.writtenTotal;
      const from = Math.max(win.fromAbs, first);
      const to = Math.min(win.fromAbs + win.count, written);
      const count = to - from;
      if (count <= 0) continue;

      const colFrom = Math.min(cols, Math.floor(((from - win.fromAbs) * cols) / win.count));
      const colTo = Math.max(colFrom + 1, Math.min(cols, Math.ceil(((to - win.fromAbs) * cols) / win.count)));
      const cover = colTo - colFrom;

      minMaxDecimate(ch.ring, from, count, cover, {
        mins: this.scratchMins,
        maxs: this.scratchMaxs,
      });

      const plot = ch.plot;
      if (plot.mins.length !== cols) {
        plot.mins = new Float32Array(cols);
        plot.maxs = new Float32Array(cols);
      }
      plot.columns = cols;
      plot.color = ch.info.color;
      plot.mins.set(this.scratchMins.subarray(0, cover), colFrom);
      plot.maxs.set(this.scratchMaxs.subarray(0, cover), colFrom);
      // columns outside the available history hold the nearest known value:
      // TracePlot has no "no data" encoding, and a flat edge beats garbage
      if (colFrom > 0) {
        plot.mins.fill(this.scratchMins[0], 0, colFrom);
        plot.maxs.fill(this.scratchMaxs[0], 0, colFrom);
      }
      if (colTo < cols) {
        plot.mins.fill(this.scratchMins[cover - 1], colTo, cols);
        plot.maxs.fill(this.scratchMaxs[cover - 1], colTo, cols);
      }

      if (ch.info.id === this.triggerCfg.source || this.chans.size === 1) {
        for (let c = 0; c < cols; c++) {
          if (plot.mins[c] < visMin) visMin = plot.mins[c];
          if (plot.maxs[c] > visMax) visMax = plot.maxs[c];
        }
      }
      traces[nTraces++] = plot;
    }
    traces.length = nTraces;
    if (visMax > visMin) this.lastVisibleVpp = visMax - visMin;

    // Leftmost column relative to the trigger point. Untriggered sweeps have
    // no real trigger instant, but reporting the same -pre*dt keeps the trace
    // pinned to the graticule's trigger-position marker instead of jumping.
    const pre = Math.floor(this.triggerCfg.position * win.count);
    frame.status = win.status;
    frame.t0 = win.anchorAbs !== null ? (win.fromAbs - win.anchorAbs) / win.sps : -pre / win.sps;
    let triggerFrac = 0;
    if (win.anchorAbs !== null && win.count > 0) {
      const f = (win.anchorFrac * cols) / win.count;
      triggerFrac = f - Math.floor(f);
    }
    frame.triggerFrac = triggerFrac;
    return frame;
  }

  // ---------- measurements / fft / export ----------

  measure(channelId: number): Measurements | null {
    const ch = this.chans.get(channelId);
    if (!ch || ch.ring.writtenTotal === 0) return null;
    const win = this.lastWindow ?? this.currentWindow();
    if (!win || !(win.sps > 0)) return null;
    // reuse one buffer: windows reach 2^20 samples and measure() runs per tick
    const need = Math.min(win.count, ch.ring.capacity);
    if (this.scratchWindow.length < need) this.scratchWindow = new Float32Array(need);
    const buf = this.scratchWindow;
    const n = ch.ring.readInto(buf, win.fromAbs, win.count);
    if (n === 0) return null;
    return measureSamples(buf.subarray(0, n), 1 / win.sps);
  }

  private ensureWorker(): Worker | null {
    if (this.workerFailed) return null;
    if (this.worker) return this.worker;
    if (typeof Worker === "undefined") {
      this.workerFailed = true;
      return null;
    }
    try {
      this.worker = new Worker(new URL("./fftWorker.ts", import.meta.url), { type: "module" });
      this.worker.onmessage = (e: MessageEvent<FftWorkResponse>) => {
        const pending = this.fftPending.get(e.data.id);
        if (pending) {
          this.fftPending.delete(e.data.id);
          pending.resolve(e.data);
        }
      };
      this.worker.onerror = () => {
        // fail all pending, fall back to sync from now on
        for (const [, p] of this.fftPending) p.reject(new Error("fft worker error"));
        this.fftPending.clear();
        this.worker?.terminate();
        this.worker = null;
        this.workerFailed = true;
      };
      return this.worker;
    } catch {
      this.workerFailed = true;
      return null;
    }
  }

  async fft(channelId: number, size: number, window: FftWindowKind): Promise<FftResult | null> {
    const ch = this.chans.get(channelId);
    if (!ch || ch.ring.writtenTotal === 0) return null;
    const sps = ch.info.sps;
    if (!(sps > 0)) return null;

    // §4: sizes 1k-8k, power of two
    let want = 1;
    while (want * 2 <= Math.floor(size)) want *= 2;
    want = Math.max(FFT_MIN_SIZE, Math.min(FFT_MAX_SIZE, want));

    const avail = Math.min(ch.ring.writtenTotal - ch.ring.firstAvailable, want);
    if (avail < 2) return null;
    const samples = new Float32Array(avail);
    const written = ch.ring.writtenTotal;
    ch.ring.readInto(samples, written - avail, avail);

    // AC-couple the FFT block: a DC offset otherwise leaks through the
    // window skirt into the low bins and hijacks peak detection
    let mean = 0;
    for (let i = 0; i < avail; i++) mean += samples[i];
    mean /= avail;
    for (let i = 0; i < avail; i++) samples[i] -= mean;

    // bin spacing of the padded transform
    let n = 1;
    while (n < avail) n <<= 1;
    if (n < 2) n = 2;
    const binHz = sps / n;

    const worker = this.ensureWorker();
    if (worker) {
      try {
        const id = this.fftSeq++;
        const resp = await new Promise<FftWorkResponse>((resolve, reject) => {
          this.fftPending.set(id, { resolve, reject });
          // transfer a copy so `samples` stays usable for the sync fallback
          const buf = samples.slice().buffer as ArrayBuffer;
          const req: FftWorkRequest = { id, samples: buf, window, binHz };
          worker.postMessage(req, [buf]);
        });
        return {
          db: new Float32Array(resp.db),
          binHz,
          size: n,
          window,
          peakHz: resp.peakHz,
          peakDb: resp.peakDb,
        };
      } catch {
        // fall through to sync
      }
    }

    const db = realFftDb(samples, window);
    const { peakHz, peakDb } = parabolicPeak(db, binHz);
    return { db, binHz, size: n, window, peakHz, peakDb };
  }

  exportCsv(): string {
    const win = this.lastWindow ?? this.currentWindow();
    const chans = [...this.chans.values()].filter(
      (c) => c.info.visible && c.ring.writtenTotal > 0,
    );
    if (!win || chans.length === 0 || !(win.sps > 0)) return "";
    const dt = 1 / win.sps;
    const t0 = win.anchorAbs !== null ? (win.fromAbs - win.anchorAbs) / win.sps : 0;

    const lines: string[] = [];
    lines.push(`t0,${t0.toExponential(6)},s`);
    lines.push(`dt,${dt.toExponential(6)},s`);
    lines.push(
      "t," + chans.map((c) => `${c.info.name}${c.info.unit ? ` (${c.info.unit})` : ""}`).join(","),
    );

    const bufs = chans.map((c) => {
      const buf = new Float32Array(win.count);
      const n = c.ring.readInto(buf, win.fromAbs, win.count);
      return { buf, n };
    });
    const rows = Math.max(...bufs.map((b) => b.n));
    for (let i = 0; i < rows; i++) {
      const t = t0 + i * dt;
      let row = t.toExponential(6);
      for (const b of bufs) {
        row += "," + (i < b.n ? String(b.buf[i]) : "");
      }
      lines.push(row);
    }
    return lines.join("\n") + "\n";
  }

  stats(): EngineStats {
    return {
      droppedFrames: this.counters.droppedFrames,
      crcErrors: this.counters.crcErrors,
      overflows: this.counters.overflows,
      deviceSps: this.deviceSps,
    };
  }

  reset(): void {
    this.chans.clear();
    this.byName.clear();
    this.nextId = 1;
    this.trigger.reset();
    this.triggerScannedTo = 0;
    this.displayAnchor = null;
    this.pendingFire = null;
    this.lastVisibleVpp = 0;
    this.lastLineAt = null;
    this.emaIntervalMs = null;
    this.cal.reset();
    this.deviceSps = null;
    this.record = null;
    this.frozen = null;
    this.counters = { droppedFrames: 0, crcErrors: 0, overflows: 0 };
    this.lastWindow = null;
    this.running = true;
  }
}
