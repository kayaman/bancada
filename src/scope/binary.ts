// Decoder for the Rust -> frontend Channel envelope (scope-architecture §2).
//
//   kind 0x01 samples: u8 kind, u8 flags, u32 first_sample_index (LE),
//                      u16 nsamples (LE), u16[nsamples] samples (LE)
//   kind 0x02 json:    u8 kind, rest UTF-8 JSON ScopeEvent
//
// Sample packing: bits 0-11 raw ADC counts, bits 12-15 channel index.

import type { ScopeEvent } from "./types";

export const KIND_SAMPLES = 0x01;
export const KIND_JSON = 0x02;

// flags (as device frame §1.1)
export const FLAG_DUAL_CHANNEL = 0x01;
export const FLAG_LAST_IN_RECORD = 0x04;
export const FLAG_OVERFLOW = 0x08;

export interface SamplesMessage {
  kind: "samples";
  flags: number;
  firstSampleIndex: number;
  samples: Uint16Array;
}

export interface EventMessage {
  kind: "event";
  event: ScopeEvent;
}

export type ScopeMessage = SamplesMessage | EventMessage;

const HEADER_BYTES = 8; // kind(1) + flags(1) + first_sample_index(4) + nsamples(2)

/** 12-bit raw ADC counts of a packed sample word. */
export function sampleValue(s: number): number {
  return s & 0x0fff;
}

/** Channel index (0 or 1) of a packed sample word. */
export function sampleChannel(s: number): number {
  return s >>> 12;
}

function toBytes(data: ArrayBuffer | number[]): Uint8Array {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  return Uint8Array.from(data);
}

/**
 * Decode one Channel message. Throws on malformed envelopes (unknown kind,
 * truncated header/payload, bad JSON).
 */
export function decodeEnvelope(data: ArrayBuffer | number[]): ScopeMessage {
  const bytes = toBytes(data);
  if (bytes.length < 1) throw new Error("scope envelope: empty message");
  const kind = bytes[0];

  if (kind === KIND_JSON) {
    const json = new TextDecoder().decode(bytes.subarray(1));
    const event = JSON.parse(json) as ScopeEvent;
    if (typeof event !== "object" || event === null || typeof (event as { ev?: unknown }).ev !== "string") {
      throw new Error("scope envelope: JSON event missing 'ev'");
    }
    return { kind: "event", event };
  }

  if (kind === KIND_SAMPLES) {
    if (bytes.length < HEADER_BYTES) {
      throw new Error(`scope envelope: truncated samples header (${bytes.length} B)`);
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const flags = view.getUint8(1);
    const firstSampleIndex = view.getUint32(2, true);
    const nsamples = view.getUint16(6, true);
    if (bytes.length < HEADER_BYTES + nsamples * 2) {
      throw new Error(
        `scope envelope: truncated samples payload (want ${nsamples * 2} B, have ${bytes.length - HEADER_BYTES})`,
      );
    }
    let samples: Uint16Array;
    const payloadOffset = bytes.byteOffset + HEADER_BYTES;
    if (payloadOffset % 2 === 0) {
      // aligned view — copy so the message owns its data independent of the
      // (possibly reused) transport buffer
      samples = new Uint16Array(bytes.buffer.slice(payloadOffset, payloadOffset + nsamples * 2));
    } else {
      samples = new Uint16Array(nsamples);
      for (let i = 0; i < nsamples; i++) {
        samples[i] = view.getUint16(HEADER_BYTES + i * 2, true);
      }
    }
    return { kind: "samples", flags, firstSampleIndex, samples };
  }

  throw new Error(`scope envelope: unknown kind 0x${kind.toString(16)}`);
}
