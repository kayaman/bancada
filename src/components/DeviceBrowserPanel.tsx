// DeviceBrowserPanel — browse a bench device's HTTP UI with a request log.
//
// The iframe never points at the device: `deviceBrowseStart` spins a
// loopback reverse proxy in Rust and the frame loads
// `http://127.0.0.1:<port>/`, which sidesteps CORS and the production
// custom-protocol origin while making the log complete — every request
// the page makes passes through the proxy and lands here as an
// `exchange` event, into the same ObsStore/ObsLog the MQTT/WS tabs use.
//
// Known limits (by design, see the device-browser spec): WebSocket
// upgrades don't pass tiny_http, and absolute links to other hosts
// escape the proxy. Bench device pages are self-contained.
//
// Mounted once and hidden with display:none (WsPanel pattern): the proxy
// keeps logging while hidden; the 4 Hz UI poll only runs while `active`.

import { useEffect, useRef, useState } from "react";
import {
  deviceBrowseSetTarget,
  deviceBrowseStart,
  deviceBrowseStop,
  type DeviceBrowseEvent,
} from "../api";
import { ObsStore } from "../obs/obsStore";
import ObsLog from "./ObsLog";

interface Props {
  active: boolean;
  notify: (msg: string, isError?: boolean) => void;
}

type ProxyState = "idle" | "listening";

const LS_KEY = "bancada.deviceBrowser.history";
const HISTORY_MAX = 12;

const loadHistory = (): string[] => {
  try {
    const a: unknown = JSON.parse(localStorage.getItem(LS_KEY) ?? "[]");
    return Array.isArray(a)
      ? a.filter((x): x is string => typeof x === "string").slice(0, HISTORY_MAX)
      : [];
  } catch {
    return [];
  }
};

/** The feed row for one proxied exchange (exported for tests). */
export function exchangeRow(
  ev: Extract<DeviceBrowseEvent, { type: "exchange" }>,
): { topic: string; payload: string } {
  const kb =
    ev.resp_bytes >= 1024 ? `${(ev.resp_bytes / 1024).toFixed(1)} KiB` : `${ev.resp_bytes} B`;
  const topic = `${ev.method} ${ev.path} → ${ev.status} (${ev.duration_ms} ms, ${kb})`;
  const payload = ev.binary
    ? `(binary ${ev.resp_bytes} B) ${ev.preview}`
    : ev.preview !== ""
      ? ev.truncated
        ? `${ev.preview}…`
        : ev.preview
      : `(${ev.content_type ?? "no content-type"}, empty body)`;
  return { topic, payload };
}

