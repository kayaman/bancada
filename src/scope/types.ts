// Shared contracts for the scope engine (src/scope/) and UI (ScopeView).
// See docs/scope-architecture.md for the wire protocols behind these types.

export type ScopeSourceKind = "plotter" | "adc";

export type TriggerMode = "auto" | "normal" | "single";
export type TriggerEdge = "rising" | "falling";
export type TriggerStatus = "auto" | "trigd" | "wait" | "stop";

export interface TriggerConfig {
  mode: TriggerMode;
  /** channel id the trigger watches */
  source: number;
  /** trigger level in channel units (volts for ADC, raw units for plotter) */
  level: number;
  edge: TriggerEdge;
  /** hysteresis band in channel units; 0 = auto (2% of visible Vpp) */
  hysteresis: number;
  /** minimum seconds between trigger fires */
  holdoff: number;
  /** horizontal trigger position, 0..1 (fraction of window that is pre-trigger) */
  position: number;
}

export interface ChannelInfo {
  id: number;
  name: string;
  color: string;
  unit: string; // "V" | ""
  visible: boolean;
  /** vertical scale: units per division (10 divisions vertical) */
  voltsPerDiv: number;
  /** vertical offset in units */
  offset: number;
  /** best-known sample rate for this channel (Hz) */
  sps: number;
  /** total samples ever written */
  written: number;
}

/** One channel's polyline for a frame: min/max pair per column. */
export interface TracePlot {
  channelId: number;
  color: string;
  /** columns = number of x positions; mins/maxs have `columns` entries, in units */
  columns: number;
  mins: Float32Array;
  maxs: Float32Array;
}

export interface RenderFrame {
  status: TriggerStatus;
  /** seconds spanned by the frame window */
  windowSec: number;
  /** time of leftmost column relative to trigger point (seconds, negative = pre-trigger) */
  t0: number;
  /** sub-sample trigger interpolation offset, in fractions of a column [0,1) */
  triggerFrac: number;
  traces: TracePlot[];
}

export interface Measurements {
  min: number;
  max: number;
  vpp: number;
  mean: number;
  rms: number;
  /** null when fewer than 2 stable periods in window */
  freq: number | null;
  period: number | null;
  /** positive duty cycle 0..1, null with freq */
  dutyPos: number | null;
}

export type FftWindowKind = "hann" | "rect" | "flattop";

export interface FftResult {
  /** magnitudes in dB (relative full scale of the channel unit) */
  db: Float32Array;
  /** Hz per bin */
  binHz: number;
  size: number;
  window: FftWindowKind;
  /** frequency of the interpolated peak, Hz */
  peakHz: number | null;
  peakDb: number | null;
}

/** JSON events surfaced from the Rust channel stream (kind=2) or engine itself. */
export type ScopeEvent =
  | { ev: "meta"; sps: number; pins: number[]; atten: number; width?: number; mode: "stream" | "single"; overflows?: number; cal?: [number, number][] }
  | { ev: "record"; trigger_idx: number; pre: number; post: number; sps: number; pin: number }
  | { ev: "err"; msg: string }
  | { ev: "drop"; frames: number }
  | { ev: "crc"; count: number }
  | { ev: "closed" };

export interface EngineStats {
  droppedFrames: number;
  crcErrors: number;
  overflows: number;
  /** actual device sample rate from last META */
  deviceSps: number | null;
}

/**
 * The scope engine: rings + trigger + measurements + decimation.
 * Implemented in src/scope/engine.ts; UI consumes only this surface.
 */
export interface IScopeEngine {
  readonly source: ScopeSourceKind;
  setSource(kind: ScopeSourceKind): void;

  /** Plotter path: feed one text line from serial://line. */
  feedText(line: string): void;
  /** ADC path: feed one binary Channel message (envelope of §2). Returns any JSON events. */
  feedBinary(data: ArrayBuffer | number[]): ScopeEvent[];

  channels(): ChannelInfo[];
  setChannel(id: number, patch: Partial<Pick<ChannelInfo, "visible" | "voltsPerDiv" | "offset" | "name">>): void;

  getTrigger(): TriggerConfig;
  setTrigger(patch: Partial<TriggerConfig>): void;
  /** run/stop control; single mode re-arms via run(true) */
  run(on: boolean): void;
  isRunning(): boolean;

  /** seconds per horizontal division (10 divisions) */
  setSecPerDiv(s: number): void;
  getSecPerDiv(): number;

  /** Produce the current display frame, decimated to `columns` x-positions. */
  renderFrame(columns: number): RenderFrame;

  /** Measurements over the currently displayed window for a channel. */
  measure(channelId: number): Measurements | null;

  /** FFT of the newest `size` samples of a channel (via worker). */
  fft(channelId: number, size: number, window: FftWindowKind): Promise<FftResult | null>;

  /** CSV of the displayed window, Rigol-style header. */
  exportCsv(): string;

  stats(): EngineStats;
  /** wipe all rings/channels (source switch, port change) */
  reset(): void;
}
