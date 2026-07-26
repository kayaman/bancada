import { useCallback, useEffect, useRef, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { cpp } from "@codemirror/lang-cpp";
import { oneDark } from "@codemirror/theme-one-dark";
import { open } from "@tauri-apps/plugin-dialog";

import * as api from "./api";
import { nextSelectedPort } from "./ports";
import type { DetectedPort, OutputLine, SketchFile, SketchYaml } from "./api";
import FileTree from "./components/FileTree";
import Toolbar from "./components/Toolbar";
import LibraryManager from "./components/LibraryManager";
import BoardsManager from "./components/BoardsManager";
import FleetManager from "./components/FleetManager";
import Console from "./components/Console";
import ScopeView from "./components/ScopeView";
import NewProject from "./components/NewProject";
import MqttPanel from "./components/MqttPanel";
import WsPanel from "./components/WsPanel";
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

export default function App() {
  // project
  const [sketchDir, setSketchDir] = useState<string | null>(null);
  const [files, setFiles] = useState<SketchFile[]>([]);
  const [sketchYaml, setSketchYaml] = useState<SketchYaml | null>(null);
  const [profile, setProfile] = useState<string | null>(null);
  /** When true the editor area shows the New Project form instead. */
  const [creatingProject, setCreatingProject] = useState(false);

  // editor — unsaved edits live in the buffer map keyed by rel_path; disk is
  // the source of truth for clean files.
  const [openFile, setOpenFile] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirtyFiles, setDirtyFiles] = useState<Set<string>>(new Set());
  const buffersRef = useRef(new Map<string, string>());

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
        : obsTab;
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
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("Bancada ready — open a sketch folder.");
  const [statusIsError, setStatusIsError] = useState(false);

  const notify = useCallback((msg: string, isError = false) => {
    setStatus(msg);
    setStatusIsError(isError);
  }, []);

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
        api.fleetSync(ps).catch(() => {});
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
      const profiles = Object.keys(yaml.profiles ?? {});
      const prof = yaml.default_profile ?? profiles[0] ?? null;
      setProfile(prof);
      const profPort = prof ? yaml.profiles?.[prof]?.port : undefined;
      if (profPort) setSelectedPort(profPort);
      buffersRef.current = new Map();
      setDirtyFiles(new Set());
      setOpenFile(null);
      setContent("");
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
      api
        .saveSettings({ last_sketch_dir: dir, last_open_file: relPath })
        .catch(() => {});
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
      notify(`Saved ${openFile}`);
    } catch (e) {
      notify(String(e), true);
    }
  }, [sketchDir, openFile, notify]);

  /** Flush every dirty buffer to disk (before compile/upload). */
  const saveAll = useCallback(async () => {
    if (!sketchDir) return;
    for (const [path, text] of buffersRef.current) {
      await api.writeSketchFile(sketchDir, path, text);
    }
    buffersRef.current.clear();
    setDirtyFiles(new Set());
  }, [sketchDir]);

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
    setBuildLines([]);
    openBottomTab("build");
    setBusy(true);
    notify("Compiling…");
    try {
      const r = await api.compileSketch(sketchDir, target.profile, target.fqbn);
      notify(r.success ? "✓ Compile OK" : "Compile failed", !r.success);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const upload = async () => {
    if (!sketchDir || !selectedPort) return;
    const target = resolveTarget();
    if (!target) return;
    await saveAll();
    if (monitorOn) await toggleMonitor(); // free the serial port
    setBuildLines([]);
    openBottomTab("build");
    setBusy(true);
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
      setBusy(false);
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
      setBusy(true);
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
        setBusy(false);
      }
    },
    [selectedPort, stopMonitorIfOn, notify],
  );

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
        onSelectProfile={selectProfile}
        onSelectPort={setSelectedPort}
        onRefreshPorts={refreshPorts}
        onVerify={verify}
        onUpload={upload}
      />

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
              files={files}
              openFile={openFile}
              dirtyFiles={dirtyFiles}
              onOpen={(p) => sketchDir && openFileInEditor(sketchDir, p)}
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
          <div className="editor-title">
            {openFile ?? "no file open"}
            {openFile && dirtyFiles.has(openFile) ? " ● unsaved (Ctrl+S)" : ""}
          </div>
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
                  g === "console" ? "build" : g === "debug" ? debugTab : obsTab,
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
      </section>

      <footer className={`statusbar ${statusIsError ? "error" : ""}`}>
        {busy ? "⏳ " : ""}
        {status}
      </footer>
    </div>
  );
}
