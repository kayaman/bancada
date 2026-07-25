// MqttPanel — MQTT observability client for the bottom panel.
//
// Reconnect is frontend-owned (plan D4): the Rust session never retries — on
// any error it emits {"ev":"closed"} and exits, and this panel schedules the
// next attempt with exponential backoff (1s→2s→4s… cap 30s), resetting the
// attempt counter only after CONNACK+SUBACK.
//
// Mounted once and hidden with display:none (ScopeView pattern) so the broker
// session survives tab switches: the channel callback keeps pushing into the
// ObsStore while hidden; the 4 Hz poll that repaints the UI only runs while
// `active`.

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { MqttBrokerCfg } from "../api";
import { nextBackoff } from "../obs/backoff";
import { ObsStore } from "../obs/obsStore";
import { redactPassword } from "../obs/redact";
import ObsLog from "./ObsLog";

interface Props {
  active: boolean;
  notify: (msg: string, isError?: boolean) => void;
}

type ConnState = "disconnected" | "connecting" | "connected" | "reconnecting";

interface DiagLine {
  text: string;
  ok: boolean;
}

export default function MqttPanel({ active, notify }: Props) {
  // ---------- message store (owned here, fed by the channel callback) ----------

  const storeRef = useRef<ObsStore | null>(null);
  if (storeRef.current === null) storeRef.current = new ObsStore(500);
  const store = storeRef.current;

  // Repaint trigger: the poll bumps this only when store.version moved.
  const [, setUiTick] = useState(0);
  const bump = () => setUiTick((t) => t + 1);
  const lastVersionRef = useRef(-1);

  // ---------- connection FSM ----------

  const [conn, setConn] = useState<ConnState>("disconnected");
  const [attempt, setAttempt] = useState(0);
  const [countdown, setCountdown] = useState(0);
  const [diag, setDiag] = useState<DiagLine[]>([]);

  const attemptRef = useRef(0);
  const userDisconnectedRef = useRef(false);
  /** A backend session may exist (connect invoked, no closed/disconnect yet). */
  const sessionActiveRef = useRef(false);
  const reconnectTimerRef = useRef<number | null>(null);
  const countdownIvRef = useRef<number | null>(null);

  // ---------- broker row state ----------

  const [presets, setPresets] = useState<MqttBrokerCfg[]>([]);
  const [presetSel, setPresetSel] = useState("custom");
  const [realUrl, setRealUrlState] = useState("");
  const realUrlRef = useRef("");
  const setRealUrl = (u: string) => {
    realUrlRef.current = u;
    setRealUrlState(u);
  };
  // The field shows redactPassword(realUrl) until focused. On focus the field
  // value is the real url (the sensor-catalog trick: the redacted form is only
  // ever *displayed*, never round-tripped — anything the user types becomes
  // the new real url directly).
  const [urlFocused, setUrlFocused] = useState(false);
  const [subFilter, setSubFilter] = useState("#");
  const subFilterRef = useRef("#");
  const [savingName, setSavingName] = useState<string | null>(null);

  // ---------- feed / publish state ----------

  const [feedFilter, setFeedFilter] = useState("");
  const [autoscroll, setAutoscroll] = useState(true);
  const [pubTopic, setPubTopic] = useState("");
  const [pubPayload, setPubPayload] = useState("");
  const [pubRetain, setPubRetain] = useState(false);

  // ---------- presets ----------

  useEffect(() => {
    api
      .loadMqttConfig()
      .then((cfg) => {
        setPresets(cfg.brokers);
        if (cfg.brokers.length > 0) {
          setPresetSel("0");
          setRealUrl(cfg.brokers[0].url);
        }
      })
      .catch((e) => notify(String(e), true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onPresetChange = (v: string) => {
    setPresetSel(v);
    const i = Number(v);
    if (v !== "custom" && presets[i]) setRealUrl(presets[i].url);
  };

  const savePreset = async () => {
    const name = (savingName ?? "").trim();
    if (!name) {
      setSavingName(null);
      return;
    }
    const brokers = [...presets, { name, url: realUrlRef.current }];
    try {
      await api.saveMqttConfig({ brokers });
      setPresets(brokers);
      setPresetSel(String(brokers.length - 1));
      setSavingName(null);
      notify(`Saved broker preset “${name}”`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  // ---------- connect / reconnect / disconnect ----------

  const appendDiag = (text: string, ok: boolean) =>
    setDiag((d) => [...d, { text, ok }].slice(-100));

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
    sessionActiveRef.current = true;
    api
      .mqttConnect(
        realUrlRef.current,
        subFilterRef.current.trim() || "#",
        (e) => {
          if (e.ev === "stage") {
            appendDiag(
              `${e.ok ? "✓" : "✗"} ${e.stage} — ${e.detail}${e.ok ? ` (${e.elapsed_ms} ms)` : ""}`,
              e.ok,
            );
            // Connected only once the subscription is live (CONNACK ok is a
            // prerequisite for a SUBACK, so this covers both per D4).
            if (e.stage === "suback" && e.ok) {
              attemptRef.current = 0;
              setAttempt(0);
              setConn("connected");
            }
          } else if (e.ev === "msg") {
            // Push even while the tab is hidden — the poll only repaints.
            store.push({
              topic: e.topic,
              payload: e.payload,
              b64: e.b64,
              retain: e.retain,
              qos: e.qos,
              ts: e.ts,
            });
          } else {
            appendDiag(`✗ closed — ${e.reason}`, false);
            sessionActiveRef.current = false;
            if (userDisconnectedRef.current) setConn("disconnected");
            else scheduleReconnect();
          }
        },
      )
      .catch((err) => {
        appendDiag(`✗ connect — ${String(err)}`, false);
        sessionActiveRef.current = false;
        if (userDisconnectedRef.current) setConn("disconnected");
        else scheduleReconnect();
      });
  };

  const startConnect = () => {
    if (!realUrlRef.current.trim()) {
      notify("Enter a broker URL first", true);
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
    if (sessionActiveRef.current) {
      sessionActiveRef.current = false;
      api.mqttDisconnect().catch((e) => notify(String(e), true));
    }
    setConn("disconnected");
  };

  // Tear down an orphaned session if the panel unmounts (ScopeView pattern).
  useEffect(
    () => () => {
      userDisconnectedRef.current = true;
      clearTimers();
      if (sessionActiveRef.current) api.mqttDisconnect().catch(() => {});
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

  // ---------- publish ----------

  const doPublish = async () => {
    if (conn !== "connected" || !pubTopic.trim()) return;
    try {
      await api.mqttPublish(pubTopic.trim(), pubPayload, pubRetain);
    } catch (e) {
      notify(String(e), true);
    }
  };

  // ---------- derived ----------

  const trimmedFilter = feedFilter.trim();
  const snap = store.snapshot(trimmedFilter || undefined);
  // The topic column always shows every topic, so an active filter can still
  // be switched by clicking another row.
  const topicSnap = trimmedFilter ? store.snapshot() : snap;
  const topics = [...topicSnap.topics].sort((a, b) => a[0].localeCompare(b[0]));
  const totalCount = topics.reduce((n, [, s]) => n + s.count, 0);

  const chipLabel =
    conn === "reconnecting"
      ? `reconnecting in ${countdown}s (attempt ${attempt})`
      : conn === "connecting"
        ? "connecting…"
        : conn;

  const pickTopic = (t: string) => {
    setFeedFilter(t);
    setPubTopic(t);
  };

  // ---------- render ----------

  return (
    <section
      className="obs-panel"
      style={active ? undefined : { display: "none" }}
    >
      {/* --- broker row --- */}
      <div className="obs-row">
        <select
          className="select small"
          value={presetSel}
          onChange={(e) => onPresetChange(e.target.value)}
          disabled={conn !== "disconnected"}
          title="Broker preset"
        >
          {presets.map((p, i) => (
            <option key={`${p.name}-${i}`} value={String(i)}>
              {p.name}
            </option>
          ))}
          <option value="custom">Custom…</option>
        </select>
        <input
          className="input mono obs-url"
          placeholder="mqtt://user:pass@host:1883"
          value={urlFocused ? realUrl : redactPassword(realUrl)}
          onFocus={() => setUrlFocused(true)}
          onBlur={() => setUrlFocused(false)}
          onChange={(e) => {
            setRealUrl(e.target.value);
            setPresetSel("custom");
          }}
          disabled={conn !== "disconnected"}
          title="Broker URL — the password is shown redacted until you edit"
        />
        <input
          className="input mono obs-sub"
          placeholder="#"
          value={subFilter}
          onChange={(e) => {
            setSubFilter(e.target.value);
            subFilterRef.current = e.target.value;
          }}
          disabled={conn === "connected"}
          title="Subscribe filter"
        />
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
        {savingName === null ? (
          <button
            className="btn small"
            onClick={() => setSavingName("")}
            title="Save the current URL as a named preset in mqtt.json"
          >
            Save preset
          </button>
        ) : (
          <input
            className="input obs-preset-name"
            autoFocus
            placeholder="preset name (Enter)"
            value={savingName}
            onChange={(e) => setSavingName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void savePreset();
              if (e.key === "Escape") setSavingName(null);
            }}
            onBlur={() => setSavingName(null)}
          />
        )}
      </div>

      {/* --- diagnostics strip --- */}
      <details className="obs-diag">
        <summary>Diagnostics{diag.length > 0 ? ` (${diag.length})` : ""}</summary>
        <div className="obs-diag-lines">
          {diag.length === 0 && (
            <div className="obs-diag-line">nothing yet — connect to a broker</div>
          )}
          {diag.map((d, i) => (
            <div
              key={i}
              className={d.ok ? "obs-diag-line ok" : "obs-diag-line fail"}
            >
              {d.text}
            </div>
          ))}
        </div>
      </details>

      {/* --- topics + feed --- */}
      <div className="obs-main">
        <div className="obs-topics">
          <div
            className={
              trimmedFilter === "" ? "obs-topic-row active" : "obs-topic-row"
            }
            onClick={() => setFeedFilter("")}
            title="Show every topic"
          >
            <span className="obs-topic-name">all</span>
            <span className="obs-count">{totalCount}</span>
          </div>
          {topics.map(([t, s]) => (
            <div
              key={t}
              className={
                trimmedFilter === t ? "obs-topic-row active" : "obs-topic-row"
              }
              onClick={() => pickTopic(t)}
              title={t}
            >
              <span className="obs-topic-name">{t}</span>
              <span className="obs-count">{s.count}</span>
              {s.ratePerSec > 0 && (
                <span className="obs-rate">{s.ratePerSec.toFixed(1)}/s</span>
              )}
              <span className="obs-topic-last">{s.last}</span>
            </div>
          ))}
        </div>

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

      {/* --- publish row --- */}
      <div className="obs-pub-row">
        <input
          className="input mono obs-pub-topic"
          placeholder="topic"
          value={pubTopic}
          onChange={(e) => setPubTopic(e.target.value)}
        />
        <input
          className="input mono obs-pub-payload"
          placeholder="payload (Enter to publish)"
          value={pubPayload}
          onChange={(e) => setPubPayload(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void doPublish()}
        />
        <label className="field">
          <input
            type="checkbox"
            checked={pubRetain}
            onChange={(e) => setPubRetain(e.target.checked)}
          />
          retain
        </label>
        <button
          className="btn small primary"
          disabled={conn !== "connected" || !pubTopic.trim()}
          onClick={doPublish}
        >
          Publish
        </button>
      </div>
    </section>
  );
}
