import { useEffect, useState } from "react";
import {
  addLocalLibrary,
  addRegistryLibraryToProfile,
  installLibrary,
  listInstalledLibraries,
  searchLibraries,
  uninstallLibrary,
  type IndexedLibrary,
  type InstalledLibrary,
  type SketchYaml,
} from "../api";
import { open } from "@tauri-apps/plugin-dialog";

interface Props {
  sketchDir: string | null;
  profile: string | null;
  onYamlChanged: (y: SketchYaml) => void;
  notify: (msg: string, isError?: boolean) => void;
}

export default function LibraryManager({
  sketchDir,
  profile,
  onYamlChanged,
  notify,
}: Props) {
  const [tab, setTab] = useState<"installed" | "search">("installed");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<IndexedLibrary[]>([]);
  const [installed, setInstalled] = useState<InstalledLibrary[]>([]);
  const [working, setWorking] = useState(false);

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
    </div>
  );
}
