import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import BoardPicker from "./BoardPicker";
import {
  createProject,
  defaultProjectParent,
  listAllBoards,
  listSketchTemplates,
  loadSettings,
  searchLibraries,
  setLastProjectParent,
  type BoardOption,
  type IndexedLibrary,
  type SketchTemplate,
} from "../api";

interface Props {
  /** FQBN of the board currently attached, preselected when known. */
  detectedFqbn: string | null;
  onCreated: (dir: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

/** `esp32:esp32:esp32s3:opts` -> `esp32s3`; mirrors project.rs for the preview. */
const profileFor = (fqbn: string) => fqbn.split(":")[2] ?? "";

export default function NewProject({
  detectedFqbn,
  onCreated,
  onCancel,
  notify,
}: Props) {
  const [name, setName] = useState("");
  const [parent, setParent] = useState("");
  const [boards, setBoards] = useState<BoardOption[]>([]);
  const [fqbn, setFqbn] = useState(detectedFqbn ?? "");
  const [profile, setProfile] = useState("");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<IndexedLibrary[]>([]);
  const [picked, setPicked] = useState<Record<string, string>>({});
  const [templates, setTemplates] = useState<SketchTemplate[]>([]);
  const [template, setTemplate] = useState("blink");
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
    listAllBoards()
      .then((b) => {
        if (!cancelled) setBoards(b);
      })
      .catch((e) => notify(String(e), true));
    // A missing template list degrades to the backend's Blink default —
    // creation must not depend on this call succeeding.
    listSketchTemplates()
      .then((t) => {
        if (!cancelled) setTemplates(t);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const effectiveProfile = profile.trim() || profileFor(fqbn);
  const dest = parent && name.trim() ? `${parent}/${name.trim()}` : "";

  const chooseParent = async () => {
    const dir = await open({ directory: true, title: "Where to create the project" });
    if (typeof dir !== "string") return;
    setParent(dir);
  };

  const doSearch = async () => {
    if (!query.trim()) return;
    setWorking(true);
    try {
      setResults(await searchLibraries(query.trim()));
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const toggle = (lib: IndexedLibrary) =>
    setPicked((p) => {
      const next = { ...p };
      if (next[lib.name]) delete next[lib.name];
      else next[lib.name] = lib.latest.version;
      return next;
    });

  const create = async () => {
    // `working` guards the same hazard DuplicateProject and RenameProject
    // guard: Enter in the name field bypasses the disabled button, so two
    // quick presses would fire two createProject calls at the same path.
    if (working || !name.trim() || !parent || !fqbn) return;
    setWorking(true);
    try {
      // Pin explicitly rather than leaving versions floating.
      const libraries = Object.entries(picked).map(([n, v]) => `${n}@${v}`);
      const res = await createProject(
        parent,
        name.trim(),
        fqbn,
        profile.trim() || null,
        libraries,
        template,
      );
      // Remembering the parent is a convenience; never fail creation over it.
      setLastProjectParent(parent).catch(() => {});
      // Both are non-fatal: the sketch exists and builds either way, so say
      // what fell short rather than hiding it behind a plain success.
      const warnings: string[] = [];
      if (res.library_errors.length) {
        warnings.push(
          `${res.library_errors.length} librar${res.library_errors.length === 1 ? "y" : "ies"} failed: ${res.library_errors.join("; ")}`,
        );
      }
      if (res.git_error) {
        warnings.push(`not put under git: ${res.git_error}`);
      }
      notify(
        warnings.length
          ? `Created ${res.dir}, but ${warnings.join("; ")}`
          : `✓ Created ${res.dir} (profile ${res.profile})`,
        warnings.length > 0,
      );
      onCreated(res.dir);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const pickedCount = Object.keys(picked).length;

  return (
    <div className="new-project">
      <div className="np-head">
        <strong>New project</strong>
        <div className="spacer" />
        <button className="btn small" onClick={onCancel} disabled={working}>
          Cancel
        </button>
        <button
          className="btn small primary"
          onClick={create}
          disabled={working || !name.trim() || !parent || !fqbn}
        >
          Create project
        </button>
      </div>

      <div className="np-body">
        <label className="field">
          Name
          <input
            className="input"
            placeholder="BlinkNode"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && create()}
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

        <label className="field">
          Board
          <BoardPicker
            boards={boards}
            value={fqbn}
            onChange={setFqbn}
            title="Board for the project's profile"
          />
        </label>
        {detectedFqbn && fqbn === detectedFqbn && (
          <div className="scope-dim">preselected from the attached board</div>
        )}
        {boards.length === 0 && (
          <div className="empty-hint">
            No installed platforms found — install a core first (a board platform
            is required, because the profile pins its version).
          </div>
        )}

        {templates.length > 0 && (
          <div className="field">
            Starter
            <div className="np-tmpl-cards" role="radiogroup" aria-label="Starter template">
              {templates.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  role="radio"
                  aria-checked={template === t.id}
                  className={`np-tmpl-card${template === t.id ? " selected" : ""}`}
                  onClick={() => setTemplate(t.id)}
                  disabled={working}
                >
                  <span className="np-tmpl-label">{t.label}</span>
                  <span className="np-tmpl-desc">{t.description}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        <label className="field">
          Profile name
          <input
            className="input"
            placeholder={profileFor(fqbn) || "derived from the board"}
            value={profile}
            onChange={(e) => setProfile(e.target.value)}
          />
        </label>
        {fqbn && (
          <div className="scope-dim">
            sketch.yaml will pin <code>{fqbn}</code> as profile{" "}
            <code>{effectiveProfile}</code>, with the installed platform version
          </div>
        )}

        <div className="np-libs">
          <div className="np-row">
            <input
              className="input"
              placeholder="search the registry to add libraries…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doSearch()}
            />
            <button className="btn small" onClick={doSearch} disabled={working}>
              Search
            </button>
            {pickedCount > 0 && (
              <span className="scope-dim">{pickedCount} selected</span>
            )}
          </div>
          <div className="np-lib-list">
            {results.map((lib) => (
              <label key={lib.name} className="np-lib">
                <input
                  type="checkbox"
                  checked={Boolean(picked[lib.name])}
                  onChange={() => toggle(lib)}
                />
                <span className="lib-name">{lib.name}</span>
                <span className="lib-version">{lib.latest.version}</span>
                <span className="lib-sentence">{lib.latest.sentence}</span>
              </label>
            ))}
            {results.length === 0 && (
              <div className="empty-hint">
                Optional. Selected libraries are pinned into the profile with
                their dependencies resolved.
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
