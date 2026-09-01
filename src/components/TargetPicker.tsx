import { useState } from "react";
import { setTargetBlockedReason } from "../toolbarModel";

interface Props {
  /** Chip targets this ESP-IDF supports, from `idf.py --list-targets`. */
  targets: string[];
  /** `CONFIG_IDF_TARGET`, or null when none has been set yet. */
  current: string | null;
  sketchDir: string | null;
  busy: boolean;
  idfAvailable: boolean;
  /** Whether the project is under git — the only undo for this operation. */
  underGit: boolean;
  /** False on a fresh project with no sdkconfig, where there is nothing to lose. */
  hasSdkconfig: boolean;
  onSetTarget: (target: string) => void;
}

/**
 * The ESP-IDF equivalent of the profile picker.
 *
 * Changing target is the one irreversible button on this bar: `idf.py
 * set-target` deletes `build/` and regenerates `sdkconfig`, discarding every
 * value the user set by hand. So selecting a target deliberately does *not*
 * apply it — it arms a confirmation that names the two concrete losses. A
 * dialog that says only "this is destructive" teaches people to click through
 * confirmations; one that names what goes lets them decide.
 *
 * The exception is a project that has no `sdkconfig` at all, where there is
 * genuinely nothing to lose. Confirming a no-op is how confirmations become
 * noise, so that case applies immediately.
 */
export default function TargetPicker(props: Props) {
  const [pending, setPending] = useState<string | null>(null);
  const blocked = setTargetBlockedReason({
    sketchDir: props.sketchDir,
    busy: props.busy,
    idfAvailable: props.idfAvailable,
  });

  const choose = (next: string) => {
    if (!next || next === props.current) {
      setPending(null);
      return;
    }
    if (!props.hasSdkconfig) {
      props.onSetTarget(next);
      setPending(null);
      return;
    }
    setPending(next);
  };

  return (
    <div className="toolbar-pair">
      <select
        className="select"
        value={pending ?? props.current ?? ""}
        onChange={(e) => choose(e.target.value)}
        disabled={blocked !== null || props.targets.length === 0}
        title={blocked ?? "ESP-IDF chip target (sdkconfig)"}
        aria-label="ESP-IDF target"
      >
        {props.current === null && <option value="">no target set…</option>}
        {props.targets.map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>

      {pending && (
        <>
          <span
            className="target-warning"
            title={
              `Setting the target deletes build/ and regenerates sdkconfig — ` +
              `every value you changed by hand is lost.` +
              (props.underGit
                ? " Bancada commits first, so it is recoverable."
                : " This project is not under git, so there is no undo.")
            }
          >
            {props.underGit ? "⚠ rebuilds sdkconfig" : "⚠ not under git"}
          </span>
          <button
            className="btn danger"
            onClick={() => {
              props.onSetTarget(pending);
              setPending(null);
            }}
            disabled={blocked !== null}
            title={blocked ?? `Delete build/ and regenerate sdkconfig for ${pending}`}
          >
            Set {pending}
          </button>
          {/* Cancel restores the shown value: a select left displaying a
              target that was never applied is a lie the user would act on. */}
          <button
            className="btn icon"
            onClick={() => setPending(null)}
            title="Keep the current target"
            aria-label="Cancel target change"
          >
            ✕
          </button>
        </>
      )}
    </div>
  );
}
