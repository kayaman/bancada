// StatusBar — the app's bottom line and the hairline progress bar above it.
// All of the wording and every progress decision live in `src/statusLine.ts`;
// this component owns exactly one thing the module cannot, a `now` that moves.
//
// The ticker runs **only while something is running**. A 500 ms interval that
// never stops would re-render the whole footer twice a second for the entire
// life of the app, and at rest there is nothing on the line that changes.

import { useEffect, useState } from "react";
import {
  type Activity,
  type LastResult,
  progressMode,
  statusLineText,
} from "../statusLine";

interface Props {
  activity: Activity | null;
  lastResult: LastResult | null;
  project: string | null;
  portName: string | null;
  busy: boolean;
  /** Parsed out of the uploader's own output; null when it says nothing. */
  measuredFraction: number | null;
  /** How long this op took last time, for the "usually ~" hint. */
  estimateMs: number | null;
  /** Elapsed against that remembered duration; the dashed fallback. */
  estimateFraction: number | null;
}

export default function StatusBar({
  activity,
  lastResult,
  project,
  portName,
  busy,
  measuredFraction,
  estimateMs,
  estimateFraction,
}: Props) {
  const [now, setNow] = useState(() => Date.now());

  // Keyed on the activity's identity rather than the object, so a parent that
  // rebuilds the prop inline every render does not re-arm the interval.
  const activityKey = activity ? `${activity.key}:${activity.startedAt}` : null;
  useEffect(() => {
    if (activityKey === null) return;
    // Read the clock immediately: `now` has been frozen since the last
    // activity ended, so without this the first frame of a build shows an
    // elapsed time measured from whenever the ticker last stopped.
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(id);
  }, [activityKey]);

  const { text, isError } = statusLineText({
    activity,
    now,
    lastResult,
    project,
    portName,
    estimateMs,
  });
  const { mode, fraction } = progressMode(
    busy,
    measuredFraction,
    estimateFraction,
  );
  const pct = Math.round(fraction * 100);

  return (
    <footer className={`statusbar${isError ? " error" : ""}`}>
      {/* Always in the DOM, pinned and 2 px tall even at rest: a bar that
          appears and disappears would shove the status text by two pixels
          every time a build starts. */}
      <div
        className="statusbar-progress"
        role="progressbar"
        aria-label="Build progress"
        aria-valuemin={0}
        aria-valuemax={100}
        {...(mode === "measured" ? { "aria-valuenow": pct } : {})}
        {...(mode === "estimate" ? { "aria-valuetext": "estimated" } : {})}
      >
        {/* Indeterminate and none get their width from CSS — one slides, the
            other is zero — so neither may be pinned by an inline style. */}
        <div
          className={`fill ${mode}`}
          style={{
            width:
              mode === "indeterminate" || mode === "none"
                ? undefined
                : `${pct}%`,
          }}
        />
      </div>
      <span className="statusbar-text">{text}</span>
    </footer>
  );
}
