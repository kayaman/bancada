import { useEffect, useState } from "react";
import {
  addPlatformToProfile,
  installCore,
  listCores,
  searchCores,
  uninstallCore,
  updateCoreIndex,
  type CoreView,
  type SketchYaml,
} from "../api";

interface Props {
  sketchDir: string | null;
  profile: string | null;
  sketchYaml: SketchYaml | null;
  onYamlChanged: (y: SketchYaml) => void;
  /** Called before a streaming operation so the build console can be shown. */
  onStreamStart: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

/**
 * The platform id inside a sketch.yaml `platform:` entry — `esp32:esp32 (3.3.11)`
 * → `esp32:esp32`. Mirrors `boards::platform_dep_id`; duplicated here only to
 * label the Remove button, never to decide what gets written.
 */
const pinnedId = (entry: string) => entry.split("(")[0].trim();

export default function BoardsManager({
  sketchDir,
  profile,
  sketchYaml,
  onYamlChanged,
  onStreamStart,
  notify,
}: Props) {
  const [tab, setTab] = useState<"installed" | "search">("installed");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<CoreView[]>([]);
  const [installed, setInstalled] = useState<CoreView[]>([]);
  const [working, setWorking] = useState(false);
  /** Chosen version per platform id; absent means "the newest". */
  const [picked, setPicked] = useState<Record<string, string>>({});
  /** Remove is two-step: this holds the id awaiting confirmation. */
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const refreshInstalled = () =>
    listCores()
      .then(setInstalled)
      .catch((e) => notify(String(e), true));

  useEffect(() => {
    refreshInstalled();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Profiles of the open sketch that pin this platform. */
  const pinnedBy = (id: string) =>
    Object.entries(sketchYaml?.profiles ?? {})
      .filter(([, p]) => (p.platforms ?? []).some((d) => pinnedId(d.platform) === id))
      .map(([name]) => name);

  const versionFor = (core: CoreView) =>
    picked[core.id] || core.versions[0] || core.latest_version || "";

  const doSearch = async () => {
    if (!query.trim()) return;
    setWorking(true);
    try {
      setResults(await searchCores(query.trim()));
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  /**
   * Install or upgrade. The version is always passed explicitly, even when it is
   * the newest, so the profile pin records a concrete version rather than
   * whatever "latest" means on some later day.
   */
  const doInstall = async (core: CoreView, version: string) => {
    if (!version) {
      notify(`No installable release for ${core.id}`, true);
      return;
    }
    setConfirmRemove(null);
    setWorking(true);
    onStreamStart();
    notify(`Installing ${core.id} ${version}…`);
    try {
      const r = await installCore(core.id, version, sketchDir, profile);
      if (!r.result.success) {
        notify(`Install of ${core.id} failed`, true);
        return;
      }
      if (r.yaml) onYamlChanged(r.yaml);
      // Non-fatal by design: the platform is installed either way.
      notify(
        r.profile_error
          ? `Installed ${core.id} ${version} — but pinning it failed: ${r.profile_error}`
          : `✓ Installed ${core.id} ${version}`,
        !!r.profile_error,
      );
      await refreshInstalled();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const doRemove = async (core: CoreView) => {
    setConfirmRemove(null);
    setWorking(true);
    onStreamStart();
    notify(`Removing ${core.id}…`);
    try {
      const r = await uninstallCore(core.id);
      notify(
        r.success ? `Removed ${core.id}` : `Removal of ${core.id} failed`,
        !r.success,
      );
      await refreshInstalled();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const doPin = async (core: CoreView) => {
    if (!sketchDir || !profile) {
      notify("Open a project with a sketch.yaml profile first", true);
      return;
    }
    setWorking(true);
    try {
      const y = await addPlatformToProfile(
        sketchDir,
        profile,
        core.id,
        core.installed_version,
      );
      onYamlChanged(y);
      notify(`Pinned ${core.id} ${core.installed_version} to ${profile}`);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const doUpdateIndex = async () => {
    setWorking(true);
    onStreamStart();
    notify("Updating platform indexes…");
    try {
      const r = await updateCoreIndex();
      notify(r.success ? "✓ Platform indexes updated" : "Index update failed", !r.success);
      await refreshInstalled();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  /** A version picker, only when there is a choice to make. */
  const versionPicker = (core: CoreView) =>
    core.versions.length > 1 ? (
      <select
        className="select small"
        value={versionFor(core)}
        disabled={working}
        title="Version to install"
        onChange={(e) => setPicked((p) => ({ ...p, [core.id]: e.target.value }))}
      >
        {core.versions.map((v) => (
          <option key={v} value={v}>
            {v}
            {v === core.installed_version ? " (installed)" : ""}
          </option>
        ))}
      </select>
    ) : null;

  const card = (core: CoreView, context: "installed" | "search") => {
    const pins = pinnedBy(core.id);
    const chosen = versionFor(core);
    return (
      <div key={core.id} className="core-card">
        <div className="core-head">
          <span className="core-name">{core.name}</span>
          {core.status === "not_installed" ? (
            <span className="core-version">{core.latest_version}</span>
          ) : (
            <span className="core-version">{core.installed_version}</span>
          )}
          {core.status === "update_available" && (
            <span className="core-badge update" title={`${core.latest_version} available`}>
              update
            </span>
          )}
          {core.status === "up_to_date" && context === "search" && (
            <span className="core-badge">installed</span>
          )}
        </div>

        <div className="core-id">{core.id}</div>
        {core.maintainer && <div className="core-meta">{core.maintainer}</div>}

        {core.boards.length > 0 && (
          <details className="core-boards">
            <summary>
              {core.boards.length} board{core.boards.length === 1 ? "" : "s"}
            </summary>
            <ul>
              {core.boards.map((b) => (
                <li key={b}>{b}</li>
              ))}
            </ul>
          </details>
        )}

        {pins.length > 0 && (
          <div className="core-meta">
            pinned by profile{pins.length === 1 ? "" : "s"}: {pins.join(", ")}
          </div>
        )}

        <div className="core-actions">
          {versionPicker(core)}
          <div className="spacer" />

          {core.status === "not_installed" && (
            <button
              className="btn small primary"
              disabled={working}
              onClick={() => doInstall(core, chosen)}
            >
              Install
            </button>
          )}

          {core.status === "update_available" && (
            <button
              className="btn small primary"
              disabled={working}
              onClick={() => doInstall(core, core.latest_version)}
              title={`Update to ${core.latest_version}`}
            >
              Update
            </button>
          )}

          {core.status !== "not_installed" &&
            chosen &&
            chosen !== core.installed_version && (
              <button
                className="btn small"
                disabled={working}
                onClick={() => doInstall(core, chosen)}
                title={`Switch to ${chosen}`}
              >
                Switch
              </button>
            )}

          {core.status !== "not_installed" && sketchDir && profile && (
            <button
              className="btn small"
              disabled={working}
              onClick={() => doPin(core)}
              title={`Pin ${core.installed_version} into the ${profile} profile`}
            >
              Pin
            </button>
          )}

          {core.status !== "not_installed" && (
            <button
              className={confirmRemove === core.id ? "btn small danger" : "btn small"}
              disabled={working}
              onClick={() =>
                confirmRemove === core.id
                  ? doRemove(core)
                  : setConfirmRemove(core.id)
              }
              title={
                pins.length > 0
                  ? `Still pinned by ${pins.join(", ")}`
                  : "Uninstall this platform"
              }
            >
              {confirmRemove === core.id
                ? pins.length > 0
                  ? "Remove anyway?"
                  : "Confirm?"
                : "Remove"}
            </button>
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="boards-manager">
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
          Search
        </button>
        <div className="spacer" />
        <button
          className="btn small"
          onClick={doUpdateIndex}
          disabled={working}
          title="Refresh the platform indexes (arduino-cli core update-index)"
        >
          ⟳ Index
        </button>
      </div>

      {tab === "search" && (
        <div className="core-search">
          <div className="search-row">
            <input
              className="input"
              placeholder="search platforms… (e.g. esp32, rp2040)"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doSearch()}
            />
            <button className="btn small" onClick={doSearch} disabled={working}>
              Search
            </button>
          </div>
          <div className="core-list">
            {results.map((c) => card(c, "search"))}
            {results.length === 0 && (
              <div className="empty-hint">
                Search results appear here. Only indexes arduino-cli already knows
                are searched — to add a third-party one, run{" "}
                <code>
                  arduino-cli config add board_manager.additional_urls &lt;url&gt;
                </code>
                .
              </div>
            )}
          </div>
        </div>
      )}

      {tab === "installed" && (
        <div className="core-list">
          {installed.map((c) => card(c, "installed"))}
          {installed.length === 0 && (
            <div className="empty-hint">
              No platforms installed. Use the Search tab to add one.
            </div>
          )}
        </div>
      )}
    </div>
  );
}
