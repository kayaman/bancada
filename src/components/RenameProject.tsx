import { useEffect, useState } from "react";
import { chatList, renameProject } from "../api";
import type { RepoState } from "../api";
import { checkProjectName, renamePlan } from "../projectRename";

interface Props {
  /** Directory of the currently open sketch. Renaming needs one open. */
  sketchDir: string;
  gitState: RepoState | null;
  onRenamed: (dir: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

/**
 * Rename the open project: the folder, its main `.ino`, and everything keyed
 * to the old path.
 *
 * A pane rather than a popover because a rename has consequences worth seeing
 * before committing to them — the destination, the `.ino` that moves with it,
 * and the state that follows. The backend does the work and is the authority
 * on every refusal; `checkProjectName` here only saves a round trip.
 */
export default function RenameProject({
  sketchDir,
  gitState,
  onRenamed,
  onCancel,
  notify,
}: Props) {
  const [name, setName] = useState("");
  const [working, setWorking] = useState(false);
  const [chats, setChats] = useState<number | null>(null);

  // The chat count is the least obvious thing that moves, so it is worth a
  // round trip to state it exactly rather than say "any chat history".
  useEffect(() => {
    let cancelled = false;
    chatList(sketchDir)
      .then((rows) => {
        if (!cancelled) setChats(rows.length);
      })
      .catch(() => {
        if (!cancelled) setChats(null);
      });
    return () => {
      cancelled = true;
    };
  }, [sketchDir]);

  const check = checkProjectName(name, sketchDir);
  const plan = check.ok ? renamePlan(sketchDir, name) : null;
  const reason = name.trim() && !check.ok ? check.reason : null;

  const rename = async () => {
    // Guard as CloneProject does: Enter bypasses the disabled button, and a
    // second concurrent rename would race the first one's directory move.
    if (working || !check.ok) return;
    setWorking(true);
    try {
      const res = await renameProject(sketchDir, name.trim());
      notify(
        res.warnings.length
          ? `Renamed to ${res.dir}, with notes: ${res.warnings.join("; ")}`
          : `✓ Renamed to ${res.name}`,
      );
      onRenamed(res.dir);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="new-project">
      <div className="np-head">
        <strong>Rename project</strong>
        <div className="spacer" />
        <button className="btn small" onClick={onCancel} disabled={working}>
          Cancel
        </button>
        <button
          className="btn small primary"
          onClick={rename}
          disabled={working || !check.ok}
          title={reason ?? "Rename the folder, its sketch, and everything keyed to the old path"}
        >
          ✎ Rename
        </button>
      </div>

      <div className="np-body">
        <div className="np-labelled">
          <span className="np-caption">Current</span>
          <div className="lib-dest">{sketchDir}</div>
        </div>

        <label className="field">
          New name
          <input
            className="input"
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && rename()}
          />
        </label>

        {reason && (
          <div className="git-pop-warning" role="note">
            {reason}
          </div>
        )}

        {plan && (
          <>
            <div className="np-labelled">
              <span className="np-caption">Becomes</span>
              <div className="lib-dest">{plan.destDir}</div>
            </div>
            <div className="np-note">
              {plan.newIno} (renamed from {plan.oldIno})
            </div>
            <div className="np-note">
              Also moves:
              <ul>
                <li>
                  {chats === null
                    ? "any Assistant chats for this project"
                    : chats === 1
                      ? "1 Assistant chat"
                      : `${chats} Assistant chats`}
                </li>
                <li>its recorded token usage</li>
                <li>the recent-projects entry</li>
                {gitState && gitState.kind !== "no_git" && (
                  <li>tracked in git — history follows the rename</li>
                )}
              </ul>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
