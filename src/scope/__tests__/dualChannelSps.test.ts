import { describe, expect, it } from "vitest";
import { ScopeEngine } from "../engine";

// Regression guard for the firmware/engine contract on META `sps`.
//
// The firmware clamps a start request to min(req, chipMax/nch, linkMax/nch),
// runs the ADC at sps*nch and reports the *per-channel* rate in META. An
// engine that treats that number as an aggregate and divides by the pin count
// halves the time base, which stretches dual-channel traces 2x horizontally
// and reports every frequency at half its true value.

function jsonEnvelope(obj: unknown): ArrayBuffer {
  const json = new TextEncoder().encode(JSON.stringify(obj));
  const out = new Uint8Array(1 + json.length);
  out[0] = 0x02;
  out.set(json, 1);
  return out.buffer;
}

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

const SPS = 100_000;
const FLAG_DUAL = 0x01;

function meta(pins: number[]) {
  return { ev: "meta", sps: SPS, pins, atten: 3, width: 12, mode: "stream" };
}

describe("META sps is per-channel", () => {
  it("reports the META rate verbatim for a single-pin stream", () => {
    const e = new ScopeEngine();
    e.setSource("adc");
    e.feedBinary(jsonEnvelope(meta([1])));
    e.feedBinary(samplesEnvelope(0, 0, [2048, 2048]));

    for (const ch of e.channels()) expect(ch.sps).toBe(SPS);
  });

  it("does not halve the rate for a dual-pin stream", () => {
    const e = new ScopeEngine();
    e.setSource("adc");
    e.feedBinary(jsonEnvelope(meta([1, 2])));
    // interleaved: channel index lives in bits 12-15
    e.feedBinary(
      samplesEnvelope(FLAG_DUAL, 0, [(0 << 12) | 2048, (1 << 12) | 2048, (0 << 12) | 2048, (1 << 12) | 2048]),
    );

    const chans = e.channels();
    expect(chans.length).toBe(2);
    for (const ch of chans) expect(ch.sps).toBe(SPS);
  });

  it("keeps the dual-pin time base identical to the single-pin one", () => {
    const single = new ScopeEngine();
    single.setSource("adc");
    single.feedBinary(jsonEnvelope(meta([1])));
    single.feedBinary(samplesEnvelope(0, 0, [2048]));

    const dual = new ScopeEngine();
    dual.setSource("adc");
    dual.feedBinary(jsonEnvelope(meta([1, 2])));
    dual.feedBinary(samplesEnvelope(FLAG_DUAL, 0, [(0 << 12) | 2048, (1 << 12) | 2048]));

    expect(dual.channels()[0].sps).toBe(single.channels()[0].sps);
  });
});
