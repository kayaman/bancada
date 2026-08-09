import { useEffect, useState } from "react";
import BoardPicker from "./BoardPicker";
import {
  initProfile,
  listAllBoards,
  loadSketchYaml,
  retargetProfile,
  type BoardOption,
  type SketchYaml,
} from "../api";
import { initialFqbn as computeInitialFqbn, profileNameForFqbn, submitPlan } from "../profileInit";

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
  /** Called with the on-disk yaml after a failed submit: the backend writes
   *  sketch.yaml before pinning board-required libraries, so a pin failure
   *  still leaves a new/retargeted profile on disk. Without this, App's
   *  sketchYaml goes stale — the profile exists but the UI doesn't show it,
   *  and retrying reports "profile already exists". */
  onYamlChanged: (yaml: SketchYaml) => void;
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
  onYamlChanged,
}: Props) {
  const startFqbn = computeInitialFqbn(mode, currentFqbn, detectedFqbn);
  const [boards, setBoards] = useState<BoardOption[]>([]);
  const [fqbn, setFqbn] = useState(startFqbn);
  const [name, setName] = useState(
    mode !== "retarget" && startFqbn ? profileNameForFqbn(startFqbn) : "",
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
    const plan = submitPlan(mode, currentProfile, name, fqbn);
    if (!plan) return; // button is disabled unless the plan is valid
    setBusy(true);
    try {
      if (plan.kind === "retarget") {
        const yaml = await retargetProfile(sketchDir, plan.profile, plan.fqbn);
        notify(`✓ Profile “${plan.profile}” now builds for ${plan.fqbn}`);
        onDone(yaml, plan.profile);
      } else {
        const yaml = await initProfile(sketchDir, plan.profile, plan.fqbn, plan.copyLibsFrom);
        notify(`✓ Profile “${plan.profile}” written to sketch.yaml`);
        onDone(yaml, plan.profile);
      }
    } catch (e) {
      notify(String(e), true);
      // The backend writes sketch.yaml before pinning board-required
      // libraries, so a pin failure still leaves a new/retargeted profile on
      // disk. Refetch and propagate so App's sketchYaml doesn't go stale —
      // without this a retry reports "profile already exists" for a profile
      // the user can't see.
      try {
        onYamlChanged(await loadSketchYaml(sketchDir));
      } catch {
        // Best effort — the submit error above is already surfaced.
      }
    } finally {
      setBusy(false);
    }
  };

  const ready = submitPlan(mode, currentProfile, name, fqbn) !== null;
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
