// WsPanel — WebSocket observability client for the bottom panel.
//
// Transport is a frontend-native `new WebSocket` (plan D3 — `csp: null` in
// tauri.conf.json allows it), kept behind small connect/send/close helpers so
// a tauri-plugin-websocket fallback would only touch those three spots.
// Reconnect follows the same frontend-owned FSM/backoff as MqttPanel; the
// attempt counter resets on a successful open.
//
// Binary frames: instead of storing base64 (and paying an atob on every row
// expansion), the payload is hex-stringed up front and stored as plain text —
// b64: false, payload "(binary N B) <hex of first 64 bytes>". The feed is a
// human debugging aid, not a capture tool, so lossy-but-readable wins.
//
// Mounted once and hidden with display:none (ScopeView pattern): the socket
// keeps feeding the ObsStore while hidden; the 4 Hz UI poll only runs while
// `active`.

import { useEffect, useRef, useState } from "react";
import { nextBackoff } from "../obs/backoff";
import { ObsStore } from "../obs/obsStore";
import ObsLog from "./ObsLog";

interface Props {
  active: boolean;
  notify: (msg: string, isError?: boolean) => void;
}

type ConnState = "disconnected" | "connecting" | "connected" | "reconnecting";

const LS_KEY = "bancada.wsUrls";
const HISTORY_MAX = 8;
const TOPIC_IN = "⇦ in";
const TOPIC_OUT = "⇨ out";

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

