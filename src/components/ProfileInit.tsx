import { useEffect, useState } from "react";
import BoardPicker from "./BoardPicker";
import {
  initProfile,
  listAllBoards,
  retargetProfile,
  type BoardOption,
  type SketchYaml,
} from "../api";
import { profileNameForFqbn } from "../profileInit";

export type ProfileFormMode = "bootstrap" | "add" | "retarget";

interface Props {
  mode: ProfileFormMode;
  sketchDir: string;
  /** FQBN detected on the selected port, preselected for new profiles. */
  detectedFqbn: string | null;
  /** Selected profile: retarget's target, add's library-copy source. */
  currentProfile: string | null;
  /** That profile's FQBN (retarget's picker preselect). */
  currentFqbn: string | null;
  onDone: (yaml: SketchYaml, profile: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

const LABEL: Record<ProfileFormMode, string> = {
  bootstrap: "New sketch.yaml profile:",
  add: "Add profile for another board:",
  retarget: "Change this profile's board:",
};

/** One-row form under the toolbar: bootstrap the first profile, add one for
 *  another board (libraries copied from the current profile), or point the
 *  current profile at a different board in place. */
export default function ProfileInit({
  mode,
  sketchDir,
  detectedFqbn,
  currentProfile,
  currentFqbn,
  onDone,
  onCancel,
  notify,
}: Props) {
  const initialFqbn = (mode === "retarget" ? currentFqbn : detectedFqbn) ?? "";
  const [boards, setBoards] = useState<BoardOption[]>([]);
  const [fqbn, setFqbn] = useState(initialFqbn);
  const [name, setName] = useState(
    mode !== "retarget" && initialFqbn ? profileNameForFqbn(initialFqbn) : "",
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
    if (mode !== "retarget" && !nameTouched)
      setName(f ? profileNameForFqbn(f) : "");
  };

  const submit = async () => {
    setBusy(true);
    try {
      if (mode === "retarget") {
        if (!currentProfile) return; // button is disabled without a profile
        const yaml = await retargetProfile(sketchDir, currentProfile, fqbn);
        notify(`✓ Profile “${currentProfile}” now builds for ${fqbn}`);
        onDone(yaml, currentProfile);
      } else {
        const yaml = await initProfile(
          sketchDir,
          name.trim(),
          fqbn,
          mode === "add" ? (currentProfile ?? undefined) : undefined,
        );
        notify(`✓ Profile “${name.trim()}” written to sketch.yaml`);
        onDone(yaml, name.trim());
      }
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const ready = mode === "retarget" ? !!fqbn : !!fqbn && !!name.trim();
  return (
    <div className="profile-init">
      <span className="profile-init-label">{LABEL[mode]}</span>
      <BoardPicker
        boards={boards}
        value={fqbn}
        onChange={pick}
        title="Board for this profile"
      />
      {mode === "retarget" ? (
        <span className="profile-init-label" title="Profile being retargeted">
          {currentProfile}
        </span>
      ) : (
        <input
          className="input"
          value={name}
          placeholder="profile name"
          onChange={(e) => {
            setName(e.target.value);
            setNameTouched(true);
          }}
        />
      )}
      <button className="btn primary" disabled={busy || !ready} onClick={submit}>
        {mode === "retarget" ? "Change board" : "Create"}
      </button>
      <button className="btn" disabled={busy} onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}
