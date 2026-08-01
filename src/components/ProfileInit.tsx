import { useEffect, useState } from "react";
import BoardPicker from "./BoardPicker";
import {
  initProfile,
  listAllBoards,
  type BoardOption,
  type SketchYaml,
} from "../api";
import { profileNameForFqbn } from "../profileInit";

interface Props {
  sketchDir: string;
  /** FQBN detected on the selected port, preselected when known. */
  detectedFqbn: string | null;
  onCreated: (yaml: SketchYaml, profile: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

/** One-row form under the toolbar: pick a board, name the profile, create
 *  sketch.yaml. Shown only while the sketch has no profiles. */
export default function ProfileInit({
  sketchDir,
  detectedFqbn,
  onCreated,
  onCancel,
  notify,
}: Props) {
  const [boards, setBoards] = useState<BoardOption[]>([]);
  const [fqbn, setFqbn] = useState(detectedFqbn ?? "");
  const [name, setName] = useState(
    detectedFqbn ? profileNameForFqbn(detectedFqbn) : "",
  );
  const [nameTouched, setNameTouched] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    listAllBoards()
      .then(setBoards)
      .catch((e) => notify(String(e), true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pick = (f: string) => {
    setFqbn(f);
    if (!nameTouched) setName(f ? profileNameForFqbn(f) : "");
  };

  const create = async () => {
    setBusy(true);
    try {
      const yaml = await initProfile(sketchDir, name.trim(), fqbn);
      notify(`✓ Profile “${name.trim()}” written to sketch.yaml`);
      onCreated(yaml, name.trim());
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="profile-init">
      <span className="profile-init-label">New sketch.yaml profile:</span>
      <BoardPicker
        boards={boards}
        value={fqbn}
        onChange={pick}
        title="Board for this profile"
      />
      <input
        className="input"
        value={name}
        placeholder="profile name"
        onChange={(e) => {
          setName(e.target.value);
          setNameTouched(true);
        }}
      />
      <button
        className="btn primary"
        disabled={busy || !fqbn || !name.trim()}
        onClick={create}
      >
        Create
      </button>
      <button className="btn" disabled={busy} onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}
