// The Serial Monitor tab: toolbar, virtualised log, send box.
//
// A leaf component — it owns its own view state (filter, prefs, scroll, tx
// history) and nothing else. The lines live in a `SerialStore` the parent
// feeds from `serial://line`; this panel polls that store's `version` at
// 10 Hz rather than re-rendering per line (Tier 3, `frontend.md` §2). A board
// at 921600 baud emits faster than React can commit, and the old monitor
// re-rendered up to 5000 rows for each one.
//
// Always mounted, hidden with `display:none` when another tab is up (the
// MqttPanel/ScopeView pattern): unmounting would throw away the scrollback
// and the scroll position every time the user looked at the build log.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import * as api from "../api";
import {
  BAUD_RATES,
  LINE_ENDINGS,
  LINE_ENDING_LABEL,
  loadUiPrefs,
  saveUiPrefs,
  withLineEnding,
  type BaudSource,
  type LineEnding,
  type SerialUiPrefs,
  type StorageLike,
} from "../serialPrefs";
import { exportText, type SerialStore } from "../serial/serialStore";
import {
  emptyHistory,
  pushHistory,
  recall,
  type TxHistory,
} from "../serial/txHistory";
import { ROW_HEIGHT, isNearBottom, visibleRange } from "../serial/virtualize";
import { fileStamp, hms } from "../timeFormat";

/** Retrying is the auto-reconnect state the parent drives after a hotplug. */
export type SerialConnection =
  | { state: "on" }
  | { state: "off" }
  | { state: "retrying"; attempt: number; max: number };

interface Props {
  /** False hides the panel; polling stops with it. */
  active: boolean;
  store: SerialStore;
  monitorOn: boolean;
  /** A flash is running: esptool holds the port, so there is nothing here to
   *  start and nothing to stop. Deliberately narrower than App's `busy` — a
   *  verify or a fleet sync does not touch the port, and greying out Stop
   *  during one (while telling the user it was "Flashing") was a lie that
   *  also took away the one control that frees a wedged monitor. */
  flashing: boolean;
  portLabel: string | null;
  baud: number;
  baudSource: BaudSource;
  sketchBaud: number | null;
  onBaudChange: (baud: number) => void;
  onUseSketchBaud: () => void;
  connection: SerialConnection;
  onToggleMonitor: () => void;
  /** Receives the data with the line ending already appended — the backend
   *  writes it verbatim. */
  onSend: (bytes: string) => Promise<void>;
  notify: (msg: string, isError?: boolean) => void;
  /** Injected in tests; `window.localStorage` in the app. */
  storage?: StorageLike;
}

/** 1234 → "1,234". Hand-rolled rather than `toLocaleString` so the count in
 *  the toolbar reads the same on every machine (and in tests). */
const group = (n: number) => String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");

const STATUS_TEXT = (c: SerialConnection) =>
  c.state === "on"
    ? "● on"
    : c.state === "retrying"
      ? `↻ retrying ${c.attempt}/${c.max}`
      : "○ off";

