import { useEffect, useRef, useState } from "react";
import {
  filterRows,
  jumpTarget,
  shortenToolchainPath,
  summaryLabel,
  type BuildModel,
  type Diagnostic,
  type JumpTarget,
  type Row,
} from "../diagnostics";

interface BuildConsoleProps {
  model: BuildModel;
  /** Absolute sketch directory; without it nothing is jumpable. */
  sketchDir: string | null;
  /** Sketch-relative names the editor can actually open, when known. */
  knownFiles?: ReadonlySet<string>;
  onJump: (t: JumpTarget) => void;
  onClear: () => void;
}

/** `path:line[:col]: severity: message`, with the path already shortened. */
function diagText(d: Diagnostic, displayPath: string): string {
  if (!d.loc) return d.message;
  const col = d.loc.col === null ? "" : `:${d.loc.col}`;
  // `fatal error` is why nothing after it was compiled; do not flatten it.
  const sev = d.fatal ? "fatal error" : d.severity;
  return `${displayPath}:${d.loc.line}${col}: ${sev}: ${d.message}`;
}

/**
 * The build tab's console: the same lines `Console` shows, but parsed — a
 * summary strip, colour by severity, and a click on a sketch-local diagnostic
 * that puts the cursor on the offending line.
 *
 * All the judgement lives in `../diagnostics`; this file only decides what a
 * row looks like.
 */
export default function BuildConsole({
  model,
  sketchDir,
  knownFiles,
  onJump,
  onClear,
}: BuildConsoleProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  // Deliberately duplicated from Console.tsx:13-28 rather than extracted: the
  // codebase has no custom hooks, and one shared hook for two consoles that
  // are diverging (this one filters rows) would be the wrong abstraction.
  const stickToBottom = useRef(true);
  const [errorsOnly, setErrorsOnly] = useState(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
  }, [model.rows]);

  // Derived, not stored: with nothing to filter to the toggle goes inert, so
  // fixing the code cannot strand an almost-empty console behind a control
  // that is now disabled and can no longer be un-pressed.
  const showErrorsOnly = errorsOnly && model.summary.errors > 0;

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    stickToBottom.current =
      el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  const label = summaryLabel(model.summary);

  const renderRow = (row: Row) => {
    if (row.kind !== "diag")
      return (
        <div
          key={row.index}
          className={
            `console-row ${row.kind}` +
            // Raw stderr is NOT warn-coloured here the way it is in the serial
            // console: on a build stream most stderr is ordinary progress.
            ("tone" in row && row.tone ? ` ${row.tone}` : "")
          }
        >
          {row.text}
        </div>
      );

    const d = row.diag;
    const target = jumpTarget(d, sketchDir, knownFiles);
    const title = d.loc ? d.loc.path : undefined;
    // Jumpable rows name the file the way the sketch does; everything else
    // gets the elided toolchain form.
    const shown = target
      ? target.rel
      : d.loc
        ? shortenToolchainPath(d.loc.path)
        : "";

    if (target)
      return (
        <button
          key={row.index}
          type="button"
          className={`console-row diag ${d.severity} clickable`}
          title={title}
          onClick={() => onJump(target)}
        >
          {diagText(d, shown)}
        </button>
      );
    return (
      <div
        key={row.index}
        className={`console-row diag ${d.severity}`}
        title={title}
      >
        {diagText(d, shown)}
      </div>
    );
  };

  return (
    <div className="console">
      {/* Always rendered, with or without a label, so the bar does not jump
          when the first diagnostic arrives (height is pinned in CSS) — and so
          the live region exists BEFORE the first summary, which is what makes
          a screen reader announce it rather than silently gain a landmark. */}
      <div
        className={label ? `build-summary ${label.tone}` : "build-summary"}
        role="status"
        aria-live="polite"
      >
        {label && <span className="build-summary-text">{label.text}</span>}
        <div className="spacer" />
        <button
          type="button"
          className={showErrorsOnly ? "btn small toggled" : "btn small"}
          aria-pressed={showErrorsOnly}
          disabled={model.summary.errors === 0}
          title="Show only errors"
          onClick={() => setErrorsOnly((v) => !v)}
        >
          errors only
        </button>
        <button type="button" className="btn small" onClick={onClear}>
          Clear
        </button>
      </div>
      <div
        className="console-scroll"
        role="log"
        aria-live="off"
        ref={scrollRef}
        onScroll={onScroll}
      >
        {filterRows(model.rows, showErrorsOnly).map(renderRow)}
      </div>
    </div>
  );
}
