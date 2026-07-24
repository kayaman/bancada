import { afterEach, describe, expect, it, vi } from "vitest";
import { ScopeEngine } from "../engine";

// ---------- helpers ----------

function samplesEnvelope(flags: number, firstSampleIndex: number, samples: number[]): ArrayBuffer {
  const buf = new ArrayBuffer(8 + samples.length * 2);
  const v = new DataView(buf);
  v.setUint8(0, 0x01);
  v.setUint8(1, flags);
  v.setUint32(2, firstSampleIndex, true);
  v.setUint16(6, samples.length, true);
  samples.forEach((s, i) => v.setUint16(8 + i * 2, s, true));
  return buf;
}

function jsonEnvelope(obj: unknown): ArrayBuffer {
  const json = new TextEncoder().encode(JSON.stringify(obj));
  const out = new Uint8Array(1 + json.length);
  out[0] = 0x02;
  out.set(json, 1);
  return out.buffer;
}

/** packed 12-bit sine on channel nibble 0: raw = 2048 + 2047*sin(2*pi*f*i/sps) */
function packedSine(freq: number, sps: number, n: number, startIndex = 0): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    const raw = Math.round(2048 + 2047 * Math.sin((2 * Math.PI * freq * (startIndex + i)) / sps));
    out.push(raw & 0x0fff);
  }
  return out;
}

const META = { ev: "meta", sps: 100_000, pins: [1], atten: 3, width: 12, mode: "stream" };

afterEach(() => {
  vi.restoreAllMocks();
});

// ---------- plotter path ----------

describe("ScopeEngine plotter path", () => {
  it("feedText creates channels with palette colors and shared sps", () => {
    let t = 0;
    vi.spyOn(performance, "now").mockImplementation(() => (t += 10)); // 10 ms/line => 100 sps
    const e = new ScopeEngine();
    expect(e.source).toBe("plotter");
    for (let i = 0; i < 50; i++) {
      e.feedText(`sig:${Math.sin((2 * Math.PI * i) / 20).toFixed(5)} ref:1.0`);
    }
    const chans = e.channels();
    expect(chans.length).toBe(2);
    expect(chans[0].name).toBe("sig");
    expect(chans[0].color).toBe("#e8c34a");
    expect(chans[1].name).toBe("ref");
    expect(chans[1].color).toBe("#4ac3e8");
    expect(chans[0].written).toBe(50);
    expect(chans[0].sps).toBeCloseTo(100, 0);
  });

  it("ignores garbage lines and renders sane traces", () => {
    let t = 0;
    vi.spyOn(performance, "now").mockImplementation(() => (t += 10));
    const e = new ScopeEngine();
    e.feedText("booting...");
    expect(e.channels().length).toBe(0);
    for (let i = 0; i < 200; i++) {
      e.feedText(String(Math.sin((2 * Math.PI * i) / 20))); // bare number -> ch1
    }
    e.setSecPerDiv(0.1); // 1 s window at ~100 sps => 100 samples
    const frame = e.renderFrame(40);
    expect(frame.traces.length).toBe(1);
    expect(frame.traces[0].columns).toBe(40);
    expect(frame.windowSec).toBeCloseTo(1, 6);
    for (let c = 0; c < 40; c++) {
      expect(frame.traces[0].mins[c]).toBeGreaterThanOrEqual(-1.01);
      expect(frame.traces[0].maxs[c]).toBeLessThanOrEqual(1.01);
      expect(frame.traces[0].mins[c]).toBeLessThanOrEqual(frame.traces[0].maxs[c]);
    }
    expect(["auto", "trigd", "wait", "stop"]).toContain(frame.status);
  });
});

// ---------- adc path ----------

