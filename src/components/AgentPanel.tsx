// AgentPanel — the Assistant bottom-panel: chat with the embedded `claude`
// CLI, auto-applied Edit/Write diff cards, and a Verify tool card that
// mirrors the build console.
//
// Mounted once and hidden with display:none (ScopeView/MqttPanel pattern) so
// the session survives tab switches. `store` (an `AgentStore`) is owned by
// App, not this panel — App-level `agent://event`/`agent://closed` listeners
// feed it so the unseen-dot bookkeeping keeps working while this panel isn't
// mounted yet. This panel only polls `store.version` to know when to repaint
// (MqttPanel's 4 Hz poll pattern, at 100 ms per the task brief).

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { AgentProbe, ChatEntry } from "../api";
import { replayChat } from "../agent/chatLog";
import type {
  AgentAlarm,
  AgentMessage,
  AgentStatus,
  AgentStore,
} from "../agent/agentStore";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { diffForToolInput, type DiffLine } from "../agent/diff";
import { alarmConsequence } from "../agent/alarmCopy";
import { parseVerifyResult } from "../agent/verifyResult";
import { activityLabel } from "../agent/activity";
import { formatTokens } from "../agent/usage";
import type { BottomTab } from "../bottomTabs";

type TurnEnd = Extract<AgentMessage, { kind: "turn_end" }>;

