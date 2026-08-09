import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import type { DetectedPort, SketchYaml } from "../api";
import { portOptions } from "../ports";
import BrandMark from "./BrandMark";
import RecentsMenu from "./RecentsMenu";

interface Props {
  sketchDir: string | null;
  sketchYaml: SketchYaml | null;
  profile: string | null;
  ports: DetectedPort[];
  selectedPort: string | null;
  busy: boolean;
  onOpenSketch: () => void;
  onOpenRecent: (dir: string) => void;
  onNewProject: () => void;
  onCloneProject: () => void;
  onCreateProfile: () => void;
  onAddProfile: () => void;
  onRetargetProfile: () => void;
  onSelectProfile: (p: string) => void;
  onSelectPort: (p: string) => void;
  onRefreshPorts: () => void;
  onVerify: () => void;
  onUpload: () => void;
}

export default function Toolbar(props: Props) {
  const profiles = Object.keys(props.sketchYaml?.profiles ?? {});
  const sketchName = props.sketchDir?.split("/").pop();
  // tauri.conf.json is the single source of truth for the version; outside a
  // Tauri shell (plain vite in a browser) the call rejects and the span
  // simply never renders.
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  return (
    <div className="toolbar">
      <div className="brand" title="Bancada — Arduino Workbench">
        <BrandMark size={20} />
        <span className="brand-name">bancada</span>
        {version && <span className="brand-version">{version}</span>}
      </div>

      <div className="toolbar-sep" />

      <div className="toolbar-group">
        <button
          className="btn"
          onClick={props.onOpenSketch}
          title={props.sketchDir ?? "Open a sketch folder"}
        >
          {sketchName ? `📁 ${sketchName}` : "📁 Open Sketch…"}
        </button>

        <RecentsMenu onOpen={props.onOpenRecent} />

        <button
          className="btn"
          onClick={props.onNewProject}
          title="Create a sketch with a sketch.yaml profile"
        >
          ＋ New Project…
        </button>

        <button
          className="btn"
          onClick={props.onCloneProject}
          title="Copy a sketch into a new project with a fresh git repo"
        >
          ⧉ Clone…
        </button>
      </div>

      <div className="toolbar-sep" />

      <div className="toolbar-group">
        {props.sketchDir && profiles.length === 0 ? (
          <button
            className="btn"
            onClick={props.onCreateProfile}
            title="This sketch has no sketch.yaml profile — create one"
          >
            ＋ Create profile…
          </button>
        ) : (
          <div className="toolbar-pair">
            <select
              className="select"
              value={props.profile ?? ""}
              onChange={(e) => props.onSelectProfile(e.target.value)}
              disabled={profiles.length === 0}
              title="Build profile (sketch.yaml)"
            >
              {profiles.length === 0 && (
                <option value="">no sketch.yaml profile</option>
              )}
              {profiles.map((p) => {
                const board = props.sketchYaml?.profiles?.[p]?.fqbn.split(":")[2];
                return (
                  <option key={p} value={p}>
                    {board ? `${p} — ${board}` : p}
                  </option>
                );
              })}
            </select>
            {props.sketchDir && (
              <>
                <button
                  className="btn icon"
                  onClick={props.onAddProfile}
                  title="Add a profile for another board (libraries copied)"
                  aria-label="Add profile"
                >
                  ＋
                </button>
                <button
                  className="btn icon"
                  onClick={props.onRetargetProfile}
                  disabled={profiles.length === 0 || !props.profile}
                  title="Change this profile's board"
                  aria-label="Change profile board"
                >
                  ✎
                </button>
              </>
            )}
          </div>
        )}

        <div className="toolbar-pair">
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
          <button
            className="btn icon"
            onClick={props.onRefreshPorts}
            title="Rescan ports"
            aria-label="Rescan ports"
          >
            ⟳
          </button>
        </div>
      </div>

      <div className="spacer" />

      <div className="toolbar-group">
        <button
          className="btn"
          onClick={props.onVerify}
          disabled={props.busy || !props.sketchDir}
          title="Compile (verify)"
        >
          ✓ Verify
        </button>
        <button
          className="btn primary"
          onClick={props.onUpload}
          disabled={props.busy || !props.sketchDir || !props.selectedPort}
          title="Compile and flash to the board"
        >
          → Flash
        </button>
      </div>
    </div>
  );
}
