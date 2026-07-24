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

type SideTab = "files" | "libraries";
type BottomTab = "build" | "serial";

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

  // ui
  const [sideTab, setSideTab] = useState<SideTab>("files");
  const [bottomTab, setBottomTab] = useState<BottomTab>("build");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("Bancada ready — open a sketch folder.");
  const [statusIsError, setStatusIsError] = useState(false);

  const notify = useCallback((msg: string, isError = false) => {
    setStatus(msg);
    setStatusIsError(isError);
  }, []);

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
    return () => {
      subs.forEach((p) => p.then((un) => un()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- project ----------

  const refreshPorts = () =>
    api.listBoards().then(setPorts).catch((e) => notify(String(e), true));

  /** Switch profile; if it pins a port in sketch.yaml, select that port too. */
  const selectProfile = (p: string) => {
    setProfile(p);
    const pinned = sketchYaml?.profiles?.[p]?.port;
    if (pinned) setSelectedPort(pinned);
  };

  const openSketch = async () => {
    const dir = await open({ directory: true, title: "Open sketch folder" });
    if (typeof dir !== "string") return;
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
      // auto-open the main .ino
      const name = dir.split("/").pop();
      const main = fs.find((f) => f.rel_path === `${name}.ino`);
      if (main) await openFileInEditor(dir, main.rel_path);
      notify(`Opened ${dir}`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const openFileInEditor = async (dir: string, relPath: string) => {
    try {
      const text =
        buffersRef.current.get(relPath) ??
        (await api.readSketchFile(dir, relPath));
      setOpenFile(relPath);
      setContent(text);
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
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  // ---------- serial monitor ----------

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
        onSelectProfile={selectProfile}
        onSelectPort={setSelectedPort}
        onRefreshPorts={refreshPorts}
        onVerify={verify}
        onUpload={upload}
        onReadMac={readMac}
      />

      <div className="main">
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
        </section>
      </div>

      <section className="bottom">
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
        </div>
        {bottomTab === "build" ? (
          <Console lines={buildLines} onClear={() => setBuildLines([])} />
        ) : (
          <Console
            lines={serialLines}
            onClear={() => setSerialLines([])}
            onSend={(d) => api.monitorSend(d).catch((e) => notify(String(e), true))}
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
