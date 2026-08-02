import { useCallback, useEffect, useRef, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { cpp } from "@codemirror/lang-cpp";
import { oneDark } from "@codemirror/theme-one-dark";
import { ask, open } from "@tauri-apps/plugin-dialog";

import * as api from "./api";
import { nextSelectedPort } from "./ports";
import { checkNewEntry, checkNewFile } from "./newFile";
import { useExplorerStore } from "./explorerStore";
import {
  affectedByDelete,
  checkRename,
  isNonEmptyDir,
  pathAfterRename,
} from "./explorerOps";
import { arrivals } from "./portWatch";
import { blockedByConflict, conflictMessage } from "./conflicts";
import type { AgentEvent, DetectedPort, OutputLine, SketchYaml } from "./api";
import { AgentStore } from "./agent/agentStore";
import EditorTabs from "./components/EditorTabs";
import FileTree from "./components/FileTree";
import Toolbar from "./components/Toolbar";
import LibraryManager from "./components/LibraryManager";
import BoardsManager from "./components/BoardsManager";
import FleetManager from "./components/FleetManager";
import Console from "./components/Console";
import ScopeView from "./components/ScopeView";
import NewProject from "./components/NewProject";
import ProfileInit from "./components/ProfileInit";
import MqttPanel from "./components/MqttPanel";
import WsPanel from "./components/WsPanel";
import AgentPanel from "./components/AgentPanel";
import {
  GROUP_LABEL,
  GROUP_OF,
  GROUP_TABS,
  TAB_LABEL,
  groupHasUnseen,
  type BottomGroup,
  type BottomTab,
} from "./bottomTabs";

type SideGroup = "software" | "hardware";
type SoftwareTab = "files" | "libraries";
type HardwareTab = "boards" | "fleet";

// Bottom panel sizing: layout preference, so it lives in localStorage (per
// machine, not part of the app settings file).
const BOTTOM_HEIGHT_KEY = "bancada.bottomHeight";
const BOTTOM_MIN = 120;
const BOTTOM_DEFAULT = 220;
const clampBottomHeight = (h: number) =>
  Math.min(Math.max(h, BOTTOM_MIN), Math.round(window.innerHeight * 0.8));

// Sidebar width, same story as the bottom panel: a layout preference, so
// localStorage rather than the settings file. Capped at half the window — the
// editor is the point of the app and must not be squeezed out.
const SIDEBAR_WIDTH_KEY = "bancada.sidebarWidth";
const SIDEBAR_COLLAPSED_KEY = "bancada.sidebarCollapsed";
const SIDEBAR_MIN = 220;
const SIDEBAR_DEFAULT = 280;
const clampSidebarWidth = (w: number) =>
  Math.min(Math.max(w, SIDEBAR_MIN), Math.round(window.innerWidth * 0.5));

const MAX_CONSOLE_LINES = 5000;
const TRIM_CONSOLE_LINES = 4000;
const appendCapped = (prev: OutputLine[], l: OutputLine): OutputLine[] =>
  prev.length >= MAX_CONSOLE_LINES
    ? [...prev.slice(-TRIM_CONSOLE_LINES), l]
    : [...prev, l];

// One store for the whole app lifetime (not panel-owned): App-level
// `agent://event` listeners feed it even before the Assistant panel has ever
// been mounted, so the bottom-tab unseen dot keeps working regardless of
// which tab is open. See src/agent/agentStore.ts.
const agentStore = new AgentStore();

/** `file_path` from an Edit/Write tool input is cwd-absolute (cwd = sketch
 *  dir); reduce it to the same rel_path shape the file tree/editor use. */
const relativeToSketchDir = (filePath: string, dir: string): string => {
  const prefix = dir.endsWith("/") ? dir : `${dir}/`;
  return filePath.startsWith(prefix) ? filePath.slice(prefix.length) : filePath;
};

export default function App() {
  // project
  const [sketchDir, setSketchDir] = useState<string | null>(null);
  // File listing lives in the explorer store (tree state travels with it);
  // setFiles below is the store action, same call sites as before.
  const files = useExplorerStore((s) => s.files);
  const setFiles = useExplorerStore((s) => s.setFiles);
  const [sketchYaml, setSketchYaml] = useState<SketchYaml | null>(null);
  const [profile, setProfile] = useState<string | null>(null);
  /** When true the editor area shows the New Project form instead. */
  const [creatingProject, setCreatingProject] = useState(false);
  /** When true a one-row profile-bootstrap form shows under the toolbar. */
  const [creatingProfile, setCreatingProfile] = useState(false);
  // Computed once per opened project (spec Risk R4): the Assistant panel
  // warns when there's no undo path for its auto-applied edits.
  const [gitWarning, setGitWarning] = useState(false);
  // Live mirror of sketchDir for the App-level agent event listeners, which
  // are registered once (empty-dep effect) and would otherwise close over a
  // stale `null`.
  const sketchDirRef = useRef<string | null>(null);
  sketchDirRef.current = sketchDir;

  // editor — unsaved edits live in the buffer map keyed by rel_path; disk is
  // the source of truth for clean files.
  const [openFile, setOpenFile] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirtyFiles, setDirtyFiles] = useState<Set<string>>(new Set());
  const buffersRef = useRef(new Map<string, string>());
  // Same story as sketchDirRef: mirrors `openFile` for the agent listeners.
  const openFileRef = useRef<string | null>(null);
  openFileRef.current = openFile;
  /** rel_paths the agent edited while a local buffer was dirty — sendToAgent
   *  refuses until the conflict is resolved (cleared inside saveAll()). */
  const agentConflictsRef = useRef<Set<string>>(new Set());
  /** Render mirror of `agentConflictsRef` — the ref is written from the
   *  agent event listener (registered once, no access to state), so the
   *  banner needs a state copy kept in step with it. */
  const [conflicts, setConflicts] = useState<string[]>([]);

  // boards
  const [ports, setPorts] = useState<DetectedPort[]>([]);
  const [selectedPort, setSelectedPort] = useState<string | null>(null);

  // consoles
  const [buildLines, setBuildLines] = useState<OutputLine[]>([]);
  const [serialLines, setSerialLines] = useState<OutputLine[]>([]);
  const [monitorOn, setMonitorOn] = useState(false);
  const [baudrate, setBaudrate] = useState(115200);
  // Live mirror of monitorOn for callbacks captured by timers (post-upload
  // auto-resume fires from a closure created while the monitor was still on).
  const monitorOnRef = useRef(false);
  monitorOnRef.current = monitorOn;
  // New-content dots on the bottom tabs: set when lines arrive for a hidden
  // tab, cleared when that tab is opened; a group button carries the dot for
  // its hidden tabs. After flashing, the first serial line inside the
  // auto-open window pulls the Serial Monitor tab forward.
  const [unseen, setUnseen] = useState<Partial<Record<BottomTab, boolean>>>({});
  const bottomTabRef = useRef<BottomTab>("build");
  const autoOpenSerialUntilRef = useRef(0);

  // ui — sidebar hierarchy: a Software/Hardware group switcher over per-group
  // sub-tabs; each group remembers its last-used tab.
  const [sideGroup, setSideGroup] = useState<SideGroup>("software");
  const [softwareTab, setSoftwareTab] = useState<SoftwareTab>("files");
  const [hardwareTab, setHardwareTab] = useState<HardwareTab>("boards");
  const sideTab = sideGroup === "software" ? softwareTab : hardwareTab;
  // Bottom panel hierarchy: Console | Debugging | Observability groups over
  // per-group sub-tabs, same memory pattern as the sidebar.
  const [bottomGroup, setBottomGroup] = useState<BottomGroup>("console");
  const [debugTab, setDebugTab] = useState<"serial" | "scope">("serial");
  const [obsTab, setObsTab] = useState<"mqtt" | "ws">("mqtt");
  const bottomTab: BottomTab =
    bottomGroup === "console"
      ? "build"
      : bottomGroup === "debug"
        ? debugTab
        : bottomGroup === "obs"
          ? obsTab
          : "agent";
  // Bottom panel expanded over the whole main area (editor stays mounted).
  const [bottomMax, setBottomMax] = useState(false);
  const [bottomHeight, setBottomHeight] = useState(() => {
    const saved = Number(localStorage.getItem(BOTTOM_HEIGHT_KEY));
    return Number.isFinite(saved) && saved >= BOTTOM_MIN
      ? clampBottomHeight(saved)
      : BOTTOM_DEFAULT;
  });
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const saved = Number(localStorage.getItem(SIDEBAR_WIDTH_KEY));
    return Number.isFinite(saved) && saved >= SIDEBAR_MIN
      ? clampSidebarWidth(saved)
      : SIDEBAR_DEFAULT;
  });
  const [sidebarCollapsed, setSidebarCollapsed] = useState(
    () => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1",
  );
  // Live-connection panels stay mounted once opened so streams/subscriptions
  // survive switching to another bottom tab.
  const [scopeMounted, setScopeMounted] = useState(false);
  const [mqttMounted, setMqttMounted] = useState(false);
  const [wsMounted, setWsMounted] = useState(false);
  const [agentMounted, setAgentMounted] = useState(false);
  /** A *user* action (Verify, Upload, scope firmware flash) is in flight. */
  const [userBusy, setUserBusy] = useState(false);
  /**
   * The agent's MCP `verify` is compiling (between `verify_started` and
   * `verify_done`).
   *
   * Deliberately a second flag rather than a second writer of the same one:
   * both used to call `setBusy`, so a user Verify click that failed fast on
   * the build gate ran its `finally { setBusy(false) }` while the agent's
   * compile was still going — clearing `busy` under the agent and
   * re-opening the port-rescan-during-build hazard the flag exists to
   * prevent. Two owners, OR'd into one effective value, cannot clear each
   * other's state.
   */
  const [agentBuilding, setAgentBuilding] = useState(false);
  const busy = userBusy || agentBuilding;
  const [status, setStatus] = useState("Bancada ready — open a sketch folder.");
  const [statusIsError, setStatusIsError] = useState(false);

  // Hotplug plumbing. busyRef mirrors `busy` for the event listener;
  // pendingScanRef queues a rescan that arrived mid-flash; prevOnlineRef
  // is null until the first fleet sync so launch-time boards aren't
  // announced as arrivals.
  const busyRef = useRef(false);
  const pendingScanRef = useRef(false);
  const scanTimerRef = useRef<number | undefined>(undefined);
  const prevOnlineRef = useRef<string[] | null>(null);

  const notify = useCallback((msg: string, isError = false) => {
    setStatus(msg);
    setStatusIsError(isError);
  }, []);

  useEffect(() => {
    busyRef.current = busy;
    if (!busy && pendingScanRef.current) {
      pendingScanRef.current = false;
      // `busy` clears before a just-flashed native-USB board finishes
      // re-enumerating; scanning immediately would read the port's brief
      // absence as a detach and drop the selection. Wait out the settle
      // window (same reasoning as the post-flash monitor restart).
      window.setTimeout(refreshPorts, 1500);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy]);

  // Opening a tab marks its content as seen.
  useEffect(() => {
    bottomTabRef.current = bottomTab;
    setUnseen((u) => (u[bottomTab] ? { ...u, [bottomTab]: false } : u));
  }, [bottomTab]);

  /** Open a bottom tab: routes to its group, remembers it there, mounts
   *  live panels on first open. Every former setBottomTab caller uses this. */
  const openBottomTab = useCallback((tab: BottomTab) => {
    const g = GROUP_OF[tab];
    setBottomGroup(g);
    if (g === "debug") setDebugTab(tab as "serial" | "scope");
    if (g === "obs") setObsTab(tab as "mqtt" | "ws");
    if (tab === "scope") setScopeMounted(true);
    if (tab === "mqtt") setMqttMounted(true);
    if (tab === "ws") setWsMounted(true);
    if (tab === "agent") setAgentMounted(true);
  }, []);

  /** Drag the handle right of the sidebar to resize it (dbl-click resets). */
  const startSidebarResize = (e: React.PointerEvent<HTMLDivElement>) => {
    const startX = e.clientX;
    const startW = sidebarWidth;
    const el = e.currentTarget;
    let w = startW;
    el.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent) => {
      w = clampSidebarWidth(startW + (ev.clientX - startX));
      setSidebarWidth(w);
    };
    const onUp = () => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(w));
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
  };

  const setCollapsed = (next: boolean) => {
    setSidebarCollapsed(next);
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, next ? "1" : "0");
  };

  /** Drag the handle above the bottom panel to resize it (dbl-click resets). */
  const startPanelResize = (e: React.PointerEvent<HTMLDivElement>) => {
    const startY = e.clientY;
    const startH = bottomHeight;
    const el = e.currentTarget;
    let h = startH;
    el.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent) => {
      h = clampBottomHeight(startH + (startY - ev.clientY));
      setBottomHeight(h);
    };
    const onUp = () => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      localStorage.setItem(BOTTOM_HEIGHT_KEY, String(h));
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
  };

  // ---------- event subscriptions ----------

  useEffect(() => {
    const subs = [
      api.onBuildLine((l) => {
        setBuildLines((prev) => appendCapped(prev, l));
        if (bottomTabRef.current !== "build")
          setUnseen((u) => ({ ...u, build: true }));
      }),
      api.onSerialLine((l) => {
        setSerialLines((prev) => appendCapped(prev, l));
        if (Date.now() < autoOpenSerialUntilRef.current) {
          // fresh output from a just-flashed sketch — bring the monitor forward
          autoOpenSerialUntilRef.current = 0;
          openBottomTab("serial");
        } else if (bottomTabRef.current !== "serial") {
          setUnseen((u) => ({ ...u, serial: true }));
        }
      }),
      api.onSerialClosed(() => setMonitorOn(false)),
      api.onPortsChanged(() => {
        if (busyRef.current) {
          // arduino-cli probing ports mid-flash can disrupt esptool — defer.
          pendingScanRef.current = true;
          return;
        }
        // USB enumeration surfaces sibling ports a beat apart; coalesce.
        window.clearTimeout(scanTimerRef.current);
        scanTimerRef.current = window.setTimeout(refreshPorts, 500);
      }),
      // Agent event plumbing lives here (App level), not inside AgentPanel:
      // the unseen dot on the Assistant tab must light up even before the
      // panel has ever been mounted (spec: agentStore is not panel-owned).
      api.onAgentEvent((ev) => {
        agentStore.push(ev);
        if (bottomTabRef.current !== "agent")
          setUnseen((u) => ({ ...u, agent: true }));
        handleAgentSideEffects(ev);
      }),
      api.onAgentClosed((p) => {
        // `p.pid` names the child that exited. The store ignores a close for
        // a session it is no longer showing (FE-C1): `newAgentSession()`
        // stops without awaiting, so session A's stdout EOF can land after
        // session B is already live and would otherwise flip B to "ended"
        // — Send disabled, panel saying "Session ended", child alive and
        // still streaming.
        agentStore.closed(p.reason, p.pid);
        // Belt and braces: the backend's own stdout reader already reaps its
        // session at EOF (so a closed window no longer leaks a listener), but
        // this stays as the second path. Pid-scoped for the same reason —
        // `agent_stop(pid)` is a no-op unless pid still matches the live
        // session, so a stale close can never kill the new one (F4).
        api.agentStop(p.pid).catch(() => {});
      }),
    ];
    api
      .cliVersion()
      .then((v) => notify(`arduino-cli ${v} detected.`))
      .catch(() =>
        notify(
          "arduino-cli not found on PATH — install it to enable builds.",
          true,
        ),
      );
    refreshPorts();
    // Restore the last session's sketch and open file, if they still exist.
    api
      .loadSettings()
      .then(async (s) => {
        if (!s.last_sketch_dir) return;
        const ok = await loadSketch(s.last_sketch_dir, s.last_open_file ?? undefined);
        if (!ok) notify("Last sketch no longer available — open a sketch folder.");
      })
      .catch(() => {});
    return () => {
      subs.forEach((p) => p.then((un) => un()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- project ----------

  const refreshPorts = () =>
    api
      .listBoards()
      .then((ps) => {
        setPorts(ps);
        // Keep a still-attached choice, but drop one whose port has gone: a
        // rescan that leaves a vanished port selected looks like it did nothing
        // and aims the next upload at a device that is no longer there.
        setSelectedPort((cur) => nextSelectedPort(ps, cur));
        // Enrol whatever is attached into the fleet, whether or not its panel is
        // open — plugging a board in is what should record it. Fire-and-forget:
        // a fleet write must never break port detection.
        api
          .fleetSync(ps)
          .then((snap) => {
            const fresh = arrivals(prevOnlineRef.current, snap);
            prevOnlineRef.current = snap.online;
            if (fresh.length > 0)
              notify(
                `⚡ ${fresh
                  .map((a) => (a.port ? `${a.name} (${a.port})` : a.name))
                  .join(", ")} attached`,
              );
          })
          .catch(() => {});
      })
      .catch((e) => notify(String(e), true));

  /** Switch profile; if it pins a port in sketch.yaml, select that port too. */
  const selectProfile = (p: string) => {
    setProfile(p);
    const pinned = sketchYaml?.profiles?.[p]?.port;
    if (pinned) setSelectedPort(pinned);
  };

  const openSketch = async () => {
    const dir = await open({ directory: true, title: "Open sketch folder" });
    if (typeof dir !== "string") return;
    await loadSketch(dir);
  };

  /** Load a sketch folder; opens `restoreFile` when present, else the main .ino. */
  const loadSketch = async (dir: string, restoreFile?: string): Promise<boolean> => {
    try {
      const [fs, yaml] = await Promise.all([
        api.listSketchFiles(dir),
        api.loadSketchYaml(dir),
      ]);
      setSketchDir(dir);
      setFiles(fs);
      setSketchYaml(yaml);
      // Fire-and-forget, same as fleetSync above: a hint, not load-bearing.
      api
        .sketchHasGit(dir)
        .then((has) => setGitWarning(!has))
        .catch(() => setGitWarning(false));
      const profiles = Object.keys(yaml.profiles ?? {});
      const prof = yaml.default_profile ?? profiles[0] ?? null;
      setProfile(prof);
      const profPort = prof ? yaml.profiles?.[prof]?.port : undefined;
      if (profPort) setSelectedPort(profPort);
      buffersRef.current = new Map();
      setDirtyFiles(new Set());
      setOpenFile(null);
      setContent("");
      setCreatingProfile(false);
      const name = dir.split("/").pop();
      const target =
        (restoreFile && fs.find((f) => f.rel_path === restoreFile)) ||
        fs.find((f) => f.rel_path === `${name}.ino`);
      if (target) {
        await openFileInEditor(dir, target.rel_path);
      } else {
        api
          .saveSettings({ last_sketch_dir: dir, last_open_file: null })
          .catch(() => {});
      }
      notify(`Opened ${dir}`);
      return true;
    } catch (e) {
      notify(String(e), true);
      return false;
    }
  };

  const openFileInEditor = async (dir: string, relPath: string) => {
    try {
      const text =
        buffersRef.current.get(relPath) ??
        (await api.readSketchFile(dir, relPath));
      setOpenFile(relPath);
      setContent(text);
      // keep the tree oriented: reveal the file's folder chain
      useExplorerStore.getState().expandTo(relPath);
      useExplorerStore.getState().select(relPath);
      api
        .saveSettings({ last_sketch_dir: dir, last_open_file: relPath })
        .catch(() => {});
    } catch (e) {
      notify(String(e), true);
    }
  };

  /** Create a validated entry via the backend (which refuses collisions and
   *  returns the fresh listing), then reveal it — files also open. */
  const doCreate = async (relPath: string, kind: "file" | "dir") => {
    if (!sketchDir) return;
    try {
      const fs =
        kind === "file"
          ? await api.createSketchFile(sketchDir, relPath)
          : await api.createSketchDir(sketchDir, relPath);
      setFiles(fs);
      useExplorerStore.getState().expandTo(relPath);
      if (kind === "file") await openFileInEditor(sketchDir, relPath);
      else useExplorerStore.getState().select(relPath);
      notify(`Created ${relPath}`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  /** Tree create row (context menu → New File / New Folder). */
  const handleCreateEntry = (
    raw: string,
    parentDir: string,
    kind: "file" | "dir",
  ): boolean => {
    if (!sketchDir) return false;
    const check = checkNewEntry(raw, parentDir, files);
    if (!check.ok) {
      notify(check.reason, true);
      return false;
    }
    void doCreate(check.relPath, kind);
    return true;
  };

  /** Editor tab strip ＋ — files only, root-relative. */
  const createNewFile = (raw: string): boolean => {
    if (!sketchDir) return false;
    const check = checkNewFile(raw, files);
    if (!check.ok) {
      notify(check.reason, true);
      return false;
    }
    void doCreate(check.relPath, "file");
    return true;
  };

  /** Rename or move (drag-drop resolves here too). Remaps buffers, dirty
   *  flags and the open file so unsaved edits survive under the new path. */
  const handleRename = async (from: string, to: string): Promise<boolean> => {
    if (!sketchDir) return false;
    const check = checkRename(from, to, files, sketchDir);
    if (!check.ok) {
      notify(check.reason, true);
      return false;
    }
    const wasDir = files.some((f) => f.rel_path === from && f.is_dir);
    try {
      const fs = await api.renameSketchEntry(sketchDir, from, to);
      useExplorerStore.getState().setFilesAfterRename(fs, from, to);
      const buffers = buffersRef.current;
      for (const [p, text] of [...buffers]) {
        const np = pathAfterRename(p, from, to, wasDir);
        if (np !== null) {
          buffers.delete(p);
          buffers.set(np, text);
        }
      }
      setDirtyFiles(
        (prev) => new Set([...prev].map((p) => pathAfterRename(p, from, to, wasDir) ?? p)),
      );
      if (openFile) {
        const np = pathAfterRename(openFile, from, to, wasDir);
        if (np !== null) {
          setOpenFile(np);
          api
            .saveSettings({ last_sketch_dir: sketchDir, last_open_file: np })
            .catch(() => {});
        }
      }
      notify(`Renamed ${from} → ${to}`);
      return true;
    } catch (e) {
      notify(String(e), true);
      return false;
    }
  };

  /** Trash an entry; non-empty folders ask first. If the open file dies,
   *  fall back to the main .ino. */
  const handleDelete = async (relPath: string) => {
    if (!sketchDir) return;
    const wasDir = files.some((f) => f.rel_path === relPath && f.is_dir);
    if (wasDir && isNonEmptyDir(files, relPath)) {
      const yes = await ask(`Move "${relPath}" and its contents to the trash?`, {
        title: "Delete folder",
        kind: "warning",
      });
      if (!yes) return;
    }
    try {
      const fs = await api.deleteSketchEntry(sketchDir, relPath);
      setFiles(fs);
      for (const p of [...buffersRef.current.keys()]) {
        if (affectedByDelete(p, relPath, wasDir)) buffersRef.current.delete(p);
      }
      setDirtyFiles(
        (prev) => new Set([...prev].filter((p) => !affectedByDelete(p, relPath, wasDir))),
      );
      if (openFile && affectedByDelete(openFile, relPath, wasDir)) {
        const name = sketchDir.split("/").pop();
        const main = fs.find((f) => f.rel_path === `${name}.ino`);
        if (main) {
          await openFileInEditor(sketchDir, main.rel_path);
        } else {
          setOpenFile(null);
          setContent("");
          api
            .saveSettings({ last_sketch_dir: sketchDir, last_open_file: null })
            .catch(() => {});
        }
      }
      notify(`Moved ${relPath} to the trash`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const saveCurrent = useCallback(async () => {
    if (!sketchDir || !openFile) return;
    const text = buffersRef.current.get(openFile);
    if (text === undefined) return; // no unsaved edits
    try {
      await api.writeSketchFile(sketchDir, openFile, text);
      buffersRef.current.delete(openFile);
      setDirtyFiles((prev) => {
        const next = new Set(prev);
        next.delete(openFile);
        return next;
      });
      // Same resolution point saveAll() is for the whole conflict set (F1):
      // this file's buffer is no longer unsaved, so it's no longer a
      // conflict either. Ctrl+S is the documented way to resolve one, so it
      // has to actually do it.
      agentConflictsRef.current.delete(openFile);
      setConflicts([...agentConflictsRef.current]);
      notify(`Saved ${openFile}`);
    } catch (e) {
      notify(String(e), true);
    }
  }, [sketchDir, openFile, notify]);

  /** Flush every dirty buffer to disk (before compile/upload, or before a
   *  message to the agent), **except** buffers flagged as agent conflicts.
   *
   *  A conflict means the agent rewrote a file on disk while the user had
   *  unsaved edits to it, and the banner tells the user to save (Ctrl+S) to
   *  resolve. But `saveAll` has three callers, and only `sendToAgent`
   *  checked `agentConflictsRef` — so a user who clicked Verify or Upload
   *  instead of saving would silently write the *stale* buffer over the
   *  agent's on-disk fix, "resolve" the conflict by clobbering it, and then
   *  compile (or flash!) the pre-fix code. Skipping conflicted paths here
   *  rather than gating in each caller means no future caller can clobber
   *  by forgetting to check: the conflicted file stays dirty and conflicted,
   *  every other buffer still flushes, and `saveCurrent` (Ctrl+S) remains
   *  the one deliberate way to resolve it. */
  const saveAll = useCallback(async () => {
    if (!sketchDir) return;
    const conflicted = agentConflictsRef.current;
    const skipped: string[] = [];
    for (const [path, text] of buffersRef.current) {
      if (conflicted.has(path)) {
        skipped.push(path);
        continue;
      }
      await api.writeSketchFile(sketchDir, path, text);
      // Safe during `for...of` over a Map: deleting the current key does
      // not disturb the iteration order of the ones still to come.
      buffersRef.current.delete(path);
    }
    // A conflict only exists while there is an unsaved buffer to lose. Drop
    // any whose buffer is gone (saved via Ctrl+S, or the file closed), so a
    // stale entry cannot wedge `sendToAgent` forever.
    for (const path of [...conflicted]) {
      if (!buffersRef.current.has(path)) conflicted.delete(path);
    }
    setDirtyFiles(new Set(skipped));
    // Mirror the ref into state so the banner renders. The ref stays the
    // source of truth for the synchronous guards below (it is written from
    // the agent event listener, which has no access to state).
    setConflicts([...conflicted]);
    if (skipped.length > 0) {
      notify(conflictMessage(skipped), true);
    }
  }, [sketchDir, notify]);

  /** Refuse a build/flash while the agent's edits and the user's disagree.
   *
   *  `saveAll` already declines to overwrite a conflicted buffer, but that
   *  alone is not enough for Verify/Upload: its warning is a transient
   *  `notify`, immediately overwritten by "Compiling…", and the build then
   *  ran anyway. The user would see a compile — or a **flash** — of the
   *  agent's on-disk version while their own unsaved edits were invisible
   *  and unmentioned. Both callers now stop here, the same way
   *  `sendToAgent` already did. */
  const refuseOnConflict = (action: string): boolean => {
    const block = blockedByConflict([...agentConflictsRef.current], action);
    if (block.blocked) notify(block.message ?? "", true);
    return block.blocked;
  };

  // Ctrl+S
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        saveCurrent();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [saveCurrent]);

  // ---------- build & flash ----------

  /** FQBN of the board detected on the selected port, if any. */
  const detectedFqbn = () =>
    ports.find((p) => p.port.address === selectedPort)?.matching_boards[0]
      ?.fqbn;

  /** Build target: sketch.yaml profile first, detected board FQBN as fallback. */
  const resolveTarget = (): { profile?: string; fqbn?: string } | null => {
    if (profile) return { profile };
    const fqbn = detectedFqbn();
    if (fqbn) return { fqbn };
    notify(
      "No sketch.yaml profile and no board detected — select a port with a recognized board.",
      true,
    );
    return null;
  };

  const verify = async () => {
    if (!sketchDir) return;
    const target = resolveTarget();
    if (!target) return;
    await saveAll();
    // After saveAll, not before: saveAll is what discovers which buffers it
    // could not flush.
    if (refuseOnConflict("Compile")) return;
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    notify("Compiling…");
    try {
      const r = await api.compileSketch(sketchDir, target.profile, target.fqbn);
      notify(r.success ? "✓ Compile OK" : "Compile failed", !r.success);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
    }
  };

  const upload = async () => {
    if (!sketchDir || !selectedPort) return;
    const target = resolveTarget();
    if (!target) return;
    await saveAll();
    // Before freeing the serial port and before anything reaches the board:
    // flashing the agent's on-disk version while the user's edits sit
    // unsaved and unmentioned is the worst outcome this guard prevents.
    if (refuseOnConflict("Upload")) return;
    if (monitorOn) await toggleMonitor(); // free the serial port
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    notify(`Building and flashing to ${selectedPort}…`);
    try {
      // Compiles as part of the flash — a sketch that fails to build stops
      // here with its compiler error and never reaches the board.
      const r = await api.uploadSketch(
        sketchDir,
        selectedPort,
        target.profile,
        target.fqbn,
      );
      notify(
        r.success
          ? `✓ Flashed via ${selectedPort}`
          : "Build or upload failed — see the Build console",
        !r.success,
      );
      // Resume capturing (native-USB boards re-enumerate after flashing,
      // so give the port a moment to come back). If the fresh sketch prints
      // anything within the window, the Serial Monitor tab opens itself.
      if (r.success) {
        autoOpenSerialUntilRef.current = Date.now() + 15000;
        // The FQBN this board was actually built for, for its fleet record.
        const usedFqbn =
          target.fqbn ??
          (target.profile
            ? sketchYaml?.profiles?.[target.profile]?.fqbn
            : undefined);
        setTimeout(() => {
          startMonitorQuiet();
          // Recorded after the re-enumeration wait for the same reason the
          // monitor is: the port has to be back before it can be resolved to a
          // board. Fire-and-forget — a good flash must not fail over this.
          if (usedFqbn) {
            api.noteBoardFqbn(selectedPort, usedFqbn).catch(() => {});
          }
        }, 1200);
      }
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
    }
  };

  // ---------- serial monitor ----------

  /** Best-effort monitor start (auto-capture); errors stay off the status bar. */
  const startMonitorQuiet = useCallback(async () => {
    if (monitorOnRef.current || !selectedPort) return;
    try {
      setSerialLines([]);
      await api.startMonitor(selectedPort, baudrate);
      setMonitorOn(true);
    } catch {
      /* port busy or gone — user can start manually */
    }
  }, [selectedPort, baudrate]);

  // Capture by default: start the monitor whenever a port is (auto-)selected.
  useEffect(() => {
    startMonitorQuiet();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedPort]);

  const toggleMonitor = async () => {
    try {
      if (monitorOn) {
        await api.stopMonitor();
        setMonitorOn(false);
      } else {
        if (!selectedPort) {
          notify("Select a port first", true);
          return;
        }
        setSerialLines([]);
        await api.startMonitor(selectedPort, baudrate);
        setMonitorOn(true);
        openBottomTab("serial");
      }
    } catch (e) {
      notify(String(e), true);
    }
  };

  // ---------- scope ----------

  /** Start the serial monitor if it is off (plotter source needs it). */
  const ensureMonitor = useCallback(async () => {
    if (monitorOn) return;
    if (!selectedPort) {
      notify("Select a port first", true);
      return;
    }
    setSerialLines([]);
    await api.startMonitor(selectedPort, baudrate);
    setMonitorOn(true);
    openBottomTab("serial");
  }, [monitorOn, selectedPort, baudrate, notify]);

  /** Stop the serial monitor if it is on (ADC streaming needs the port). */
  const stopMonitorIfOn = useCallback(async () => {
    if (!monitorOn) return;
    await api.stopMonitor();
    setMonitorOn(false);
  }, [monitorOn]);

  /** Compile + flash the scope companion firmware sketch (build console shows progress). */
  const flashScopeFirmware = useCallback(
    async (dir: string, chipProfile: string): Promise<boolean> => {
      if (!selectedPort) {
        notify("Select a port first", true);
        return false;
      }
      await stopMonitorIfOn().catch(() => {});
      setBuildLines([]);
      openBottomTab("build");
      setUserBusy(true);
      notify("Compiling companion firmware…");
      try {
        const c = await api.compileSketch(dir, chipProfile);
        if (!c.success) {
          notify("Companion firmware compile failed", true);
          return false;
        }
        notify(`Flashing companion firmware to ${selectedPort}…`);
        const u = await api.uploadSketch(dir, selectedPort, chipProfile);
        notify(
          u.success
            ? "✓ Companion firmware flashed"
            : "Companion firmware upload failed",
          !u.success,
        );
        return u.success;
      } catch (e) {
        notify(String(e), true);
        return false;
      } finally {
        setUserBusy(false);
      }
    },
    [selectedPort, stopMonitorIfOn, notify],
  );

  // ---------- agent (Assistant panel) ----------

  /** Refresh the tree and, if the agent touched the file open in the editor,
   *  either pull the fresh content in or flag a conflict — called from
   *  `handleAgentSideEffects`, which only has refs (registered once, on
   *  mount) so this reads sketchDirRef/openFileRef/buffersRef, never state
   *  directly. */
  const handleAgentFileChange = (filePath: string) => {
    const dir = sketchDirRef.current;
    if (!dir) return;
    api
      .listSketchFiles(dir)
      .then(setFiles)
      .catch(() => {
        /* best-effort — a missed tree refresh isn't worth surfacing */
      });

    const rel = relativeToSketchDir(filePath, dir);
    if (rel !== openFileRef.current) return;
    if (buffersRef.current.has(rel)) {
      // The user has unsaved edits to the same file the agent just changed
      // on disk — reading it in now would silently discard one side. There
      // is no "discard" action in this editor (a dirty buffer always wins on
      // reopen), so the only real way out is to save it.
      agentConflictsRef.current.add(rel);
      setConflicts([...agentConflictsRef.current]);
      notify(
        `The assistant edited ${rel} while you had unsaved changes — save the file (Ctrl+S) to resolve the conflict before sending another message.`,
        true,
      );
      return;
    }
    api
      .readSketchFile(dir, rel)
      .then(setContent)
      .catch((e) => notify(String(e), true));
  };

  /** Reacts to the raw agent://event stream: build-gate `busy` coupling, and
   *  pulling in (or flagging a conflict for) an Edit/Write the agent just
   *  applied to the file open in the editor. Registered once (empty-dep
   *  mount effect) — must not close over component state, only refs and
   *  stable setters/callbacks. */
  const handleAgentSideEffects = (ev: AgentEvent) => {
    // Only this session's verify events move the flag: one from a session
    // the user already stopped would otherwise leave the toolbar disabled
    // (verify_started with no matching done) or clear it under a live build
    // (a stale done). `agentStore` applies the same guard to what it renders.
    if (ev.type === "verify_started" || ev.type === "verify_done") {
      const pid = typeof ev.pid === "number" ? ev.pid : undefined;
      const ours = agentStore.snapshot().pid;
      if (pid !== undefined && ours !== undefined && pid !== ours) return;
      setAgentBuilding(ev.type === "verify_started");
      return;
    }
    // The host has already killed the child, so no verify_done is coming.
    if (ev.type === "security_alarm") {
      setAgentBuilding(false);
      return;
    }
    if (ev.type !== "user") return;
    const content = ev.message?.content;
    if (!Array.isArray(content)) return;
    for (const block of content) {
      if (block.type !== "tool_result") continue;
      const toolUseId = block.tool_use_id;
      if (typeof toolUseId !== "string") continue;
      const msg = agentStore
        .snapshot()
        .messages.find((m) => m.kind === "tool" && m.id === toolUseId);
      if (!msg || msg.kind !== "tool") continue;
      if (msg.name !== "Edit" && msg.name !== "Write") continue;
      // A failed Edit/Write (e.g. old_string not found) never touched the
      // file — treating it as a change would raise a false-positive conflict
      // on a file the agent never actually wrote.
      if (msg.status === "error") continue;
      const input = msg.input;
      const filePath =
        typeof input === "object" && input !== null
          ? (input as Record<string, unknown>).file_path
          : undefined;
      if (typeof filePath !== "string") continue;
      handleAgentFileChange(filePath);
    }
  };

  /** Save-then-send, auto-starting the session on the first message with the
   *  same target Verify uses. Refuses while a dirty-conflict is unresolved.
   *  The auto-start (target resolution + `agentStart`) happens *before*
   *  `userSent` — failing to resolve a target or spawn the child must not
   *  leave a phantom "sent" bubble with nothing behind it. */
  const sendToAgent = async (text: string) => {
    if (agentConflictsRef.current.size > 0) {
      notify(
        "Resolve the assistant's file conflict first — save the file (Ctrl+S), then try again.",
        true,
      );
      return;
    }
    if (!sketchDir) {
      notify("Open a sketch first.", true);
      return;
    }
    await saveAll();
    // saveAll refuses to flush a conflicted buffer, so re-check: a conflict
    // raised between the guard above and here still blocks the send.
    if (agentConflictsRef.current.size > 0) return; // saveAll already notified
    if (agentStore.snapshot().status === "idle") {
      const target = resolveTarget();
      if (!target) return; // resolveTarget already notified
      try {
        // The pid identifies this session for every host event that can
        // outlive it (FE-C1) — record it before any of them can arrive.
        const pid = await api.agentStart(sketchDir, target.profile, target.fqbn);
        agentStore.sessionStarted(pid);
      } catch (e) {
        notify(String(e), true);
        return;
      }
    }
    agentStore.userSent(text);
    try {
      await api.agentSend(text);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const interruptAgent = () => {
    api.agentInterrupt().catch((e) => notify(String(e), true));
  };

  /** Stop the session and drop the transcript back to a clean slate. */
  const newAgentSession = () => {
    api.agentStop().catch(() => {});
    agentStore.clear();
    agentConflictsRef.current.clear();
    setConflicts([]);
    // The stopped session's verify (if any) will never emit `verify_done`
    // now — the backend cancels it — so release the flag here rather than
    // leaving the toolbar disabled forever.
    setAgentBuilding(false);
  };

  // ---------- render ----------

  return (
    <div className="app">
      <Toolbar
        sketchDir={sketchDir}
        sketchYaml={sketchYaml}
        profile={profile}
        ports={ports}
        selectedPort={selectedPort}
        busy={busy}
        onOpenSketch={openSketch}
        onNewProject={() => setCreatingProject(true)}
        onCreateProfile={() => setCreatingProfile(true)}
        onSelectProfile={selectProfile}
        onSelectPort={setSelectedPort}
        onRefreshPorts={refreshPorts}
        onVerify={verify}
        onUpload={upload}
      />

      {creatingProfile && sketchDir && (
        <ProfileInit
          sketchDir={sketchDir}
          detectedFqbn={detectedFqbn() ?? null}
          onCreated={(yaml, prof) => {
            setSketchYaml(yaml);
            setProfile(prof);
            setCreatingProfile(false);
          }}
          onCancel={() => setCreatingProfile(false)}
          notify={notify}
        />
      )}

      {/* Persistent, because the status bar is not. `saveAll` used to warn
          about an unflushed conflicted buffer with a transient notify that
          "Compiling…" overwrote a moment later, leaving the user looking at
          a build of the assistant's on-disk version with no sign their own
          edits were still unsaved. This stays until the conflict is
          actually resolved, and Verify/Upload refuse while it is up. */}
      {conflicts.length > 0 && (
        <div className="conflict-banner" role="alert">
          <span className="conflict-banner-icon" aria-hidden="true">
            ⚠
          </span>
          <span>
            <strong>Unresolved edit conflict</strong> — {conflictMessage(conflicts)}{" "}
            Compile and Upload are blocked until then.
          </span>
        </div>
      )}

      <div className="main" style={bottomMax ? { display: "none" } : undefined}>
        {/* Collapsed by `display: none` rather than unmounted — the same trick
            `.main` uses when the bottom panel is maximized — so a search, a
            fleet list or a half-filled form survives hiding the pane. */}
        <aside
          className="sidebar"
          style={
            sidebarCollapsed ? { display: "none" } : { width: sidebarWidth }
          }
        >
          <div className="side-groups">
            <button
              className={
                sideGroup === "software"
                  ? "side-group-btn active"
                  : "side-group-btn"
              }
              onClick={() => setSideGroup("software")}
              title="The sketch: files and libraries"
            >
              ▦ Software
            </button>
            <button
              className={
                sideGroup === "hardware"
                  ? "side-group-btn active"
                  : "side-group-btn"
              }
              onClick={() => setSideGroup("hardware")}
              title="The bench: board platforms and your device fleet"
            >
              ⚙ Hardware
            </button>
            <button
              className="side-collapse-btn"
              onClick={() => setCollapsed(true)}
              title="Collapse the sidebar"
              aria-label="Collapse the sidebar"
            >
              ‹
            </button>
          </div>
          <div className="panel-tabs">
            {sideGroup === "software" ? (
              <>
                <button
                  className={sideTab === "files" ? "tab active" : "tab"}
                  onClick={() => setSoftwareTab("files")}
                >
                  Files
                </button>
                <button
                  className={sideTab === "libraries" ? "tab active" : "tab"}
                  onClick={() => setSoftwareTab("libraries")}
                >
                  Libraries
                </button>
              </>
            ) : (
              <>
                <button
                  className={sideTab === "boards" ? "tab active" : "tab"}
                  onClick={() => setHardwareTab("boards")}
                  title="Install and update board platforms (arduino-cli cores)"
                >
                  Boards
                </button>
                <button
                  className={sideTab === "fleet" ? "tab active" : "tab"}
                  onClick={() => setHardwareTab("fleet")}
                  title="The physical boards Bancada has seen, remembered by MAC address"
                >
                  Fleet
                </button>
              </>
            )}
          </div>
          {sideTab === "files" && (
            <FileTree
              sketchDir={sketchDir}
              openFile={openFile}
              dirtyFiles={dirtyFiles}
              onOpen={(p) => sketchDir && openFileInEditor(sketchDir, p)}
              onCreateEntry={handleCreateEntry}
              onRenameTo={handleRename}
              onDelete={handleDelete}
            />
          )}
          {sideTab === "libraries" && (
            <LibraryManager
              sketchDir={sketchDir}
              profile={profile}
              onYamlChanged={setSketchYaml}
              notify={notify}
            />
          )}
          {sideTab === "boards" && (
            <BoardsManager
              sketchDir={sketchDir}
              profile={profile}
              sketchYaml={sketchYaml}
              onYamlChanged={setSketchYaml}
              onStreamStart={() => {
                setBuildLines([]);
                openBottomTab("build");
              }}
              notify={notify}
            />
          )}
          {sideTab === "fleet" && (
            <FleetManager
              ports={ports}
              onStreamStart={() => {
                setBuildLines([]);
                openBottomTab("build");
              }}
              notify={notify}
            />
          )}
        </aside>

        {sidebarCollapsed ? (
          <button
            className="sidebar-rail"
            onClick={() => setCollapsed(false)}
            title="Show sidebar"
            aria-label="Show sidebar"
          >
            ›
          </button>
        ) : (
          <div
            className="sidebar-resize-handle"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize sidebar"
            title="Drag to resize — double-click to reset"
            onPointerDown={startSidebarResize}
            onDoubleClick={() => {
              setSidebarWidth(SIDEBAR_DEFAULT);
              localStorage.setItem(SIDEBAR_WIDTH_KEY, String(SIDEBAR_DEFAULT));
            }}
          />
        )}

        <section className="editor-area">
          {creatingProject ? (
            <NewProject
              detectedFqbn={detectedFqbn() ?? null}
              onCreated={async (dir) => {
                setCreatingProject(false);
                await loadSketch(dir);
              }}
              onCancel={() => setCreatingProject(false)}
              notify={notify}
            />
          ) : (
            <>
          <EditorTabs
            files={files}
            openFile={openFile}
            dirtyFiles={dirtyFiles}
            onOpen={(p) => sketchDir && openFileInEditor(sketchDir, p)}
            onCreate={createNewFile}
          />
          <CodeMirror
            className="editor"
            value={content}
            height="100%"
            theme={oneDark}
            extensions={[cpp()]}
            onChange={(value) => {
              if (openFile) {
                buffersRef.current.set(openFile, value);
                setDirtyFiles((prev) => new Set(prev).add(openFile));
              }
            }}
            editable={!!openFile}
          />
            </>
          )}
        </section>
      </div>

      <section
        className={bottomMax ? "bottom maximized" : "bottom"}
        style={bottomMax ? undefined : { height: bottomHeight }}
      >
        {!bottomMax && (
          <div
            className="panel-resize-handle"
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize bottom panel"
            title="Drag to resize — double-click to reset"
            onPointerDown={startPanelResize}
            onDoubleClick={() => {
              setBottomHeight(BOTTOM_DEFAULT);
              localStorage.setItem(BOTTOM_HEIGHT_KEY, String(BOTTOM_DEFAULT));
            }}
          />
        )}
        <div className="panel-tabs">
          {(Object.keys(GROUP_TABS) as BottomGroup[]).map((g) => (
            <button
              key={g}
              className={
                bottomGroup === g
                  ? "bottom-group-btn active"
                  : "bottom-group-btn"
              }
              onClick={() =>
                openBottomTab(
                  g === "console"
                    ? "build"
                    : g === "debug"
                      ? debugTab
                      : g === "obs"
                        ? obsTab
                        : "agent",
                )
              }
            >
              {GROUP_LABEL[g]}
              {groupHasUnseen(g, bottomGroup, unseen) && (
                <span className="tab-dot">●</span>
              )}
            </button>
          ))}
          {GROUP_TABS[bottomGroup].length > 1 && (
            <>
              <div className="tab-sep" />
              {GROUP_TABS[bottomGroup].map((t) => (
                <button
                  key={t}
                  className={bottomTab === t ? "tab active" : "tab"}
                  onClick={() => openBottomTab(t)}
                >
                  {TAB_LABEL[t]}
                  {unseen[t] && <span className="tab-dot">●</span>}
                </button>
              ))}
            </>
          )}
          <div className="spacer" />
          {bottomTab === "serial" && (
            <>
              <select
                className="select small"
                value={baudrate}
                onChange={(e) => setBaudrate(Number(e.target.value))}
                disabled={monitorOn}
              >
                {[9600, 19200, 57600, 115200, 230400, 921600].map((b) => (
                  <option key={b} value={b}>
                    {b} baud
                  </option>
                ))}
              </select>
              <button className="btn small" onClick={toggleMonitor}>
                {monitorOn ? "Stop" : "Start"}
              </button>
            </>
          )}
          <button
            className="btn small icon"
            onClick={() => setBottomMax((m) => !m)}
            title={bottomMax ? "Restore panel" : "Maximize panel"}
          >
            {bottomMax ? "❐" : "⛶"}
          </button>
        </div>
        {bottomTab === "build" && (
          <Console lines={buildLines} onClear={() => setBuildLines([])} />
        )}
        {bottomTab === "serial" && (
          <Console
            lines={serialLines}
            onClear={() => setSerialLines([])}
            onSend={(d) => api.monitorSend(d).catch((e) => notify(String(e), true))}
          />
        )}
        {mqttMounted && (
          <MqttPanel active={bottomTab === "mqtt"} notify={notify} />
        )}
        {wsMounted && <WsPanel active={bottomTab === "ws"} notify={notify} />}
        {scopeMounted && (
          <ScopeView
            active={bottomTab === "scope"}
            selectedPort={selectedPort}
            busy={busy}
            monitorOn={monitorOn}
            baudrate={baudrate}
            notify={notify}
            onEnsureMonitor={ensureMonitor}
            onStopMonitor={stopMonitorIfOn}
            onFlashFirmware={flashScopeFirmware}
          />
        )}
        {agentMounted && (
          <AgentPanel
            active={bottomTab === "agent"}
            sketchDir={sketchDir}
            store={agentStore}
            onSend={sendToAgent}
            onInterrupt={interruptAgent}
            onNewSession={newAgentSession}
            openBottomTab={openBottomTab}
            gitWarning={gitWarning}
          />
        )}
      </section>

      <footer className={`statusbar ${statusIsError ? "error" : ""}`}>
        {busy ? "⏳ " : ""}
        {status}
      </footer>
    </div>
  );
}
