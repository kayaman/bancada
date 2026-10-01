// Enclosure panel — the third pillar, alongside Software (Assistant) and
// Hardware (Build/Serial/Scope): compose-then-send the seed prompt for
// enclosure-maker's own AI assistant, then mirror its live tool-call
// activity here while it works in its own window.
//
// Always mounted, hidden with display:none when another tab is active (same
// pattern as every other bottom panel). `store` is App-owned, not
// panel-owned, for the same reason `agentStore` is: the unseen dot must work
// before this panel has ever been mounted.

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { AgentStore } from "../agent/agentStore";
import { MessageView, TurnSummaryView, type TurnEnd } from "./AgentPanel";
import type { BottomTab } from "../bottomTabs";

interface Props {
  active: boolean;
  sketchDir: string | null;
  store: AgentStore;
  openBottomTab: (tab: BottomTab) => void;
  notify: (msg: string, isError?: boolean) => void;
}

const POLL_MS = 100;

export default function EnclosurePanel({ active, sketchDir, store, openBottomTab, notify }: Props) {
  const [draft, setDraft] = useState("");
  const [loadingDraft, setLoadingDraft] = useState(false);
  const [sending, setSending] = useState(false);
  const [sent, setSent] = useState(false);

  const loadDraft = () => {
    if (!sketchDir) return;
    setLoadingDraft(true);
    api
      .previewEnclosurePrompt(sketchDir)
      .then(setDraft)
      .catch((err) => notify(String(err), true))
      .finally(() => setLoadingDraft(false));
  };

  // Fill the compose box the first time this project's tab is opened.
  const loadedForRef = useRef<string | null>(null);
  useEffect(() => {
    if (!active || !sketchDir || loadedForRef.current === sketchDir) return;
    loadedForRef.current = sketchDir;
    loadDraft();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, sketchDir]);

  const send = async () => {
    if (!sketchDir || !draft.trim()) return;
    setSending(true);
    try {
      const pid = await api.sendToEnclosureMaker(sketchDir, draft);
      store.sessionStarted(pid);
      setSent(true);
      notify("Opening enclosure-maker…");
    } catch (err) {
      notify(String(err), true);
    } finally {
      setSending(false);
    }
  };

  const sendAnother = () => {
    // Clears the transcript and supersedes the old pid, so a straggling
    // event from the session just left behind can't paint into the next one
    // (same AgentStore.clear() guard the Assistant tab's "New session" uses).
    store.clear();
    setSent(false);
    loadDraft();
  };

  // ---------- repaint on store changes (only while shown) ----------

  const [, setTick] = useState(0);
  const lastVersionRef = useRef(-1);
  useEffect(() => {
    if (!active || !sent) return;
    const iv = window.setInterval(() => {
      if (store.version !== lastVersionRef.current) {
        lastVersionRef.current = store.version;
        setTick((t) => t + 1);
      }
    }, POLL_MS);
    return () => window.clearInterval(iv);
  }, [active, sent, store]);

  const [viewTurn, setViewTurn] = useState<TurnEnd | null>(null);
  const snap = store.snapshot();

  return (
    <section
      className="enclosure-panel"
      style={active ? undefined : { display: "none" }}
    >
      {!sent ? (
        <>
          <div className="bom-toolbar">
            <span className="bom-count">
              Review the request before it's sent to enclosure-maker:
            </span>
            <div className="spacer" />
            <button
              className="btn small"
              disabled={loadingDraft || !sketchDir}
              onClick={loadDraft}
              title="Regenerate the draft from the current BOM and board"
            >
              {loadingDraft ? "Loading…" : "Regenerate"}
            </button>
            <button
              className="btn small primary"
              disabled={sending || loadingDraft || !sketchDir || !draft.trim()}
              onClick={() => void send()}
              title="Send this prompt to enclosure-maker"
            >
              {sending ? "Opening…" : "Send to Enclosure-maker →"}
            </button>
          </div>
          <textarea
            className="enclosure-prompt"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder={
              sketchDir
                ? "Loading a draft from this project's BOM and board…"
                : "Open a project first."
            }
            disabled={loadingDraft || sending}
          />
        </>
      ) : viewTurn ? (
        <TurnSummaryView turn={viewTurn} onBack={() => setViewTurn(null)} />
      ) : (
        <>
          <div className="bom-toolbar">
            <span className="bom-count">
              Mirroring enclosure-maker's assistant
              {snap.status === "ended" ? " — session ended" : "…"}
            </span>
            <div className="spacer" />
            <button className="btn small" onClick={sendAnother}>
              Send another →
            </button>
          </div>
          <div className="agent-scroll">
            {snap.messages.length === 0 && (
              <p className="agent-empty">
                Waiting for enclosure-maker to start…
              </p>
            )}
            {snap.messages.map((msg, i) => (
              <MessageView
                key={i}
                msg={msg}
                openBottomTab={openBottomTab}
                onOpenTurn={setViewTurn}
              />
            ))}
          </div>
        </>
      )}
    </section>
  );
}
