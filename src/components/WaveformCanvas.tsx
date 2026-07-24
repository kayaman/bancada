// WaveformCanvas — the instrument display. Plain Canvas 2D driven by a rAF
// loop: graticule, min/max decimated traces, draggable trigger level line,
// cursors (X/Y/track) and status overlays. HiDPI aware.

import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { IScopeEngine, RenderFrame, TriggerStatus } from "../scope/types";

export type CursorMode = "off" | "x" | "y" | "track";

export interface WaveformCanvasHandle {
  /** PNG snapshot of the current display (Export PNG). */
  snapshotPng(): Promise<Blob>;
}

interface Props {
  engine: IScopeEngine;
  cursorMode: CursorMode;
  /** drawing is skipped while the scope view is hidden */
  active: boolean;
  /** trigger status of the latest frame — called on change only */
  onStatus?: (s: TriggerStatus) => void;
  /** a canvas drag moved the trigger level (engine already updated) */
  onTriggerLevel?: (level: number) => void;
}

type DragKind = "trig" | "x1" | "x2" | "y1" | "y2";

const BG = "#10141b";
const GRID = "rgba(139, 147, 161, 0.14)";
const GRID_AXIS = "rgba(139, 147, 161, 0.32)";
const GRID_TICK = "rgba(139, 147, 161, 0.38)";
const HIT_PX = 4;

// ---------- formatting helpers (also used by ScopeView) ----------

export function fmtSI(v: number | null | undefined, unit = ""): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return "—";
  const a = Math.abs(v);
  let scaled = v;
  let prefix = "";
  if (a >= 1e6) {
    scaled = v / 1e6;
    prefix = "M";
  } else if (a >= 1e3) {
    scaled = v / 1e3;
    prefix = "k";
  } else if (a >= 1 || a === 0) {
    // as-is
  } else if (a >= 1e-3) {
    scaled = v * 1e3;
    prefix = "m";
  } else if (a >= 1e-6) {
    scaled = v * 1e6;
    prefix = "µ";
  } else {
    scaled = v * 1e9;
    prefix = "n";
  }
  const num = String(Number(scaled.toPrecision(4)));
  const suffix = `${prefix}${unit}`;
  return suffix ? `${num} ${suffix}` : num;
}

export const fmtTime = (s: number | null | undefined) => fmtSI(s, "s");
export const fmtHz = (v: number | null | undefined) => fmtSI(v, "Hz");

// ---------- unit ↔ pixel transforms (10 vertical divisions) ----------
// Convention: `offset` is the channel value sitting at the vertical center.

interface VScale {
  voltsPerDiv: number;
  offset: number;
}

const yOfValue = (v: number, s: VScale, H: number) =>
  H / 2 - ((v - s.offset) / s.voltsPerDiv) * (H / 10);
const valueOfY = (y: number, s: VScale, H: number) =>
  s.offset + ((H / 2 - y) * 10 * s.voltsPerDiv) / H;

function drawGraticule(ctx: CanvasRenderingContext2D, W: number, H: number) {
  ctx.lineWidth = 1;
  ctx.strokeStyle = GRID;
  ctx.beginPath();
  for (let i = 1; i < 10; i++) {
    const x = Math.round((W * i) / 10) + 0.5;
    ctx.moveTo(x, 0);
    ctx.lineTo(x, H);
    const y = Math.round((H * i) / 10) + 0.5;
    ctx.moveTo(0, y);
    ctx.lineTo(W, y);
  }
  ctx.stroke();
  // center axes, slightly brighter
  ctx.strokeStyle = GRID_AXIS;
  ctx.beginPath();
  const cx = Math.round(W / 2) + 0.5;
  const cy = Math.round(H / 2) + 0.5;
  ctx.moveTo(cx, 0);
  ctx.lineTo(cx, H);
  ctx.moveTo(0, cy);
  ctx.lineTo(W, cy);
  ctx.stroke();
  // dotted minor ticks along the center axes (5 per division)
  ctx.fillStyle = GRID_TICK;
  for (let i = 0; i <= 50; i++) {
    const x = (W * i) / 50;
    ctx.fillRect(x, H / 2 - 2, 1, 4);
    const y = (H * i) / 50;
    ctx.fillRect(W / 2 - 2, y, 4, 1);
  }
}

