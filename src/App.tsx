import { useCallback, useEffect, useRef, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { cpp } from "@codemirror/lang-cpp";
import { oneDark } from "@codemirror/theme-one-dark";
import { open } from "@tauri-apps/plugin-dialog";

import * as api from "./api";
import type { DetectedPort, OutputLine, SketchFile, SketchYaml } from "./api";
import FileTree from "./components/FileTree";
import Toolbar from "./components/Toolbar";
import LibraryManager from "./components/LibraryManager";
import Console from "./components/Console";
import ScopeView from "./components/ScopeView";
import NewProject from "./components/NewProject";

type SideTab = "files" | "libraries";
type BottomTab = "build" | "serial" | "scope";

// Bottom panel sizing: layout preference, so it lives in localStorage (per
// machine, not part of the app settings file).
const BOTTOM_HEIGHT_KEY = "bancada.bottomHeight";
const BOTTOM_MIN = 120;
const BOTTOM_DEFAULT = 220;
const clampBottomHeight = (h: number) =>
  Math.min(Math.max(h, BOTTOM_MIN), Math.round(window.innerHeight * 0.8));

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

  // ui
  const [sideTab, setSideTab] = useState<SideTab>("files");
  const [bottomTab, setBottomTab] = useState<BottomTab>("build");
  // Bottom panel expanded over the whole main area (editor stays mounted).
  const [bottomMax, setBottomMax] = useState(false);
  const [bottomHeight, setBottomHeight] = useState(() => {
    const saved = Number(localStorage.getItem(BOTTOM_HEIGHT_KEY));
    return Number.isFinite(saved) && saved >= BOTTOM_MIN
      ? clampBottomHeight(saved)
      : BOTTOM_DEFAULT;
  });
  // ScopeView stays mounted once opened so streams/subscriptions survive
  // switching to another bottom tab.
  const [scopeMounted, setScopeMounted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("Bancada ready — open a sketch folder.");
  const [statusIsError, setStatusIsError] = useState(false);

  const notify = useCallback((msg: string, isError = false) => {
    setStatus(msg);
    setStatusIsError(isError);
  }, []);

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
      api.onBuildLine((l) => setBuildLines((prev) => appendCapped(prev, l))),
      api.onSerialLine((l) => setSerialLines((prev) => appendCapped(prev, l))),
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
        // Auto-select the first serial port so the monitor can start capturing
        // by default (a profile-pinned or user-chosen port is never overridden).
        setSelectedPort(
          (cur) =>
            cur ??
            ps.find((p) => p.port.protocol === "serial")?.port.address ??
            null,
        );
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
    setBottomTab("build");
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
    setBottomTab("build");
    setBusy(true);
    notify(`Uploading to ${selectedPort}…`);
    try {
      const r = await api.uploadSketch(
        sketchDir,
        selectedPort,
        target.profile,
        target.fqbn,
      );
      notify(r.success ? `✓ Flashed via ${selectedPort}` : "Upload failed", !r.success);
      // Resume capturing (native-USB boards re-enumerate after flashing,
      // so give the port a moment to come back).
      if (r.success) setTimeout(() => startMonitorQuiet(), 1200);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const readMac = async () => {
    if (!selectedPort) return;
    if (monitorOn) await toggleMonitor();
    setBusy(true);
    notify("Reading MAC via esptool…");
    try {
      const info = await api.readBoardMac(selectedPort);
      notify(`MAC ${info.mac}${info.chip_type ? ` — ${info.chip_type}` : ""}`);
      setBuildLines((prev) => [
        ...prev,
        ...info.raw_output
          .split("\n")
          .map((line) => ({ stream: "stdout" as const, line })),
      ]);
      setBottomTab("build");
      setTimeout(() => startMonitorQuiet(), 800); // resume after ROM-bootloader reset
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
        setBottomTab("serial");
      }
    } catch (e) {
      notify(String(e), true);
    }
  };

  // ---------- scope ----------

  const openScopeTab = () => {
    setScopeMounted(true);
    setBottomTab("scope");
  };

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
    setBottomTab("serial");
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
      setBottomTab("build");
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
        onReadMac={readMac}
      />

      <div className="main" style={bottomMax ? { display: "none" } : undefined}>
        <aside className="sidebar">
          <div className="panel-tabs">
            <button
              className={sideTab === "files" ? "tab active" : "tab"}
              onClick={() => setSideTab("files")}
            >
              Files
            </button>
            <button
              className={sideTab === "libraries" ? "tab active" : "tab"}
              onClick={() => setSideTab("libraries")}
            >
              Libraries
            </button>
          </div>
          {sideTab === "files" ? (
            <FileTree
              files={files}
              openFile={openFile}
              dirtyFiles={dirtyFiles}
              onOpen={(p) => sketchDir && openFileInEditor(sketchDir, p)}
            />
          ) : (
            <LibraryManager
              sketchDir={sketchDir}
              profile={profile}
              onYamlChanged={setSketchYaml}
              notify={notify}
            />
          )}
        </aside>

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
          <button
            className={bottomTab === "build" ? "tab active" : "tab"}
            onClick={() => setBottomTab("build")}
          >
            Build Output
          </button>
          <button
            className={bottomTab === "serial" ? "tab active" : "tab"}
            onClick={() => setBottomTab("serial")}
          >
            Serial Monitor {monitorOn ? "●" : ""}
          </button>
          <button
            className={bottomTab === "scope" ? "tab active" : "tab"}
            onClick={openScopeTab}
          >
            ∿ Oscilloscope
          </button>
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
