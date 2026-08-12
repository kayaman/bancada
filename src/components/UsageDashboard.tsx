// UsageDashboard — app-wide Assistant spend: one row per project from the
// cumulative usage record (usage.json), expandable into per-session rows
// that replay inline. Totals survive chat pruning, so a project can show
// more sessions than it has surviving files — the gap is stated, not
// padded with placeholder rows. Fetches on mount and on ⟳; deliberately no
// live updates while a session streams (project-usage-totals spec).

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { ProjectUsage, SessionEntry } from "../api";
import { replayChat } from "../agent/chatLog";
import type { AgentStore } from "../agent/agentStore";
import { formatTokens } from "../agent/usage";
import {
  grandTotals,
  projectName,
  prunedCount,
  stemToDisplay,
} from "../usageDashboard";
import { ReplayView, TurnSummaryView, type TurnEnd } from "./AgentPanel";
import type { BottomTab } from "../bottomTabs";

interface Props {
  onClose: () => void;
  openBottomTab: (tab: BottomTab) => void;
}

export default function UsageDashboard({ onClose, openBottomTab }: Props) {
  const [rows, setRows] = useState<ProjectUsage[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** sketch_dir of the project whose sessions are expanded. */
  const [expanded, setExpanded] = useState<string | null>(null);
  /** Ref mirroring expanded state for guarding stale chatListUsage responses;
   *  updated synchronously whenever expanded is set. */
  const expandedRef = useRef<string | null>(null);
  const [sessions, setSessions] = useState<SessionEntry[] | null>(null);
  const [replay, setReplay] = useState<{
    store: AgentStore;
    title: string;
  } | null>(null);
  const [viewTurn, setViewTurn] = useState<TurnEnd | null>(null);

  const refresh = () => {
    setError(null);
    api
      .usageOverview()
      .then(setRows)
      .catch((e) => {
        setRows([]);
        setError(String(e));
      });
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, []);

  const toggleProject = (row: ProjectUsage) => {
    setReplay(null);
    setViewTurn(null);
    if (expanded === row.sketch_dir) {
      setExpanded(null);
      expandedRef.current = null;
      setSessions(null);
      return;
    }
    setExpanded(row.sketch_dir);
    expandedRef.current = row.sketch_dir;
    setSessions(null);
    // Capture sketch_dir at call time; only apply response if it still matches
    // the expanded row (guards against slow responses landing after a collapse
    // or switch to another project).
    const requestedDir = row.sketch_dir;
    api
      .chatListUsage(requestedDir)
      .then((s) => {
        if (requestedDir === expandedRef.current) setSessions(s);
      })
      .catch(() => {
        if (requestedDir === expandedRef.current) setSessions([]);
      });
  };

  const openSession = (row: ProjectUsage, s: SessionEntry) => {
    api
      .chatLoad(row.sketch_dir, s.file)
      .then((lines) => setReplay({ store: replayChat(lines), title: s.title }))
      .catch(() => {});
  };

  if (replay) {
    return (
      <div className="usage-dash">
        <div className="panel-tabs">
          <button
            className="btn small"
            onClick={() =>
              viewTurn ? setViewTurn(null) : setReplay(null)
            }
          >
            ← Back
          </button>
          <span className="usage-title">{replay.title}</span>
          <div className="spacer" />
          <button className="btn small" onClick={onClose} title="Close" aria-label="Close">
            ✕
          </button>
        </div>
        <div className="usage-replay">
          {viewTurn ? (
            <TurnSummaryView turn={viewTurn} onBack={() => setViewTurn(null)} />
          ) : (
            <ReplayView
              store={replay.store}
              openBottomTab={openBottomTab}
              onOpenTurn={setViewTurn}
            />
          )}
        </div>
      </div>
    );
  }

  const totals = grandTotals(rows ?? []);
  return (
    <div className="usage-dash">
      <div className="panel-tabs">
        <span className="usage-title">📊 Assistant usage</span>
        {rows && rows.length > 0 && (
          <span className="usage-grand">
            {totals.projects} project{totals.projects === 1 ? "" : "s"} · Σ $
            {totals.costUsd.toFixed(4)} · {formatTokens(totals.inputTokens)} in
            / {formatTokens(totals.outputTokens)} out
          </span>
        )}
        <div className="spacer" />
        <button className="btn small" onClick={refresh} title="Refresh" aria-label="Refresh">
          ⟳
        </button>
        <button className="btn small" onClick={onClose} title="Close" aria-label="Close">
          ✕
        </button>
      </div>
      <div className="usage-list">
        {error && <div className="usage-error">{error}</div>}
        {rows && rows.length === 0 && !error && (
          <div className="empty-hint">
            No Assistant usage recorded yet — costs appear here after the
            first chat.
          </div>
        )}
        {(rows ?? []).map((r) => (
          <div key={r.sketch_dir} className="usage-card">
            <button className="usage-row" onClick={() => toggleProject(r)}>
              <span className="usage-name">{projectName(r.sketch_dir)}</span>
              <span className="usage-path">{r.sketch_dir}</span>
              <span className="usage-stats">
                <span>Σ ${r.cost_usd.toFixed(4)}</span>
                <span>
                  {formatTokens(r.input_tokens)} in /{" "}
                  {formatTokens(r.output_tokens)} out
                </span>
                <span>{r.turns} turns</span>
                <span>
                  {r.sessions} session{r.sessions === 1 ? "" : "s"}
                </span>
                {r.last_chat && (
                  <span>last {stemToDisplay(r.last_chat)}</span>
                )}
              </span>
            </button>
            {expanded === r.sketch_dir && sessions && (
              <div className="usage-sessions">
                {sessions.map((s) => (
                  <button
                    key={s.file}
                    className="usage-session-row"
                    onClick={() => openSession(r, s)}
                  >
                    <span className="usage-session-title">{s.title}</span>
                    <span className="usage-session-stats">
                      {stemToDisplay(s.file.replace(/\.ndjson$/, ""))} · $
                      {s.cost_usd.toFixed(4)} · {formatTokens(s.input_tokens)}{" "}
                      in / {formatTokens(s.output_tokens)} out
                    </span>
                  </button>
                ))}
                {prunedCount(r, sessions.length) > 0 && (
                  <div className="usage-pruned">
                    {prunedCount(r, sessions.length)} older session
                    {prunedCount(r, sessions.length) === 1 ? "" : "s"} pruned —
                    still counted in the totals above.
                  </div>
                )}
                {sessions.length === 0 && (
                  <div className="usage-pruned">
                    No surviving chat files — totals above are the banked
                    record.
                  </div>
                )}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
