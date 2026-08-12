import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import type { DetectedPort, RepoState, SketchYaml, Visibility } from "../api";
import { portOptions } from "../ports";
import { defaultRepoName } from "../publishRepo";
import { buildBlockedReason, retargetBlockedReason } from "../toolbarModel";
import BrandMark from "./BrandMark";
import GitPill from "./GitPill";
import ProjectMenu from "./ProjectMenu";

interface Props {
  sketchDir: string | null;
  sketchYaml: SketchYaml | null;
  profile: string | null;
  ports: DetectedPort[];
  selectedPort: string | null;
  busy: boolean;
  gitState: RepoState | null;
  ghAvailable: boolean;
  onOpenProject: () => void;
  onOpenRecent: (dir: string) => void;
  onNewProject: () => void;
  onRenameProject: () => void;
  onDuplicateProject: () => void;
  onOpenUsage: () => void;
  onCreateProfile: () => void;
  onAddProfile: () => void;
  onRetargetProfile: () => void;
  onSelectProfile: (p: string) => void;
  onSelectPort: (p: string) => void;
  onRefreshPorts: () => void;
  onVerify: () => void;
  onUpload: () => void;
  onGitCommit: (message: string) => void;
  onGitSync: () => void;
  onGitInit: () => void;
  onGitInitHere: () => void;
  onGitCreateRemote: (
    name: string,
    visibility: Visibility,
    description: string | null,
  ) => void;
  onGitSetRemote: (url: string) => void;
}

export default function Toolbar(props: Props) {
  const profiles = Object.keys(props.sketchYaml?.profiles ?? {});
  // A disabled control must say why — the rule `gitStatus.syncDisabledReason`
  // was written for. These three used to show a static description instead.
  const build = {
    sketchDir: props.sketchDir,
    selectedPort: props.selectedPort,
    busy: props.busy,
  };
  const verifyReason = buildBlockedReason("verify", build);
  const flashReason = buildBlockedReason("flash", build);
  const retargetReason = retargetBlockedReason(profiles, props.profile);
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

      {/* Project: one affordance. It names what is open and holds every
          action on it, so `＋` and `✎` each mean one thing on this bar. */}
      <div className="toolbar-group toolbar-group-project">
        <ProjectMenu
          sketchDir={props.sketchDir}
          onOpen={props.onOpenProject}
          onOpenRecent={props.onOpenRecent}
          onNew={props.onNewProject}
          onDuplicate={props.onDuplicateProject}
          onRename={props.onRenameProject}
        />
      </div>

      <div className="toolbar-sep" />

      <div className="toolbar-group">
        {props.sketchDir && profiles.length === 0 ? (
          <button
            className="btn"
            onClick={props.onCreateProfile}
            title="This project has no sketch.yaml profile — create one"
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
                  disabled={retargetReason !== null}
                  title={retargetReason ?? "Change this profile's board"}
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

        {props.sketchDir && (
          <GitPill
            state={props.gitState}
            busy={props.busy}
            ghAvailable={props.ghAvailable}
            defaultRepoName={defaultRepoName(props.sketchDir)}
            onCommit={props.onGitCommit}
            onSync={props.onGitSync}
            onInit={props.onGitInit}
            onInitHere={props.onGitInitHere}
            onCreateRemote={props.onGitCreateRemote}
            onSetRemote={props.onGitSetRemote}
          />
        )}
      </div>

      <div className="spacer" />

      {/* Build: pinned `flex: none` in CSS. These are the two controls that
          must never be the ones squeezed off the bar. Usage sits here rather
          than with the project actions — it reports on every project, not
          the open one. */}
      <div className="toolbar-group toolbar-group-build">
        <button
          className="btn icon"
          onClick={props.onOpenUsage}
          title="Token usage and cost, across all projects"
          aria-label="Usage"
        >
          📊
        </button>
        <button
          className="btn"
          onClick={props.onVerify}
          disabled={verifyReason !== null}
          title={verifyReason ?? "Compile (verify)"}
        >
          ✓ Verify
        </button>
        <button
          className="btn primary"
          onClick={props.onUpload}
          disabled={flashReason !== null}
          title={flashReason ?? "Compile and flash to the board"}
        >
          → Flash
        </button>
      </div>
    </div>
  );
}