describe("ScopeEngine adc path", () => {
  function adcEngine(): ScopeEngine {
    const e = new ScopeEngine();
    e.setSource("adc");
    const evs = e.feedBinary(jsonEnvelope(META));
    expect(evs.length).toBe(1);
    expect(evs[0].ev).toBe("meta");
    return e;
  }

  it("routes samples to CH1 and converts raw counts to volts (default cal)", () => {
    const e = adcEngine();
    expect(e.stats().deviceSps).toBe(100_000);
    e.feedBinary(samplesEnvelope(0, 0, packedSine(1000, 100_000, 4096)));
    const chans = e.channels();
    expect(chans.length).toBe(1);
    expect(chans[0].name).toBe("CH1");
    expect(chans[0].unit).toBe("V");
    expect(chans[0].sps).toBe(100_000);
    expect(chans[0].written).toBe(4096);

    e.setSecPerDiv(0.001); // 10 ms window => 1000 samples
    const frame = e.renderFrame(100);
    expect(frame.traces.length).toBe(1);
    // default cal: 3.3 * raw / 4095 => values in [~0, 3.3]
    for (let c = 0; c < 100; c++) {
      expect(frame.traces[0].mins[c]).toBeGreaterThanOrEqual(-0.01);
      expect(frame.traces[0].maxs[c]).toBeLessThanOrEqual(3.31);
    }
  });

  it("routes channel nibble 1 to CH2", () => {
    const e = adcEngine();
    e.feedBinary(samplesEnvelope(0x01, 0, [(1 << 12) | 2048, (0 << 12) | 1024]));
    const names = e.channels().map((c) => c.name).sort();
    expect(names).toEqual(["CH1", "CH2"]);
  });

  it("anchors on the host trigger and measures the tone", () => {
    const e = adcEngine();
    e.setSecPerDiv(0.001); // 10 ms window, 1000 samples, pre=post=500
    e.setTrigger({ level: 1.65, edge: "rising", mode: "auto" });
    e.feedBinary(samplesEnvelope(0, 0, packedSine(1000, 100_000, 4096)));
    e.renderFrame(200);
    e.feedBinary(samplesEnvelope(0, 4096, packedSine(1000, 100_000, 4096, 4096)));
    const frame = e.renderFrame(200);
    expect(frame.status).toBe("trigd");
    // anchored: t0 = -position * window = -5 ms
    expect(frame.t0).toBeCloseTo(-0.005, 4);

    const m = e.measure(1);
    expect(m).not.toBeNull();
    expect(m!.freq).not.toBeNull();
    expect(Math.abs(m!.freq! - 1000) / 1000).toBeLessThan(0.01);
    expect(m!.vpp).toBeGreaterThan(3.0);
    expect(m!.vpp).toBeLessThan(3.4);
  });

  it("fft on the newest samples finds the tone (sync path in node)", async () => {
    const e = adcEngine();
    e.feedBinary(samplesEnvelope(0, 0, packedSine(1000, 100_000, 4096)));
    const r = await e.fft(1, 4096, "hann");
    expect(r).not.toBeNull();
    expect(r!.size).toBe(4096);
    expect(r!.binHz).toBeCloseTo(100_000 / 4096, 6);
    expect(r!.db.length).toBe(4096 / 2 + 1);
    expect(r!.peakHz).not.toBeNull();
    expect(Math.abs(r!.peakHz! - 1000)).toBeLessThan(5);
  });

  it("single-shot record freezes the display at trigger_idx", () => {
    const e = adcEngine();
    e.feedBinary(
      jsonEnvelope({ ev: "record", trigger_idx: 100, pre: 100, post: 300, sps: 1000, pin: 1 }),
    );
    const rec = packedSine(50, 1000, 400);
    e.feedBinary(samplesEnvelope(0, 0, rec.slice(0, 200)));
    expect(e.isRunning()).toBe(true); // not frozen until last-in-record
    e.feedBinary(samplesEnvelope(0x04, 200, rec.slice(200))); // FLAG_LAST_IN_RECORD
    expect(e.isRunning()).toBe(false);
    e.setSecPerDiv(0.02); // 200 ms window at 1 kS/s => 200 samples
    const frame = e.renderFrame(50);
    expect(frame.status).toBe("stop");
    expect(frame.traces.length).toBe(1);
    // run(true) unfreezes
    e.run(true);
    expect(e.isRunning()).toBe(true);
  });

  it("counts drop/crc/overflow events and closed stops the run", () => {
    const e = adcEngine();
    e.feedBinary(jsonEnvelope({ ev: "drop", frames: 4 }));
    e.feedBinary(jsonEnvelope({ ev: "crc", count: 2 }));
    e.feedBinary(samplesEnvelope(0x08, 0, [100])); // overflow flag
    const st = e.stats();
    expect(st.droppedFrames).toBe(4);
    expect(st.crcErrors).toBe(2);
    expect(st.overflows).toBe(1);
    const evs = e.feedBinary(jsonEnvelope({ ev: "closed" }));
    expect(evs[0].ev).toBe("closed");
    expect(e.isRunning()).toBe(false);
  });

  it("exportCsv produces header rows and data", () => {
    const e = adcEngine();
    e.feedBinary(samplesEnvelope(0, 0, packedSine(1000, 100_000, 2048)));
    e.setSecPerDiv(0.0005); // 5 ms => 500 samples
    e.renderFrame(100);
    const csv = e.exportCsv();
    const lines = csv.trim().split("\n");
    expect(lines[0].startsWith("t0,")).toBe(true);
    expect(lines[1].startsWith("dt,")).toBe(true);
    expect(lines[2].startsWith("t,CH1")).toBe(true);
    expect(lines.length).toBeGreaterThan(100);
    const firstRow = lines[3].split(",");
    expect(Number.isFinite(Number(firstRow[0]))).toBe(true);
    expect(Number.isFinite(Number(firstRow[1]))).toBe(true);
  });

  it("reset wipes channels, counters and state", () => {
    const e = adcEngine();
    e.feedBinary(samplesEnvelope(0, 0, [1, 2, 3]));
    e.feedBinary(jsonEnvelope({ ev: "drop", frames: 1 }));
    e.reset();
    expect(e.channels().length).toBe(0);
    expect(e.stats().droppedFrames).toBe(0);
    expect(e.stats().deviceSps).toBeNull();
    expect(e.isRunning()).toBe(true);
  });

  it("META cal table overrides the default conversion", () => {
    const e = new ScopeEngine();
    e.setSource("adc");
    // cal: 0 raw -> 0 mV, 4096 raw -> 4096 mV (i.e. 1 mV per count)
    e.feedBinary(jsonEnvelope({ ...META, sps: 1000, cal: [[0, 0], [4096, 4096]] }));
    e.feedBinary(samplesEnvelope(0, 0, [1000]));
    e.setSecPerDiv(0.001);
    const frame = e.renderFrame(4);
    expect(frame.traces.length).toBe(1);
    // 1000 raw -> 1.000 V
    expect(frame.traces[0].maxs[3]).toBeCloseTo(1.0, 3);
  });
});