export default function WsPanel({ active, notify }: Props) {
  // ---------- message store ----------

  const storeRef = useRef<ObsStore | null>(null);
  if (storeRef.current === null) storeRef.current = new ObsStore(500);
  const store = storeRef.current;

  const [, setUiTick] = useState(0);
  const bump = () => setUiTick((t) => t + 1);
  const lastVersionRef = useRef(-1);

  // ---------- connection FSM ----------

  const [conn, setConn] = useState<ConnState>("disconnected");
  const [attempt, setAttempt] = useState(0);
  const [countdown, setCountdown] = useState(0);

  const wsRef = useRef<WebSocket | null>(null);
  const attemptRef = useRef(0);
  const userDisconnectedRef = useRef(false);
  const reconnectTimerRef = useRef<number | null>(null);
  const countdownIvRef = useRef<number | null>(null);

  // ---------- URL + history ----------

  const [history, setHistory] = useState<string[]>(loadHistory);
  const [url, setUrlState] = useState(() => loadHistory()[0] ?? "ws://");
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

  // ---------- feed / send state ----------

  const [feedFilter, setFeedFilter] = useState("");
  const [autoscroll, setAutoscroll] = useState(true);
  const [sendText, setSendText] = useState("");

  // ---------- connect / reconnect / disconnect ----------

  const push = (topic: string, payload: string) =>
    store.push({ topic, payload, b64: false, retain: false, qos: 0, ts: Date.now() });

  const clearTimers = () => {
    if (reconnectTimerRef.current !== null) {
      window.clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (countdownIvRef.current !== null) {
      window.clearInterval(countdownIvRef.current);
      countdownIvRef.current = null;
    }
  };

  const scheduleReconnect = () => {
    if (reconnectTimerRef.current !== null) return; // already scheduled
    attemptRef.current += 1;
    setAttempt(attemptRef.current);
    const delay = nextBackoff(attemptRef.current);
    setConn("reconnecting");
    setCountdown(Math.ceil(delay / 1000));
    countdownIvRef.current = window.setInterval(
      () => setCountdown((c) => Math.max(0, c - 1)),
      1000,
    );
    reconnectTimerRef.current = window.setTimeout(() => {
      reconnectTimerRef.current = null;
      if (countdownIvRef.current !== null) {
        window.clearInterval(countdownIvRef.current);
        countdownIvRef.current = null;
      }
      doConnect();
    }, delay);
  };

  const doConnect = () => {
    clearTimers();
    setConn("connecting");
    let ws: WebSocket;
    try {
      ws = new WebSocket(urlRef.current);
    } catch (e) {
      // Invalid URL throws synchronously — no point retrying it.
      notify(String(e), true);
      setConn("disconnected");
      return;
    }
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onopen = () => {
      if (wsRef.current !== ws) return;
      attemptRef.current = 0;
      setAttempt(0);
      setConn("connected");
      remember(urlRef.current);
    };
    ws.onmessage = (ev) => {
      // Push even while the tab is hidden — the poll only repaints.
      const data: unknown = ev.data;
      if (typeof data === "string") {
        push(TOPIC_IN, data);
      } else if (data instanceof ArrayBuffer) {
        const bytes = new Uint8Array(data);
        const hex = Array.from(bytes.slice(0, 64), (b) =>
          b.toString(16).padStart(2, "0"),
        ).join(" ");
        push(TOPIC_IN, `(binary ${bytes.length} B) ${hex}`);
      }
    };
    ws.onclose = (ev) => {
      if (wsRef.current !== ws) return; // a stale socket, superseded already
      wsRef.current = null;
      if (userDisconnectedRef.current) {
        setConn("disconnected");
      } else {
        push(TOPIC_IN, `(closed${ev.code ? ` code ${ev.code}` : ""}${ev.reason ? ` — ${ev.reason}` : ""})`);
        scheduleReconnect();
      }
    };
    // Errors carry no detail at the JS level; the close event that follows
    // drives the reconnect.
    ws.onerror = () => {};
  };

  const startConnect = () => {
    const u = urlRef.current.trim();
    if (!u || u === "ws://") {
      notify("Enter a WebSocket URL first", true);
      return;
    }
    userDisconnectedRef.current = false;
    attemptRef.current = 0;
    setAttempt(0);
    doConnect();
  };

  const doDisconnect = () => {
    userDisconnectedRef.current = true;
    clearTimers();
    wsRef.current?.close();
    wsRef.current = null;
    setConn("disconnected");
  };

  // Close an orphaned socket if the panel unmounts (streamingRef pattern).
  useEffect(
    () => () => {
      userDisconnectedRef.current = true;
      clearTimers();
      wsRef.current?.close();
      wsRef.current = null;
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  // ---------- 4 Hz UI poll (only while shown; the store fills regardless) ----------

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

  // ---------- send ----------

  const doSend = () => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN || !sendText) return;
    ws.send(sendText);
    push(TOPIC_OUT, sendText); // echo outgoing frames into the feed
    setSendText("");
  };

  // ---------- render ----------

  const snap = store.snapshot(feedFilter.trim() || undefined);
  const chipLabel =
    conn === "reconnecting"
      ? `reconnecting in ${countdown}s (attempt ${attempt})`
      : conn === "connecting"
        ? "connecting…"
        : conn;

  return (
    <section
      className="obs-panel"
      style={active ? undefined : { display: "none" }}
    >
      {/* --- URL row --- */}
      <div className="obs-row">
        <input
          className="input mono obs-url"
          list="ws-url-history"
          placeholder="ws://host:port/path"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          disabled={conn !== "disconnected"}
        />
        <datalist id="ws-url-history">
          {history.map((u) => (
            <option key={u} value={u} />
          ))}
        </datalist>
        {conn === "disconnected" ? (
          <button className="btn small primary" onClick={startConnect}>
            Connect
          </button>
        ) : (
          <button className="btn small" onClick={doDisconnect}>
            Disconnect
          </button>
        )}
        <span className={`obs-chip ${conn}`}>{chipLabel}</span>
        <div className="spacer" />
      </div>

      {/* --- feed (no topic column — direction pseudo-topics only) --- */}
      <div className="obs-main">
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

      {/* --- send row --- */}
      <div className="obs-pub-row">
        <input
          className="input mono obs-pub-payload"
          placeholder="send text frame… (Enter)"
          value={sendText}
          onChange={(e) => setSendText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && doSend()}
        />
        <button
          className="btn small primary"
          disabled={conn !== "connected" || !sendText}
          onClick={doSend}
        >
          Send
        </button>
      </div>
    </section>
  );
}