export default function SerialMonitor({
  active,
  store,
  monitorOn,
  flashing,
  portLabel,
  baud,
  baudSource,
  sketchBaud,
  onBaudChange,
  onUseSketchBaud,
  connection,
  onToggleMonitor,
  onSend,
  notify,
  storage,
}: Props) {
  const prefsStorage = storage ?? window.localStorage;
  const [prefs, setPrefs] = useState<SerialUiPrefs>(() =>
    loadUiPrefs(prefsStorage),
  );
  const [filter, setFilter] = useState("");
  const [tx, setTx] = useState("");
  const [history, setHistory] = useState<TxHistory>(emptyHistory);
  // Bumped by the poll below; the store itself is not React state.
  const [, setTick] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(0);
  /** Whether the view is still stuck to the bottom. Session-local on purpose:
   *  scrolling up to read something is a look, not a preference change, and
   *  writing `autoscroll:false` to storage for it meant one glance at an old
   *  line disabled autoscroll for every future launch. The stored pref is the
   *  *intent* ("follow the tail"); this flag is whether we are following it
   *  right now. Scrolling back to the bottom re-follows, as in a terminal. */
  const [following, setFollowing] = useState(true);

  const logRef = useRef<HTMLDivElement>(null);
  const lastVersionRef = useRef(-1);
  const scrollTopRef = useRef(0);
  const rafRef = useRef<number | null>(null);
  /** A send is awaiting the backend. A ref, not state: the guard must be
   *  true for the *next* keydown in the same tick, before any re-render. */
  const sendingRef = useRef(false);

  /** Persist alongside every change — the prefs' whole point is surviving a
   *  relaunch, and a "save" button for three toggles would be absurd. The
   *  write happens here rather than inside the `setPrefs` updater, which must
   *  stay pure (React may call it twice). */
  const updatePrefs = (patch: Partial<SerialUiPrefs>) => {
    const next = { ...prefs, ...patch };
    saveUiPrefs(prefsStorage, next);
    setPrefs(next);
  };

  // ---------- 10 Hz poll (only while shown; the store fills regardless) ----

  useEffect(() => {
    if (!active) return;
    // A swapped store starts from an unknown version, so force the first
    // tick to publish it rather than comparing against the old one's.
    lastVersionRef.current = -1;
    const iv = window.setInterval(() => {
      const el = logRef.current;
      // No ResizeObserver in jsdom (and none needed before first paint):
      // reading clientHeight on the tick we already have is cheap enough.
      if (el && typeof ResizeObserver === "undefined") {
        setViewport((v) => (v === el.clientHeight ? v : el.clientHeight));
      }
      if (store.version !== lastVersionRef.current) {
        lastVersionRef.current = store.version;
        setTick((t) => t + 1);
      }
    }, 100);
    return () => window.clearInterval(iv);
  }, [active, store]);

  // Measured height, when the browser will tell us properly.
  useEffect(() => {
    const el = logRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => setViewport(el.clientHeight));
    ro.observe(el);
    setViewport(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  useEffect(
    () => () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    },
    [],
  );

  // ---------- derived ----------

  const trimmed = filter.trim();
  const snap = store.snapshot(trimmed || undefined);
  const autoFollow = prefs.autoscroll && following;
  // While following, the window is derived from the row count rather than
  // from the last scroll event. At >80 lines/s the rows are added and the
  // element is scrolled in the same commit, but `scrollTop` state only
  // catches up a frame later via the rAF in `onScroll` — so the window was
  // computed from a position several hundred rows stale and the viewport
  // painted a blank band on every tick. The bottom of the list is arithmetic
  // we already have; there is no need to ask the DOM where it went.
  const anchored = autoFollow
    ? Math.max(0, snap.rows.length * ROW_HEIGHT - viewport)
    : scrollTop;
  const range = visibleRange(anchored, viewport, snap.rows.length);
  const window_ = snap.rows.slice(range.start, range.end);

  // Layout, not passive: this runs after the commit that added the rows (so
  // scrollHeight is already the new one) but *before* the browser paints, so
  // the frame that shows the new rows is already scrolled to them. As a plain
  // effect it painted the pre-scroll position first and a fast feed flickered.
  useLayoutEffect(() => {
    const el = logRef.current;
    if (!el || !autoFollow) return;
    el.scrollTop = el.scrollHeight;
  });

  // ---------- handlers ----------

  const onScroll = () => {
    const el = logRef.current;
    if (!el) return;
    scrollTopRef.current = el.scrollTop;
    const near = isNearBottom(el.scrollTop, el.clientHeight, el.scrollHeight);
    // Scrolling away un-follows and scrolling back to the bottom re-follows —
    // the gesture a terminal uses. It moves the session flag only: the stored
    // pref belongs to the Autoscroll button, and nothing else may write it.
    if (near !== following) setFollowing(near);
    // One frame, one state write: a fast wheel fires scroll far more often
    // than the browser paints.
    if (rafRef.current !== null) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      setScrollTop(scrollTopRef.current);
    });
  };

  const doSend = async () => {
    const line = tx;
    // Leaning on Enter while a write is slow would queue three copies of the
    // same command at a board still chewing the first.
    if (!monitorOn || !line || sendingRef.current) return;
    sendingRef.current = true;
    try {
      await onSend(withLineEnding(line, prefs.lineEnding));
      // Echo only what was actually accepted, and only the typed text — the
      // line ending is wire detail, not something to show back.
      store.push("tx", line, Date.now());
      setHistory((h) => pushHistory(h, line));
      setTx("");
    } catch (e) {
      notify(String(e), true);
    } finally {
      sendingRef.current = false;
    }
  };

  const onTxKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void doSend();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const r = recall(history, e.key === "ArrowUp" ? "up" : "down", tx);
      setHistory(r.history);
      setTx(r.value);
    } else if (e.key === "Escape") {
      setTx("");
    }
  };

  const doExport = async () => {
    try {
      const path = await save({
        defaultPath: `bancada-serial-${fileStamp(new Date())}.txt`,
        filters: [{ name: "Text", extensions: ["txt", "log"] }],
      });
      if (!path) return;
      await api.saveTextFile(path, exportText(snap.rows, {
        timestamps: prefs.timestamps,
      }));
      notify(`Saved ${path}`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  // ---------- toolbar bits ----------

  const bauds = BAUD_RATES.includes(baud as (typeof BAUD_RATES)[number])
    ? [...BAUD_RATES]
    : [...BAUD_RATES, baud].sort((a, b) => a - b);

  const startTitle = flashing
    ? "Flashing — the monitor restarts when the flash finishes"
    : portLabel === null
      ? "Select a port first"
      : monitorOn
        ? "Stop the serial monitor"
        : "Start the serial monitor";

  const countText = trimmed
    ? `${group(snap.matched)} of ${group(snap.visibleTotal)} lines`
    : `${group(snap.visibleTotal)} lines`;
  // The count shows what is on screen; the title admits what the ring ate,
  // so a user reading a boot log knows the top of it is gone.
  const countTitle =
    snap.dropped > 0
      ? `${group(snap.total)} lines seen, +${group(snap.dropped)} dropped from the buffer`
      : `${group(snap.total)} lines seen`;

  return (
    <section
      className="serial-monitor"
      style={active ? undefined : { display: "none" }}
    >
      <div className="serial-toolbar">
        <button
          className={monitorOn ? "btn small" : "btn small primary"}
          onClick={onToggleMonitor}
          disabled={flashing || portLabel === null}
          title={startTitle}
        >
          {monitorOn ? "Stop" : "Start"}
        </button>
        <span
          className="serial-status"
          data-state={connection.state}
          role="status"
        >
          {STATUS_TEXT(connection)}
        </span>
        <span className="serial-port" title={portLabel ?? "no port selected"}>
          {portLabel ?? "no port"}
        </span>
        <select
          className="select small"
          aria-label="Baud rate"
          value={baud}
          onChange={(e) => onBaudChange(Number(e.target.value))}
        >
          {bauds.map((b) => (
            <option key={b} value={b}>
              {b} baud
            </option>
          ))}
        </select>
        {baudSource === "override" && sketchBaud !== null && (
          <button
            className="btn small"
            onClick={onUseSketchBaud}
            title={`Serial.begin(${sketchBaud}) found in the sketch`}
          >
            Use sketch&apos;s {sketchBaud}
          </button>
        )}
        <select
          className="select small"
          aria-label="Line ending"
          value={prefs.lineEnding}
          onChange={(e) =>
            updatePrefs({ lineEnding: e.target.value as LineEnding })
          }
        >
          {LINE_ENDINGS.map((e) => (
            <option key={e} value={e}>
              {LINE_ENDING_LABEL[e]}
            </option>
          ))}
        </select>
        <button
          className={prefs.timestamps ? "btn small toggled" : "btn small"}
          onClick={() => updatePrefs({ timestamps: !prefs.timestamps })}
          aria-pressed={prefs.timestamps}
          title="Stamp each line with the time it arrived"
        >
          Timestamps
        </button>
        <button
          className={prefs.autoscroll ? "btn small toggled" : "btn small"}
          onClick={() => {
            const on = !prefs.autoscroll;
            updatePrefs({ autoscroll: on });
            // Switching it back on catches up with the tail, whatever the
            // scroll position had un-followed to. Off leaves the flag alone:
            // it means nothing while the pref is off, and clobbering it would
            // strand the view mid-log when the pref comes back on.
            if (on) setFollowing(true);
          }}
          aria-pressed={prefs.autoscroll}
          title="Keep the newest line in view"
        >
          Autoscroll
        </button>
        <button
          className="btn small"
          onClick={() => store.setPaused(!snap.paused)}
          aria-pressed={snap.paused}
          title={
            snap.paused
              ? "Resume the log (nothing was lost)"
              : "Freeze the log — lines keep arriving behind it"
          }
        >
          {snap.paused
            ? snap.bufferedWhilePaused > 0
              ? `▶ Resume (${group(snap.bufferedWhilePaused)} buffered)`
              : "▶ Resume"
            : "⏸ Pause"}
        </button>
        <input
          className="input mono serial-filter"
          aria-label="Filter lines"
          placeholder="filter…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <span className="serial-count" title={countTitle}>
          {countText}
        </span>
        <button
          className="btn small"
          onClick={() => store.clear()}
          title="Drop the scrollback"
        >
          Clear
        </button>
        <button
          className="btn small"
          onClick={() => void doExport()}
          title="Save the visible lines to a text file"
        >
          Export
        </button>
      </div>

      <div className="serial-log" ref={logRef} onScroll={onScroll}>
        {snap.rows.length === 0 ? (
          <div className="empty-hint">
            {trimmed
              ? "no lines match the filter"
              : "no serial output yet — press Start"}
          </div>
        ) : (
          <div
            className="serial-log-spacer"
            style={{ height: range.totalHeight }}
          >
            <div
              className="serial-log-window"
              style={{ transform: `translateY(${range.offsetY}px)` }}
            >
              {window_.map((r) => (
                <div key={r.seq} className={`serial-row ${r.stream}`}>
                  {prefs.timestamps && (
                    <span className="serial-ts">{hms(r.ts)}</span>
                  )}
                  {r.stream === "tx" ? `❯ ${r.text}` : r.text}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>

      <div className="serial-tx">
        <input
          className="input mono"
          aria-label="Send to board"
          placeholder="send to board… (Enter)"
          value={tx}
          onChange={(e) => setTx(e.target.value)}
          onKeyDown={onTxKeyDown}
        />
        <button
          className="btn small"
          onClick={() => void doSend()}
          disabled={!monitorOn}
          title={monitorOn ? "Send the line" : "Start the monitor first"}
        >
          Send
        </button>
      </div>
    </section>
  );
}
