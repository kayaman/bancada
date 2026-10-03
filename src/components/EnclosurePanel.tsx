// Enclosure panel — one of the app's three big divisions (Software,
// Hardware, Enclosure — see the sidebar switcher in App.tsx): compose the
// seed prompt for enclosure-maker's own AI assistant, then embed its chat
// right here, in this pane, in this same window — not a second native
// window. enclosure-maker is merged into this workspace (see
// enclosure-maker/README.md); its chat server has no Tauri dependency of
// its own, so the embedded iframe talks to it purely over its own
// WebSocket, same as it would in a window of its own.
//
// The chat session drives FreeCAD directly through FreeCAD's own MCP
// server — FreeCAD's own GUI window holds the live model, not this pane.
// There is no embedded 3D canvas here; this iframe is chat only.
//
// Two phases: "compose" (nothing sent yet — draft prompt, editable, Send
// button) and "viewer" (an enclosure project already exists for this
// sketch — the embedded chat, with a way back to compose to revise and
// resend). Rendered in the editor area only while the Enclosure division is
// active (App.tsx's showPane-style ternary) — not always-mounted like the
// bottom panels, since there's no App-level state to preserve across visits
// beyond "does a project exist yet", which is re-checked on every entry.

import { useEffect, useRef, useState } from "react";
import * as api from "../api";

interface Props {
  sketchDir: string | null;
  notify: (msg: string, isError?: boolean) => void;
}

type Phase = "checking" | "compose" | "viewer";

export default function EnclosurePanel({ sketchDir, notify }: Props) {
  const [phase, setPhase] = useState<Phase>("checking");
  const [draft, setDraft] = useState("");
  const [loadingDraft, setLoadingDraft] = useState(false);
  const [sending, setSending] = useState(false);
  const [viewerUrl, setViewerUrl] = useState<string | null>(null);

  const loadDraft = () => {
    if (!sketchDir) return;
    setLoadingDraft(true);
    api
      .previewEnclosurePrompt(sketchDir)
      .then(setDraft)
      .catch((err) => notify(String(err), true))
      .finally(() => setLoadingDraft(false));
  };

  // On entering this division (mount) or the open project changing: if an
  // enclosure project already exists for this sketch, resume straight to
  // the viewer — no reason to re-show the compose box or inject a fresh
  // chat message just from navigating here. Otherwise show compose, with a
  // freshly generated draft.
  const checkedForRef = useRef<string | null>(null);
  useEffect(() => {
    if (!sketchDir) {
      setPhase("compose");
      return;
    }
    if (checkedForRef.current === sketchDir) return;
    checkedForRef.current = sketchDir;
    setPhase("checking");
    api
      .hasEnclosureProject(sketchDir)
      .then((exists) => {
        if (!exists) {
          setPhase("compose");
          loadDraft();
          return;
        }
        return api
          .resumeEnclosurePreview(sketchDir)
          .then((url) => {
            setViewerUrl(url);
            setPhase("viewer");
          })
          .catch((err) => {
            notify(String(err), true);
            setPhase("compose");
            loadDraft();
          });
      })
      .catch(() => {
        setPhase("compose");
        loadDraft();
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sketchDir]);

  const send = async () => {
    if (!sketchDir || !draft.trim()) return;
    setSending(true);
    try {
      const url = await api.openEnclosurePreview(sketchDir, draft);
      setViewerUrl(url);
      setPhase("viewer");
    } catch (err) {
      notify(String(err), true);
    } finally {
      setSending(false);
    }
  };

  const editPrompt = () => {
    setPhase("compose");
    loadDraft();
  };

  if (phase === "checking") {
    return (
      <section className="enclosure-panel">
        <p className="agent-empty">Checking for an existing enclosure project…</p>
      </section>
    );
  }

  if (phase === "viewer" && viewerUrl) {
    return (
      <section className="enclosure-panel enclosure-panel-viewer">
        <div className="bom-toolbar">
          <span className="bom-count">
            Enclosure — chat with the assistant; the model itself opens and
            updates in FreeCAD's own window
          </span>
          <div className="spacer" />
          <button className="btn small" onClick={editPrompt} title="Revise the prompt and send again">
            ← Edit prompt
          </button>
        </div>
        <iframe
          key={viewerUrl}
          className="enclosure-viewer"
          src={viewerUrl}
          title="Enclosure assistant chat"
        />
      </section>
    );
  }

  return (
    <section className="enclosure-panel">
      <div className="bom-toolbar">
        <span className="bom-count">
          Review the request before it's handed off to enclosure-maker:
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
          title="Start the enclosure-maker chat session with this prompt"
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
    </section>
  );
}
