// ScopeView — self-contained oscilloscope view. Owns the ScopeEngine
// instance, its serial-line subscription (plotter source) and the ADC
// streaming session. Lives in the bottom panel as the Oscilloscope tab;
// stays mounted while hidden so streams survive tab switches.

import { useCallback, useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import * as api from "../api";
import type { ScopeCaps } from "../api";
import { ScopeEngine } from "../scope/engine";
import type {
  ChannelInfo,
  EngineStats,
  FftResult,
  FftWindowKind,
  IScopeEngine,
  Measurements,
  ScopeEvent,
  ScopeSourceKind,
  TriggerConfig,
  TriggerStatus,
} from "../scope/types";
import WaveformCanvas, {
  fmtHz,
  fmtSI,
  fmtTime,
  type CursorMode,
  type WaveformCanvasHandle,
} from "./WaveformCanvas";

export interface ScopeViewProps {
  /** view is currently shown (component stays mounted while hidden) */
  active: boolean;
  selectedPort: string | null;
  busy: boolean;
  monitorOn: boolean;
  baudrate: number;
  notify: (msg: string, isError?: boolean) => void;
  /** start the serial monitor if it is off (plotter source needs it) */
  onEnsureMonitor: () => Promise<void>;
  /** stop the serial monitor if it is on (ADC needs the port exclusively) */
  onStopMonitor: () => Promise<void>;
  /** compile + upload the companion firmware sketch; resolves to success */
  onFlashFirmware: (sketchDir: string, profile: string) => Promise<boolean>;
}

const PLOTTER_TB = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2];
const ADC_TB = [1e-5, 2e-5, 5e-5, 1e-4, 2e-4, 5e-4, ...PLOTTER_TB];
const VDIV = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2];
const SPS_PRESETS = [
  1000, 2000, 5000, 10000, 20000, 50000, 100000, 200000, 500000, 1000000,
  2000000,
];
const ATTEN_LABEL: Record<number, string> = {
  0: "0–0.95 V",
  1: "0–1.25 V",
  2: "0–1.75 V",
  3: "0–3.1 V",
};
const ATTEN_RANGE: Record<number, number> = {
  0: 0.95,
  1: 1.25,
  2: 1.75,
  3: 3.1,
};
const STATUS_LABEL: Record<TriggerStatus, string> = {
  auto: "Auto",
  trigd: "Trig'd",
  wait: "Wait",
  stop: "Stop",
};
const FW_CHIPS = ["esp32", "esp32s3", "esp32c3"];

const timestamp = () => {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
};

const blobToBase64 = (blob: Blob): Promise<string> =>
  new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => {
      const s = String(r.result);
      resolve(s.slice(s.indexOf(",") + 1));
    };
    r.onerror = () => reject(r.error);
    r.readAsDataURL(blob);
  });

// ---------- FFT spectrum canvas ----------

