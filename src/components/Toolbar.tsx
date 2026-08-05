import type { DetectedPort, SketchYaml } from "../api";
import { portOptions } from "../ports";

interface Props {
  sketchDir: string | null;
  sketchYaml: SketchYaml | null;
  profile: string | null;
  ports: DetectedPort[];
  selectedPort: string | null;
  busy: boolean;
  onOpenSketch: () => void;
  onNewProject: () => void;
  onCreateProfile: () => void;
  onSelectProfile: (p: string) => void;
  onSelectPort: (p: string) => void;
  onRefreshPorts: () => void;
  onVerify: () => void;
  onUpload: () => void;
}

export default function Toolbar(props: Props) {
  const profiles = Object.keys(props.sketchYaml?.profiles ?? {});
  const sketchName = props.sketchDir?.split("/").pop();

  return (
    <div className="toolbar">
      <button className="btn" onClick={props.onOpenSketch}>
        {sketchName ? `📁 ${sketchName}` : "📁 Open Sketch…"}
      </button>

      <button
        className="btn"
        onClick={props.onNewProject}
        title="Create a sketch with a sketch.yaml profile"
      >
        ＋ New Project…
      </button>

      {props.sketchDir && profiles.length === 0 ? (
        <button
          className="btn"
          onClick={props.onCreateProfile}
          title="This sketch has no sketch.yaml profile — create one"
        >
          ＋ Create profile…
        </button>
      ) : (
        <select
          className="select"
          value={props.profile ?? ""}
          onChange={(e) => props.onSelectProfile(e.target.value)}
          disabled={profiles.length === 0}
          title="Build profile (sketch.yaml)"
        >
          {profiles.length === 0 && <option value="">no sketch.yaml profile</option>}
          {profiles.map((p) => {
            const board = props.sketchYaml?.profiles?.[p]?.fqbn.split(":")[2];
            return (
              <option key={p} value={p}>
                {board ? `${p} — ${board}` : p}
              </option>
            );
          })}
        </select>
      )}

      <select
        className="select"
        value={props.selectedPort ?? ""}
        onChange={(e) => props.onSelectPort(e.target.value)}
        title="Serial port"
      >
        <option value="">select port…</option>
        {portOptions(props.ports, props.selectedPort).map((o) => (
          <option key={o.address} value={o.address} disabled={o.missing}>
            {o.label}
          </option>
        ))}
      </select>
      <button className="btn icon" onClick={props.onRefreshPorts} title="Rescan ports">
        ⟳
      </button>

      <div className="spacer" />

      <button
        className="btn action"
        onClick={props.onVerify}
        disabled={props.busy || !props.sketchDir}
        title="Compile (verify)"
      >
        ✓ Verify
      </button>
      <button
        className="btn action primary"
        onClick={props.onUpload}
        disabled={props.busy || !props.sketchDir || !props.selectedPort}
        title="Compile and flash to the board"
      >
        → Flash
      </button>
    </div>
  );
}
