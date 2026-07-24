import { useEffect, useState } from "react";
import {
  addLocalLibrary,
  addRegistryLibraryToProfile,
  createLibrary,
  installLibrary,
  listInstalledLibraries,
  searchLibraries,
  sketchbookLibrariesDir,
  uninstallLibrary,
  LIBRARY_CATEGORIES,
  type IndexedLibrary,
  type InstalledLibrary,
  type LibrarySpec,
  type SketchYaml,
} from "../api";
import { open } from "@tauri-apps/plugin-dialog";

interface Props {
  sketchDir: string | null;
  profile: string | null;
  onYamlChanged: (y: SketchYaml) => void;
  notify: (msg: string, isError?: boolean) => void;
}

const EMPTY_FORM: LibrarySpec = {
  name: "",
  version: "0.1.0",
  author: "",
  maintainer: "",
  sentence: "",
  paragraph: "",
  category: "Other",
  url: "",
  architectures: "*",
};

/** Mirrors the backend's folder derivation, for the destination preview only. */
const folderFor = (name: string) => name.trim().replace(/\s+/g, "_");

export default function LibraryManager({
  sketchDir,
  profile,
  onYamlChanged,
  notify,
}: Props) {
  const [tab, setTab] = useState<"installed" | "search" | "new">("installed");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<IndexedLibrary[]>([]);
  const [installed, setInstalled] = useState<InstalledLibrary[]>([]);
  const [working, setWorking] = useState(false);
  const [libsDir, setLibsDir] = useState<string | null>(null);
  const [form, setForm] = useState<LibrarySpec>(EMPTY_FORM);
  const [newWarnings, setNewWarnings] = useState<string[]>([]);

  const refreshInstalled = () =>
    listInstalledLibraries().then(setInstalled).catch((e) => notify(String(e), true));

  useEffect(() => {
    refreshInstalled();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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

  const doInstall = async (lib: IndexedLibrary) => {
    setWorking(true);
    try {
      await installLibrary(lib.name);
      notify(`Installed ${lib.name} ${lib.latest.version}`);
      await refreshInstalled();
      // Also pin it in the sketch.yaml profile when a project is open.
      if (sketchDir && profile) {
        const y = await addRegistryLibraryToProfile(
          sketchDir,
          profile,
          lib.name,
          lib.latest.version,
        );
        onYamlChanged(y);
      }
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const doUninstall = async (name: string) => {
    setWorking(true);
    try {
      await uninstallLibrary(name);
      notify(`Removed ${name}`);
      await refreshInstalled();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const addLocal = async () => {
    if (!sketchDir || !profile) {
      notify("Open a sketch with a sketch.yaml profile first", true);
      return;
    }
    const dir = await open({ directory: true, title: "Choose a library folder" });
    if (typeof dir !== "string") return;
    try {
      const y = await addLocalLibrary(sketchDir, profile, dir);
      onYamlChanged(y);
      notify(`Added local library: ${dir}`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  // Resolved lazily: the lookup spawns arduino-cli and most sessions never
  // open this tab.
  useEffect(() => {
    if (tab !== "new" || libsDir) return;
    sketchbookLibrariesDir()
      .then(setLibsDir)
      .catch((e) => notify(String(e), true));
  }, [tab, libsDir, notify]);

  const patchForm = (p: Partial<LibrarySpec>) => setForm((f) => ({ ...f, ...p }));

  const doCreate = async () => {
    const name = form.name.trim();
    if (!name) {
      notify("Library name is required", true);
      return;
    }
    setWorking(true);
    setNewWarnings([]);
    try {
      const res = await createLibrary({ ...form, name }, sketchDir, profile);
      setNewWarnings(res.warnings);
      if (res.yaml) onYamlChanged(res.yaml);
      // One notify per outcome: the status bar is a single line, so a second
      // call would clobber the first.
      notify(
        res.profile_error
          ? `Created ${res.dir}, but the profile was not updated: ${res.profile_error}`
          : `✓ Created ${res.dir}`,
        Boolean(res.profile_error),
      );
      // Keep author/maintainer — someone making several libraries types it once.
      setForm({ ...EMPTY_FORM, author: form.author, maintainer: form.maintainer });
      await refreshInstalled();
      setTab("installed");
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="library-manager">
      <div className="panel-tabs">
        <button
          className={tab === "installed" ? "tab active" : "tab"}
          onClick={() => setTab("installed")}
        >
          Installed
        </button>
        <button
          className={tab === "search" ? "tab active" : "tab"}
          onClick={() => setTab("search")}
        >
          Registry
        </button>
        <button
          className={tab === "new" ? "tab active" : "tab"}
          onClick={() => setTab("new")}
        >
          New
        </button>
        <div className="spacer" />
        <button className="btn small" onClick={addLocal} title="Add a local/proprietary library folder to this sketch's profile (sketch.yaml dir: entry)">
          + Local…
        </button>
      </div>

      {tab === "search" && (
        <div className="lib-search">
          <div className="search-row">
            <input
              className="input"
              placeholder="search the Arduino library registry…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doSearch()}
            />
            <button className="btn small" onClick={doSearch} disabled={working}>
              Search
            </button>
          </div>
          <div className="lib-list">
            {results.map((lib) => (
              <div key={lib.name} className="lib-card">
                <div className="lib-head">
                  <span className="lib-name">{lib.name}</span>
                  <span className="lib-version">{lib.latest.version}</span>
                </div>
                <div className="lib-sentence">{lib.latest.sentence}</div>
                <div className="lib-actions">
                  <span className="lib-author">{lib.latest.author}</span>
                  <button
                    className="btn small"
                    disabled={working}
                    onClick={() => doInstall(lib)}
                  >
                    Install
                  </button>
                </div>
              </div>
            ))}
            {results.length === 0 && (
              <div className="empty-hint">Search results appear here.</div>
            )}
          </div>
        </div>
      )}

      {tab === "installed" && (
        <div className="lib-list">
          {installed.map((lib) => (
            <div key={`${lib.name}@${lib.install_dir}`} className="lib-card">
              <div className="lib-head">
                <span className="lib-name">{lib.name}</span>
                <span className="lib-version">{lib.version}</span>
                {lib.location !== "user" && (
                  <span className="lib-badge">{lib.location}</span>
                )}
              </div>
              <div className="lib-sentence">{lib.sentence}</div>
              <div className="lib-actions">
                <span className="lib-author">{lib.author}</span>
                {lib.location === "user" && (
                  <button
                    className="btn small danger"
                    disabled={working}
                    onClick={() => doUninstall(lib.name)}
                  >
                    Remove
                  </button>
                )}
              </div>
            </div>
          ))}
          {installed.length === 0 && (
            <div className="empty-hint">No libraries installed yet.</div>
          )}
        </div>
      )}

      {tab === "new" && (
        <div className="lib-new-form">
          <label className="field">
            Name *
            <input
              className="input"
              placeholder="MySensor"
              value={form.name}
              onChange={(e) => patchForm({ name: e.target.value })}
              onKeyDown={(e) => e.key === "Enter" && doCreate()}
            />
          </label>
          {libsDir && (
            <div className="lib-dest">
              {libsDir}/{folderFor(form.name) || "…"}
            </div>
          )}

          {sketchDir && profile ? (
            <div className="empty-hint">
              Also pinned into profile <strong>{profile}</strong> as an absolute{" "}
              <code>dir:</code> entry — profile builds exclude globally installed
              libraries, so without it the build cannot see this library.
            </div>
          ) : (
            <div className="empty-hint">
              No active sketch.yaml profile — the library is created in the
              sketchbook only. Profile builds are isolated, so add it to a
              profile later with <strong>+ Local…</strong>.
            </div>
          )}

          <label className="field">
            Version
            <input
              className="input"
              value={form.version}
              onChange={(e) => patchForm({ version: e.target.value })}
            />
          </label>
          <label className="field">
            Author
            <input
              className="input"
              placeholder="Your Name <you@example.com>"
              value={form.author}
              onChange={(e) => patchForm({ author: e.target.value })}
            />
          </label>
          <label className="field">
            Maintainer
            <input
              className="input"
              placeholder="same as author"
              value={form.maintainer}
              onChange={(e) => patchForm({ maintainer: e.target.value })}
            />
          </label>
          <label className="field">
            Summary
            <input
              className="input"
              placeholder="one-line description"
              value={form.sentence}
              onChange={(e) => patchForm({ sentence: e.target.value })}
            />
          </label>
          <label className="field">
            Description
            <input
              className="input"
              placeholder="longer description"
              value={form.paragraph}
              onChange={(e) => patchForm({ paragraph: e.target.value })}
            />
          </label>
          <label className="field">
            Category
            <select
              className="select small"
              value={form.category}
              onChange={(e) => patchForm({ category: e.target.value })}
            >
              {LIBRARY_CATEGORIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            URL
            <input
              className="input"
              placeholder="https://…"
              value={form.url}
              onChange={(e) => patchForm({ url: e.target.value })}
            />
          </label>
          <label className="field">
            Architectures
            <input
              className="input"
              title="comma-separated, or * for all"
              value={form.architectures}
              onChange={(e) => patchForm({ architectures: e.target.value })}
            />
          </label>

          {newWarnings.map((w) => (
            <div key={w} className="empty-hint">
              ⚠ {w}
            </div>
          ))}

          <div className="lib-new-actions">
            <button
              className="btn small primary"
              onClick={doCreate}
              disabled={working || !form.name.trim()}
            >
              Create library
            </button>
            <span className="scope-dim">
              generates library.properties, src/, keywords.txt and an example
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
