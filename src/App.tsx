import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import CodeMirror, { type ReactCodeMirrorRef } from "@uiw/react-codemirror";
import { cpp } from "@codemirror/lang-cpp";
import { oneDark } from "@codemirror/theme-one-dark";
import { ask, open } from "@tauri-apps/plugin-dialog";

import * as api from "./api";
import { matchesAccel, parseAccel } from "./keys";
import { boardOffer } from "./boardOffer";
import { silentSerialWarning } from "./boardOptions";
import { MAX_RECAPTURE_ATTEMPTS, recapturePlan } from "./monitorRecovery";
import {
  flashTargetMismatch,
  missingPortName,
  nextSelectedPort,
  portName,
  visibleBoard,
  withPath,
} from "./ports";
import { checkNewEntry } from "./newFile";
import { badgeCount, parseBuildOutput, type JumpTarget } from "./diagnostics";
import { gotoLine } from "./editorGoto";
import { useExplorerStore } from "./explorerStore";
import {
  affectedByDelete,
  checkRename,
  isNonEmptyDir,
  pathAfterRename,
} from "./explorerOps";
import {
  closeAll,
  closeOthers,
  closeTab,
  deleteTabs,
  openTab,
  renameTabs,
} from "./editorTabs";
import { arrivals, bridgeArrivals } from "./portWatch";
import { blockedByConflict, conflictMessage } from "./conflicts";
import {
  detectBaud,
  effectiveBaud,
  loadBaudOverrides,
  overrideFor,
  saveBaudOverride,
} from "./serialPrefs";
import { SerialStore } from "./serial/serialStore";
import {
  type ToastState,
  dismissToast,
  emptyToasts,
  expireToasts,
  kindOfNotify,
  nextExpiry,
  pushToast,
} from "./notifications";
import type { Activity, ActivityKey, LastResult } from "./statusLine";
import {
  type BuildProgress,
  reduceBuildLine,
  startProgress,
} from "./buildProgress";
import { loadDurations, recordDuration } from "./buildHistory";
import { projectButtonLabel } from "./toolbarModel";
import type { AgentEvent, DetectedPort, OutputLine, SketchYaml } from "./api";
import { AgentStore } from "./agent/agentStore";
import { ChatRecorder, chatFileName, applyChatOps } from "./agent/chatLog";
import { distillFacts } from "./agent/continueChat";
import { createResumeWatch, type ResumeWatch } from "./agent/resumeWatch";
import EditorTabs from "./components/EditorTabs";
import FileTree from "./components/FileTree";
import Toolbar from "./components/Toolbar";
import LibraryManager from "./components/LibraryManager";
import BoardsManager from "./components/BoardsManager";
import FleetManager from "./components/FleetManager";
import BuildConsole from "./components/BuildConsole";
import SerialMonitor, {
  type SerialConnection,
} from "./components/SerialMonitor";
import ScopeView from "./components/ScopeView";
import NewProject from "./components/NewProject";
import DuplicateProject from "./components/DuplicateProject";
import RenameProject from "./components/RenameProject";
import BoardOffer from "./components/BoardOffer";
import ProfileInit, { type ProfileFormMode } from "./components/ProfileInit";
import UsageDashboard from "./components/UsageDashboard";
import MqttPanel from "./components/MqttPanel";
import WsPanel from "./components/WsPanel";
import DeviceBrowserPanel from "./components/DeviceBrowserPanel";
import AgentPanel from "./components/AgentPanel";
import BottomTabBar from "./components/BottomTabBar";
import ToastStack from "./components/ToastStack";
import StatusBar from "./components/StatusBar";
import { type BottomTab } from "./bottomTabs";

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

// Global accelerators; the specs are literals, so the parses cannot fail.
const ACCEL_SAVE = parseAccel("Ctrl+S")!;
const ACCEL_OPEN = parseAccel("Ctrl+O")!;

const MAX_CONSOLE_LINES = 5000;
const TRIM_CONSOLE_LINES = 4000;
/** Append a whole frame's worth of build lines at once, capped and trimmed
 *  in one new array rather than one per line. A compile emits thousands of
 *  lines in a burst, and the build console re-parses the whole buffer on
 *  every change, so appending per event costs a parse per line; this
 *  coalesces a frame into a single state write. The serial log does not come
 *  through here — it lives in `serialStore`, outside React state. Pure. */
const appendManyCapped = (
  prev: OutputLine[],
  batch: readonly OutputLine[],
): OutputLine[] => {
  if (batch.length === 0) return prev;
  const next = [...prev, ...batch];
  return next.length > MAX_CONSOLE_LINES
    ? next.slice(-TRIM_CONSOLE_LINES)
    : next;
};

/** CodeMirror 6 splits incoming text on `/\r\n?|\n/` and joins it back with
 *  `\n`, so a CRLF file's document is *shorter* than the string handed to it.
 *  Comparing the editor against a normalised copy is what lets a diagnostic
 *  jump into a CRLF sketch at all. */
const normalizeEol = (s: string) => s.replace(/\r\n?/g, "\n");

/** One frame of grace on the toast-expiry timer — see the effect that uses
 *  it for why firing early is the dangerous direction. */
const EXPIRY_SLACK_MS = 16;

/** Whether a status-bar activity belongs to the Assistant rather than the
 *  user. The two must never close or clobber each other's work — see
 *  `beginActivity`/`endActivity`. */
const isAgentKey = (k: ActivityKey): boolean => k.startsWith("agent_");

// One store for the whole app lifetime (not panel-owned): App-level
// `agent://event` listeners feed it even before the Assistant panel has ever
// been mounted, so the bottom-tab unseen dot keeps working regardless of
// which tab is open. See src/agent/agentStore.ts.
const agentStore = new AgentStore();
// Its shadow: every store mutation is also recorded to the chat log, so a
// saved chat replays into an identical transcript. Recording is
// fire-and-forget (a failed append never breaks a live chat).
const chatRecorder = new ChatRecorder();

// The serial log, App-owned for the same reason `agentStore` is:
// `serial://line` arrives while the Monitor tab is hidden — from the
// recapture ladder's auto-start and from the agent's own `serial_read` — so
// neither the scrollback nor the unseen dot may depend on a panel having been
// mounted. Outside React state as well as outside the panel: a board at
// 921600 baud emits faster than React can commit. See
// src/serial/serialStore.ts.
const serialStore = new SerialStore();

/** `file_path` from an Edit/Write tool input is cwd-absolute (cwd = sketch
 *  dir); reduce it to the same rel_path shape the file tree/editor use. */
const relativeToSketchDir = (filePath: string, dir: string): string => {
  const prefix = dir.endsWith("/") ? dir : `${dir}/`;
  return filePath.startsWith(prefix) ? filePath.slice(prefix.length) : filePath;
};

/** Set by `continueChat`, consumed by `sendToAgent`'s idle branch: either a
 *  native `--resume <sessionId>` attempt, or (no recorded session_id) a
 *  facts-only fresh spawn from the start. Cleared once consumed (a
 *  confirmed resume, or a fallback respawn) or by `teardownAgentSession`, so
 *  a later plain message can never accidentally resume a stale session.
 *  `sketchDir` is captured from `sketchDirRef.current` at stash-creation time
 *  (not read again from the `sketchDir` render closure later) so a project
 *  switch mid-flight can never make `fallbackRespawn` spawn into the wrong
 *  directory — it compares this against the live ref instead. */
