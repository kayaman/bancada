import { describe, expect, it } from "vitest";
import {
  decodeEnvelope,
  FLAG_LAST_IN_RECORD,
  sampleChannel,
  sampleValue,
} from "../binary";

function samplesEnvelope(
  flags: number,
  firstSampleIndex: number,
  samples: number[],
): ArrayBuffer {
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

describe("decodeEnvelope", () => {
  it("decodes a samples envelope from ArrayBuffer", () => {
    const packed = [(0 << 12) | 100, (1 << 12) | 4095, (0 << 12) | 0];
    const msg = decodeEnvelope(samplesEnvelope(0x01, 12345, packed));
    expect(msg.kind).toBe("samples");
    if (msg.kind !== "samples") return;
    expect(msg.flags).toBe(0x01);
    expect(msg.firstSampleIndex).toBe(12345);
    expect(Array.from(msg.samples)).toEqual(packed);
  });

  it("decodes a samples envelope from number[]", () => {
    const packed = [(1 << 12) | 0x0abc];
    const bytes = Array.from(new Uint8Array(samplesEnvelope(FLAG_LAST_IN_RECORD, 7, packed)));
    const msg = decodeEnvelope(bytes);
    expect(msg.kind).toBe("samples");
    if (msg.kind !== "samples") return;
    expect(msg.flags).toBe(FLAG_LAST_IN_RECORD);
    expect(msg.firstSampleIndex).toBe(7);
    expect(msg.samples.length).toBe(1);
    expect(sampleValue(msg.samples[0])).toBe(0x0abc);
    expect(sampleChannel(msg.samples[0])).toBe(1);
  });

  it("handles large first_sample_index (u32)", () => {
    const msg = decodeEnvelope(samplesEnvelope(0, 0xfffffffe, [1]));
    if (msg.kind !== "samples") throw new Error("expected samples");
    expect(msg.firstSampleIndex).toBe(0xfffffffe);
  });

  it("decodes a JSON event envelope", () => {
    const meta = { ev: "meta", sps: 50000, pins: [1], atten: 3, mode: "stream" };
    const msg = decodeEnvelope(jsonEnvelope(meta));
    expect(msg.kind).toBe("event");
    if (msg.kind !== "event") return;
    expect(msg.event).toEqual(meta);
  });

  it("decodes JSON event from number[]", () => {
    const bytes = Array.from(new Uint8Array(jsonEnvelope({ ev: "drop", frames: 3 })));
    const msg = decodeEnvelope(bytes);
    if (msg.kind !== "event") throw new Error("expected event");
    expect(msg.event).toEqual({ ev: "drop", frames: 3 });
  });

  it("throws on unknown kind / truncated payload", () => {
    expect(() => decodeEnvelope(new Uint8Array([0x7f]).buffer)).toThrow();
    // header claims 4 samples but only 1 present
    const buf = samplesEnvelope(0, 0, [1]);
    new DataView(buf).setUint16(6, 4, true);
    expect(() => decodeEnvelope(buf)).toThrow();
    expect(() => decodeEnvelope([])).toThrow();
  });

  it("unpacks 12-bit value and channel nibble", () => {
    expect(sampleValue(0xf123)).toBe(0x123);
    expect(sampleChannel(0xf123)).toBe(0xf);
    expect(sampleChannel(0x0fff)).toBe(0);
    expect(sampleValue(0x1fff)).toBe(0x0fff);
  });
});
