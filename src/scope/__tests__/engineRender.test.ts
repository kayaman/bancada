// Render-path contract: buffer reuse (no per-frame allocation), column
// alignment of partially-filled windows, t0 on untriggered sweeps, FFT size
// clamping, and source isolation of feedText.

import { describe, expect, it } from "vitest";
import { ScopeEngine } from "../engine";

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

/** ADC engine at 1 kS/s with a 1 mV-per-count cal table (raw N -> N/1000 V). */
function adcEngine(sps = 1000): ScopeEngine {
  const e = new ScopeEngine();
  e.setSource("adc");
  e.feedBinary(
    jsonEnvelope({
      ev: "meta",
      sps,
      pins: [1],
      atten: 3,
      width: 12,
      mode: "stream",
      cal: [
        [0, 0],
        [4096, 4096],
      ],
    }),
  );
  return e;
}

describe("renderFrame allocation reuse", () => {
  it("returns the same frame and trace buffers across frames at a fixed width", () => {
    const e = adcEngine();
    e.feedBinary(samplesEnvelope(0, 0, [100, 200, 300, 400]));
    e.setSecPerDiv(0.001);

    const a = e.renderFrame(200);
    const aMins = a.traces[0].mins;
    const aMaxs = a.traces[0].maxs;
    const b = e.renderFrame(200);

    expect(b).toBe(a); // frame object reused
    expect(b.traces[0]).toBe(a.traces[0]); // TracePlot reused
    expect(b.traces[0].mins).toBe(aMins); // column buffers reused
    expect(b.traces[0].maxs).toBe(aMaxs);
  });

  it("reallocates only when the column count changes", () => {
    const e = adcEngine();
    e.feedBinary(samplesEnvelope(0, 0, [100, 200, 300, 400]));
    const wide = e.renderFrame(200).traces[0].mins;
    const narrow = e.renderFrame(50).traces[0].mins;
    expect(narrow).not.toBe(wide);
    expect(narrow.length).toBe(50);
    // ...and is stable again at the new width
    expect(e.renderFrame(50).traces[0].mins).toBe(narrow);
  });

  it("drops traces for hidden channels without leaking stale entries", () => {
    const e = adcEngine();
    e.feedBinary(samplesEnvelope(0x01, 0, [(0 << 12) | 500, (1 << 12) | 900]));
    expect(e.renderFrame(20).traces.length).toBe(2);
    e.setChannel(2, { visible: false });
    const f = e.renderFrame(20);
    expect(f.traces.length).toBe(1);
    expect(f.traces[0].channelId).toBe(1);
  });
});

describe("partial windows keep column alignment", () => {
  it("does not stretch a half-filled ring across the whole width", () => {
    const e = adcEngine(1000);
    // 20 samples of a ramp; window is 100 samples (0.01 s/div * 10 div)
    const ramp: number[] = [];
    for (let i = 0; i < 20; i++) ramp.push(i * 100);
    e.feedBinary(samplesEnvelope(0, 0, ramp));
    e.setSecPerDiv(0.01);

    const f = e.renderFrame(10);
    expect(f.traces.length).toBe(1);
    const maxs = f.traces[0].maxs;
    // 20 of 100 samples => data occupies columns 0..1; the rest holds the
    // last known value (1900 raw = 1.9 V). A stretched trace would ramp.
    expect(maxs[1]).toBeCloseTo(1.9, 3);
    expect(maxs[5]).toBeCloseTo(1.9, 3);
    expect(maxs[9]).toBeCloseTo(1.9, 3);
    // first column really is the start of the ramp, not a rescaled midpoint
    expect(maxs[0]).toBeLessThan(1.0);
  });

  it("fills the full width once the window is covered", () => {
    const e = adcEngine(1000);
    const ramp: number[] = [];
    for (let i = 0; i < 100; i++) ramp.push(i * 40);
    e.feedBinary(samplesEnvelope(0, 0, ramp));
    e.setSecPerDiv(0.01); // exactly 100 samples

    const maxs = e.renderFrame(10).traces[0].maxs;
    for (let c = 1; c < 10; c++) expect(maxs[c]).toBeGreaterThan(maxs[c - 1]);
  });
});

describe("t0 on untriggered sweeps", () => {
  it("reports -position * window so the trace stays on the trigger marker", () => {
    const e = adcEngine(1000);
    const flat: number[] = [];
    for (let i = 0; i < 200; i++) flat.push(2000); // never crosses level 0
    e.feedBinary(samplesEnvelope(0, 0, flat));
    e.setSecPerDiv(0.01); // 0.1 s window, 100 samples, position 0.5

    const f = e.renderFrame(50);
    expect(f.windowSec).toBeCloseTo(0.1, 9);
    expect(f.t0).toBeCloseTo(-0.05, 9);
    expect(f.triggerFrac).toBe(0);

    e.setTrigger({ position: 0.25 });
    expect(e.renderFrame(50).t0).toBeCloseTo(-0.025, 9);
  });
});

describe("fft size clamping (§4: 1k..8k)", () => {
  it("clamps an undersized request up to 1024", async () => {
    const e = adcEngine(100_000);
    const n: number[] = [];
    for (let i = 0; i < 4096; i++) {
      n.push(Math.round(2048 + 2000 * Math.sin((2 * Math.PI * 1000 * i) / 100_000)) & 0x0fff);
    }
    e.feedBinary(samplesEnvelope(0, 0, n));
    const r = await e.fft(1, 64, "hann");
    expect(r).not.toBeNull();
    expect(r!.size).toBe(1024);
    expect(r!.db.length).toBe(1024 / 2 + 1);
  });

  it("clamps an oversized request down to 8192", async () => {
    const e = adcEngine(100_000);
    const n: number[] = [];
    for (let i = 0; i < 9000; i++) {
      n.push(Math.round(2048 + 2000 * Math.sin((2 * Math.PI * 1000 * i) / 100_000)) & 0x0fff);
    }
    e.feedBinary(samplesEnvelope(0, 0, n));
    const r = await e.fft(1, 1 << 20, "hann");
    expect(r).not.toBeNull();
    expect(r!.size).toBe(8192);
    expect(r!.peakHz).not.toBeNull();
    expect(Math.abs(r!.peakHz! - 1000)).toBeLessThan(5);
  });

  it("rounds a non-power-of-two request down", async () => {
    const e = adcEngine(100_000);
    const n: number[] = [];
    for (let i = 0; i < 4096; i++) n.push(2048);
    e.feedBinary(samplesEnvelope(0, 0, n));
    const r = await e.fft(1, 3000, "hann");
    expect(r!.size).toBe(2048);
  });
});

describe("source isolation", () => {
  it("feedText is ignored while the ADC source owns the display", () => {
    const e = adcEngine();
    e.feedText("temp:23.5 hum:40");
    expect(e.channels().length).toBe(0);
    e.feedBinary(samplesEnvelope(0, 0, [1000]));
    expect(e.channels().map((c) => c.name)).toEqual(["CH1"]);
  });

  it("switching source wipes channels and counters", () => {
    const e = adcEngine();
    e.feedBinary(jsonEnvelope({ ev: "drop", frames: 3 }));
    e.feedBinary(samplesEnvelope(0, 0, [1000]));
    expect(e.channels().length).toBe(1);
    e.setSource("plotter");
    expect(e.channels().length).toBe(0);
    expect(e.stats().droppedFrames).toBe(0);
    expect(e.stats().deviceSps).toBeNull();
  });
});
