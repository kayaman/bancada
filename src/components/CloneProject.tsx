import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  cloneProject,
  defaultProjectParent,
  loadSettings,
  saveSettings,
} from "../api";

interface Props {
  /** Directory of the currently open sketch, prefilled as the source. */
  sourceDir: string | null;
  onCreated: (dir: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

const basename = (dir: string) => dir.split("/").pop() ?? "";
const defaultNameFor = (dir: string) =>
  basename(dir) ? `${basename(dir)}-copy` : "";

export default function CloneProject({
  sourceDir,
  onCreated,
  onCancel,
  notify,
}: Props) {
  const [source, setSource] = useState(sourceDir ?? "");
  const [name, setName] = useState(defaultNameFor(sourceDir ?? ""));
  // Once the user edits the name, changing the source stops refreshing it.
  const [nameTouched, setNameTouched] = useState(false);
  const [parent, setParent] = useState("");
  const [working, setWorking] = useState(false);

  // Default the location to wherever the last project went, falling back to
  // ~/Projects — or the home directory itself for users who don't keep one.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const [settings, fallback] = await Promise.all([
        loadSettings().catch(() => ({}) as Awaited<ReturnType<typeof loadSettings>>),
        defaultProjectParent().catch(() => ""),
      ]);
      if (cancelled) return;
      setParent(settings.last_new_project_parent || fallback);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const dest = parent && name.trim() ? `${parent}/${name.trim()}` : "";

  const changeSource = (dir: string) => {
    setSource(dir);
    if (!nameTouched) setName(defaultNameFor(dir));
  };

  const chooseSource = async () => {
    const dir = await open({ directory: true, title: "Sketch folder to clone" });
    if (typeof dir !== "string") return;
    changeSource(dir);
  };

  const chooseParent = async () => {
    const dir = await open({ directory: true, title: "Where to create the project" });
    if (typeof dir !== "string") return;
    setParent(dir);
  };

  const clone = async () => {
    if (!source || !name.trim() || !parent) return;
    setWorking(true);
    try {
      const res = await cloneProject(source, parent, name.trim());
      // Remembering the parent is a convenience; never fail cloning over it.
      saveSettings({ last_new_project_parent: parent }).catch(() => {});
      notify(
        res.warnings.length
          ? `Cloned with notes: ${res.warnings.join("; ")}`
          : `✓ Cloned into ${res.dir}`,
      );
      onCreated(res.dir);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="new-project">
      <div className="np-head">
        <strong>Clone project</strong>
        <div className="spacer" />
        <button className="btn small" onClick={onCancel} disabled={working}>
          Cancel
        </button>
        <button
          className="btn small primary"
          onClick={clone}
          disabled={working || !source || !name.trim() || !parent}
        >
          ⧉ Clone
        </button>
      </div>

      <div className="np-body">
        <label className="field">
          Source sketch
          <span className="np-row">
            <input
              className="input"
              value={source}
              onChange={(e) => changeSource(e.target.value)}
            />
            <button className="btn small" onClick={chooseSource} disabled={working}>
              Choose…
            </button>
          </span>
        </label>

        <label className="field">
          New name
          <input
            className="input"
            autoFocus
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setNameTouched(true);
            }}
            onKeyDown={(e) => e.key === "Enter" && clone()}
          />
        </label>

        <label className="field">
          Location
          <span className="np-row">
            <input
              className="input"
              value={parent}
              onChange={(e) => setParent(e.target.value)}
            />
            <button className="btn small" onClick={chooseParent} disabled={working}>
              Choose…
            </button>
          </span>
        </label>

        {dest && <div className="lib-dest">{dest}</div>}
      </div>
    </div>
  );
}
