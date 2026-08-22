// ObsLog — dumb, shared message feed for the observability panels (MQTT and
// WebSocket). Owns no data: rows come from an ObsStore snapshot in the parent
// and every mutation (pause, clear, filter) goes back up through callbacks.
// Click a row to expand it: pretty-printed JSON when the payload parses,
// otherwise the raw text; binary (base64) payloads get a hex-dump preview.

import { useEffect, useRef, useState } from "react";
import type { ObsMsg } from "../obs/obsStore";
import { hms } from "../timeFormat";

interface Props {
  rows: ObsMsg[];
  paused: boolean;
  /** Messages held back while paused — shown on the Resume button. */
  bufferedCount: number;
  autoscroll: boolean;
  filter: string;
  onTogglePause: () => void;
  onToggleAutoscroll: () => void;
  onFilterChange: (f: string) => void;
  onClear: () => void;
}

/** offset + hex + ascii columns for the first 256 bytes of a base64 payload. */
const hexDump = (b64: string): string => {
  let bin: string;
  try {
    bin = atob(b64);
  } catch {
    return "(payload is not valid base64)";
  }
  const n = Math.min(bin.length, 256);
  const lines: string[] = [];
  for (let off = 0; off < n; off += 16) {
    const end = Math.min(off + 16, n);
    let hex = "";
    let ascii = "";
    for (let i = off; i < end; i++) {
      const b = bin.charCodeAt(i);
      hex += b.toString(16).padStart(2, "0") + " ";
      ascii += b >= 0x20 && b < 0x7f ? bin[i] : ".";
    }
    lines.push(
      `${off.toString(16).padStart(4, "0")}  ${hex.padEnd(48)} ${ascii}`,
    );
  }
  if (bin.length > 256) lines.push(`… ${bin.length - 256} more bytes`);
  return lines.length ? lines.join("\n") : "(empty payload)";
};

const expandedBody = (m: ObsMsg) => {
  if (m.b64) return <pre className="obs-hex">{hexDump(m.payload)}</pre>;
  let text = m.payload;
  try {
    text = JSON.stringify(JSON.parse(m.payload), null, 2);
  } catch {
    /* not JSON — show the raw payload */
  }
  return <pre className="obs-hex">{text}</pre>;
};

export default function ObsLog({
  rows,
  paused,
  bufferedCount,
  autoscroll,
  filter,
  onTogglePause,
  onToggleAutoscroll,
  onFilterChange,
  onClear,
}: Props) {
  // The expanded row is remembered as the message itself: the ring buffer
  // reuses objects across snapshots, so reference equality survives repaints;
  // the field comparison is a fallback in case the store ever copies.
  const [expanded, setExpanded] = useState<ObsMsg | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  const isExpanded = (m: ObsMsg) =>
    expanded !== null &&
    (m === expanded ||
      (m.ts === expanded.ts &&
        m.topic === expanded.topic &&
        m.payload === expanded.payload));

  // Autoscroll on new rows (Console.tsx precedent, opt-in via the toggle).
  const lastTs = rows.length > 0 ? rows[rows.length - 1].ts : 0;
  useEffect(() => {
    if (autoscroll) endRef.current?.scrollIntoView({ block: "end" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows.length, lastTs, autoscroll]);

  return (
    <div className="obs-log">
      <div className="obs-row">
        <button
          className="btn small"
          onClick={onTogglePause}
          title={paused ? "Resume the feed" : "Pause the feed (messages keep counting)"}
        >
          {paused
            ? bufferedCount > 0
              ? `▶ Resume (${bufferedCount} buffered)`
              : "▶ Resume"
            : "⏸ Pause"}
        </button>
        <button
          className={autoscroll ? "btn small toggled" : "btn small"}
          onClick={onToggleAutoscroll}
          title="Keep the newest message in view"
        >
          Autoscroll
        </button>
        <input
          className="input mono obs-filter"
          placeholder="filter…"
          value={filter}
          onChange={(e) => onFilterChange(e.target.value)}
        />
        <div className="spacer" />
        <button className="btn small" onClick={onClear}>
          Clear
        </button>
      </div>

      <div className="obs-log-list">
        {rows.length === 0 && <div className="empty-hint">no messages yet</div>}
        {rows.map((m, i) => {
          const ex = isExpanded(m);
          return (
            <div
              key={`${m.ts}-${i}`}
              className={ex ? "obs-log-row expanded" : "obs-log-row"}
              onClick={() => setExpanded(ex ? null : m)}
            >
              <div className="obs-log-line">
                <span className="obs-time">{hms(m.ts)}</span>
                <span className="obs-topic" title={m.topic}>
                  {m.topic}
                </span>
                <span className="obs-payload">
                  {m.b64 ? "(binary, base64)" : m.payload}
                </span>
                {m.retain && <span className="obs-badge">retain</span>}
                {m.qos > 0 && <span className="obs-badge">q{m.qos}</span>}
              </div>
              {ex && expandedBody(m)}
            </div>
          );
        })}
        <div ref={endRef} />
      </div>
    </div>
  );
}