function drawSpectrum(canvas: HTMLCanvasElement, r: FftResult, color: string) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const W = canvas.clientWidth;
  const H = canvas.clientHeight;
  if (W < 20 || H < 20 || r.db.length < 2) return;
  const dpr = window.devicePixelRatio || 1;
  const pw = Math.round(W * dpr);
  const ph = Math.round(H * dpr);
  if (canvas.width !== pw || canvas.height !== ph) {
    canvas.width = pw;
    canvas.height = ph;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = "#10141b";
  ctx.fillRect(0, 0, W, H);

  const padL = 34;
  const padR = 6;
  const padT = 6;
  const padB = 16;
  const iw = W - padL - padR;
  const ih = H - padT - padB;

  ctx.font = "9px monospace";
  // dB gridlines, 0 → -100
  for (let db = 0; db >= -100; db -= 20) {
    const y = padT + (-db / 100) * ih;
    ctx.strokeStyle = "rgba(139, 147, 161, 0.14)";
    ctx.beginPath();
    ctx.moveTo(padL, y);
    ctx.lineTo(W - padR, y);
    ctx.stroke();
    ctx.fillStyle = "rgba(139, 147, 161, 0.7)";
    ctx.textAlign = "right";
    ctx.textBaseline = "middle";
    ctx.fillText(String(db), padL - 4, y);
  }
  // linear Hz axis, 5 steps
  const fMax = r.binHz * (r.db.length - 1);
  for (let i = 0; i <= 5; i++) {
    const x = padL + (iw * i) / 5;
    ctx.strokeStyle = "rgba(139, 147, 161, 0.1)";
    ctx.beginPath();
    ctx.moveTo(x, padT);
    ctx.lineTo(x, H - padB);
    ctx.stroke();
    ctx.fillStyle = "rgba(139, 147, 161, 0.7)";
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.fillText(
      fmtHz((fMax * i) / 5),
      Math.min(Math.max(x, padL + 16), W - 24),
      H - padB + 3,
    );
  }
  // spectrum trace
  ctx.strokeStyle = color;
  ctx.lineWidth = 1;
  ctx.beginPath();
  const n = r.db.length;
  for (let i = 0; i < n; i++) {
    const x = padL + (iw * i) / (n - 1);
    const db = Math.max(-100, Math.min(0, r.db[i]));
    const y = padT + (-db / 100) * ih;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
  // peak readout
  if (r.peakHz !== null && r.peakDb !== null) {
    ctx.fillStyle = color;
    ctx.font = "11px monospace";
    ctx.textAlign = "right";
    ctx.textBaseline = "top";
    ctx.fillText(
      `peak ${fmtHz(r.peakHz)} ${r.peakDb.toFixed(1)} dB`,
      W - 8,
      padT + 2,
    );
  }
}

function SpectrumCanvas({
  engine,
  channelId,
  size,
  window: win,
}: {
  engine: IScopeEngine;
  channelId: number;
  size: number;
  window: FftWindowKind;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let live = true;
    let pending = false;
    const tick = () => {
      if (pending) return;
      pending = true;
      engine
        .fft(channelId, size, win)
        .then((r) => {
          if (live && r && ref.current) {
            const color =
              engine.channels().find((c) => c.id === channelId)?.color ??
              "#2dd4bf";
            drawSpectrum(ref.current, r, color);
          }
        })
        .catch(() => {})
        .finally(() => {
          pending = false;
        });
    };
    tick();
    const iv = window.setInterval(tick, 200);
    return () => {
      live = false;
      window.clearInterval(iv);
    };
  }, [engine, channelId, size, win]);
  return <canvas ref={ref} className="spectrum-canvas" />;
}

// ---------- main view ----------