export default function DeviceBrowserPanel({ active, notify }: Props) {
  const storeRef = useRef<ObsStore | null>(null);
  if (storeRef.current === null) storeRef.current = new ObsStore(500);
  const store = storeRef.current;

  const [, setUiTick] = useState(0);
  const bump = () => setUiTick((t) => t + 1);
  const lastVersionRef = useRef(-1);

  const [proxy, setProxy] = useState<ProxyState>("idle");
  const [frameSrc, setFrameSrc] = useState<string | null>(null);
  const [frameNonce, setFrameNonce] = useState(0); // bump to force a reload
  const [showLog, setShowLog] = useState(true);

  const [history, setHistory] = useState<string[]>(loadHistory);
  const [url, setUrlState] = useState(() => loadHistory()[0] ?? "http://");
  const urlRef = useRef(url);
  const setUrl = (u: string) => {
    urlRef.current = u;
    setUrlState(u);
  };

  const remember = (u: string) =>
    setHistory((h) => {
      const next = [u, ...h.filter((x) => x !== u)].slice(0, HISTORY_MAX);
      try {
        localStorage.setItem(LS_KEY, JSON.stringify(next));
      } catch {
        /* private mode etc — history is a convenience only */
      }
      return next;
    });

  const [feedFilter, setFeedFilter] = useState("");
  const [autoscroll, setAutoscroll] = useState(true);

  const push = (topic: string, payload: string) =>
    store.push({ topic, payload, b64: false, retain: false, qos: 0, ts: Date.now() });

  const onEvent = (ev: DeviceBrowseEvent) => {
    switch (ev.type) {
      case "stage":
        setFrameSrc(`http://127.0.0.1:${ev.port}/`);
        setProxy("listening");
        break;
      case "exchange": {
        const row = exchangeRow(ev);
        push(row.topic, row.payload);
        break;
      }
      case "error":
        push(`✗ ${ev.path}`, ev.message);
        break;
      case "closed":
        setProxy("idle");
        break;
    }
  };

  const doGo = async () => {
    const u = urlRef.current.trim();
    if (!u || u === "http://") {
      notify("Enter a device URL first, e.g. http://unoq.local", true);
      return;
    }
    try {
      if (proxy === "listening") {
        // Same origin, new target: the frame keeps its port.
        await deviceBrowseSetTarget(u);
        setFrameNonce((n) => n + 1);
      } else {
        await deviceBrowseStart(u, onEvent);
      }
      remember(u);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const doStop = async () => {
    try {
      await deviceBrowseStop();
    } catch {
      /* already gone */
    }
    setProxy("idle");
    setFrameSrc(null);
  };

  // Stop the proxy if the panel truly unmounts (project close tears the
  // whole app section down; display:none keeps us mounted otherwise).
  useEffect(
    () => () => {
      void deviceBrowseStop();
    },
    [],
  );

  // 4 Hz UI poll — only while shown; the store fills regardless.
  useEffect(() => {
    if (!active) return;
    const iv = window.setInterval(() => {
      store.tick();
      if (store.version !== lastVersionRef.current) {
        lastVersionRef.current = store.version;
        bump();
      }
    }, 250);
    return () => window.clearInterval(iv);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  const snap = store.snapshot(feedFilter.trim() || undefined);

  return (
    <section className="obs-panel" style={active ? undefined : { display: "none" }}>
      <div className="obs-row">
        <input
          className="input mono obs-url"
          list="device-url-history"
          placeholder="http://unoq.local"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void doGo()}
        />
        <datalist id="device-url-history">
          {history.map((u) => (
            <option key={u} value={u} />
          ))}
        </datalist>
        <button className="btn small primary" onClick={() => void doGo()}>
          Go
        </button>
        {proxy === "listening" && (
          <>
            <button
              className="btn small"
              onClick={() => setFrameNonce((n) => n + 1)}
              title="Reload the page"
            >
              ⟳
            </button>
            <button className="btn small" onClick={() => void doStop()}>
              Stop
            </button>
          </>
        )}
        <span className={`obs-chip ${proxy === "listening" ? "connected" : "disconnected"}`}>
          {proxy}
        </span>
        <button
          className="btn small"
          onClick={() => setShowLog((v) => !v)}
          title="Toggle the request log"
        >
          {showLog ? "▾ log" : "▴ log"}
        </button>
        <div className="spacer" />
      </div>

      <div className="devweb-split">
        {frameSrc ? (
          <iframe
            key={frameNonce}
            className="devweb-frame"
            src={frameSrc}
            title="Device page"
            referrerPolicy="no-referrer"
          />
        ) : (
          <div className="devweb-empty">no device open</div>
        )}
        {showLog && (
          <div className="devweb-log">
            <ObsLog
              rows={snap.msgs}
              paused={snap.paused}
              bufferedCount={snap.bufferedWhilePaused}
              autoscroll={autoscroll}
              filter={feedFilter}
              onTogglePause={() => {
                store.setPaused(!snap.paused);
                bump();
              }}
              onToggleAutoscroll={() => setAutoscroll((v) => !v)}
              onFilterChange={setFeedFilter}
              onClear={() => {
                store.clear();
                bump();
              }}
            />
          </div>
        )}
      </div>
    </section>
  );
}