const WaveformCanvas = forwardRef<WaveformCanvasHandle, Props>(
  function WaveformCanvas(
    { engine, cursorMode, active, onStatus, onTriggerLevel },
    ref,
  ) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    // props mirrored into refs so the single rAF loop sees fresh values
    const modeRef = useRef<CursorMode>(cursorMode);
    modeRef.current = cursorMode;
    const activeRef = useRef(active);
    activeRef.current = active;
    const onStatusRef = useRef(onStatus);
    onStatusRef.current = onStatus;
    const onTrigLevelRef = useRef(onTriggerLevel);
    onTrigLevelRef.current = onTriggerLevel;

    const lastStatus = useRef<TriggerStatus | null>(null);
    const frameRef = useRef<RenderFrame | null>(null);
    // cursor positions as fractions of width/height (resize-stable)
    const curs = useRef({ x1: 0.35, x2: 0.65, y1: 0.35, y2: 0.65 });
    const drag = useRef<DragKind | null>(null);

    useImperativeHandle(
      ref,
      () => ({
        snapshotPng: () =>
          new Promise<Blob>((resolve, reject) => {
            const c = canvasRef.current;
            if (!c) return reject(new Error("canvas not mounted"));
            c.toBlob(
              (b) => (b ? resolve(b) : reject(new Error("PNG encode failed"))),
              "image/png",
            );
          }),
      }),
      [],
    );

    // ---------- rAF draw loop ----------

    useEffect(() => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      let raf = 0;

      const drawCursors = (
        W: number,
        H: number,
        frame: RenderFrame,
        trigSource: number,
        trigCh: VScale & { unit: string; color: string } | undefined,
      ) => {
        const mode = modeRef.current;
        if (mode === "off") return;
        const lines: string[] = [];
        const unit = trigCh?.unit ?? "";
        ctx.save();
        ctx.setLineDash([3, 3]);
        ctx.strokeStyle = "rgba(45, 212, 191, 0.9)";
        ctx.lineWidth = 1;
        if (mode === "x" || mode === "track") {
          for (const f of [curs.current.x1, curs.current.x2]) {
            const x = Math.round(f * W) + 0.5;
            ctx.beginPath();
            ctx.moveTo(x, 0);
            ctx.lineTo(x, H);
            ctx.stroke();
          }
          const t1 = frame.t0 + curs.current.x1 * frame.windowSec;
          const t2 = frame.t0 + curs.current.x2 * frame.windowSec;
          const dt = t2 - t1;
          lines.push(`X1 ${fmtTime(t1)}  X2 ${fmtTime(t2)}`);
          lines.push(
            `ΔX ${fmtTime(dt)}  1/ΔX ${dt !== 0 ? fmtHz(1 / Math.abs(dt)) : "—"}`,
          );
          if (mode === "track") {
            const tr =
              frame.traces.find((t) => t.channelId === trigSource) ??
              frame.traces[0];
            if (tr && tr.columns > 0) {
              const vAt = (f: number) => {
                const i = Math.min(
                  tr.columns - 1,
                  Math.max(0, Math.round(f * tr.columns)),
                );
                return (tr.mins[i] + tr.maxs[i]) / 2;
              };
              const v1 = vAt(curs.current.x1);
              const v2 = vAt(curs.current.x2);
              lines.push(`Y@X1 ${fmtSI(v1, unit)}  Y@X2 ${fmtSI(v2, unit)}`);
              lines.push(`ΔY ${fmtSI(v2 - v1, unit)}`);
              // dot markers where the cursors meet the trace
              const chScale = trigCh;
              if (chScale) {
                ctx.setLineDash([]);
                ctx.fillStyle = tr.color;
                for (const [f, v] of [
                  [curs.current.x1, v1],
                  [curs.current.x2, v2],
                ] as const) {
                  ctx.beginPath();
                  ctx.arc(f * W, yOfValue(v, chScale, H), 3, 0, Math.PI * 2);
                  ctx.fill();
                }
              }
            }
          }
        } else {
          // mode === "y"
          for (const f of [curs.current.y1, curs.current.y2]) {
            const y = Math.round(f * H) + 0.5;
            ctx.beginPath();
            ctx.moveTo(0, y);
            ctx.lineTo(W, y);
            ctx.stroke();
          }
          if (trigCh) {
            const v1 = valueOfY(curs.current.y1 * H, trigCh, H);
            const v2 = valueOfY(curs.current.y2 * H, trigCh, H);
            lines.push(`Y1 ${fmtSI(v1, unit)}  Y2 ${fmtSI(v2, unit)}`);
            lines.push(`ΔY ${fmtSI(v2 - v1, unit)}`);
          }
        }
        ctx.restore();

        if (lines.length) {
          ctx.save();
          ctx.font = "11px monospace";
          ctx.textBaseline = "top";
          ctx.textAlign = "left";
          const w = Math.max(...lines.map((l) => ctx.measureText(l).width)) + 12;
          const h = lines.length * 14 + 8;
          ctx.fillStyle = "rgba(16, 20, 27, 0.85)";
          ctx.fillRect(6, 6, w, h);
          ctx.strokeStyle = "#303743";
          ctx.strokeRect(6.5, 6.5, w, h);
          ctx.fillStyle = "#d7dce4";
          lines.forEach((l, i) => ctx.fillText(l, 12, 11 + i * 14));
          ctx.restore();
        }
      };

      const draw = () => {
        raf = requestAnimationFrame(draw);
        if (!activeRef.current) return;
        const W = canvas.clientWidth;
        const H = canvas.clientHeight;
        if (W < 20 || H < 20) return;
        const dpr = window.devicePixelRatio || 1;
        const pw = Math.round(W * dpr);
        const ph = Math.round(H * dpr);
        if (canvas.width !== pw || canvas.height !== ph) {
          canvas.width = pw;
          canvas.height = ph;
        }
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

        ctx.fillStyle = BG;
        ctx.fillRect(0, 0, W, H);
        drawGraticule(ctx, W, H);

        const frame = engine.renderFrame(Math.floor(W));
        frameRef.current = frame;
        if (frame.status !== lastStatus.current) {
          lastStatus.current = frame.status;
          onStatusRef.current?.(frame.status);
        }

        const chans = engine.channels();
        const chById = new Map(chans.map((c) => [c.id, c]));

        // ---- traces: one vertical min→max segment per column ----
        for (const t of frame.traces) {
          const ch = chById.get(t.channelId);
          if (!ch || !ch.visible || t.columns === 0) continue;
          const xStep = W / t.columns;
          ctx.strokeStyle = t.color || ch.color;
          ctx.lineWidth = 1;
          ctx.beginPath();
          let down = true; // alternate sweep direction to stay connected
          for (let i = 0; i < t.columns; i++) {
            const x = (i + 0.5 - frame.triggerFrac) * xStep;
            let yHi = yOfValue(t.maxs[i], ch, H);
            let yLo = yOfValue(t.mins[i], ch, H);
            if (yLo - yHi < 1) {
              const m = (yLo + yHi) / 2;
              yHi = m - 0.5;
              yLo = m + 0.5;
            }
            if (i === 0) ctx.moveTo(x, down ? yHi : yLo);
            else ctx.lineTo(x, down ? yHi : yLo);
            ctx.lineTo(x, down ? yLo : yHi);
            down = !down;
          }
          ctx.stroke();
        }

        const trig = engine.getTrigger();
        const trigCh = chById.get(trig.source);

        // ---- trigger level line (dashed, draggable) + "T" position marker ----
        if (trigCh) {
          const y = yOfValue(trig.level, trigCh, H);
          ctx.save();
          ctx.strokeStyle = trigCh.color;
          ctx.globalAlpha = 0.8;
          ctx.setLineDash([5, 4]);
          ctx.beginPath();
          ctx.moveTo(0, y);
          ctx.lineTo(W, y);
          ctx.stroke();
          ctx.restore();

          const tx = trig.position * W;
          ctx.fillStyle = trigCh.color;
          ctx.beginPath();
          ctx.moveTo(tx - 5, 0);
          ctx.lineTo(tx + 5, 0);
          ctx.lineTo(tx, 7);
          ctx.closePath();
          ctx.fill();
          ctx.font = "10px monospace";
          ctx.textAlign = "center";
          ctx.textBaseline = "top";
          ctx.fillText("T", tx, 9);
        }

        drawCursors(W, H, frame, trig.source, trigCh);

        // ---- status overlays ----
        if (frame.status === "wait") {
          ctx.save();
          ctx.font = "12px monospace";
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          const msg = "Waiting for trigger…";
          const w = ctx.measureText(msg).width + 20;
          ctx.fillStyle = "rgba(16, 20, 27, 0.8)";
          ctx.fillRect(W / 2 - w / 2, 14, w, 22);
          ctx.fillStyle = "#fbbf24";
          ctx.fillText(msg, W / 2, 25);
          ctx.restore();
        }
        const sps =
          trigCh?.sps ?? chans.find((c) => c.visible)?.sps ?? null;
        ctx.save();
        ctx.font = "10px monospace";
        ctx.textAlign = "right";
        ctx.textBaseline = "bottom";
        ctx.fillStyle = "rgba(139, 147, 161, 0.7)";
        ctx.fillText(
          `${sps ? fmtSI(sps, "S/s") : "— S/s"} · ${fmtTime(frame.windowSec)} window`,
          W - 8,
          H - 6,
        );
        ctx.restore();
      };

      raf = requestAnimationFrame(draw);
      // ResizeObserver keeps the backing store in sync promptly on layout
      // changes (per-frame check covers the rest).
      const ro = new ResizeObserver(() => {
        /* sizes are re-checked at the top of every frame */
      });
      ro.observe(canvas);
      return () => {
        cancelAnimationFrame(raf);
        ro.disconnect();
      };
    }, [engine]);

    // ---------- pointer interaction (trigger level + cursors) ----------

    const hitTest = (
      px: number,
      py: number,
      W: number,
      H: number,
    ): DragKind | null => {
      const mode = modeRef.current;
      if (mode === "x" || mode === "track") {
        if (Math.abs(px - curs.current.x1 * W) <= HIT_PX) return "x1";
        if (Math.abs(px - curs.current.x2 * W) <= HIT_PX) return "x2";
      } else if (mode === "y") {
        if (Math.abs(py - curs.current.y1 * H) <= HIT_PX) return "y1";
        if (Math.abs(py - curs.current.y2 * H) <= HIT_PX) return "y2";
      }
      const trig = engine.getTrigger();
      const ch = engine.channels().find((c) => c.id === trig.source);
      if (ch && Math.abs(py - yOfValue(trig.level, ch, H)) <= HIT_PX + 1)
        return "trig";
      return null;
    };

    const onPointerDown = (e: ReactPointerEvent<HTMLCanvasElement>) => {
      const c = canvasRef.current;
      if (!c) return;
      const r = c.getBoundingClientRect();
      const k = hitTest(e.clientX - r.left, e.clientY - r.top, r.width, r.height);
      if (!k) return;
      drag.current = k;
      c.setPointerCapture(e.pointerId);
      e.preventDefault();
    };

    const onPointerMove = (e: ReactPointerEvent<HTMLCanvasElement>) => {
      const c = canvasRef.current;
      if (!c) return;
      const r = c.getBoundingClientRect();
      const px = e.clientX - r.left;
      const py = e.clientY - r.top;
      const k = drag.current;
      if (!k) {
        const hover = hitTest(px, py, r.width, r.height);
        c.style.cursor =
          hover === "x1" || hover === "x2"
            ? "ew-resize"
            : hover
              ? "ns-resize"
              : "default";
        return;
      }
      const fx = Math.min(1, Math.max(0, px / r.width));
      const fy = Math.min(1, Math.max(0, py / r.height));
      if (k === "x1") curs.current.x1 = fx;
      else if (k === "x2") curs.current.x2 = fx;
      else if (k === "y1") curs.current.y1 = fy;
      else if (k === "y2") curs.current.y2 = fy;
      else {
        const trig = engine.getTrigger();
        const ch = engine.channels().find((cc) => cc.id === trig.source);
        if (ch) {
          const level = valueOfY(fy * r.height, ch, r.height);
          engine.setTrigger({ level });
          onTrigLevelRef.current?.(level);
        }
      }
    };

    const onPointerUp = (e: ReactPointerEvent<HTMLCanvasElement>) => {
      drag.current = null;
      const c = canvasRef.current;
      if (c?.hasPointerCapture(e.pointerId)) c.releasePointerCapture(e.pointerId);
    };

    return (
      <canvas
        ref={canvasRef}
        className="wave-canvas"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      />
    );
  },
);

export default WaveformCanvas;