export default function ScopeView({
  active,
  selectedPort,
  busy,
  monitorOn,
  baudrate,
  notify,
  onEnsureMonitor,
  onStopMonitor,
  onFlashFirmware,
}: ScopeViewProps) {
  const engineRef = useRef<IScopeEngine | null>(null);
  if (engineRef.current === null) engineRef.current = new ScopeEngine();
  const engine = engineRef.current;

  const waveRef = useRef<WaveformCanvasHandle>(null);

  // display state (polled from the engine at ~4 Hz)
  const [source, setSourceState] = useState<ScopeSourceKind>("plotter");
  const [running, setRunning] = useState(() => engine.isRunning());
  const [status, setStatus] = useState<TriggerStatus>("stop");
  const [secPerDiv, setSecPerDivState] = useState(() => engine.getSecPerDiv());
  const [trig, setTrig] = useState<TriggerConfig>(() => engine.getTrigger());
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [meas, setMeas] = useState<Record<number, Measurements | null>>({});
  const [stats, setStats] = useState<EngineStats>({
    droppedFrames: 0,
    crcErrors: 0,
    overflows: 0,
    deviceSps: null,
  });
  const [measOpen, setMeasOpen] = useState(true);
  const [cursorMode, setCursorMode] = useState<CursorMode>("off");

  // fft
  const [fftOn, setFftOn] = useState(false);
  const [fftSize, setFftSize] = useState(4096);
  const [fftWindow, setFftWindow] = useState<FftWindowKind>("hann");
  const [fftCh, setFftCh] = useState<number | null>(null);

  // adc
  const [caps, setCaps] = useState<ScopeCaps | null>(null);
  const [probeFailed, setProbeFailed] = useState(false);
  const [adcPins, setAdcPins] = useState<number[]>([]);
  const [adcSps, setAdcSps] = useState(50000);
  const [adcAtten, setAdcAtten] = useState(3);
  const [streaming, setStreaming] = useState(false);
  const [pre, setPre] = useState(2048);
  const [post, setPost] = useState(6144);
  const [fwDir, setFwDir] = useState<string | null>(null);
  const [fwChip, setFwChip] = useState("esp32s3");

  const sourceRef = useRef(source);
  sourceRef.current = source;
  const streamingRef = useRef(false);
  streamingRef.current = streaming;

  // ---------- engine polling (~4 Hz) ----------

  useEffect(() => {
    const iv = window.setInterval(() => {
      const chans = engine.channels();
      setChannels(chans);
      setRunning(engine.isRunning());
      setStats(engine.stats());
      const m: Record<number, Measurements | null> = {};
      for (const ch of chans) if (ch.visible) m[ch.id] = engine.measure(ch.id);
      setMeas(m);
    }, 250);
    return () => window.clearInterval(iv);
  }, [engine]);

  // ---------- plotter source: shared serial monitor stream ----------

  useEffect(() => {
    const un = api.onSerialLine((l) => {
      if (sourceRef.current === "plotter" && l.stream === "stdout")
        engine.feedText(l.line);
    });
    return () => {
      un.then((f) => f());
    };
  }, [engine]);

  // stop an orphaned ADC stream if the view unmounts
  useEffect(
    () => () => {
      if (streamingRef.current) api.scopeStop().catch(() => {});
    },
    [],
  );

  // ---------- run/stop ----------

  const toggleRun = useCallback(() => {
    engine.run(!engine.isRunning());
    setRunning(engine.isRunning());
  }, [engine]);

  // Space toggles run/stop while the scope view is shown
  useEffect(() => {
    if (!active) return;
    const h = (e: KeyboardEvent) => {
      if (e.code !== "Space") return;
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.tagName === "SELECT" ||
          t.isContentEditable)
      )
        return;
      e.preventDefault();
      toggleRun();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [active, toggleRun]);

  const armSingle = () => {
    engine.setTrigger({ mode: "single" });
    setTrig(engine.getTrigger());
    engine.run(true);
    setRunning(true);
  };

  // ---------- trigger / timebase / channels ----------

  const patchTrig = useCallback(
    (p: Partial<TriggerConfig>) => {
      engine.setTrigger(p);
      setTrig(engine.getTrigger());
    },
    [engine],
  );

  const onCanvasTrigLevel = useCallback(() => {
    setTrig(engine.getTrigger());
  }, [engine]);

  const changeTb = (v: number) => {
    engine.setSecPerDiv(v);
    setSecPerDivState(engine.getSecPerDiv());
  };

  const patchChannel = (
    id: number,
    p: Partial<Pick<ChannelInfo, "visible" | "voltsPerDiv" | "offset" | "name">>,
  ) => {
    engine.setChannel(id, p);
    setChannels(engine.channels());
  };

  const tbBase = source === "adc" ? ADC_TB : PLOTTER_TB;
  const tbOptions = tbBase.includes(secPerDiv)
    ? tbBase
    : [...tbBase, secPerDiv].sort((a, b) => a - b);
  const vdivOptions = (cur: number) =>
    VDIV.includes(cur) ? VDIV : [...VDIV, cur].sort((a, b) => a - b);

  // ---------- source switching ----------

  const stopStream = useCallback(async () => {
    try {
      await api.scopeStop();
    } catch (e) {
      notify(String(e), true);
    }
    setStreaming(false);
  }, [notify]);

  const changeSource = (k: ScopeSourceKind) => {
    if (k === source) return;
    if (streaming) void stopStream();
    engine.setSource(k);
    setSourceState(k);
    setChannels(engine.channels());
  };

  // ---------- ADC panel ----------

  const handleScopeEvent = useCallback(
    (ev: ScopeEvent) => {
      switch (ev.ev) {
        case "err":
          notify(`Scope device: ${ev.msg}`, true);
          break;
        case "closed":
          setStreaming(false);
          notify("Scope stream closed.");
          break;
        default:
          break; // meta/record/drop/crc are reflected in engine.stats()
      }
    },
    [notify],
  );

  const probe = async () => {
    if (!selectedPort) {
      notify("Select a port first", true);
      return;
    }
    setProbeFailed(false);
    try {
      if (monitorOn) await onStopMonitor();
      const c = await api.scopeProbe(selectedPort);
      setCaps(c);
      setAdcPins(c.pins.length ? [c.pins[0]] : []);
      setAdcAtten(c.atten.includes(3) ? 3 : (c.atten[0] ?? 3));
      setAdcSps(Math.min(50000, c.max_stream_sps));
      notify(`Scope firmware: ${c.chip} fw ${c.fw} — proto ${c.proto}`);
    } catch (e) {
      setCaps(null);
      setProbeFailed(true);
      notify(`Probe failed: ${e}`, true);
    }
  };

  const startStream = async () => {
    if (!selectedPort || !caps || adcPins.length === 0) return;
    try {
      if (monitorOn) await onStopMonitor(); // port exclusivity
      engine.setSource("adc");
      engine.reset();
      await api.scopeStart(
        selectedPort,
        caps.baud,
        { sps: adcSps, pins: adcPins, atten: adcAtten },
        (msg) => {
          const evs = engine.feedBinary(msg);
          for (const ev of evs) handleScopeEvent(ev);
        },
      );
      setStreaming(true);
      engine.run(true);
      setRunning(true);
      notify(`ADC streaming pin ${adcPins.join("+")} @ ${fmtSI(adcSps, "S/s")}`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const singleShot = async () => {
    if (!caps || adcPins.length === 0) return;
    const range = ATTEN_RANGE[adcAtten] ?? 3.1;
    const t = engine.getTrigger();
    const raw = Math.max(
      0,
      Math.min(4095, Math.round((4095 * t.level) / range)),
    );
    try {
      await api.scopeSingle({
        sps: adcSps,
        pin: adcPins[0],
        atten: adcAtten,
        pre,
        post,
        level: raw,
        edge: t.edge === "rising" ? "r" : "f",
      });
      notify("Single-shot armed on device.");
    } catch (e) {
      notify(String(e), true);
    }
  };

  const installFw = async () => {
    try {
      const dir = await api.scopeInstallFirmware("");
      setFwDir(dir);
      notify(`Companion firmware written to ${dir} — pick a chip and flash.`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const flashFw = async () => {
    if (!fwDir) return;
    notify("Building companion firmware — watch the Build Output tab.");
    const ok = await onFlashFirmware(fwDir, fwChip);
    if (ok) notify("Companion firmware flashed — press Probe again.");
  };

  const spsOptions = caps
    ? Array.from(
        new Set(
          SPS_PRESETS.filter((v) => v <= caps.max_stream_sps).concat(
            caps.max_stream_sps,
          ),
        ),
      ).sort((a, b) => a - b)
    : [];

  // ---------- export ----------

  const exportCsv = async () => {
    try {
      const csv = engine.exportCsv();
      const path = await save({
        defaultPath: `bancada-scope-${timestamp()}.csv`,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!path) return;
      await api.saveTextFile(path, csv);
      notify(`Saved ${path}`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const exportPng = async () => {
    const h = waveRef.current;
    if (!h) return;
    try {
      const blob = await h.snapshotPng();
      const b64 = await blobToBase64(blob);
      const path = await save({
        defaultPath: `bancada-scope-${timestamp()}.png`,
        filters: [{ name: "PNG", extensions: ["png"] }],
      });
      if (!path) return;
      await api.saveBinaryFile(path, b64);
      notify(`Saved ${path}`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  // ---------- derived ----------

  const visibleChannels = channels.filter((c) => c.visible);
  const fftChannel =
    fftCh !== null && channels.some((c) => c.id === fftCh)
      ? fftCh
      : channels.some((c) => c.id === trig.source)
        ? trig.source
        : (channels[0]?.id ?? null);
  const statsBad =
    stats.droppedFrames > 0 || stats.crcErrors > 0 || stats.overflows > 0;
  const noiseRej = trig.hysteresis > 0;
  const round4 = (v: number) => Number(v.toPrecision(4));

  const toggleNoiseRej = (on: boolean) => {
    if (!on) {
      patchTrig({ hysteresis: 0 });
      return;
    }
    const vpp = meas[trig.source]?.vpp;
    const srcCh = channels.find((c) => c.id === trig.source);
    const base =
      vpp && vpp > 0 ? vpp : srcCh ? srcCh.voltsPerDiv * 10 : 1;
    patchTrig({ hysteresis: 0.05 * base });
  };

  // ---------- render ----------

  return (
    <section
      className="scope-view"
      style={active ? undefined : { display: "none" }}
    >
      <div className="scope-controls">
        {/* --- top control strip --- */}
        <div className="scope-row">
          <label className="field">
            Source
            <select
              className="select small"
              value={source}
              onChange={(e) => changeSource(e.target.value as ScopeSourceKind)}
            >
              <option value="plotter">Plotter</option>
              <option value="adc">ADC</option>
            </select>
          </label>
          <button className="btn small" onClick={toggleRun} title="Run/Stop (Space)">
            {running ? "⏸ Stop" : "▶ Run"}
          </button>
          <button className="btn small" onClick={armSingle} title="Arm single trigger">
            Single
          </button>
          <span className={`trig-chip ${status}`}>{STATUS_LABEL[status]}</span>
          <label className="field">
            Time/div
            <select
              className="select small"
              value={String(secPerDiv)}
              onChange={(e) => changeTb(Number(e.target.value))}
            >
              {tbOptions.map((v) => (
                <option key={v} value={String(v)}>
                  {fmtTime(v)}
                </option>
              ))}
            </select>
          </label>
          <div className="spacer" />
          {stats.deviceSps !== null && (
            <span className="scope-dim">
              device {fmtSI(stats.deviceSps, "S/s")}
            </span>
          )}
          {statsBad && (
            <span className="scope-stats-bad">
              drops {stats.droppedFrames} · crc {stats.crcErrors} · ovf{" "}
              {stats.overflows}
            </span>
          )}
        </div>

        {/* --- channel chips --- */}
        <div className="scope-row">
          {channels.length === 0 && (
            <span className="scope-dim">
              no channels yet —{" "}
              {source === "plotter"
                ? "waiting for plotter data on the serial monitor"
                : "probe and start ADC streaming"}
            </span>
          )}
          {channels.map((ch) => (
            <span key={ch.id} className={`ch-chip${ch.visible ? "" : " off"}`}>
              <button
                className="ch-dot"
                style={{ background: ch.color }}
                onClick={() => patchChannel(ch.id, { visible: !ch.visible })}
                title="Toggle channel visibility"
              />
              <span className="ch-name">{ch.name}</span>
              <select
                className="select small"
                value={String(ch.voltsPerDiv)}
                onChange={(e) =>
                  patchChannel(ch.id, { voltsPerDiv: Number(e.target.value) })
                }
                title="Vertical scale (units per division)"
              >
                {vdivOptions(ch.voltsPerDiv).map((v) => (
                  <option key={v} value={String(v)}>
                    {fmtSI(v, ch.unit)}/div
                  </option>
                ))}
              </select>
              <button
                className="btn small"
                onClick={() =>
                  patchChannel(ch.id, { offset: ch.offset + ch.voltsPerDiv / 2 })
                }
                title="Shift trace down (offset +½ div)"
              >
                −
              </button>
              <button
                className="btn small"
                onClick={() =>
                  patchChannel(ch.id, { offset: ch.offset - ch.voltsPerDiv / 2 })
                }
                title="Shift trace up (offset −½ div)"
              >
                +
              </button>
            </span>
          ))}
        </div>

        {/* --- trigger controls --- */}
        <div className="scope-row">
          <label className="field">
            Trig
            <select
              className="select small"
              value={String(trig.source)}
              onChange={(e) => patchTrig({ source: Number(e.target.value) })}
            >
              {channels.length === 0 && <option value={String(trig.source)}>—</option>}
              {channels.map((ch) => (
                <option key={ch.id} value={String(ch.id)}>
                  {ch.name}
                </option>
              ))}
            </select>
          </label>
          <select
            className="select small"
            value={trig.edge}
            onChange={(e) =>
              patchTrig({ edge: e.target.value as TriggerConfig["edge"] })
            }
            title="Trigger edge"
          >
            <option value="rising">↗ rising</option>
            <option value="falling">↘ falling</option>
          </select>
          <select
            className="select small"
            value={trig.mode}
            onChange={(e) =>
              patchTrig({ mode: e.target.value as TriggerConfig["mode"] })
            }
            title="Trigger mode"
          >
            <option value="auto">auto</option>
            <option value="normal">normal</option>
            <option value="single">single</option>
          </select>
          <label className="field">
            Level
            <input
              className="input num"
              type="number"
              step="any"
              value={round4(trig.level)}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isFinite(v)) patchTrig({ level: v });
              }}
            />
          </label>
          <label className="field">
            Pos
            <input
              type="range"
              min={0}
              max={100}
              value={Math.round(trig.position * 100)}
              onChange={(e) =>
                patchTrig({ position: Number(e.target.value) / 100 })
              }
              title="Trigger position (% pre-trigger)"
            />
            <span className="scope-dim">{Math.round(trig.position * 100)}%</span>
          </label>
          <label className="field">
            Holdoff
            <input
              className="input num"
              type="number"
              min={0}
              step={0.001}
              value={trig.holdoff}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isFinite(v) && v >= 0) patchTrig({ holdoff: v });
              }}
              title="Minimum seconds between trigger fires"
            />
          </label>
          <label className="field">
            <input
              type="checkbox"
              checked={noiseRej}
              onChange={(e) => toggleNoiseRej(e.target.checked)}
            />
            noise rej
          </label>
        </div>

        {/* --- ADC panel --- */}
        {source === "adc" && (
          <div className="scope-row adc-panel">
            <button
              className="btn small"
              onClick={probe}
              disabled={!selectedPort || busy || streaming}
              title="Detect the companion scope firmware on the selected port"
            >
              Probe
            </button>
            {caps && (
              <span className="scope-dim mono-dim">
                {caps.chip} fw {caps.fw} · {fmtSI(caps.max_stream_sps, "S/s")}{" "}
                stream / {fmtSI(caps.max_sps, "S/s")} burst
              </span>
            )}
            {caps && (
              <>
                <label className="field">
                  Pin
                  <select
                    className="select small"
                    value={String(adcPins[0] ?? "")}
                    onChange={(e) =>
                      setAdcPins((prev) => {
                        const a = Number(e.target.value);
                        return prev.length > 1 && prev[1] !== a
                          ? [a, prev[1]]
                          : [a];
                      })
                    }
                    disabled={streaming}
                  >
                    {caps.pins.map((p) => (
                      <option key={p} value={String(p)}>
                        GPIO{p}
                      </option>
                    ))}
                  </select>
                </label>
                {caps.maxch > 1 && (
                  <label className="field">
                    Pin 2
                    <select
                      className="select small"
                      value={String(adcPins[1] ?? "")}
                      onChange={(e) =>
                        setAdcPins((prev) => {
                          const v = e.target.value;
                          return v === ""
                            ? prev.slice(0, 1)
                            : [prev[0], Number(v)];
                        })
                      }
                      disabled={streaming || adcPins.length === 0}
                    >
                      <option value="">—</option>
                      {caps.pins
                        .filter((p) => p !== adcPins[0])
                        .map((p) => (
                          <option key={p} value={String(p)}>
                            GPIO{p}
                          </option>
                        ))}
                    </select>
                  </label>
                )}
                <label className="field">
                  Rate
                  <select
                    className="select small"
                    value={String(adcSps)}
                    onChange={(e) => setAdcSps(Number(e.target.value))}
                    disabled={streaming}
                  >
                    {spsOptions.map((v) => (
                      <option key={v} value={String(v)}>
                        {fmtSI(v, "S/s")}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="field">
                  Range
                  <select
                    className="select small"
                    value={String(adcAtten)}
                    onChange={(e) => setAdcAtten(Number(e.target.value))}
                    disabled={streaming}
                  >
                    {caps.atten.map((a) => (
                      <option key={a} value={String(a)}>
                        {ATTEN_LABEL[a] ?? `atten ${a}`}
                      </option>
                    ))}
                  </select>
                </label>
                {streaming ? (
                  <button className="btn small danger" onClick={stopStream}>
                    ■ Stop stream
                  </button>
                ) : (
                  <button
                    className="btn small primary"
                    onClick={startStream}
                    disabled={!selectedPort || adcPins.length === 0}
                  >
                    ▶ Start stream
                  </button>
                )}
                <label className="field">
                  Pre
                  <input
                    className="input num"
                    type="number"
                    min={0}
                    value={pre}
                    onChange={(e) =>
                      setPre(Math.max(0, Math.round(Number(e.target.value) || 0)))
                    }
                    title="Pre-trigger samples (device single-shot)"
                  />
                </label>
                <label className="field">
                  Post
                  <input
                    className="input num"
                    type="number"
                    min={1}
                    value={post}
                    onChange={(e) =>
                      setPost(Math.max(1, Math.round(Number(e.target.value) || 1)))
                    }
                    title="Post-trigger samples (device single-shot)"
                  />
                </label>
                <button
                  className="btn small"
                  onClick={singleShot}
                  disabled={!streaming}
                  title="Device-side triggered burst using the current trigger level/edge"
                >
                  ⚡ Single (device)
                </button>
              </>
            )}
            {probeFailed && (
              <span className="fw-flow">
                <span className="scope-dim">no scope firmware?</span>
                <select
                  className="select small"
                  value={fwChip}
                  onChange={(e) => setFwChip(e.target.value)}
                >
                  {FW_CHIPS.map((c) => (
                    <option key={c} value={c}>
                      {c}
                    </option>
                  ))}
                </select>
                {fwDir === null ? (
                  <button className="btn small" onClick={installFw}>
                    Install companion firmware…
                  </button>
                ) : (
                  <button
                    className="btn small primary"
                    onClick={flashFw}
                    disabled={busy || !selectedPort}
                  >
                    Compile & flash ({fwChip})
                  </button>
                )}
              </span>
            )}
          </div>
        )}

        {/* --- plotter hint --- */}
        {source === "plotter" && !monitorOn && (
          <div className="scope-hint">
            Plotter reads the serial monitor stream — the monitor is not
            running.
            <button
              className="btn small"
              onClick={() =>
                onEnsureMonitor().catch((e) => notify(String(e), true))
              }
              disabled={!selectedPort || busy}
            >
              Start monitor @ {baudrate}
            </button>
          </div>
        )}
      </div>

      {/* --- main area: canvas + measurements --- */}
      <div className="scope-main">
        <div className="scope-canvas-col">
          <div className="wave-wrap">
            <WaveformCanvas
              ref={waveRef}
              engine={engine}
              cursorMode={cursorMode}
              active={active}
              onStatus={setStatus}
              onTriggerLevel={onCanvasTrigLevel}
            />
          </div>
          {fftOn && fftChannel !== null && (
            <div className="spectrum-wrap">
              <SpectrumCanvas
                engine={engine}
                channelId={fftChannel}
                size={fftSize}
                window={fftWindow}
              />
            </div>
          )}
        </div>

        {measOpen ? (
          <aside className="scope-measure">
            <div className="meas-head">
              <span>Measure</span>
              <button
                className="btn small"
                onClick={() => setMeasOpen(false)}
                title="Collapse measurements"
              >
                ▸
              </button>
            </div>
            {visibleChannels.length === 0 && (
              <div className="scope-dim meas-empty">no visible channels</div>
            )}
            {visibleChannels.map((ch) => {
              const m = meas[ch.id];
              const rows: [string, string][] = [
                ["min", fmtSI(m?.min, ch.unit)],
                ["max", fmtSI(m?.max, ch.unit)],
                ["vpp", fmtSI(m?.vpp, ch.unit)],
                ["mean", fmtSI(m?.mean, ch.unit)],
                ["rms", fmtSI(m?.rms, ch.unit)],
                ["freq", fmtHz(m?.freq)],
                ["period", fmtTime(m?.period)],
                [
                  "duty+",
                  m?.dutyPos !== null && m?.dutyPos !== undefined
                    ? `${(m.dutyPos * 100).toFixed(1)} %`
                    : "—",
                ],
              ];
              return (
                <div key={ch.id} className="meas-block">
                  <div className="meas-ch" style={{ color: ch.color }}>
                    {ch.name}
                  </div>
                  {rows.map(([k, v]) => (
                    <div key={k} className="meas-row">
                      <span>{k}</span>
                      <span>{v}</span>
                    </div>
                  ))}
                </div>
              );
            })}
          </aside>
        ) : (
          <button
            className="scope-measure-toggle"
            onClick={() => setMeasOpen(true)}
            title="Show measurements"
          >
            ◂
          </button>
        )}
      </div>

      {/* --- bottom strip --- */}
      <div className="scope-footer">
        <label className="field">
          Cursors
          <select
            className="select small"
            value={cursorMode}
            onChange={(e) => setCursorMode(e.target.value as CursorMode)}
          >
            <option value="off">off</option>
            <option value="x">X</option>
            <option value="y">Y</option>
            <option value="track">track</option>
          </select>
        </label>
        <button
          className={fftOn ? "btn small toggled" : "btn small"}
          onClick={() => setFftOn((v) => !v)}
        >
          FFT
        </button>
        {fftOn && (
          <>
            <select
              className="select small"
              value={fftWindow}
              onChange={(e) => setFftWindow(e.target.value as FftWindowKind)}
              title="FFT window"
            >
              <option value="hann">hann</option>
              <option value="rect">rect</option>
              <option value="flattop">flattop</option>
            </select>
            <select
              className="select small"
              value={String(fftSize)}
              onChange={(e) => setFftSize(Number(e.target.value))}
              title="FFT size"
            >
              {[1024, 4096, 8192].map((s) => (
                <option key={s} value={String(s)}>
                  {s}
                </option>
              ))}
            </select>
            <select
              className="select small"
              value={fftChannel !== null ? String(fftChannel) : ""}
              onChange={(e) => setFftCh(Number(e.target.value))}
              title="FFT channel"
            >
              {channels.map((ch) => (
                <option key={ch.id} value={String(ch.id)}>
                  {ch.name}
                </option>
              ))}
            </select>
          </>
        )}
        <div className="spacer" />
        <button className="btn small" onClick={exportCsv}>
          Export CSV
        </button>
        <button className="btn small" onClick={exportPng}>
          Export PNG
        </button>
      </div>
    </section>
  );
}
