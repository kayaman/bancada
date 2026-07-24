// Vite module worker: computes the FFT spectrum + peak off the main thread.
// Created by engine.ts via
//   new Worker(new URL("./fftWorker.ts", import.meta.url), { type: "module" })
// The engine falls back to the synchronous path when Worker is unavailable
// (vitest / node), so this file is never imported there.

import { parabolicPeak, realFftDb } from "./fft";
import type { FftWindowKind } from "./types";

export interface FftWorkRequest {
  id: number;
  /** transferred buffer holding Float32 samples */
  samples: ArrayBuffer;
  window: FftWindowKind;
  binHz: number;
}

export interface FftWorkResponse {
  id: number;
  /** transferred buffer holding Float32 dB bins */
  db: ArrayBuffer;
  peakHz: number | null;
  peakDb: number | null;
}

interface WorkerCtx {
  onmessage: ((e: MessageEvent<FftWorkRequest>) => void) | null;
  postMessage(msg: FftWorkResponse, transfer: Transferable[]): void;
}

const ctx = self as unknown as WorkerCtx;

ctx.onmessage = (e: MessageEvent<FftWorkRequest>) => {
  const { id, samples, window, binHz } = e.data;
  const db = realFftDb(new Float32Array(samples), window);
  const { peakHz, peakDb } = parabolicPeak(db, binHz);
  const buf = db.buffer as ArrayBuffer;
  ctx.postMessage({ id, db: buf, peakHz, peakDb }, [buf]);
};