interface ContinuationStash {
  file: string;
  sessionId?: string;
  facts: string;
  sketchDir: string;
}

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
  /** When true the editor area shows the Duplicate Project form instead. */
  const [duplicatingProject, setDuplicatingProject] = useState(false);
  /** When true the editor area shows the Rename Project form instead. */
  const [renamingProject, setRenamingProject] = useState(false);
  /** When true the editor area shows the usage dashboard instead. */
  const [showingUsage, setShowingUsage] = useState(false);
  /** When set, the one-row profile form shows under the toolbar. */
  const [profileForm, setProfileForm] = useState<ProfileFormMode | null>(null);

  /**
   * The editor area hosts one form at a time. Opening any of them closes the
   * rest — expressed once here rather than as a reset list repeated at every
   * call site, which is how the fifth pane would have been forgotten.
   */
  const showPane = (
    pane: "new" | "duplicate" | "rename" | "usage" | null,
    profileMode: ProfileFormMode | null = null,
  ) => {
    setCreatingProject(pane === "new");
    setDuplicatingProject(pane === "duplicate");
    setRenamingProject(pane === "rename");
    setShowingUsage(pane === "usage");
    setProfileForm(profileMode);
  };
  /** Uncover the editor without touching the profile strip. `ProfileInit`
   *  renders under the toolbar, not in the editor area, so it is not in the
   *  way of anything — and dropping a half-filled profile form to show a
   *  file would be a surprising thing to do to the user. Expressed through
   *  `showPane` so the pane list stays in exactly one place. */
  const showEditor = () => showPane(null, profileForm);
  /** True when the editor itself is what the editor area shows. Each of the
   *  four panes `showPane` opens *replaces* CodeMirror (see the ternary in
   *  the render), so while one is up `editorRef.current` is null and a
   *  parked diagnostic jump has to wait for this to flip back — hence its
   *  place in that effect's deps.
   *  (`renamingProject` with no `sketchDir` renders the editor regardless,
   *  but a jump needs a sketchDir to be requested at all, so that corner
   *  cannot strand one.) */
  const editorShowing =
    !creatingProject &&
    !duplicatingProject &&
    !renamingProject &&
    !showingUsage;
  // Live mirror of sketchDir for the App-level agent event listeners, which
  // are registered once (empty-dep effect) and would otherwise close over a
  // stale `null`.
  const sketchDirRef = useRef<string | null>(null);
  sketchDirRef.current = sketchDir;

  // The toolbar pill's whole world; null while no sketch is open.
  const [gitState, setGitState] = useState<api.RepoState | null>(null);
  /** Last fleet snapshot, so a port can be named by its nickname rather than
   *  by a device path the kernel hands out in plug order. */
  const [fleet, setFleet] = useState<api.FleetSnapshot | null>(null);
  /** Board ids the user has waved away this session. Not persisted — a
   *  new session is a new bench, and the offer is cheap to re-answer. */
  const [offerDismissed, setOfferDismissed] = useState<ReadonlySet<string>>(
    new Set(),
  );
  /** Board id → cooldown expiry (ms). A flash resets the board and it
   *  re-enumerates, so without this the act of flashing would offer the
   *  project you are already working in, every time. */
  const flashCooldownRef = useRef<Map<string, number>>(new Map());
  /** Set by a first Open click when opening would discard something. */
  const [offerArmed, setOfferArmed] = useState(false);
  const [offerDrift, setOfferDrift] = useState<api.ProjectDrift | null>(null);
  const [ghOk, setGhOk] = useState(false);
  const gitStateRefreshRef = useRef<(dir: string) => void>(() => {});

  const refreshGitState = useCallback((dir: string) => {
    // Fire-and-forget like fleetSync: a hint, not load-bearing. Guarded
    // against staleness on both paths — a slow response for a project the
    // user has since switched away from must not clobber the new one's
    // state, whether it resolves or rejects.
    api
      .gitState(dir)
      .then((s) => {
        if (dir === sketchDirRef.current) setGitState(s);
      })
      .catch(() => {
        if (dir === sketchDirRef.current) setGitState(null);
      });
  }, []);
  gitStateRefreshRef.current = refreshGitState;

  // editor — unsaved edits live in the buffer map keyed by rel_path; disk is
  // the source of truth for clean files.
  const [openFile, setOpenFile] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirtyFiles, setDirtyFiles] = useState<Set<string>>(new Set());
  const buffersRef = useRef(new Map<string, string>());
  /** The live CodeMirror handle, so a compiler diagnostic can move the
   *  cursor. `.view` is null until the editor has mounted. */
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  /** A jump asked for while the target file was still being opened; the
   *  effect below applies it once the editor is showing that document. */
  const pendingGotoRef = useRef<JumpTarget | null>(null);
  // Editor tab strip — the explicit open set (superset-of-one containing
  // `openFile`), and the tab armed for "close again to discard" (a dirty
  // close request arms once; any other action disarms it).
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [armedTab, setArmedTab] = useState<string | null>(null);
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
  /** Build lines that arrived this frame, waiting for the scheduled flush.
   *  Written from the `[]`-deps `onBuildLine` subscription, so a ref. */
  const pendingBuildRef = useRef<OutputLine[]>([]);
  /** Whether a flush is already booked for this frame. A plain sentinel: the
   *  flush is never cancelled, so the rAF handle would be dead weight. */
  const buildFlushScheduledRef = useRef(false);
  const [monitorOn, setMonitorOn] = useState(false);
  /** Per-sketch baud overrides, persisted. No entry for a sketch means "use
   *  whatever its own `Serial.begin` asks for". */
  const [baudOverrides, setBaudOverrides] = useState(() =>
    loadBaudOverrides(localStorage),
  );
  /** The picker's rate with no project open. There is no key to persist it
   *  under — and a bare port with no sketch folder is a real bench case (a
   *  board somebody else flashed, printing at 9600) — so it lives for the
   *  session. Dropping it on the floor was the regression: the `<select>`
   *  moved and the monitor stayed where it was. */
  const [noProjectBaud, setNoProjectBaud] = useState<number | null>(null);
  /** The rate the open sketch calls `Serial.begin` with, or null when that is
   *  not one clear answer. Re-sniffed on load and on save, never per
   *  keystroke — `detectBaud` re-parses the main `.ino` and every open
   *  buffer each time. */
  const [sketchBaud, setSketchBaud] = useState<number | null>(null);
  /** Keeping the name `baudrate` for the *effective* rate is deliberate:
   *  every consumer (the monitor start, the target mirrored to Rust for the
   *  agent, ScopeView) wants the rate the port is actually opened at, not the
   *  raw override — and none of them had to change. */
  const { baud: baudrate, source: baudSource } = effectiveBaud(
    overrideFor(sketchDir, baudOverrides, noProjectBaud),
    sketchBaud,
  );
  // Live mirror of monitorOn for callbacks captured by timers (post-upload
  // auto-resume fires from a closure created while the monitor was still on).
  const monitorOnRef = useRef(false);
  monitorOnRef.current = monitorOn;
  /** The standing request for capture, as opposed to whether a child is
   *  currently alive. Cleared only by an *explicit* stop — the Stop
   *  button, the scope taking the port, the pre-flash handoff — so an
   *  unexpected close can be told apart from one we asked for.
   *
   *  Read during render by `serialConnection` (the Monitor's status chip), so
   *  every transition that clears or re-arms it must also write state — the
   *  chip will not re-render for a ref on its own. */
  const monitorWantedRef = useRef(false);
  /** Recapture attempts since the last successful start. */
  const recaptureAttemptRef = useRef(0);
  /** Render mirror of `recaptureAttemptRef` for the Monitor's status chip.
   *  Written once per rung of the ladder, not per line. */
  const [recaptureAttempt, setRecaptureAttempt] = useState(0);
  const recaptureTimerRef = useRef<number | undefined>(undefined);
  /** The session id of the monitor child we are listening to. A
   *  `serial://closed` naming any other session comes from a reader we have
   *  already replaced — a baud restart, a recapture — and must be ignored,
   *  or it flips the toggle off underneath a live monitor. */
  const monitorSessionRef = useRef<number | null>(null);
  /** The rate the live monitor child was actually opened at.
   *
   *  `baudrate` is *derived* — switching project, or saving a sketch whose
   *  `Serial.begin` changed, moves it under a child that is still reading at
   *  the old rate. This ref, not the picker, is the truth about the port: it
   *  drives the toolbar while a monitor is up and the effect that re-opens
   *  when the two drift apart. */
  const openBaudRef = useRef<number | null>(null);
  // New-content dots on the bottom tabs: set when lines arrive for a hidden
  // tab, cleared when that tab is opened.
  const [unseen, setUnseen] = useState<Partial<Record<BottomTab, boolean>>>({});
  const bottomTabRef = useRef<BottomTab>("build");
  /** The build console's parsed view of `buildLines` — diagnostics, memory
   *  report and summary. Memoised because the badge, the tab bar and the
   *  console all read it, and the parse walks the whole buffer. */
  const buildModel = useMemo(() => parseBuildOutput(buildLines), [buildLines]);
  /** Sketch-relative paths the editor can actually open — files only, since
   *  `files` lists directories too and the editor cannot open one.
   *  Diagnostics naming anything else (a core header, a library) stay
   *  unclickable. */
  const knownFiles = useMemo(
    () => new Set(files.filter((f) => !f.is_dir).map((f) => f.rel_path)),
    [files],
  );

  // ui — sidebar hierarchy: a Software/Hardware group switcher over per-group
  // sub-tabs; each group remembers its last-used tab.
  const [sideGroup, setSideGroup] = useState<SideGroup>("software");
  const [softwareTab, setSoftwareTab] = useState<SoftwareTab>("files");
  const [hardwareTab, setHardwareTab] = useState<HardwareTab>("fleet");
  const sideTab = sideGroup === "software" ? softwareTab : hardwareTab;
  // Bottom panel: one flat row of seven tabs (no groups, nothing to
  // remember) — order and labels live in `./bottomTabs`.
  const [bottomTab, setBottomTab] = useState<BottomTab>("build");
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
  const [webMounted, setWebMounted] = useState(false);
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
  /** The Assistant's "Allow uploads" arm switch — per session, off by
   *  default; lives here (not panel-local) because it must survive the
   *  panel unmounting and ride `agentStart` for a pre-session toggle. */
  const [uploadsArmed, setUploadsArmed] = useState(false);
  const uploadsArmedRef = useRef(false);
  uploadsArmedRef.current = uploadsArmed;
  /** An agent flash is in flight — suppresses the monitor auto-start effect
   *  (a monitor grabbing the port mid-flash kills the flash). */
  const agentFlashingRef = useRef(false);
  /** "Continue this chat" bookkeeping — see `ContinuationStash` and
   *  `sendToAgent`/`fallbackRespawn`/`teardownAgentSession` below. */
  const continuationRef = useRef<ContinuationStash | null>(null);
  /** Armed only while a native `--resume` attempt's outcome is unknown; see
   *  src/agent/resumeWatch.ts. */
  const resumeWatchRef = useRef<ResumeWatch | null>(null);
  /** Bumped by `teardownAgentSession` (every call) and by `continueChat`'s
   *  entry, so it strictly increases across any session boundary — a
   *  teardown, a "New session", a project switch, or a second `continueChat`
   *  — including ones that land *during* an in-flight `fallbackRespawn`'s
   *  awaits. `fallbackRespawn` captures the epoch at the moment its watch
   *  was armed and re-checks it after every await: a mismatch means the
   *  world moved on while it was suspended, and it must abort rather than
   *  write into (or spawn a child for) whatever chat/session is now live. */
  const teardownEpochRef = useRef(0);
  /** Every `notify` lands here as a card in the corner. The old single-slot
   *  status string could only ever show the newest message, so a warning was
   *  erased by whatever the same action said next — the reason so much of the
   *  code below used to reason about what would "overwrite" what. The stack
   *  keeps up to four, errors stay until dismissed, and the status bar is
   *  free to say what is *happening* instead. */
  const [toasts, setToasts] = useState<ToastState>(emptyToasts);
  /** What is running right now, or null at rest. Must be set and cleared in
   *  lockstep with `busy` — `progressMode` draws nothing while `busy` is
   *  false, so an activity without it shows a clock above an empty track. */
  const [activity, setActivity] = useState<Activity | null>(null);
  /** Mirror of `activity` for `endActivity`, which needs the start time and
   *  is itself called from `[]`-deps handlers that may not read state.
   *
   *  Written by hand rather than assigned during render, because it is read
   *  synchronously in the same tick it is set — a render mirror would still
   *  hold the previous value at that point. Every writer of `activity` below
   *  (`beginActivity`, `endActivity`, `clearAgentActivity`, and the one label
   *  rewrite in `flashScopeFirmware`) must keep this in step. */
  const activityRef = useRef<Activity | null>(null);
  /** The last thing that finished, for the bar's second tier. */
  const [lastResult, setLastResult] = useState<LastResult | null>(null);
  /** Parsed out of the uploader's own output. The ref is the one the
   *  `[]`-deps `onBuildLine` subscription reads and writes; the state exists
   *  only to repaint. */
  const [progress, setProgress] = useState<BuildProgress | null>(null);
  const progressRef = useRef<BuildProgress | null>(null);

  // Hotplug plumbing. busyRef mirrors `busy` for the event listener;
  // pendingScanRef queues a rescan that arrived mid-flash; prevOnlineRef
  // is null until the first fleet sync so launch-time boards aren't
  // announced as arrivals.
  const busyRef = useRef(false);
  const pendingScanRef = useRef(false);
  const scanTimerRef = useRef<number | undefined>(undefined);
  const prevOnlineRef = useRef<string[] | null>(null);
  /** Bridge-port addresses at the last sync — null until the first, so
   *  bridges present at launch are not announced (same rule as online). */
  const prevUnidentifiedRef = useRef<string[] | null>(null);

  const notify = useCallback((msg: string, isError = false) => {
    setToasts((s) => pushToast(s, kindOfNotify(msg, isError), msg, Date.now()));
  }, []);

  // Nothing in `notifications.ts` owns a clock, so the expiry timer is here:
  // one timeout armed for the *soonest* deadline in the stack, re-armed after
  // every push and every sweep. `nextExpiry` returning null (an empty stack,
  // or nothing but errors, which never expire) means arm nothing at all —
  // this must not become a poll that runs for the life of the app.
  //
  // The re-arm is what the slack protects. `expireToasts` keeps anything with
  // `expiresAt > now` and returns the **same reference** when it dropped
  // nothing, so React bails out of the render, this effect never re-runs, and
  // the timer is never armed again — one tick landing a millisecond early
  // would strand a live toast on screen for good. A frame of slack puts the
  // callback safely past the deadline, so the sweep always removes something
  // and always produces a new reference to re-arm from.
  useEffect(() => {
    const next = nextExpiry(toasts);
    if (next === null) return;
    const id = window.setTimeout(
      () => setToasts((s) => expireToasts(s, Date.now())),
      Math.max(0, next - Date.now()) + EXPIRY_SLACK_MS,
    );
    return () => window.clearTimeout(id);
  }, [toasts]);

  /** Announce that something long-running has started. `op` additionally arms
   *  the build-progress parser, which then feeds off `onBuildLine`.
   *
   *  Callers must set `busy` (`setUserBusy`/`setAgentBuilding`) from the same
   *  place: the bar draws no progress at all while `busy` is false.
   *
   *  Asymmetric by design. A user action taking the bar over is the user
   *  asking for it, so it wins. An *agent event* arriving while the user's
   *  own build is running is not — it would relabel a live bar "Assistant
   *  compiling…" and reset a progress parser mid-flash — so it is dropped,
   *  and `busy` keeps the bar honest either way. */
  const beginActivity = useCallback(
    (key: ActivityKey, label: string, op?: "compile" | "upload") => {
      const cur = activityRef.current;
      if (isAgentKey(key) && cur && !isAgentKey(cur.key)) return;
      const a: Activity = { key, label, startedAt: Date.now() };
      activityRef.current = a;
      setActivity(a);
      if (op) {
        const p = startProgress(op);
        progressRef.current = p;
        setProgress(p);
      }
    },
    [],
  );

  /** Close out an activity `beginActivity` opened: bank the duration for the
   *  next run's "usually ~" hint, leave the verdict on the bar, and stop the
   *  clock.
   *
   *  `expected` names the key (or keys) the caller is entitled to close, and
   *  a mismatch is a silent no-op. Call this from a `finally`: an IPC call
   *  that throws must not leave "Compiling…" counting up forever. */
  const endActivity = useCallback(
    (
      expected: ActivityKey | readonly ActivityKey[],
      ok: boolean,
      label: string,
    ) => {
      const a = activityRef.current;
      // Only the owner may close it. Two things go wrong without this check.
      // Closing *nothing*: a straggler `verify_done` from a torn-down
      // session finds the store's pid already cleared, so the guard upstream
      // waves it through, and "✓ Assistant compile in 0:00" appears for a
      // build nobody ran. Worse, closing *somebody else's*: that same
      // straggler would end the USER's live compile — blanking the bar and
      // banking its 300 ms as this project's remembered compile time, which
      // then poisons every later "usually ~" estimate.
      const owners = typeof expected === "string" ? [expected] : expected;
      if (!a || !owners.includes(a.key)) return;
      const now = Date.now();
      const durationMs = now - a.startedAt;
      const dir = sketchDirRef.current;
      // The estimate is read back under `compile`/`upload` only, so those
      // are the only keys worth banking. The agent's builds are deliberately
      // left out: the "usually ~" hint should describe a run the user
      // watched.
      if (dir && (a.key === "compile" || a.key === "upload"))
        recordDuration(window.localStorage, dir, a.key, durationMs, now);
      setLastResult({ ok, label, durationMs, at: now });
      setActivity(null);
      activityRef.current = null;
      progressRef.current = null;
      setProgress(null);
    },
    [],
  );

  /** Drop the clock and the bar without a verdict, for the paths that end an
   *  agent op without knowing how it went: a security kill, a torn-down
   *  session. Scoped to the agent's own activities on purpose — "New
   *  session" while the *user* has a compile running must not blank a bar
   *  that has nothing to do with the agent. */
  const clearAgentActivity = useCallback(() => {
    const a = activityRef.current;
    if (!a || !isAgentKey(a.key)) return;
    setActivity(null);
    activityRef.current = null;
    progressRef.current = null;
    setProgress(null);
  }, []);

  useEffect(() => {
    busyRef.current = busy;
    if (!busy && pendingScanRef.current) {
      // The flag is cleared by refreshPorts on *success*, not here: a scan
      // consumed before it ran was lost for good when arduino-cli failed,
      // and the backend never re-emits for a set it already recorded.
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

  // Whether `gh` is installed and on PATH — gates the "Create on GitHub"
  // option in the git pill's remote-setup popover. Not a check that it's
  // authenticated; an auth failure surfaces later, at create time.
  useEffect(() => {
    api.ghAvailable().then(setGhOk).catch(() => setGhOk(false));
  }, []);

  /** Open a bottom tab, mounting its live panel on first open. Call this
   *  rather than setBottomTab — the mount latches are the whole point. */
  const openBottomTab = useCallback((tab: BottomTab) => {
    setBottomTab(tab);
    if (tab === "scope") setScopeMounted(true);
    if (tab === "mqtt") setMqttMounted(true);
    if (tab === "ws") setWsMounted(true);
    if (tab === "web") setWebMounted(true);
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
    // `pointercancel` as well as `pointerup`: a cancelled drag (an
    // interrupted touch, the browser reclaiming the capture) releases the
    // pointer capture but fires no `pointerup`, so tearing down on `up`
    // alone left `onMove` bound to the handle — after which merely hovering
    // the divider resized the panel.
    const onUp = () => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("pointercancel", onUp);
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(w));
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("pointercancel", onUp);
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
    // Same `pointercancel` teardown as the sidebar handle above.
    const onUp = () => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("pointercancel", onUp);
      localStorage.setItem(BOTTOM_HEIGHT_KEY, String(h));
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("pointercancel", onUp);
  };

  // ---------- event subscriptions ----------

  useEffect(() => {
    const subs = [
      api.onBuildLine((l) => {
        // Coalesced, unlike the serial feed: a compile arrives in bursts of
        // thousands of lines, and every `buildLines` change re-parses the
        // whole buffer for the console and the badge. One state write per
        // frame keeps that parse off the critical path.
        pendingBuildRef.current.push(l);
        if (!buildFlushScheduledRef.current) {
          buildFlushScheduledRef.current = true;
          // Bare rAF, here and in the pending-goto effect: this code only
          // ever runs in the webview, which has it. No fallback to keep in
          // step with it.
          requestAnimationFrame(() => {
            const batch = pendingBuildRef.current;
            pendingBuildRef.current = [];
            buildFlushScheduledRef.current = false;
            setBuildLines((prev) => appendManyCapped(prev, batch));
          });
        }
        if (bottomTabRef.current !== "build")
          setUnseen((u) => (u.build ? u : { ...u, build: true }));
        // The progress bar is fed from the same stream the console shows.
        // Via the ref, never state: this subscription is registered once.
        // `reduceBuildLine` returns the same reference for a line it has
        // nothing to say about — which is most of them — so the common case
        // costs one comparison and no render.
        const cur = progressRef.current;
        if (cur) {
          const next = reduceBuildLine(cur, l.line);
          if (next !== cur) {
            progressRef.current = next;
            setProgress(next);
          }
        }
      }),
      api.onSerialLine((l) => {
        // Straight into the store: no state write, so a chatty board costs
        // no renders at all while the panel polls at 10 Hz.
        serialStore.push(l.stream, l.line, Date.now());
        if (bottomTabRef.current !== "serial")
          setUnseen((u) => (u.serial ? u : { ...u, serial: true }));
      }),
      api.onSerialClosed(({ session }) => {
        // A stale reader from an earlier monitor: both the baud restart and
        // the recapture ladder replace the child, and the old one's stdout
        // EOF can land after the new one is already live.
        if (
          monitorSessionRef.current !== null &&
          session !== monitorSessionRef.current
        )
          return;
        serialStore.push("info", "— monitor closed —", Date.now());
        setMonitorOn(false);
        // A native-USB board re-enumerates on every reset, taking the
        // monitor child with it. Nothing else brings it back: the
        // auto-start effects need `selectedPort` to *change*, and the
        // port returns at the same address — often without the 2 s poll
        // ever observing the gap, so no ports://changed fires either.
        scheduleRecaptureRef.current?.();
      }),
      // The agent's serial_read auto-start: keep the Monitor toggle honest
      // (and monitorOnRef with it, so startMonitorQuiet never double-starts).
      api.onSerialStarted((p) => {
        // Adopt its session, or the very next close would look stale and be
        // dropped — leaving the toggle stuck on over a dead child.
        monitorSessionRef.current = p.session;
        openBaudRef.current = p.baud;
        serialStore.push(
          "info",
          `— monitor started by the assistant at ${openBaudRef.current} baud —`,
          Date.now(),
        );
        setMonitorOn(true);
      }),
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
        // A native `--resume` attempt's outcome is unknown until it proves
        // itself — the watch buffers everything until then and replays it
        // through `deliverAgentEvent` (its `onDeliver`), so a consumed event
        // must not also be routed through the ordinary path below.
        if (resumeWatchRef.current?.offerEvent(ev)) return;
        deliverAgentEvent(ev);
      }),
      api.onAgentClosed((p) => {
        // Same reasoning as above: a `closed` for the pid a resume watch is
        // still waiting on is that attempt failing (the child exited before
        // ever proving itself alive), not this session ending — the watch's
        // `onFailed` (`fallbackRespawn`) takes over from here.
        if (resumeWatchRef.current?.offerClosed(p)) return;
        // `p.pid` names the child that exited. The store ignores a close for
        // a session it is no longer showing (FE-C1): `newAgentSession()`
        // stops without awaiting, so session A's stdout EOF can land after
        // session B is already live and would otherwise flip B to "ended"
        // — Send disabled, panel saying "Session ended", child alive and
        // still streaming.
        agentStore.closed(p.reason, p.pid);
        // Follow the store's pid-guard verdict (FE-C1): a stale close from
        // a superseded session must not write a foreign `closed` op into
        // the NEW session's file and stop its recording for good. If the
        // store still says "running", the close wasn't ours.
        if (agentStore.snapshot().status === "ended") {
          // The child died. Any verify or flash it had in flight will never
          // report back, so release the build gate and stop the clock here
          // or the toolbar stays disabled and the bar reads "Assistant
          // compiling… 14:07" for as long as the window is open. Both are
          // scoped: `clearAgentActivity` leaves a user build alone, and the
          // store's pid guard has already established this close was ours.
          //
          // `agentFlashingRef` is deliberately NOT cleared: an agent flash
          // runs in the backend and can outlive the child that asked for
          // it, and lifting the monitor-suppression while esptool still
          // holds the port is the one failure this whole ladder exists to
          // prevent. A new session clears it (`teardownAgentSession`).
          setAgentBuilding(false);
          clearAgentActivity();
          chatRecorder.record({ op: "closed", reason: p.reason, pid: p.pid });
          chatRecorder.stop();
        }
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
        if (!ok) notify("Last project no longer available — open a project folder.");
      })
      .catch(() => {});
    return () => {
      subs.forEach((p) => p.then((un) => un()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- project ----------

  const refreshPorts = () => {
    if (busyRef.current) {
      // A timer armed before `busy` flipped true can still fire mid-flash;
      // re-defer rather than let arduino-cli probe ports under esptool.
      pendingScanRef.current = true;
      return;
    }
    api
      .listBoards()
      .then((ps) => {
        pendingScanRef.current = false;
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
            // Identified boards and bridge ports announce alike — a CH343
            // board can never reach snap.online, and its plug-in used to
            // produce no feedback at all.
            const fresh = [
              ...arrivals(prevOnlineRef.current, snap),
              ...bridgeArrivals(prevUnidentifiedRef.current, snap),
            ];
            prevOnlineRef.current = snap.online;
            prevUnidentifiedRef.current = snap.unidentified.map(
              (p) => p.port.address,
            );
            // Kept so every port the UI names can use the nickname the user
            // chose — see ports.portName.
            setFleet(snap);
            if (fresh.length > 0)
              notify(
                `⚡ ${fresh
                  .map((a) => withPath(a.name, a.port))
                  .join(", ")} attached`,
              );
          })
          .catch(() => {});
      })
      .catch((e) => notify(String(e), true));
  };

  /** The selected port, named the way the picker names it. Falls back to
   *  the bare address when nothing is selected. */
  const selectedPortName = () => {
    if (!selectedPort) return "no port";
    const p = ports.find((d) => d.port.address === selectedPort);
    return p ? portName(p, fleet) : missingPortName(selectedPort, fleet);
  };

  // A flash resets the board; it drops off the bus and comes back. Long
  // enough to outlast that, short enough that a deliberate unplug/replug a
  // minute later is heard.
  const FLASH_COOLDOWN_MS = 60_000;

  /** Board ids still inside their post-flash cooldown. */
  const offerSuppressed = useMemo(() => {
    const now = Date.now();
    const live = new Set<string>();
    for (const [id, until] of flashCooldownRef.current) {
      if (until > now) live.add(id);
    }
    return live;
    // Recomputed whenever the fleet does — the 2 s hotplug poll is what
    // makes an expired cooldown take effect, and no offer can appear
    // without a fleet change anyway.
  }, [fleet]);

  const offer = boardOffer(fleet, sketchDir, offerDismissed, offerSuppressed);

  // Drift is one subprocess per offer, not per scan.
  useEffect(() => {
    if (!offer) {
      setOfferDrift(null);
      return;
    }
    let cancelled = false;
    setOfferDrift(null);
    api
      .projectDrift(offer.rec.project_dir, offer.rec.commit)
      .then((d) => {
        if (!cancelled) setOfferDrift(d);
      })
      .catch(() => {
        if (!cancelled) setOfferDrift({ kind: "missing" });
      });
    return () => {
      cancelled = true;
    };
  }, [offer?.boardId, offer?.rec.project_dir, offer?.rec.commit]);

  // A new board's offer starts unarmed — the warning belongs to the offer
  // the user actually clicked, not to the next one.
  useEffect(() => setOfferArmed(false), [offer?.boardId]);

  /**
   * Why opening this project now would lose something, or null.
   *
   * `loadSketch` discards unsaved buffers and tears down a live Assistant
   * session without asking. Every other caller is a deliberate click; this
   * one appears unbidden, so it arms first and commits on the second click —
   * the same idiom a dirty tab close uses.
   */
  const offerOpenCost = (): string | null => {
    if (dirtyFiles.size > 0) {
      const n = dirtyFiles.size;
      return `${n} unsaved file${n === 1 ? "" : "s"} would be discarded`;
    }
    const agent = agentStore.snapshot().status;
    if (agent === "starting" || agent === "running")
      return "the Assistant session would be stopped";
    return null;
  };

  const acceptOffer = () => {
    if (!offer) return;
    const cost = offerOpenCost();
    if (cost && !offerArmed) {
      setOfferArmed(true);
      return;
    }
    setOfferDismissed(new Set([...offerDismissed, offer.boardId]));
    void loadSketch(offer.rec.project_dir);
  };

  const dismissOffer = () => {
    if (offer) setOfferDismissed(new Set([...offerDismissed, offer.boardId]));
  };

  /** Switch profile; if it pins a port in sketch.yaml, select that port too. */
  const selectProfile = (p: string) => {
    setProfile(p);
    const pinned = sketchYaml?.profiles?.[p]?.port;
    if (pinned) setSelectedPort(pinned);
  };

  const openSketch = async () => {
    const dir = await open({ directory: true, title: "Open project folder" });
    if (typeof dir !== "string") return;
    await loadSketch(dir);
  };

  /** Re-sniff the rate the sketch opens `Serial` at.
   *
   *  Every open buffer votes alongside the main `.ino` (its dirty buffer when
   *  there is one, the file on disk otherwise), so an unsaved edit to the rate
   *  is reflected. It is *not* every file in the sketch — an unopened `.cpp`
   *  calling `Serial.begin` does not vote. This runs on load, on save and on
   *  an agent edit only, never per keystroke: `detectBaud` re-parses each
   *  source it is handed, comments and `#define`s and all. */
  const refreshSketchBaud = useCallback(async (dir: string) => {
    const rel = `${dir.split("/").pop()}.ino`;
    // A dir with no main `.ino` (or one that will not read) simply votes
    // with nothing; the picker falls back to the default.
    const disk = await api.readSketchFile(dir, rel).catch(() => "");
    // The project can change while that read is in flight — a late answer for
    // the sketch we just left must not set the picker for the one we opened.
    if (sketchDirRef.current !== dir) return;
    const others = [...buffersRef.current.entries()]
      .filter(([path]) => path !== rel)
      .map(([, text]) => text);
    setSketchBaud(detectBaud([buffersRef.current.get(rel) ?? disk, ...others]));
  }, []);

  /** Load a sketch folder; opens `restoreFile` when present, else the main .ino. */
  const loadSketch = async (dir: string, restoreFile?: string): Promise<boolean> => {
    try {
      const [fs, yaml] = await Promise.all([
        api.listSketchFiles(dir),
        api.loadSketchYaml(dir),
      ]);
      // A sketchDir change is a hard agent boundary: the Σ chip, transcript
      // and chat recording are per-project. Placed after the awaits so a
      // failed open leaves the current project's session intact; skipped on
      // a same-dir reopen, which changes nothing about the project scope.
      if (sketchDirRef.current !== dir) teardownAgentSession("project switched");
      setSketchDir(dir);
      // The baud picker follows the project, so re-sniff before anything can
      // render the old sketch's rate against the new one's dir.
      void refreshSketchBaud(dir);
      setFiles(fs);
      setSketchYaml(yaml);
      // Reset synchronously before kicking off the async refresh: otherwise
      // the pill would render the previous project's state — bound to the
      // new project's handlers — for however long git_state takes to answer.
      // The pill briefly disappearing is honest; a stale one lying is not.
      setGitState(null);
      refreshGitState(dir);
      const profiles = Object.keys(yaml.profiles ?? {});
      const prof = yaml.default_profile ?? profiles[0] ?? null;
      setProfile(prof);
      const profPort = prof ? yaml.profiles?.[prof]?.port : undefined;
      if (profPort) setSelectedPort(profPort);
      buffersRef.current = new Map();
      // Placed with the editor reset, after the awaits: a project that fails
      // to open leaves the current one — and any jump parked against it —
      // alone. Past this point the old file is gone, and a target kept here
      // would fire at the first same-named file the new project opens.
      pendingGotoRef.current = null;
      setDirtyFiles(new Set());
      setOpenFile(null);
      setContent("");
      setOpenTabs([]);
      setArmedTab(null);
      // Opening a sketch dismisses any open form — a leftover New Project or
      // The Duplicate form would otherwise keep covering the editor.
      showPane(null);
      const name = dir.split("/").pop();
      const target =
        (restoreFile && fs.find((f) => f.rel_path === restoreFile)) ||
        fs.find((f) => f.rel_path === `${name}.ino`);
      if (target) {
        await openFileInEditor(dir, target.rel_path);
      } else {
        api.setLastSketch(dir, null).catch(() => {});
      }
      notify(`Opened ${dir}`);
      // One recency hook for every route in: picker, restore, new, duplicate and
      // a recents-click all funnel through loadSketch.
      api.pushRecentProject(dir).catch(() => {});
      return true;
    } catch (e) {
      notify(String(e), true);
      // A dir that no longer opens (moved, deleted) prunes itself.
      api.removeRecentProject(dir).catch(() => {});
      return false;
    }
  };

  /** Central file-open routine — every path that puts a file in the editor
   *  (tree click, create, delete's main.ino fallback, tab select, initial
   *  sketch load) funnels through here, so this is also the one place that
   *  opens its tab and disarms any pending close-confirmation.
   *
   *  Returns whether the file actually reached the editor. Every caller but
   *  `jumpToDiagnostic` ignores it — the failure is already a toast — but a
   *  parked jump has to know, and it cannot ask `openFileRef`: React has not
   *  re-rendered by the time this promise resolves, so that mirror still
   *  holds the previous file. */
  const openFileInEditor = async (
    dir: string,
    relPath: string,
  ): Promise<boolean> => {
    try {
      const text =
        buffersRef.current.get(relPath) ??
        (await api.readSketchFile(dir, relPath));
      setOpenFile(relPath);
      setContent(text);
      setOpenTabs((prev) => openTab(prev, relPath));
      setArmedTab(null);
      // keep the tree oriented: reveal the file's folder chain
      useExplorerStore.getState().expandTo(relPath);
      useExplorerStore.getState().select(relPath);
      api.setLastSketch(dir, relPath).catch(() => {});
      return true;
    } catch (e) {
      notify(String(e), true);
      return false;
    }
  };

  /** A click on a compiler diagnostic: show the file, put the cursor on the
   *  offending line. The file is often not the one on screen, and opening it
   *  is async, so the request is parked in `pendingGotoRef` and applied by
   *  the effect below once the editor is showing that document.
   *
   *  The parked target is the *token* for that request: every site that acts
   *  on it checks identity first and clears the ref before moving the
   *  cursor, so a second click always supersedes the first and no target is
   *  ever consumed twice. */
  const jumpToDiagnostic = async (t: JumpTarget) => {
    if (!sketchDir) return;
    // The user asked to see code, so retire whatever form is covering the
    // editor area (New/Duplicate/Rename project, the usage dashboard) —
    // otherwise the jump lands behind it, invisibly. The profile strip is
    // left alone; it covers nothing.
    showEditor();
    pendingGotoRef.current = t;
    const view = editorRef.current?.view;
    if (t.rel === openFileRef.current && view) {
      // Already the open document: nothing will re-render, so no effect
      // would fire. Jump now.
      pendingGotoRef.current = null;
      gotoLine(view, t.line, t.col);
      return;
    }
    const opened = await openFileInEditor(sketchDir, t.rel);
    // A diagnostic can name a file that has since been renamed or deleted.
    // Left parked, that target would fire at whatever file next happens to
    // open under that name. Only unpark our own: an overlapping second click
    // has already replaced it, and its open is still in flight.
    if (!opened && pendingGotoRef.current === t) pendingGotoRef.current = null;
  };

  /** Applies a parked jump once the editor is actually showing the file.
   *
   *  Child effects run before parent effects, so by the time this runs
   *  CodeMirror has usually already applied the new `value` — but "usually"
   *  is not "always". The guard is therefore an equality test on the whole
   *  document (cheap at sketch sizes; the length compare in front of it is
   *  the fast reject) against an EOL-normalised copy of `content`, so a
   *  same-length *different* document cannot pass. A mismatch retries on the
   *  next frame; if that still disagrees the jump is dropped. Turning "wrong
   *  document" into "no jump" is the safe direction: a wrong jump would
   *  silently point the user at an unrelated line.
   *
   *  Both paths claim the target — identity-check, then clear — before
   *  moving the cursor, so a jump requested in between supersedes rather
   *  than duplicates this one.
   *
   *  `editorShowing` is a dependency because of the one case where neither
   *  of the other two changes: a form was covering the editor and the
   *  diagnostic names the file that was already open, so re-opening it sets
   *  `openFile`/`content` to what they already were. Only the editor coming
   *  back tells us to look again. */
  useEffect(() => {
    const p = pendingGotoRef.current;
    const v = editorRef.current?.view;
    if (!p || !v || p.rel !== openFile) return;
    const want = normalizeEol(content);
    const shows = (d: { length: number; toString(): string }) =>
      d.length === want.length && d.toString() === want;
    if (shows(v.state.doc)) {
      pendingGotoRef.current = null;
      gotoLine(v, p.line, p.col);
      return;
    }
    requestAnimationFrame(() => {
      const v2 = editorRef.current?.view;
      if (v2 && pendingGotoRef.current === p && shows(v2.state.doc)) {
        pendingGotoRef.current = null;
        gotoLine(v2, p.line, p.col);
      }
    });
  }, [openFile, content, editorShowing]);

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
      // Otherwise a rename of a conflicted file leaves the conflict entry
      // pointing at a path that no longer exists — sendToAgent's guard
      // never trips, and Verify/Flash can clobber the assistant's on-disk
      // fix (now living under `to`) with the stale buffer still keyed under
      // `from`.
      agentConflictsRef.current = new Set(
        [...agentConflictsRef.current].map((p) => pathAfterRename(p, from, to, wasDir) ?? p),
      );
      setConflicts([...agentConflictsRef.current]);
      setOpenTabs((prev) => renameTabs(prev, from, to, wasDir));
      if (armedTab) {
        const np = pathAfterRename(armedTab, from, to, wasDir);
        if (np !== null) setArmedTab(np);
      }
      if (openFile) {
        const np = pathAfterRename(openFile, from, to, wasDir);
        if (np !== null) {
          setOpenFile(np);
          api.setLastSketch(sketchDir, np).catch(() => {});
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
      // Otherwise deleting a conflicted file leaves its entry in the set
      // forever — nothing can resolve a conflict on a path that no longer
      // exists, wedging sendToAgent's guard behind an unresolvable banner.
      for (const p of [...agentConflictsRef.current]) {
        if (affectedByDelete(p, relPath, wasDir)) agentConflictsRef.current.delete(p);
      }
      setConflicts([...agentConflictsRef.current]);
      const remainingTabs = deleteTabs(openTabs, relPath, wasDir);
      setOpenTabs(remainingTabs);
      if (armedTab && affectedByDelete(armedTab, relPath, wasDir)) setArmedTab(null);
      if (openFile && affectedByDelete(openFile, relPath, wasDir)) {
        const name = sketchDir.split("/").pop();
        const main = fs.find((f) => f.rel_path === `${name}.ino`);
        if (main) {
          await openFileInEditor(sketchDir, main.rel_path);
        } else {
          const next = remainingTabs[0] ?? null;
          if (next) {
            await openFileInEditor(sketchDir, next);
          } else {
            setOpenFile(null);
            setContent("");
            api.setLastSketch(sketchDir, null).catch(() => {});
          }
        }
      }
      notify(`Moved ${relPath} to the trash`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  /** Tab strip select — arming is a one-shot; any other action disarms it. */
  const handleTabSelect = (rel: string) => {
    setArmedTab(null);
    if (rel !== openFile && sketchDir) void openFileInEditor(sketchDir, rel);
  };

  /** Tab strip close (✕, middle-click, or the confirmed second click on an
   *  armed tab). A dirty buffer arms once instead of closing — the second
   *  close (armedTab === rel) discards it. */
  const handleTabClose = (rel: string) => {
    if (dirtyFiles.has(rel) && armedTab !== rel) {
      setArmedTab(rel);
      notify(`${rel} has unsaved changes — close again to discard`);
      return;
    }
    const result = closeTab(openTabs, rel, openFile);
    setOpenTabs(result.tabs);
    setArmedTab(null);
    buffersRef.current.delete(rel);
    setDirtyFiles((prev) => {
      if (!prev.has(rel)) return prev;
      const next = new Set(prev);
      next.delete(rel);
      return next;
    });
    if (agentConflictsRef.current.delete(rel)) setConflicts([...agentConflictsRef.current]);
    if (rel === openFile) {
      if (result.nextActive && sketchDir) {
        void openFileInEditor(sketchDir, result.nextActive);
      } else {
        setOpenFile(null);
        setContent("");
        if (sketchDir) api.setLastSketch(sketchDir, null).catch(() => {});
      }
    }
  };

  /** Context-menu "Close others" — dirty tabs are never bulk-closed. */
  const handleCloseOthers = (rel: string) => {
    const before = openTabs;
    const kept = closeOthers(before, rel, dirtyFiles);
    const keptDirty = kept.filter((t) => t !== rel && dirtyFiles.has(t)).length;
    setOpenTabs(kept);
    setArmedTab(null);
    for (const t of before) {
      if (!kept.includes(t)) buffersRef.current.delete(t);
    }
    if (openFile && !kept.includes(openFile) && sketchDir) {
      void openFileInEditor(sketchDir, rel);
    }
    if (keptDirty > 0) {
      notify(`Kept ${keptDirty} unsaved tab${keptDirty === 1 ? "" : "s"}`);
    }
  };

  /** Context-menu "Close all" — dirty tabs are never bulk-closed. */
  const handleCloseAll = () => {
    const before = openTabs;
    const kept = closeAll(before, dirtyFiles);
    setOpenTabs(kept);
    setArmedTab(null);
    for (const t of before) {
      if (!kept.includes(t)) buffersRef.current.delete(t);
    }
    if (openFile && !kept.includes(openFile)) {
      const next = kept[0] ?? null;
      if (next && sketchDir) {
        void openFileInEditor(sketchDir, next);
      } else {
        setOpenFile(null);
        setContent("");
        if (sketchDir) api.setLastSketch(sketchDir, null).catch(() => {});
      }
    }
    if (kept.length > 0) {
      notify(`Kept ${kept.length} unsaved tab${kept.length === 1 ? "" : "s"}`);
    }
  };

  const saveCurrent = useCallback(async () => {
    if (!sketchDir || !openFile) return;
    const text = buffersRef.current.get(openFile);
    if (text === undefined) return; // no unsaved edits
    try {
      await api.writeSketchFile(sketchDir, openFile, text);
      buffersRef.current.delete(openFile);
      // A saved edit may well be the `Serial.begin` rate itself.
      void refreshSketchBaud(sketchDir);
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
      setArmedTab(null);
      notify(`Saved ${openFile}`);
    } catch (e) {
      notify(String(e), true);
    }
  }, [sketchDir, openFile, notify, refreshSketchBaud]);

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
    setArmedTab(null);
    if (skipped.length > 0) {
      notify(conflictMessage(skipped), true);
    }
    refreshGitState(sketchDir);
    void refreshSketchBaud(sketchDir);
  }, [sketchDir, notify, refreshGitState, refreshSketchBaud]);

  /** Refuse a build/flash while the agent's edits and the user's disagree.
   *
   *  `saveAll` already declines to overwrite a conflicted buffer, but that
   *  alone is not enough for Verify/Upload: it only *warned*, and the build
   *  ran anyway. The user would see a compile — or a **flash** — of the
   *  agent's on-disk version while their own unsaved edits sat unwritten.
   *  Both callers now stop here, the same way `sendToAgent` already did.
   *  (The warning itself is a sticky error toast now, but a notice the user
   *  can dismiss is still no substitute for refusing the build.) */
  const refuseOnConflict = (action: string): boolean => {
    const block = blockedByConflict([...agentConflictsRef.current], action);
    if (block.blocked) notify(block.message ?? "", true);
    return block.blocked;
  };

  // Global accelerators: Ctrl+S save, Ctrl+O open. Both fire in inputs too —
  // Ctrl+O must preventDefault there anyway, and keys.ts scopes the
  // isTypingTarget guard to unmodified keys. openSketch is a per-render
  // const, so this re-subscribes every render — the codebase's tolerated
  // cost over a useCallback chain.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (matchesAccel(e, ACCEL_SAVE)) {
        e.preventDefault();
        saveCurrent();
      } else if (matchesAccel(e, ACCEL_OPEN)) {
        e.preventDefault();
        void openSketch();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [saveCurrent, openSketch]);

  // ---------- build & flash ----------

  /** FQBN of the board detected on the selected port, if any. Skips hidden
   *  umbrella entries (`esp32_family`) — compiling against those fails. */
  const detectedFqbn = () => {
    const p = ports.find((p) => p.port.address === selectedPort);
    return p ? visibleBoard(p)?.fqbn : undefined;
  };

  /** Build target: sketch.yaml profile first, detected board FQBN as fallback. */
  const resolveTarget = (): { profile?: string; fqbn?: string } | null => {
    if (profile) return { profile };
    const fqbn = detectedFqbn();
    if (fqbn) return { fqbn };
    notify(
      "No sketch.yaml profile, and this port reports no board identity (USB bridge) — create a profile to set the board.",
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
    beginActivity("compile", "Compiling…", "compile");
    // Declared out here because the `finally` cannot see `r`. The activity
    // has to be closed there and not on the happy path: an IPC call that
    // throws would otherwise leave "Compiling…" counting up for good.
    let ok = false;
    try {
      const r = await api.compileSketch(sketchDir, target.profile, target.fqbn);
      ok = r.success;
      notify(r.success ? "✓ Compile OK" : "Compile failed", !r.success);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      endActivity("compile", ok, ok ? "Compiled" : "Compile failed");
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
    // The profile silently outranks the detected board, so a disagreement is
    // announced as an error toast, which sticks until dismissed — it stays on
    // screen for the whole build and past the end of it.
    const profileFqbn = target.profile
      ? sketchYaml?.profiles?.[target.profile]?.fqbn
      : undefined;
    const detected = detectedFqbn();
    // Both facts are already known here: which kind of port is selected, and
    // whether the profile turns USB CDC on. When they disagree the board
    // flashes perfectly and prints nothing — the failure that is hardest to
    // read, because nothing about it looks like a configuration problem.
    const silent = silentSerialWarning(selectedPort, profileFqbn ?? target.fqbn);
    if (flashTargetMismatch(profileFqbn, detected)) {
      notify(
        `⚠ ${selectedPortName()} reports ${detected}, but profile “${target.profile}” builds for ${profileFqbn} — flashing the profile's board anyway…`,
        true,
      );
    } else if (silent) {
      notify(`⚠ ${silent}`, true);
    }
    // No `else` toast: the toasts above are *warnings*, and "building and
    // flashing…" is not an outcome — what is running belongs to the activity
    // channel, which says it immediately below (frontend.md §1, "two
    // announcement channels, and they do not mix").
    beginActivity("upload", `Flashing to ${selectedPortName()}…`, "upload");
    let ok = false;
    try {
      // Compiles as part of the flash — a sketch that fails to build stops
      // here with its compiler error and never reaches the board.
      const r = await api.uploadSketch(
        sketchDir,
        selectedPort,
        target.profile,
        target.fqbn,
      );
      ok = r.success;
      notify(
        r.success
          ? `✓ Flashed via ${selectedPort}`
          : "Build or upload failed — see the Build console",
        !r.success,
      );
      // Straight to debugging: the monitor tab opens now, and capture
      // resumes after the settle below (native-USB boards re-enumerate
      // after flashing, so give the port a moment to come back). A silent
      // sketch used to leave the user on the Build console with the
      // monitor secretly running.
      if (r.success) {
        openBottomTab("serial");
        // The FQBN this board was actually built for, for its fleet record.
        const usedFqbn =
          target.fqbn ??
          (target.profile
            ? sketchYaml?.profiles?.[target.profile]?.fqbn
            : undefined);
        setTimeout(() => {
          // Re-arms the standing request the flash cleared, so a port
          // that is still re-enumerating is chased rather than missed.
          requestCapture();
          // Recorded after the re-enumeration wait for the same reason the
          // monitor is: the port has to be back before it can be resolved to a
          // board. Fire-and-forget — a good flash must not fail over this.
          if (usedFqbn) {
            api.noteBoardFqbn(selectedPort, usedFqbn).catch(() => {});
          }
          // Outside the FQBN branch on purpose: the board re-enumerates
          // whether or not one was resolved, and offering the project just
          // flashed from would be noise either way.
          const flashed = fleet?.boards.find(
            (b) => b.last_port === selectedPort,
          );
          if (flashed) {
            flashCooldownRef.current.set(
              flashed.id,
              Date.now() + FLASH_COOLDOWN_MS,
            );
          }
        }, 1200);
      }
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      endActivity("upload", ok, ok ? "Flashed" : "Flash failed");
    }
  };

  // ---------- project git (checkpoint & sync) ----------

  const gitCommit = async (message: string) => {
    if (!sketchDir) return;
    await saveAll();
    try {
      const outcome = await api.gitCommit(sketchDir, message);
      notify(outcome === "committed" ? "✓ Committed" : "Nothing to commit");
    } catch (e) {
      notify(String(e), true);
    } finally {
      refreshGitState(sketchDir);
    }
  };

  const gitSync = async () => {
    if (!sketchDir) return;
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    beginActivity("sync", "Syncing…");
    let ok = false;
    try {
      const outcome = await api.gitSync(sketchDir);
      const msg: Record<api.SyncOutcome, [string, boolean]> = {
        synced: ["✓ Synced with origin", false],
        dirty_tree: ["Uncommitted changes — commit first", true],
        diverged: [
          "Diverged with conflicts — rebase aborted; resolve in a terminal, then Sync again",
          true,
        ],
        no_remote: ["No remote configured", true],
        not_root: ["Sync works from the repository root", true],
      };
      const [text, isErr] = msg[outcome];
      ok = !isErr;
      notify(text, isErr);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      endActivity("sync", ok, ok ? "Synced" : "Sync failed");
      refreshGitState(sketchDir);
    }
  };

  const gitInit = async () => {
    if (!sketchDir) return;
    const dir = sketchDir;
    try {
      const s = await api.gitInit(dir);
      // Guarded like refreshGitState: a slow init for a project the user has
      // since switched away from must not clobber the new one's state.
      if (dir === sketchDirRef.current) setGitState(s);
      notify("✓ Repository initialized (credential .gitignore + baseline commit)");
    } catch (e) {
      notify(String(e), true);
    }
  };

  /** Init a nested sketch as a repository of its own, so its flashes can be
   *  tagged. Guarded like gitInit against a project switch mid-flight. */
  const gitInitHere = async () => {
    if (!sketchDir) return;
    const dir = sketchDir;
    try {
      const s = await api.gitInitHere(dir);
      if (dir === sketchDirRef.current) setGitState(s);
      notify("✓ This sketch is now its own repository — flashes will be tagged");
    } catch (e) {
      notify(String(e), true);
    }
  };

  const gitCreateRemote = async (
    name: string,
    visibility: api.Visibility,
    description: string | null,
  ) => {
    if (!sketchDir) return;
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    beginActivity("remote", `Creating ${visibility} GitHub repo ${name}…`);
    let ok = false;
    try {
      await api.gitCreateRemote(sketchDir, name, visibility, description);
      ok = true;
      notify(`✓ Created and pushed to ${name}`);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      endActivity("remote", ok, ok ? "Repo created" : "Repo creation failed");
      refreshGitState(sketchDir);
    }
  };

  const gitSetRemote = async (url: string) => {
    if (!sketchDir) return;
    setBuildLines([]);
    openBottomTab("build");
    setUserBusy(true);
    beginActivity("remote", "Setting remote and pushing…");
    let ok = false;
    try {
      await api.gitSetRemote(sketchDir, url);
      ok = true;
      notify("✓ Remote set and pushed");
    } catch (e) {
      notify(String(e), true);
    } finally {
      setUserBusy(false);
      endActivity("remote", ok, ok ? "Remote set" : "Remote setup failed");
      refreshGitState(sketchDir);
    }
  };

  // ---------- serial monitor ----------

  /** Best-effort monitor start (auto-capture); errors are never announced. */
  /**
   * Try again later, while the standing request holds.
   *
   * Driven by two different failures, which is the point: a monitor that
   * *closed* (the child died with the port) and a start that *would not
   * open* (the port is not back yet). A failed start emits no
   * `serial://closed`, so hanging recovery off that event alone left the
   * monitor dead after every flash.
   */
  const scheduleRecapture = useCallback(() => {
    const busyNow = busyRef.current || agentFlashingRef.current;
    const plan = recapturePlan({
      wanted: monitorWantedRef.current,
      busy: busyNow,
      attempt: recaptureAttemptRef.current,
    });
    if (!plan.retry) {
      // Exhausted, as opposed to *deferred*: a flash owning the port also
      // stops the ladder, and that case must keep its standing request —
      // the post-flash `requestCapture()` is what resumes from it. Only a
      // genuine give-up clears the intent, and it has to say so: a chip
      // frozen at "↻ 5/5" forever reads as "still trying" when the board has
      // in fact been unplugged and carried off.
      if (
        monitorWantedRef.current &&
        !busyNow &&
        recaptureAttemptRef.current >= MAX_RECAPTURE_ATTEMPTS
      ) {
        monitorWantedRef.current = false;
        recaptureAttemptRef.current = 0;
        setRecaptureAttempt(0);
        serialStore.push("info", "— gave up re-opening the port —", Date.now());
      }
      return;
    }
    recaptureAttemptRef.current += 1;
    // Mirrored for the status chip. Once per rung of the ladder, so the cost
    // is a render per retry rather than one per serial line.
    setRecaptureAttempt(recaptureAttemptRef.current);
    window.clearTimeout(recaptureTimerRef.current);
    recaptureTimerRef.current = window.setTimeout(
      () => void startMonitorQuietRef.current?.(),
      plan.delayMs,
    );
  }, []);

  const startMonitorQuiet = useCallback(async () => {
    if (monitorOnRef.current) return;
    // Automatic capture must never take the port from esptool — the manual
    // Start button (toggleMonitor) is the deliberate override. Checked here
    // and not only where a recapture is scheduled, because the ladder
    // decides a second or more before it acts and a flash can start in
    // between.
    if (busyRef.current || agentFlashingRef.current) return;
    // No port yet: a native-USB board takes a couple of seconds to come back
    // after a reset. Keep asking rather than giving up silently.
    if (!selectedPort) {
      scheduleRecapture();
      return;
    }
    try {
      // The returned session id is what lets a `serial://closed` from the
      // child we just replaced be told apart from this one dying.
      monitorSessionRef.current = await api.startMonitor(
        selectedPort,
        baudrate,
      );
      openBaudRef.current = baudrate;
      setMonitorOn(true);
      // A marker rather than a wipe: the scrollback from before a reset is
      // usually the half you wanted to read (the stack trace, the last
      // print), so the log is never cleared on start.
      serialStore.push(
        "info",
        `— monitor started at ${openBaudRef.current} baud —`,
        Date.now(),
      );
      // Capture is wanted from here until something explicitly stops it, and
      // the ladder resets so the next dropout starts from 1 s again.
      monitorWantedRef.current = true;
      recaptureAttemptRef.current = 0;
      setRecaptureAttempt(0);
    } catch {
      // The port exists but will not open — still re-enumerating, or briefly
      // held by a dying previous child. This is the case that used to end
      // capture for good after a flash.
      scheduleRecapture();
    }
  }, [selectedPort, baudrate, scheduleRecapture]);

  /**
   * Ask for capture, and keep asking.
   *
   * The standing request has to be re-armed explicitly because the flash
   * path cleared it to free the port for esptool.
   */
  const requestCapture = useCallback(() => {
    monitorWantedRef.current = true;
    recaptureAttemptRef.current = 0;
    setRecaptureAttempt(0);
    void startMonitorQuiet();
  }, [startMonitorQuiet]);

  // The serial://closed handler is registered once, in an empty-dep effect,
  // so it cannot close over the current callback.
  const startMonitorQuietRef = useRef(startMonitorQuiet);
  startMonitorQuietRef.current = startMonitorQuiet;
  const scheduleRecaptureRef = useRef(scheduleRecapture);
  scheduleRecaptureRef.current = scheduleRecapture;

  // Capture by default: start the monitor whenever a port is (auto-)selected.
  useEffect(() => {
    startMonitorQuiet();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedPort]);

  // Mirror the UI's selected target into Rust so the agent's upload and
  // serial_read tools flash/watch the board the user is looking at.
  useEffect(() => {
    api.setSelectedTarget(selectedPort, baudrate).catch(() => {});
  }, [selectedPort, baudrate]);

  // Capture by default, part 2: the Serial Monitor tab being visible is a
  // standing request for capture. The port-selection auto-start above is a
  // one-shot whose failure is deliberately silent, so a lost attempt (port
  // briefly held by a dying previous monitor child, say) left the monitor
  // dead until a manual Start. Edge-triggered on tab/port — monitor state
  // is read via refs inside startMonitorQuiet, deliberately NOT a dep, so
  // Stop while staying on the tab stays stopped; leaving and returning
  // re-requests capture. busyRef guards flash-time port contention.
  useEffect(() => {
    if (bottomTab !== "serial" || busyRef.current || agentFlashingRef.current)
      return;
    startMonitorQuiet();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bottomTab, selectedPort, startMonitorQuiet]);

  const toggleMonitor = async () => {
    try {
      if (monitorOn) {
        // An explicit stop, exactly like stopMonitorIfOn's — and the flash
        // path calls *this* one to free the port. Without clearing the
        // intent here, the recapture ladder took the port straight back off
        // esptool mid-flash.
        monitorWantedRef.current = false;
        window.clearTimeout(recaptureTimerRef.current);
        await api.stopMonitor();
        setMonitorOn(false);
      } else {
        if (!selectedPort) {
          notify("Select a port first", true);
          return;
        }
        monitorSessionRef.current = await api.startMonitor(
          selectedPort,
          baudrate,
        );
        openBaudRef.current = baudrate;
        setMonitorOn(true);
        serialStore.push(
          "info",
          `— monitor started at ${openBaudRef.current} baud —`,
          Date.now(),
        );
        openBottomTab("serial");
      }
    } catch (e) {
      notify(String(e), true);
    }
  };

  /** In-flight latch for the restart below. Two quick baud changes — or one
   *  racing the drift effect — must not interleave their stop/start pairs
   *  into two children fighting over one port. */
  const restartingRef = useRef(false);

  /**
   * Re-open the live monitor at `next`.
   *
   * Shared by the baud picker and the drift effect, and the only place that
   * hands the port over to a new child of our own making. Stop strictly
   * before start, and disarm the recapture ladder before either — left armed
   * it chases the port we are deliberately letting go and re-opens it at the
   * *old* rate.
   */
  const restartMonitorAt = useCallback(
    async (next: number) => {
      // The busy guard lives here as well as in the two callers: they decide
      // a beat before the port actually changes hands, and esptool must win
      // every race. A restart skipped this way is not lost — the new rate
      // applies at the post-flash start.
      if (busyRef.current || agentFlashingRef.current) return;
      if (!selectedPort || restartingRef.current) return;
      restartingRef.current = true;
      try {
        monitorWantedRef.current = false;
        window.clearTimeout(recaptureTimerRef.current);
        await api.stopMonitor();
        setMonitorOn(false);
        // Between here and the start below `monitorSessionRef` still names
        // the child we just killed, so its `serial://closed` is *accepted* —
        // harmlessly: it writes the "closed" marker and finds the ladder
        // already disarmed, so nothing chases the port out from under us.
        monitorSessionRef.current = await api.startMonitor(selectedPort, next);
        openBaudRef.current = next;
        setMonitorOn(true);
        monitorWantedRef.current = true;
        recaptureAttemptRef.current = 0;
        setRecaptureAttempt(0);
        serialStore.push(
          "info",
          `— monitor restarted at ${openBaudRef.current} baud —`,
          Date.now(),
        );
      } catch (e) {
        // The stop landed and the re-open did not. Without re-arming here the
        // standing request is gone *and* the ladder is disarmed, so nothing
        // ever brings the monitor back — the user is left staring at a dead
        // panel wondering why Start is the only thing that works.
        monitorWantedRef.current = true;
        scheduleRecapture();
        notify(String(e), true);
      } finally {
        restartingRef.current = false;
      }
    },
    [selectedPort, notify, scheduleRecapture],
  );

  /**
   * Set (or, with `null`, clear) this sketch's baud override — and re-open
   * the port at the new rate if the monitor is running.
   *
   * Changing the number without re-opening the port is the classic way to
   * spend ten minutes reading mojibake: the child already holds the port at
   * the old rate and nothing else would ever restart it. When there is
   * nothing to restart — no monitor, no port, or a flash owning the port —
   * the choice is still stored and simply applies at the next start.
   */
  const changeBaud = async (baud: number | null) => {
    // Persisted per sketch when there is one to key it under; otherwise held
    // for the session. Either way the picker moves and the port follows —
    // returning early with no project open left the `<select>` showing a rate
    // nothing was listening at.
    if (sketchDir)
      setBaudOverrides(saveBaudOverride(localStorage, sketchDir, baud));
    else setNoProjectBaud(baud);
    const next = effectiveBaud(baud ?? undefined, sketchBaud).baud;
    // `monitorOnRef`, not the render value: this handler is called from the
    // panel's own event closure, which may predate the last close.
    if (
      !monitorOnRef.current ||
      !selectedPort ||
      busyRef.current ||
      agentFlashingRef.current
    )
      return;
    await restartMonitorAt(next);
  };

  // Baud drift. The picker's rate is *derived* — opening another project, or
  // saving a sketch whose `Serial.begin` changed, moves it under a child that
  // is still reading at the old rate. A monitor quietly decoding at the wrong
  // rate is the most confusing failure this panel has (it looks like a broken
  // board), so re-open rather than let the two disagree. `openBaudRef` is
  // null only before the first start, which nothing has drifted from yet.
  useEffect(() => {
    if (!monitorOn || busyRef.current || agentFlashingRef.current) return;
    if (openBaudRef.current === null || openBaudRef.current === baudrate)
      return;
    void restartMonitorAt(baudrate);
  }, [monitorOn, baudrate, restartMonitorAt]);

  // ---------- scope ----------

  /** Start the serial monitor if it is off (plotter source needs it). */
  const ensureMonitor = useCallback(async () => {
    if (monitorOn) return;
    if (!selectedPort) {
      notify("Select a port first", true);
      return;
    }
    monitorSessionRef.current = await api.startMonitor(selectedPort, baudrate);
    openBaudRef.current = baudrate;
    setMonitorOn(true);
    serialStore.push(
      "info",
      `— monitor started at ${openBaudRef.current} baud —`,
      Date.now(),
    );
    openBottomTab("serial");
  }, [monitorOn, selectedPort, baudrate, notify]);

  /** Stop the serial monitor if it is on (ADC streaming needs the port). */
  const stopMonitorIfOn = useCallback(async () => {
    // Clear the intent even when no child is alive: the scope and the flash
    // path both call this to claim the port, and a recapture already in
    // flight would take it straight back off them.
    monitorWantedRef.current = false;
    window.clearTimeout(recaptureTimerRef.current);
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
      // One activity spanning both halves — the user asked for "flash the
      // companion firmware", not for a compile and then an upload. The label
      // is re-worded in place when the second half starts; `startedAt` is
      // untouched so the clock keeps running through the handover.
      //
      // The *progress parser*, though, is per half. Armed as "upload" here it
      // would read this compile's own `Sketch uses …` as the handover to
      // flashing and show "uploading" for the whole compile; it gets a fresh
      // upload parser below, once there really is an upload.
      beginActivity("firmware", "Compiling companion firmware…", "compile");
      let ok = false;
      try {
        const c = await api.compileSketch(dir, chipProfile);
        if (!c.success) {
          notify("Companion firmware compile failed", true);
          return false;
        }
        const relabel = (a: Activity | null) =>
          a && {
            ...a,
            label: `Flashing companion firmware to ${selectedPortName()}…`,
          };
        activityRef.current = relabel(activityRef.current);
        setActivity(relabel);
        // Second half: a parser that has seen none of the compile's output.
        const flashProgress = startProgress("upload");
        progressRef.current = flashProgress;
        setProgress(flashProgress);
        const u = await api.uploadSketch(dir, selectedPort, chipProfile);
        ok = u.success;
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
        endActivity(
          "firmware",
          ok,
          ok ? "Companion firmware flashed" : "Companion firmware failed",
        );
      }
    },
    [selectedPort, stopMonitorIfOn, notify, beginActivity, endActivity],
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
    gitStateRefreshRef.current(dir);
    // The assistant rewriting `Serial.begin` is exactly the case where a
    // silently stale picker costs a bench session.
    void refreshSketchBaud(dir);

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
      // Same bar as a user-driven build: whose build it is changes the
      // wording, not whether the machine looks busy.
      if (ev.type === "verify_started") {
        // Same reset `verify()` does, so the console and the Build badge
        // describe this compile and not the last one. No tab switch: the
        // agent's build is not what the user is looking at, and the unseen
        // dot plus the error badge already say it happened.
        setBuildLines([]);
        beginActivity("agent_compile", "Assistant compiling…");
      } else {
        endActivity(
          ["agent_compile"],
          ev.success === true,
          "Assistant compile",
        );
      }
      return;
    }
    // An agent flash: disable the toolbar's build buttons like a verify,
    // and suppress the monitor auto-start effect until the port is free
    // again — a monitor stealing the port mid-flash kills the flash.
    if (ev.type === "upload_started" || ev.type === "upload_done") {
      const pid = typeof ev.pid === "number" ? ev.pid : undefined;
      const ours = agentStore.snapshot().pid;
      if (pid !== undefined && ours !== undefined && pid !== ours) return;
      const flashing = ev.type === "upload_started";
      agentFlashingRef.current = flashing;
      setAgentBuilding(flashing);
      // The agent's flash streams through the same build console, so the
      // same parser reads its progress — hence the `"upload"` op.
      if (ev.type === "upload_started") {
        setBuildLines([]);
        beginActivity("agent_upload", "Assistant flashing…", "upload");
      } else {
        endActivity(["agent_upload"], ev.success === true, "Assistant flash");
      }
      if (!flashing) openBottomTab("serial");
      return;
    }
    // The host has already killed the child, so no verify_done is coming.
    if (ev.type === "security_alarm") {
      setAgentBuilding(false);
      agentFlashingRef.current = false;
      // No verdict: the child was killed mid-op, so nothing finished. A
      // `lastResult` here would leave "✗ Assistant compile" on the bar for a
      // compile that was never allowed to fail on its own terms.
      clearAgentActivity();
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

  /** Ingest one `agent://event`: push into the store, shadow it to the chat
   *  recorder, flag the tab unseen, run the side-effect handler. Factored
   *  out of the App-level listener so a `ResumeWatch`'s buffered/flushed
   *  events go through the exact same path a live event would. */
  const deliverAgentEvent = (ev: AgentEvent) => {
    // Between teardownAgentSession's clear()/prepareContinuation() and the
    // next sessionStarted, the store sits idle with no pid — the only
    // events that can still land in that window are a just-torn-down
    // session's flushed stragglers (queued before agentStop resolved).
    // Pushing one would repaint a store nobody owns yet; recording one
    // would write a dead session's tail into whatever chat is live now (or
    // start a new file for none at all). Skip both; side effects below
    // already carry their own pid guards.
    const snap = agentStore.snapshot();
    const betweenSessions = snap.status === "idle" && snap.pid === undefined;
    if (!betweenSessions) {
      agentStore.push(ev);
      // Deltas are transient duplicates of the authoritative assistant
      // event that follows them — recording them would write the same
      // text twice and fire one IPC append per streamed fragment. Replay
      // of completed turns is unchanged without them; only an
      // interrupted turn's partial text is lost from history.
      if (ev.type !== "stream_event") chatRecorder.record({ op: "push", ev });
    }
    if (bottomTabRef.current !== "agent")
      setUnseen((u) => ({ ...u, agent: true }));
    handleAgentSideEffects(ev);
  };

  /** Save-then-send, auto-starting the session on the first message with the
   *  same target Verify uses — or, when `continueChat` just stashed a saved
   *  chat, resuming it instead of starting blank: `agentStart` gets the
   *  stash's `sessionId` (native `--resume`) or, when the chat never
   *  recorded one, its `facts` straight away (no resume attempted). A
   *  native attempt is watched: its outcome is unknowable until the child
   *  either proves itself (`system/init`, via `deliverAgentEvent`) or dies
   *  first (`fallbackRespawn`). Refuses while a dirty-conflict is
   *  unresolved. The auto-start (target resolution + `agentStart`) happens
   *  *before* `userSent` — failing to resolve a target or spawn the child
   *  must not leave a phantom "sent" bubble with nothing behind it. */
  const sendToAgent = async (text: string) => {
    if (agentConflictsRef.current.size > 0) {
      notify(
        "Resolve the assistant's file conflict first — save the file (Ctrl+S), then try again.",
        true,
      );
      return;
    }
    if (!sketchDir) {
      notify("Open a project first.", true);
      return;
    }
    // Captured before the first await below — re-checked after every await
    // in this function so a teardown/"New session"/project switch that lands
    // mid-send can't bind the spawned (or about-to-spawn) child to a project
    // it no longer belongs to, unrecorded.
    const epoch = teardownEpochRef.current;
    await saveAll();
    // A teardown/continue raced ahead of us during the save — this send no
    // longer belongs to any session we still own. Bail silently: nothing
    // user-visible has happened yet (no bubble, no spawn).
    if (teardownEpochRef.current !== epoch) return;
    // saveAll refuses to flush a conflicted buffer, so re-check: a conflict
    // raised between the guard above and here still blocks the send.
    if (agentConflictsRef.current.size > 0) return; // saveAll already notified
    // Arm the recorder before the session can produce its first op. Writes
    // nothing until the first record, so a send that fails below leaves no
    // empty chat file behind. `continueChat` already armed it via `resume`
    // (appending to the SAME file) — `active` is true either way.
    if (!chatRecorder.active) {
      const now = new Date();
      chatRecorder.start(
        chatFileName(now),
        { sketchDir, startedAt: now.toISOString() },
        (f, l) => api.chatAppend(sketchDir, f, l),
      );
    }
    if (agentStore.snapshot().status === "idle") {
      const target = resolveTarget();
      if (!target) return; // resolveTarget already notified
      const stash = continuationRef.current;
      try {
        // The pid identifies this session for every host event that can
        // outlive it (FE-C1) — record it before any of them can arrive.
        // A continuation session always starts unarmed regardless of the
        // toggle's current value — same dangerous-by-default invariant as
        // "New session", deliberately not inherited from a chat's past.
        const pid = await api.agentStart(
          sketchDir,
          target.profile,
          target.fqbn,
          stash ? false : uploadsArmedRef.current,
          stash?.sessionId,
          stash?.sessionId ? undefined : stash?.facts,
        );
        // A teardown/continue raced ahead of us while the child was
        // spawning: this pid belongs to no chat we still track. Reap it
        // (pid-scoped, so it can never touch whatever session superseded
        // this send) and bail — touching neither store nor recorder.
        if (teardownEpochRef.current !== epoch) {
          void api.agentStop(pid);
          return;
        }
        agentStore.sessionStarted(pid);
        chatRecorder.record({ op: "sessionStarted", pid });
        // A continuation spawn (native-resume attempt or facts-only) always
        // starts unarmed on the backend (the `stash ? false : ...` above) —
        // snap the UI toggle to match so a fast click on "Allow uploads"
        // between `continueChat` and this send can't leave it showing armed
        // while the child is actually locked.
        if (stash) {
          setUploadsArmed(false);
          // Belt-and-suspenders: the backend already started unarmed, but a
          // toggle click that raced in between the spawn and this line could
          // have flipped it. Best-effort re-disarm so backend and UI cannot
          // diverge.
          api.agentSetUploadsArmed(false).catch(() => {});
        }
        if (stash?.sessionId) {
          // Native resume attempted: hold events back until the child
          // proves itself or dies trying. Capture the teardown epoch now —
          // this is the moment the fallback is "armed" — so a teardown or a
          // newer `continueChat` that lands during `fallbackRespawn`'s later
          // awaits is detectable even though the stash-identity check alone
          // cannot see it (see `fallbackRespawn`'s doc).
          const armEpoch = teardownEpochRef.current;
          resumeWatchRef.current = createResumeWatch({
            pid,
            onConfirmed: () => {
              // The attempt is live — whether proven by `system/init` or by
              // the watch's own timeout backstop (which fires `onConfirmed`
              // even with nothing buffered — `onDeliver` alone can't signal
              // that case). The stash has done its job either way.
              continuationRef.current = null;
            },
            onDeliver: (ev) => {
              deliverAgentEvent(ev as AgentEvent);
            },
            onFailed: () => {
              resumeWatchRef.current = null;
              void fallbackRespawn(pid, stash, text, armEpoch);
            },
          });
        } else if (stash) {
          // No session_id was ever recorded for this chat — a facts-only
          // spawn from the start, no resume attempted. Consumed immediately.
          continuationRef.current = null;
        }
      } catch (e) {
        notify(String(e), true);
        return;
      }
    }
    agentStore.userSent(text);
    chatRecorder.record({ op: "userSent", text });
    try {
      await api.agentSend(text);
    } catch (e) {
      notify(String(e), true);
    }
  };

  /** A native `--resume` attempt died before proving itself (its pid's
   *  `closed` arrived with no `system/init` ever seen) — reap it and start a
   *  fresh session carrying the stash's distilled facts instead, then resend
   *  the message that triggered the attempt (its `userSent` op and store
   *  bubble already exist from `sendToAgent` — only the wire send is redone,
   *  never duplicated).
   *
   *  Guarded by stash identity (`continuationRef.current !== stash` means
   *  `teardownAgentSession` ran or a newer `continueChat` replaced it) AND
   *  by `epoch`, the `teardownEpochRef` snapshot taken at the moment this
   *  fallback was armed (in `sendToAgent`, when the resume watch was
   *  created). Stash identity alone is a single point-in-time check made
   *  before any `await` in this function; it cannot see a teardown or a
   *  second `continueChat` that lands *during* one of the awaits below (the
   *  window between `agentStop`/`agentStart` resolving), which would
   *  otherwise leave an orphaned child running, write `sessionStarted`/
   *  `userSent` ops into whatever chat is now live, or send this stash's
   *  `pendingText` into a session it no longer belongs to. `epoch` is
   *  re-checked after every await for exactly that reason, and the function
   *  aborts silently (reaping any child it already spawned, pid-scoped so it
   *  can never touch a successor session) the instant it detects the world
   *  moved on without it.
   *
   *  `stash.sketchDir` (captured at stash-creation time from
   *  `sketchDirRef.current`, not read again from a render closure) is used
   *  for the respawn — and re-checked against the live `sketchDirRef`
   *  before spawning — so a project switch mid-flight can never respawn
   *  into the wrong directory. */
  const fallbackRespawn = async (
    oldPid: number,
    stash: ContinuationStash,
    pendingText: string,
    epoch: number,
  ) => {
    if (continuationRef.current !== stash) return; // superseded — drop it
    if (teardownEpochRef.current !== epoch) return; // a teardown/continue raced ahead of us
    continuationRef.current = null; // consumed: this is the fallback spawn
    await api.agentStop(oldPid).catch(() => {});
    if (teardownEpochRef.current !== epoch) return; // moved on while stopping the old child
    if (sketchDirRef.current !== stash.sketchDir) return; // project switched under us
    const target = resolveTarget();
    if (!target) return; // resolveTarget already notified
    let pid2: number | undefined;
    try {
      pid2 = await api.agentStart(
        stash.sketchDir,
        target.profile,
        target.fqbn,
        false,
        undefined,
        stash.facts,
      );
      if (teardownEpochRef.current !== epoch) {
        // Superseded while spawning: this child belongs to no chat we still
        // track. Reap it (pid-scoped, so it can never kill whatever
        // teardownAgentSession/continueChat already started in its place)
        // and stop — touching neither store nor recorder, which now belong
        // to the session that superseded this attempt.
        await api.agentStop(pid2).catch(() => {});
        return;
      }
      // Matches sendToAgent's continuation-spawn invariant: always unarmed,
      // and the toggle must show it.
      setUploadsArmed(false);
      // Belt-and-suspenders: the backend already started unarmed (`false`
      // above), but a toggle click racing in between the spawn and this line
      // could have flipped it. Best-effort re-disarm so backend and UI
      // cannot diverge.
      api.agentSetUploadsArmed(false).catch(() => {});
      agentStore.sessionStarted(pid2);
      chatRecorder.record({ op: "sessionStarted", pid: pid2 });
      await api.agentSend(pendingText);
    } catch (e) {
      if (teardownEpochRef.current !== epoch) return; // don't stomp a successor's state
      notify(String(e), true);
      // pid-scoped: agentStore.closed(reason) with no pid matches ANY
      // current session (see AgentStore.isOurs), which could flip a
      // successor session that has since taken over to "ended". Scope it to
      // whichever pid this attempt actually owns.
      agentStore.closed("continuation fallback failed", pid2 ?? oldPid);
      chatRecorder.record({ op: "closed", reason: "continuation fallback failed" });
      chatRecorder.stop();
    }
  };

  const interruptAgent = () => {
    // Stop must never trigger fallbackRespawn: a pending resume watch that
    // outlives the interrupt would see the child die (interrupted, same as
    // any other exit) and treat it as a failed resume attempt, respawning a
    // fresh session and re-sending the just-interrupted message. Cancel the
    // watch and drop the continuation stash first so neither is left to fire.
    resumeWatchRef.current?.cancel();
    resumeWatchRef.current = null;
    continuationRef.current = null;
    api.agentInterrupt().catch((e) => notify(String(e), true));
  };

  /** Arm/disarm agent uploads: local state for the pre-session case, and the
   *  live session's flag via the backend (a no-op when none is running). */
  const toggleUploadsArmed = (armed: boolean) => {
    setUploadsArmed(armed);
    api.agentSetUploadsArmed(armed).catch((e) => notify(String(e), true));
  };

  /** Hard session boundary: stop the child, close the recording, drop the
   *  transcript/usage and agent-related UI flags. Idempotent — safe when no
   *  session exists (startup restore, double-switch). */
  const teardownAgentSession = (reason: string) => {
    // Every teardown is a session boundary an in-flight `fallbackRespawn`
    // must be able to detect across its awaits — see `teardownEpochRef`'s
    // doc and `fallbackRespawn`.
    teardownEpochRef.current++;
    // Replays stay honest: a torn-down chat ends with a closed line, not a
    // silent truncation. Status guard: an armed-but-never-started recorder
    // (agentStart threw before userSent → still "idle") must not flush a
    // meta+closed-only file, and after a natural end ("ended") the
    // onAgentClosed listener already wrote the real closed op.
    const status = agentStore.snapshot().status;
    if (chatRecorder.active && status !== "idle" && status !== "ended") {
      chatRecorder.record({ op: "closed", reason });
    }
    api.agentStop().catch(() => {});
    chatRecorder.stop();
    agentStore.clear();
    agentConflictsRef.current.clear();
    setConflicts([]);
    // The stopped session's verify (if any) will never emit `verify_done`
    // now — the backend cancels it — so release the flag here rather than
    // leaving the toolbar disabled forever.
    setAgentBuilding(false);
    agentFlashingRef.current = false;
    // Same reasoning, for the bar: no `verify_done` is coming, so the clock
    // has to be stopped here or it counts up forever. No verdict — the op
    // was cancelled, not decided.
    clearAgentActivity();
    // Arming is per-session and dangerous-by-default: a new session starts
    // locked no matter what the last one was allowed to do.
    setUploadsArmed(false);
    // A pending resume watch must not deliver or fail into whatever comes
    // next, and a stale continuation stash must never be picked up by a
    // later plain message — see `ContinuationStash` and `fallbackRespawn`.
    resumeWatchRef.current?.cancel();
    resumeWatchRef.current = null;
    continuationRef.current = null;
  };

  /** Stop the session and drop the transcript back to a clean slate. */
  const newAgentSession = () => teardownAgentSession("new session");

  /** "Continue this chat" (History → ▶): re-entry gate, then tear down any
   *  live session (killing it, and — critically — cancelling any pending
   *  resume watch and dropping any prior continuation stash so a stale
   *  fallback can never respawn over this one; `uploadsArmed` lands back at
   *  false, same as any fresh session). Replays the saved chat's ops into
   *  the LIVE singleton store via `prepareContinuation()` (never `clear()`
   *  — the whole point is to keep the transcript, sessionId, usage and raw
   *  log), stashes what `sendToAgent`'s idle branch needs to actually
   *  resume, and arms the recorder to append to the SAME file rather than
   *  start a new one. */
  const continueChat = async (file: string) => {
    if (!sketchDir) return; // re-entry gate — no project, nothing to continue into
    // This call is itself a session boundary — bump the epoch on entry, in
    // addition to the bump inside `teardownAgentSession` right below, so any
    // `fallbackRespawn` already in flight (from whatever session existed
    // before this call) is guaranteed stale as of this exact line, before
    // any `await` here gives it a chance to run. See `teardownEpochRef`'s
    // doc.
    teardownEpochRef.current++;
    teardownAgentSession("continued another chat");
    // Captured *after* both bumps above (this call's own, plus
    // teardownAgentSession's) — the epoch this continuation owns from here
    // on. Re-checked after the chatLoad await below so a second ▶, a "New
    // session", or a project switch that lands mid-load can't merge its
    // transcript into (or resurrect) a torn-down chat. Same pattern
    // `fallbackRespawn` uses for its own awaits.
    const epoch = teardownEpochRef.current;
    const startDir = sketchDirRef.current;
    try {
      const lines = await api.chatLoad(sketchDir, file);
      // The project may have been closed or switched while chatLoad was in
      // flight — re-read the live ref rather than trusting the `sketchDir`
      // closure captured before the await. Also bail if a teardown/continue
      // raced ahead of us: no state writes past this point belong to us.
      if (teardownEpochRef.current !== epoch) return;
      const dir = sketchDirRef.current;
      if (!dir || dir !== startDir) return;
      applyChatOps(agentStore, lines);
      agentStore.prepareContinuation();
      const snap = agentStore.snapshot();
      continuationRef.current = {
        file,
        sessionId: snap.sessionId,
        facts: distillFacts(snap),
        sketchDir: dir,
      };
      chatRecorder.resume(file, (f, l) => api.chatAppend(dir, f, l));
    } catch (e) {
      notify(String(e), true);
    }
  };

  // ---------- status bar inputs ----------

  /** How long this op took last time, for the "usually ~" hint and for the
   *  dashed estimate bar. Only compiles and uploads are remembered — a sync
   *  is dominated by the network and a remembered number would be a lie. */
  const estimate = useMemo(
    () =>
      activity &&
      sketchDir &&
      (activity.key === "compile" || activity.key === "upload")
        ? (loadDurations(window.localStorage, sketchDir)?.[
            activity.key === "compile" ? "compileMs" : "uploadMs"
          ] ?? null)
        : null,
    [activity, sketchDir],
  );

  /** A flash owns the serial port; nothing else here does. `busy` is wider
   *  than that — a verify or a fleet sync sets it too — and the Monitor's
   *  Start/Stop button must only be taken away for the case that actually
   *  holds the port. Read off the activity rather than `agentFlashingRef`
   *  because a ref does not re-render the button when it changes. */
  const flashOwnsPort =
    activity !== null &&
    (activity.key === "upload" ||
      activity.key === "agent_upload" ||
      activity.key === "firmware");

  /** What the Monitor's status chip shows. `retrying` only while the standing
   *  request is still live. The attempt counter alone is not enough: a
   *  deliberate Stop (and the flash handoff, which goes through the same
   *  path) clears the standing request but leaves the counter where the last
   *  ladder left it, so a stale "↻ 3/5" would sit under a monitor the user
   *  switched off. A genuine give-up zeroes both. */
  const serialConnection: SerialConnection = monitorOn
    ? { state: "on" }
    : recaptureAttempt > 0 && monitorWantedRef.current
      ? {
          state: "retrying",
          attempt: recaptureAttempt,
          max: MAX_RECAPTURE_ATTEMPTS,
        }
      : { state: "off" };

  // No clock here: `StatusBar` owns the only ticking `now` in the app and
  // derives the estimate fraction from it. A second interval up here would
  // re-render the whole tree twice a second to move one dashed bar.

  // ---------- render ----------

  return (
    <div className="app">
      <Toolbar
        sketchDir={sketchDir}
        sketchYaml={sketchYaml}
        profile={profile}
        ports={ports}
        selectedPort={selectedPort}
        fleet={fleet}
        busy={busy}
        onOpenProject={openSketch}
        onOpenRecent={(dir) => void loadSketch(dir)}
        onNewProject={() => showPane("new")}
        onDuplicateProject={() => showPane("duplicate")}
        onRenameProject={() => showPane("rename")}
        onOpenUsage={() => showPane("usage")}
        onCreateProfile={() => showPane(null, "bootstrap")}
        onAddProfile={() => showPane(null, "add")}
        onRetargetProfile={() => showPane(null, "retarget")}
        onSelectProfile={selectProfile}
        onSelectPort={setSelectedPort}
        onRefreshPorts={refreshPorts}
        onVerify={verify}
        onUpload={upload}
        gitState={gitState}
        ghAvailable={ghOk}
        onGitCommit={gitCommit}
        onGitSync={gitSync}
        onGitInit={gitInit}
        onGitInitHere={gitInitHere}
        onGitCreateRemote={gitCreateRemote}
        onGitSetRemote={gitSetRemote}
      />

      {profileForm && sketchDir && (
        <ProfileInit
          key={`${profileForm}:${profile ?? ""}`}
          mode={profileForm}
          sketchDir={sketchDir}
          detectedFqbn={detectedFqbn() ?? null}
          currentProfile={profile}
          currentFqbn={profile ? (sketchYaml?.profiles?.[profile]?.fqbn ?? null) : null}
          onDone={(yaml, prof) => {
            setSketchYaml(yaml);
            setProfileForm(null);
            // Select the touched profile; applies its pinned port if any.
            selectProfile(prof);
          }}
          onCancel={() => setProfileForm(null)}
          notify={notify}
          onYamlChanged={setSketchYaml}
        />
      )}

      {/* Tied to the conflict itself, not to a notification. `saveAll` used
          to warn about an unflushed conflicted buffer with a transient
          status line that "Compiling…" overwrote a moment later, leaving the
          user looking at a build of the assistant's on-disk version with no
          sign their own edits were still unsaved. A toast would survive that
          now, but it is still dismissible; this stays until the conflict is
          actually resolved, and Verify/Upload refuse while it is up. */}
      {offer && (
        <BoardOffer
          offer={offer}
          drift={
            offerDrift === null
              ? undefined
              : offerDrift.kind === "ahead"
                ? offerDrift.commits
                : null
          }
          missing={offerDrift?.kind === "missing"}
          armed={offerArmed}
          armedReason={offerOpenCost()}
          onOpen={acceptOffer}
          onDismiss={dismissOffer}
        />
      )}

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
              title="The project: files and libraries"
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
                  className={sideTab === "fleet" ? "tab active" : "tab"}
                  onClick={() => setHardwareTab("fleet")}
                  title="The physical boards Bancada has seen, remembered by MAC address"
                >
                  Fleet
                </button>
                <button
                  className={sideTab === "boards" ? "tab active" : "tab"}
                  onClick={() => setHardwareTab("boards")}
                  title="Install and update board platforms (arduino-cli cores)"
                >
                  Boards
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
          ) : duplicatingProject ? (
            <DuplicateProject
              sourceDir={sketchDir}
              onCreated={async (dir) => {
                setDuplicatingProject(false);
                await loadSketch(dir);
              }}
              onCancel={() => setDuplicatingProject(false)}
              notify={notify}
            />
          ) : renamingProject && sketchDir ? (
            <RenameProject
              sketchDir={sketchDir}
              gitState={gitState}
              onRenamed={async (dir) => {
                setRenamingProject(false);
                // The old path is gone; loadSketch re-points everything that
                // the backend migration did not (tabs, buffers, git state).
                await loadSketch(dir);
              }}
              onCancel={() => setRenamingProject(false)}
              notify={notify}
            />
          ) : showingUsage ? (
            <UsageDashboard
              onClose={() => setShowingUsage(false)}
              openBottomTab={openBottomTab}
            />
          ) : (
            <>
          <EditorTabs
            tabs={openTabs}
            active={openFile}
            dirty={dirtyFiles}
            armed={armedTab}
            onSelect={handleTabSelect}
            onClose={handleTabClose}
            onCloseOthers={handleCloseOthers}
            onCloseAll={handleCloseAll}
          />
          <CodeMirror
            ref={editorRef}
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
        <BottomTabBar
          active={bottomTab}
          unseen={unseen}
          badges={{ build: badgeCount(buildModel.summary) }}
          onOpen={openBottomTab}
          maximized={bottomMax}
          onToggleMaximize={() => setBottomMax((m) => !m)}
        />
        {bottomTab === "build" && (
          <BuildConsole
            model={buildModel}
            sketchDir={sketchDir}
            knownFiles={knownFiles}
            onJump={jumpToDiagnostic}
            onClear={() => setBuildLines([])}
          />
        )}
        {/* Always mounted, hidden by `active`: unmounting would throw away
            the scrollback, the filter and the scroll position every time the
            user glanced at the build log. `onSend` gets the bytes with the
            line ending already appended — the backend writes them verbatim. */}
        <SerialMonitor
          active={bottomTab === "serial"}
          store={serialStore}
          monitorOn={monitorOn}
          flashing={flashOwnsPort}
          portLabel={selectedPort ? selectedPortName() : null}
          baud={monitorOn ? (openBaudRef.current ?? baudrate) : baudrate}
          baudSource={baudSource}
          sketchBaud={sketchBaud}
          onBaudChange={(b) => void changeBaud(b)}
          onUseSketchBaud={() => void changeBaud(null)}
          connection={serialConnection}
          onToggleMonitor={() => void toggleMonitor()}
          onSend={(bytes) => api.monitorSend(bytes)}
          notify={notify}
        />
        {mqttMounted && (
          <MqttPanel active={bottomTab === "mqtt"} notify={notify} />
        )}
        {wsMounted && <WsPanel active={bottomTab === "ws"} notify={notify} />}
        {webMounted && (
          <DeviceBrowserPanel active={bottomTab === "web"} notify={notify} />
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
        {agentMounted && (
          <AgentPanel
            // Remount per project: panel-local state (History list/replay,
            // totals, turn view, draft) belongs to one sketch's chats, and a
            // sketchDir change is a hard boundary — resetting by key can't
            // miss a state added later the way a reset effect could.
            key={sketchDir ?? ""}
            active={bottomTab === "agent"}
            sketchDir={sketchDir}
            store={agentStore}
            onSend={sendToAgent}
            onInterrupt={interruptAgent}
            onNewSession={newAgentSession}
            onContinueChat={continueChat}
            openBottomTab={openBottomTab}
            gitWarning={gitState?.kind === "no_git"}
            uploadsArmed={uploadsArmed}
            onToggleUploadsArmed={toggleUploadsArmed}
          />
        )}
      </section>

      <ToastStack
        toasts={toasts.toasts}
        onDismiss={(id) => setToasts((s) => dismissToast(s, id))}
      />

      <StatusBar
        activity={activity}
        lastResult={lastResult}
        project={sketchDir ? projectButtonLabel(sketchDir) : null}
        portName={selectedPort ? selectedPortName() : null}
        busy={busy}
        measuredFraction={progress?.fraction ?? null}
        estimateMs={estimate}
        note={progress?.note ?? null}
      />
    </div>
  );
}
