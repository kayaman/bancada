import { useCallback, useRef, useState } from "react";
import Menu from "./Menu";
import type { RepoState, Visibility } from "../api";
import {
  flashTaggingNote,
  parentName,
  pillLabel,
  popoverMode,
  suggestedMessage,
  syncDisabledReason,
} from "../gitStatus";
import { checkRepoName, publishBlockedReason } from "../publishRepo";

interface Props {
  state: RepoState | null;
  busy: boolean;
  ghAvailable: boolean;
  /** Prefill for the gh repo name: the sketch folder's name. */
  defaultRepoName: string;
  onCommit: (message: string) => void;
  onSync: () => void;
  onInit: () => void;
  /** Init a nested sketch as a repository of its own — see git_init_here. */
  onInitHere: () => void;
  onCreateRemote: (name: string, visibility: Visibility, description: string | null) => void;
  onSetRemote: (url: string) => void;
}

/** Toolbar git pill: one glanceable state, and a popover holding the two
 *  actions (Commit / Sync) or the setup that must come first. Rendered only
 *  while a sketch is open — the caller passes state=null otherwise. */
export default function GitPill(props: Props) {
  const btnRef = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const [message, setMessage] = useState("");
  const [repoName, setRepoName] = useState("");
  const [visibility, setVisibility] = useState<Visibility>("private");
  const [description, setDescription] = useState("");
  const [url, setUrl] = useState("");

  const label = pillLabel(props.state);
  const close = useCallback(() => setAnchor(null), []);
  if (!props.state || label === null) return null;
  const state = props.state;

  const toggle = () => {
    if (anchor) {
      setAnchor(null);
      return;
    }
    if (state.kind === "root") setMessage(state.suggested_message);
    else if (state.kind === "nested") setMessage(suggestedMessage(state.dirty));
    else setMessage("");
    setRepoName(props.defaultRepoName);
    // Private every time the popover opens: publishing publicly should be a
    // decision made now, never one inherited from the last sketch.
    setVisibility("private");
    setDescription("");
    const r = btnRef.current?.getBoundingClientRect();
    if (r) setAnchor({ x: r.left, y: r.bottom + 4 });
  };

  const mode = popoverMode(state);
  const syncReason = syncDisabledReason(state);
  const secrets = state.kind === "root" ? state.tracked_secrets : [];
  const dirtyCount = state.kind === "root" || state.kind === "nested" ? state.dirty.length : 0;
  const tagNote = flashTaggingNote(state);

  const nameCheck = checkRepoName(repoName);
  const publishReason =
    publishBlockedReason(state, visibility, props.ghAvailable) ??
    (nameCheck.ok ? null : nameCheck.reason);
  const publish = () => {
    props.onCreateRemote(repoName.trim(), visibility, description.trim() || null);
    close();
  };

  // Offered from `setup_remote` and from `init` alike: a sketch that is not
  // yet under git should reach GitHub in one action, not by initializing on
  // one trip and finding this row on the next. The backend inits first.
  const publishBlock = props.ghAvailable && (
    <>
      <div className="git-pop-row">
        <input
          className="input"
          value={repoName}
          onChange={(e) => setRepoName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && publishReason === null && !props.busy) publish();
          }}
          placeholder="repository name"
          title="Created on your GitHub account"
        />
        <button
          className={`btn small ${visibility === "public" ? "toggled" : ""}`}
          disabled={props.busy}
          title={
            visibility === "public"
              ? "Anyone can read this repository. Click to make it private."
              : "Only you can read this repository. Click to make it public."
          }
          onClick={() => setVisibility(visibility === "public" ? "private" : "public")}
        >
          {visibility}
        </button>
      </div>
      <div className="git-pop-row">
        <input
          className="input"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && publishReason === null && !props.busy) publish();
          }}
          placeholder="description (optional)"
        />
        <button
          className="btn small"
          disabled={props.busy || publishReason !== null}
          title={publishReason ?? `Create and push a ${visibility} repository`}
          onClick={publish}
        >
          Create on GitHub
        </button>
      </div>
    </>
  );

  const commitRow = (
    <div className="git-pop-row">
      <input
        className="input"
        value={message}
        onChange={(e) => setMessage(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && message.trim() && dirtyCount > 0) {
            props.onCommit(message.trim());
            close();
          }
        }}
        placeholder="commit message"
        title="Checkpoint everything in this project"
      />
      <button
        className="btn small primary"
        disabled={props.busy || !message.trim() || dirtyCount === 0}
        onClick={() => {
          props.onCommit(message.trim());
          close();
        }}
      >
        Commit
      </button>
    </div>
  );

  return (
    <>
      <button
        ref={btnRef}
        className={`git-pill ${dirtyCount > 0 || state.kind === "no_git" ? "attention" : ""}`}
        onClick={toggle}
        title="Git — checkpoint & sync"
        aria-haspopup="menu"
        aria-expanded={anchor !== null}
      >
        {label}
      </button>
      {anchor && (
        <Menu x={anchor.x} y={anchor.y} onClose={close} anchorRef={btnRef}>
          {secrets.length > 0 && (
            <div className="git-pop-warning" role="note">
              ⚠ tracked despite .gitignore: {secrets.join(", ")}
            </div>
          )}
          {tagNote && (
            <div className="git-pop-note" role="note">
              {tagNote}
            </div>
          )}
          {mode === "init" && (
            <>
              <button
                className="ctx-item"
                disabled={props.busy}
                onClick={() => {
                  props.onInit();
                  close();
                }}
              >
                Initialize repository
              </button>
              {publishBlock}
            </>
          )}
          {mode === "nested" && state.kind === "nested" && (
            <>
              {commitRow}
              <div
                className="git-pop-note"
                title="Pushing would publish the whole parent repository — more than this button promises."
              >
                sync is up to the {parentName(state.root)} repo
              </div>
              <button
                className="ctx-item"
                disabled={props.busy}
                title="Creates a repository inside the parent one. The parent will list this sketch as an embedded repository."
                onClick={() => {
                  props.onInitHere();
                  close();
                }}
              >
                Give this sketch its own repository
              </button>
            </>
          )}
          {(mode === "actions" || mode === "setup_remote") && commitRow}
          {mode === "actions" && (
            <button
              className="ctx-item"
              disabled={props.busy || syncReason !== null}
              title={syncReason ?? "fetch, rebase, push"}
              onClick={() => {
                props.onSync();
                close();
              }}
            >
              ⇅ Sync
            </button>
          )}
          {mode === "setup_remote" && (
            <>
              {publishBlock}
              <div className="git-pop-row">
                <input
                  className="input"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="or paste a git remote URL"
                />
                <button
                  className="btn small"
                  disabled={props.busy || !url.trim()}
                  onClick={() => {
                    props.onSetRemote(url.trim());
                    close();
                  }}
                >
                  Set remote
                </button>
              </div>
            </>
          )}
        </Menu>
      )}
    </>
  );
}
