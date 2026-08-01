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
import type { AgentProbe } from "../api";
import type { AgentMessage, AgentStatus, AgentStore } from "../agent/agentStore";
import { diffForToolInput, type DiffLine } from "../agent/diff";
import { splitFences } from "../agent/fences";
import type { BottomTab } from "../bottomTabs";

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

  // ---------- autoscroll (Console.tsx pattern: stick to bottom unless the
  // user has scrolled up to read something earlier) ----------

  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
    // `tick` (not `snap`) so this only runs when the store actually changed.
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
        {snap.messages.length === 0 && (
          <div className="agent-empty">
            Ask the assistant to make a change, then Verify.
          </div>
        )}
        {snap.messages.map((m, i) => (
          <MessageView key={i} msg={m} openBottomTab={openBottomTab} />
        ))}
        {snap.closedReason && (
          <div className="agent-closed">
            Session ended — {snap.closedReason}
          </div>
        )}
      </div>

      <div className="agent-footer">
        <span className="agent-status">
          {statusLabel(snap.status, snap.verifyRunning)}
        </span>
        {snap.lastResult &&
          snap.lastResult.costUsd !== undefined &&
          snap.lastResult.numTurns !== undefined && (
            <span className="agent-cost">
              ${snap.lastResult.costUsd.toFixed(4)} · {snap.lastResult.numTurns}{" "}
              turns
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
        <button className="btn small" onClick={onNewSession}>
          New session
        </button>
      </div>

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
    </section>
  );
}

// ---------- message / tool rendering ----------

function MessageView({
  msg,
  openBottomTab,
}: {
  msg: AgentMessage;
  openBottomTab: (tab: BottomTab) => void;
}) {
  switch (msg.kind) {
    case "user":
      return <div className="agent-msg agent-msg-user">{msg.text}</div>;
    case "assistant":
      return (
        <div className="agent-msg agent-msg-assistant">
          {splitFences(msg.text).map((seg, i) =>
            seg.kind === "code" ? (
              <pre key={i} className="agent-code">
                {seg.content}
              </pre>
            ) : (
              <p key={i} className="agent-text">
                {seg.content}
              </p>
            ),
          )}
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

/** `run_verify` (src-tauri/src/lib.rs) always builds its result text as
 *  `"success: <bool>\nexit_code: <n>\n\n<summary>"` on the tool's normal
 *  path (isError is reserved for the tool failing to run at all — see the
 *  "error" branch above). Tolerant of an unexpected shape: an unparsed
 *  `success` line just means `success` comes back `undefined`. */
function parseVerifyResult(
  result: string,
): { success?: boolean; exitCode?: number; summary: string } {
  const lines = result.split("\n");
  const successMatch = /^success:\s*(true|false)\s*$/.exec(lines[0] ?? "");
  if (!successMatch) return { summary: result };
  let bodyStart = 1;
  let exitCode: number | undefined;
  const exitMatch = /^exit_code:\s*(-?\d+)\s*$/.exec(lines[1] ?? "");
  if (exitMatch) {
    exitCode = Number(exitMatch[1]);
    bodyStart = 2;
  }
  while (bodyStart < lines.length && lines[bodyStart] === "") bodyStart++;
  return {
    success: successMatch[1] === "true",
    exitCode,
    summary: lines.slice(bodyStart).join("\n"),
  };
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