/** Markdown as the assistant wrote it — GFM, raw HTML stays escaped. */
function Md({ text }: { text: string }) {
  return (
    <div className="agent-markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}

interface AgentPanelProps {
  active: boolean;
  sketchDir: string | null;
  store: AgentStore;
  onSend: (text: string) => Promise<void>;
  onInterrupt: () => void;
  /** Stop the session and clear the transcript. */
  onNewSession: () => void;
  openBottomTab: (tab: BottomTab) => void;
  /** True when the sketch dir has no `.git` — edits auto-apply with no undo. */
  gitWarning: boolean;
}

const POLL_MS = 100;

export default function AgentPanel({
  active,
  sketchDir,
  store,
  onSend,
  onInterrupt,
  onNewSession,
  openBottomTab,
  gitWarning,
}: AgentPanelProps) {
  // ---------- repaint on store changes (only while shown) ----------

  const [tick, setTick] = useState(0);
  const lastVersionRef = useRef(-1);

  useEffect(() => {
    if (!active) return;
    const iv = window.setInterval(() => {
      if (store.version !== lastVersionRef.current) {
        lastVersionRef.current = store.version;
        setTick((t) => t + 1);
      }
    }, POLL_MS);
    return () => window.clearInterval(iv);
  }, [active, store]);

  // ---------- CLI availability, checked once ----------

  const [probe, setProbe] = useState<AgentProbe | null>(null);
  useEffect(() => {
    api
      .agentProbe()
      .then(setProbe)
      .catch((e) => setProbe({ ok: false, error: String(e) }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- input ----------

  const [draft, setDraft] = useState("");

  // ---------- turn summary view (a turn_end divider was clicked) ----------

  const [viewTurn, setViewTurn] = useState<TurnEnd | null>(null);

  // ---------- chat history (list of saved chats, and one replayed) ----------

  const [histList, setHistList] = useState<ChatEntry[] | null>(null);
  const [histStore, setHistStore] = useState<AgentStore | null>(null);

  const openHistory = () => {
    if (!sketchDir) return;
    setViewTurn(null);
    setHistStore(null);
    api
      .chatList(sketchDir)
      .then(setHistList)
      .catch(() => setHistList([]));
  };

  const openChat = (file: string) => {
    if (!sketchDir) return;
    api
      .chatLoad(sketchDir, file)
      .then((lines) => setHistStore(replayChat(lines)))
      .catch(() => {});
  };

  const deleteChat = (file: string) => {
    if (!sketchDir) return;
    api
      .chatDelete(sketchDir, file)
      .then(() => api.chatList(sketchDir).then(setHistList))
      .catch(() => {});
  };

  const closeHistory = () => {
    setViewTurn(null);
    if (histStore) setHistStore(null); // replay → back to the list
    else setHistList(null); // list → back to the live chat
  };

  // ---------- autoscroll (Console.tsx pattern: stick to bottom unless the
  // user has scrolled up to read something earlier) ----------

  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

  useEffect(() => {
    // A live stream must not yank the scroll while the user is reading a
    // replay, the history list, or a turn summary — those views share this
    // scroll container but follow the *live* store's tick.
    if (viewTurn || histList || histStore) return;
    const el = scrollRef.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
    // `tick` (not `snap`) so this only runs when the store actually changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    stickToBottom.current =
      el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  const send = () => {
    const text = draft.trim();
    if (!text) return;
    setDraft("");
    onSend(text).catch(() => {
      /* App.sendToAgent already reports errors via notify() */
    });
  };

  // ---------- empty states ----------

  if (sketchDir == null) {
    return (
      <section
        className="agent-panel"
        style={active ? undefined : { display: "none" }}
      >
        <div className="agent-empty">Open a sketch first.</div>
      </section>
    );
  }

  if (probe && !probe.ok) {
    return (
      <section
        className="agent-panel"
        style={active ? undefined : { display: "none" }}
      >
        <div className="agent-empty">
          <p>
            The Assistant needs the <code>claude</code> CLI on your PATH,
            signed in to Claude Code.
          </p>
          <p>Install it, sign in, then reopen this panel.</p>
          {probe.error && <p className="agent-empty-detail">{probe.error}</p>}
        </div>
      </section>
    );
  }

  const snap = store.snapshot();
  const disabled = snap.status === "ended";

  return (
    <section
      className="agent-panel"
      style={active ? undefined : { display: "none" }}
    >
      <div className="agent-scroll" ref={scrollRef} onScroll={onScroll}>
        {viewTurn ? (
          <TurnSummaryView turn={viewTurn} onBack={() => setViewTurn(null)} />
        ) : histStore ? (
          <ReplayView
            store={histStore}
            openBottomTab={openBottomTab}
            onOpenTurn={setViewTurn}
          />
        ) : histList ? (
          <HistListView
            entries={histList}
            onOpen={openChat}
            onDelete={deleteChat}
          />
        ) : (
          <>
        {snap.messages.length === 0 && (
          <div className="agent-empty">
            Ask the assistant to make a change, then Verify.
          </div>
        )}
        {snap.messages.map((m, i) => (
          <MessageView
            key={i}
            msg={m}
            openBottomTab={openBottomTab}
            onOpenTurn={setViewTurn}
          />
        ))}
        {/* Above the "session ended" line and styled to be impossible to
            mistake for a normal card: this is the host having *refused*
            something and killed the child, not the session merely finishing.
            See AgentSecurityAlarmEvent in src/agent/types.ts. */}
        {snap.alarm && <AlarmView alarm={snap.alarm} />}
        {snap.closedReason && (
          <div className="agent-closed">
            Session ended — {snap.closedReason}
          </div>
        )}
          </>
        )}
      </div>

      <div className="agent-footer">
        <span className="agent-status">
          {activityLabel(snap) ?? statusLabel(snap.status, snap.verifyRunning)}
        </span>
        {(snap.sessionUsage.costUsd > 0 ||
          snap.sessionUsage.inputTokens > 0) && (
          <span
            className="agent-cost"
            title="Session total: cost · input+output tokens"
          >
            Σ ${snap.sessionUsage.costUsd.toFixed(4)} ·{" "}
            {formatTokens(
              snap.sessionUsage.inputTokens + snap.sessionUsage.outputTokens,
            )}{" "}
            tokens
          </span>
        )}
        {gitWarning && (
          <span
            className="agent-git-warning"
            title="Edits auto-apply with no per-edit approval — without git there is no undo."
          >
            ⚠ not under git
          </span>
        )}
        <div className="spacer" />
        {snap.status === "running" && (
          <button className="btn small" onClick={onInterrupt}>
            Stop
          </button>
        )}
        <button className="btn small" onClick={openHistory} title="Saved chats for this sketch">
          🕘 History
        </button>
        <button
          className="btn small"
          onClick={() => {
            // Views from the old session must not linger into the new one.
            setViewTurn(null);
            setHistList(null);
            setHistStore(null);
            onNewSession();
          }}
        >
          New session
        </button>
      </div>

      {histList || histStore ? (
        <div className="agent-back-bar">
          <button className="btn small" onClick={closeHistory}>
            ← {histStore ? "Back to history" : "Back to current chat"}
          </button>
          <span className="agent-back-hint">
            {histStore
              ? "Read-only replay of a saved chat."
              : "Saved chats for this sketch — click one to read it."}
          </span>
        </div>
      ) : (
        <div className="agent-input-row">
          <textarea
            className="input mono agent-input"
            placeholder="Message the assistant… (Enter to send, Shift+Enter for newline)"
            value={draft}
            disabled={disabled}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
          />
          <button
            className="btn small primary"
            disabled={disabled || !draft.trim()}
            onClick={send}
          >
            Send
          </button>
        </div>
      )}
    </section>
  );
}

// ---------- security alarm ----------

/**
 * The alarm banner. The wording lives in `../agent/alarmCopy` — it is the
 * only safety text a real user sees, so it is a tested pure function rather
 * than string literals in JSX. See that module for why the copy is per-kind
 * and why nothing here reassures.
 */
function AlarmView({ alarm }: { alarm: AgentAlarm }) {
  return (
    <div className="agent-alarm" role="alert">
      <div className="agent-alarm-head">⛔ Bancada stopped the assistant</div>
      <p className="agent-alarm-detail">{alarm.detail}</p>
      <p className="agent-alarm-hint">{alarmConsequence(alarm.kind)}</p>
    </div>
  );
}

// ---------- message / tool rendering ----------

function MessageView({
  msg,
  openBottomTab,
  onOpenTurn,
}: {
  msg: AgentMessage;
  openBottomTab: (tab: BottomTab) => void;
  onOpenTurn: (turn: TurnEnd) => void;
}) {
  switch (msg.kind) {
    case "user":
      return <div className="agent-msg agent-msg-user">{msg.text}</div>;
    case "assistant":
      return (
        <div className="agent-msg agent-msg-assistant">
          <Md text={msg.text} />
        </div>
      );
    case "turn_end":
      return (
        <div className="agent-turn-end">
          <span className="agent-turn-end-label">{turnEndLabel(msg)}</span>
          <button
            className="agent-turn-end-details"
            onClick={() => onOpenTurn(msg)}
          >
            details ▸
          </button>
        </div>
      );
    case "stderr":
      return (
        <details className="agent-stderr">
          <summary>stderr</summary>
          <pre className="agent-stderr-body">{msg.line}</pre>
        </details>
      );
    case "tool":
      return <ToolCard msg={msg} openBottomTab={openBottomTab} />;
    default:
      return null;
  }
}

function ToolCard({
  msg,
  openBottomTab,
}: {
  msg: Extract<AgentMessage, { kind: "tool" }>;
  openBottomTab: (tab: BottomTab) => void;
}) {
  if (msg.name === "Edit" || msg.name === "Write") {
    const diff = diffForToolInput(msg.name, msg.input);
    const filePath = stringField(msg.input, "file_path");
    return (
      <div className="agent-tool">
        <div className="agent-tool-head">
          <span
            className={
              msg.status === "error" ? "agent-tool-icon fail" : "agent-tool-icon"
            }
          >
            {msg.status === "running" ? "⟳" : msg.status === "error" ? "✗" : "✎"}
          </span>
          <span className="agent-tool-file">{filePath ?? msg.name}</span>
        </div>
        {diff && <DiffView lines={diff} />}
        {msg.status === "error" && msg.result && (
          <div className="agent-tool-summary">{truncate(msg.result, 400)}</div>
        )}
      </div>
    );
  }

  if (isVerifyTool(msg.name)) {
    if (msg.status === "running") {
      return (
        <div className="agent-tool agent-tool-verify">
          <span className="agent-spinner" aria-hidden="true">
            ⟳
          </span>{" "}
          Verifying…
        </div>
      );
    }
    if (msg.status === "error") {
      // `run_verify` (src-tauri/src/lib.rs) only sets isError for the tool
      // genuinely failing to run (arduino-cli missing, build gate busy) —
      // NOT for a normal failed build, which it deliberately reports with
      // isError:false so the model keeps iterating. This branch is that
      // "could not run at all" case.
      return (
        <div className="agent-tool agent-tool-verify">
          <div className="agent-tool-head">
            <span className="agent-tool-icon fail">✗</span>
            <span>Verify could not run</span>
          </div>
          {msg.result && (
            <div className="agent-tool-summary">{truncate(msg.result, 400)}</div>
          )}
        </div>
      );
    }
    // A normal completed verify always has isError:false — the real
    // pass/fail lives in the "success: <bool>\nexit_code: <n>\n\n<summary>"
    // text `run_verify` builds, not in msg.status.
    const { success, exitCode, summary } = parseVerifyResult(msg.result ?? "");
    const ok = success === true;
    return (
      <div className="agent-tool agent-tool-verify">
        <div className="agent-tool-head">
          <span className={ok ? "agent-tool-icon ok" : "agent-tool-icon fail"}>
            {success === undefined ? "•" : ok ? "✓" : "✗"}
          </span>
          <span>
            {success === undefined
              ? "Verify finished"
              : ok
                ? "Verify passed"
                : "Verify failed"}
            {exitCode !== undefined ? ` (exit ${exitCode})` : ""}
          </span>
        </div>
        {summary && <div className="agent-tool-summary">{truncate(summary, 400)}</div>}
        <button className="btn small" onClick={() => openBottomTab("build")}>
          Open Console ↗
        </button>
      </div>
    );
  }

  return (
    <div className="agent-tool agent-tool-generic">
      🔍 {msg.name}({argsSummary(msg.input)})
    </div>
  );
}

function DiffView({ lines }: { lines: DiffLine[] }) {
  return (
    <pre className="agent-diff">
      {lines.map((l, i) => (
        <div key={i} className={`agent-diff-line agent-diff-${l.kind}`}>
          {(l.kind === "del" ? "-" : l.kind === "add" ? "+" : " ") + l.text}
        </div>
      ))}
    </pre>
  );
}

// ---------- chat history replay ----------

/** A saved chat through the same components as a live one — including the
 *  session-end line and any alarm, or the replay wouldn't be faithful. */
function ReplayView({
  store,
  openBottomTab,
  onOpenTurn,
}: {
  store: AgentStore;
  openBottomTab: (tab: BottomTab) => void;
  onOpenTurn: (turn: TurnEnd) => void;
}) {
  const snap = store.snapshot();
  return (
    <>
      {snap.messages.map((m, i) => (
        <MessageView
          key={i}
          msg={m}
          openBottomTab={openBottomTab}
          onOpenTurn={onOpenTurn}
        />
      ))}
      {snap.alarm && <AlarmView alarm={snap.alarm} />}
      {snap.closedReason && (
        <div className="agent-closed">Session ended — {snap.closedReason}</div>
      )}
    </>
  );
}

// ---------- chat history list ----------

function HistListView({
  entries,
  onOpen,
  onDelete,
}: {
  entries: ChatEntry[];
  onOpen: (file: string) => void;
  onDelete: (file: string) => void;
}) {
  if (entries.length === 0) {
    return (
      <div className="agent-empty">No saved chats for this sketch yet.</div>
    );
  }
  return (
    <div className="agent-hist-list">
      {entries.map((e) => (
        <div key={e.file} className="agent-hist-row">
          <button className="agent-hist-open" onClick={() => onOpen(e.file)}>
            <span className="agent-hist-title">{e.title}</span>
            <span className="agent-hist-date">
              {e.file.replace(/\.ndjson$/, "").replace("T", " ")}
            </span>
          </button>
          <button
            className="btn small danger icon"
            onClick={() => onDelete(e.file)}
            title="Delete this saved chat"
          >
            🗑
          </button>
        </div>
      ))}
    </div>
  );
}

// ---------- turn summary view ----------

function turnEndLabel(msg: TurnEnd): string {
  const u = msg.usage;
  if (!u) return "turn finished";
  const parts = [
    u.numTurns !== undefined ? `${u.numTurns} turns` : null,
    `${formatTokens(u.inputTokens)} in / ${formatTokens(u.outputTokens)} out`,
    u.costUsd !== undefined ? `$${u.costUsd.toFixed(4)}` : null,
  ];
  return parts.filter(Boolean).join(" · ");
}

/** The "info at the end" view: the turn's wrap-up, rendered richly. */
function TurnSummaryView({
  turn,
  onBack,
}: {
  turn: TurnEnd;
  onBack: () => void;
}) {
  const u = turn.usage;
  return (
    <div className="agent-turn-view">
      <div className="agent-turn-view-bar">
        <button className="btn small" onClick={onBack}>
          ← Back to chat
        </button>
        <span className="agent-turn-view-title">Turn summary</span>
      </div>
      {turn.summary ? (
        <Md text={turn.summary} />
      ) : (
        <p className="agent-empty">This turn produced no summary text.</p>
      )}
      {u && (
        <dl className="agent-usage-grid">
          <dt>input</dt>
          <dd>
            {formatTokens(u.inputTokens)}
            {u.cacheReadTokens > 0 &&
              ` (${formatTokens(u.cacheReadTokens)} cached)`}
          </dd>
          <dt>output</dt>
          <dd>{formatTokens(u.outputTokens)}</dd>
          {u.costUsd !== undefined && (
            <>
              <dt>cost</dt>
              <dd>${u.costUsd.toFixed(4)}</dd>
            </>
          )}
          {u.numTurns !== undefined && (
            <>
              <dt>turns</dt>
              <dd>{u.numTurns}</dd>
            </>
          )}
          {u.durationMs !== undefined && (
            <>
              <dt>duration</dt>
              <dd>{(u.durationMs / 1000).toFixed(1)}s</dd>
            </>
          )}
        </dl>
      )}
      {turn.tools.length > 0 && (
        <ul className="agent-turn-tools">
          {turn.tools.map((t, i) => (
            <li key={i}>
              <span
                className={
                  t.status === "error"
                    ? "agent-tool-icon fail"
                    : "agent-tool-icon"
                }
              >
                {t.status === "ok" ? "✓" : t.status === "error" ? "✗" : "…"}
              </span>{" "}
              {t.name}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// ---------- small helpers ----------

function statusLabel(status: AgentStatus, verifyRunning: boolean): string {
  if (verifyRunning) return "Verifying…";
  switch (status) {
    case "idle":
      return "Not started";
    case "starting":
      return "Starting…";
    case "running":
      return "Running";
    case "ended":
      return "Session ended";
    default:
      return status;
  }
}

/** The wire tool_use name is `mcp__bancada__verify` (confirmed: this is the
 *  exact string `agent_args()` requires in `--allowedTools`,
 *  core/src/agent.rs) — a bare `verify` is also tolerated since that's the
 *  name the MCP `tools/list` response itself advertises. */
function isVerifyTool(name: string): boolean {
  return name === "verify" || name.endsWith("__verify");
}

function stringField(input: unknown, key: string): string | undefined {
  if (typeof input !== "object" || input === null) return undefined;
  const v = (input as Record<string, unknown>)[key];
  return typeof v === "string" ? v : undefined;
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n) + "…" : s;
}

/** One-liner arg summary for tool cards this panel doesn't render specially. */
function argsSummary(input: unknown): string {
  if (typeof input !== "object" || input === null) return "";
  const entries = Object.entries(input as Record<string, unknown>);
  if (entries.length === 0) return "";
  return entries
    .slice(0, 2)
    .map(([k, v]) => {
      const text = typeof v === "string" ? v : JSON.stringify(v);
      return `${k}: ${truncate(text, 40)}`;
    })
    .join(", ");
}
