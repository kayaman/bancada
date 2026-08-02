// Typed wrappers around Bancada's Tauri commands and events.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AgentEvent, AgentProbe } from "./agent/types";

export type { AgentEvent, AgentProbe } from "./agent/types";

// ---------- types mirrored from Rust ----------

export interface Port {
  address: string;
  label: string;
  protocol: string;
  protocol_label: string;
  /**
   * USB descriptor fields — typically `vid`, `pid` and `serialNumber`. On an
   * ESP32 with native USB, `serialNumber` is the chip's MAC address.
   */
  properties: Record<string, string>;
  /**
   * arduino-cli's identity string for the port. For a USB-serial bridge this is
   * the *bridge's* serial (often "0001"), so it is not unique per board.
   */
  hardware_id: string;
}

export interface MatchingBoard {
  name: string;
  fqbn: string;
  /** Umbrella entries like `esp32:esp32:esp32_family` are flagged hidden. */
  is_hidden: boolean;
}

export interface DetectedPort {
  port: Port;
  matching_boards: MatchingBoard[];
}

export interface LibraryRelease {
  version: string;
  author: string;
  sentence: string;
  paragraph: string;
  website: string;
}

export interface IndexedLibrary {
  name: string;
  latest: LibraryRelease;
  available_versions: string[];
}

export interface InstalledLibrary {
  name: string;
  version: string;
  author: string;
  sentence: string;
  location: string;
  install_dir: string;
}

/** Fields for a brand-new library; blanks are defaulted by the backend. */
export interface LibrarySpec {
  name: string;
  version: string;
  author: string;
  maintainer: string;
  sentence: string;
  paragraph: string;
  category: string;
  url: string;
  architectures: string;
}

export interface CreatedLibrary {
  dir: string;
  files: string[];
  warnings: string[];
  /** Rewritten sketch.yaml, when the library was pinned to a profile. */
  yaml?: SketchYaml | null;
  /** Set when the library was created but pinning it to the profile failed. */
  profile_error?: string | null;
}

/** The categories the Arduino library specification permits. */
export const LIBRARY_CATEGORIES = [
  "Display",
  "Communication",
  "Signal Input/Output",
  "Sensors",
  "Device Control",
  "Timing",
  "Data Storage",
  "Data Processing",
  "Other",
] as const;

/** A tag from `git ls-remote`, resolved to the commit it points at. */
export interface RemoteTag {
  name: string;
  commit: string;
}

/** A pinned remote library recorded in bancada.yaml. */
export interface ManifestEntry {
  alias: string;
  ref: string;
  commit: string;
  vendor: string;
}

export interface Manifest {
  version: number;
  libraries: ManifestEntry[];
}

export interface GhAdded {
  alias: string;
  ref: string;
  commit: string;
  vendor: string;
  gitignored: boolean;
  yaml?: SketchYaml | null;
  profile_error?: string | null;
}

export interface GhRestored {
  restored: string[];
  errors: string[];
  yaml?: SketchYaml | null;
}

export interface SketchFile {
  rel_path: string;
  is_dir: boolean;
}

export type LibraryDep = string | { dir: string };

export interface Profile {
  fqbn: string;
  platforms?: { platform: string; platform_index_url?: string }[];
  libraries?: LibraryDep[];
  notes?: string;
  port?: string;
}

export interface SketchYaml {
  default_profile?: string;
  profiles?: Record<string, Profile>;
}

export interface OutputLine {
  stream: "stdout" | "stderr";
  line: string;
}

export interface RunResult {
  success: boolean;
  exit_code: number;
}

export interface ChipInfo {
  mac: string;
  chip_type?: string;
  raw_output: string;
}

export interface AppSettings {
  last_sketch_dir?: string | null;
  last_open_file?: string | null;
  last_new_project_parent?: string | null;
}

/** Where an installed platform stands relative to its newest release. */
export type CoreStatus = "not_installed" | "up_to_date" | "update_available";

/**
 * One platform (core), already flattened by the backend.
 *
 * `versions` is newest-first and excludes releases arduino-cli cannot build on
 * this host, so it can feed a version picker directly. `boards` carries names
 * only — `core list`/`core search` do not expose FQBNs.
 */
export interface CoreView {
  id: string;
  name: string;
  maintainer: string;
  website: string;
  /** Empty when not installed. */
  installed_version: string;
  latest_version: string;
  status: CoreStatus;
  versions: string[];
  boards: string[];
}

export interface InstalledCore {
  result: RunResult;
  /** Rewritten sketch.yaml, when the platform was pinned to a profile. */
  yaml?: SketchYaml | null;
  /** Set when the platform installed but pinning it to the profile failed. */
  profile_error?: string | null;
}

/** How confidently a port identifies one specific physical board. */
export type BoardIdKind = "mac" | "serial";

/** A remembered physical board. */
export interface FleetEntry {
  /** Normalised MAC (lower case, colon-separated), or the USB serial. */
  id: string;
  id_kind: BoardIdKind;
  nickname?: string | null;
  chip_type?: string | null;
  board_name?: string | null;
  /** Every FQBN this board has been built for, in first-seen order. */
  fqbns: string[];
  last_port?: string | null;
  vid?: string | null;
  pid?: string | null;
  /** Epoch seconds. */
  first_seen: number;
  last_seen: number;
}

/** One selectable board, flattened from `board listall`. */
export interface BoardOption {
  fqbn: string;
  name: string;
  platform_id: string;
  platform_name: string;
}

export interface CreatedProject {
  dir: string;
  name: string;
  profile: string;
  /** Libraries that could not be added; the project is still usable. */
  library_errors: string[];
}

export interface ScopeCaps {
  baud: number;
  proto: number;
  chip: string;
  fw: string;
  max_sps: number;
  max_stream_sps: number;
  pins: number[];
  atten: number[];
  maxch: number;
}

export interface ScopeStreamCfg {
  sps: number;
  pins: number[];
  atten: number;
}

export interface ScopeSingleCfg {
  sps: number;
  pin: number;
  atten: number;
  pre: number;
  post: number;
  level: number;
  edge: "r" | "f";
}

// ---------- commands ----------

export const cliVersion = () => invoke<string>("cli_version");
export const listBoards = () => invoke<DetectedPort[]>("list_boards");

export const listSketchFiles = (sketchDir: string) =>
  invoke<SketchFile[]>("list_sketch_files", { sketchDir });
/** Whether `sketchDir` is under git — backs the Assistant panel's no-undo warning. */
export const sketchHasGit = (sketchDir: string) =>
  invoke<boolean>("sketch_has_git", { sketchDir });
export const readSketchFile = (sketchDir: string, relPath: string) =>
  invoke<string>("read_sketch_file", { sketchDir, relPath });
export const writeSketchFile = (
  sketchDir: string,
  relPath: string,
  content: string,
) => invoke<void>("write_sketch_file", { sketchDir, relPath, content });

// Explorer mutations. Each returns the refreshed file list so the tree is
// always disk-coherent in one round trip.
export const createSketchFile = (sketchDir: string, relPath: string) =>
  invoke<SketchFile[]>("create_sketch_file", { sketchDir, relPath });
export const createSketchDir = (sketchDir: string, relPath: string) =>
  invoke<SketchFile[]>("create_sketch_dir", { sketchDir, relPath });
export const renameSketchEntry = (sketchDir: string, from: string, to: string) =>
  invoke<SketchFile[]>("rename_sketch_entry", { sketchDir, from, to });
export const deleteSketchEntry = (sketchDir: string, relPath: string) =>
  invoke<SketchFile[]>("delete_sketch_entry", { sketchDir, relPath });

export const loadSketchYaml = (sketchDir: string) =>
  invoke<SketchYaml>("load_sketch_yaml", { sketchDir });
/** Create sketch.yaml (when absent) with a first profile for `fqbn`. */
export const initProfile = (sketchDir: string, profile: string, fqbn: string) =>
  invoke<SketchYaml>("init_profile", { sketchDir, profile, fqbn });
export const addLocalLibrary = (
  sketchDir: string,
  profile: string,
  libDir: string,
) => invoke<SketchYaml>("add_local_library", { sketchDir, profile, libDir });
export const addRegistryLibraryToProfile = (
  sketchDir: string,
  profile: string,
  name: string,
  version: string,
) =>
  invoke<SketchYaml>("add_registry_library_to_profile", {
    sketchDir,
    profile,
    name,
    version,
  });

export const searchLibraries = (query: string) =>
  invoke<IndexedLibrary[]>("search_libraries", { query });
export const listInstalledLibraries = () =>
  invoke<InstalledLibrary[]>("list_installed_libraries");
export const installLibrary = (name: string, version?: string) =>
  invoke<void>("install_library", { name, version: version ?? null });
export const uninstallLibrary = (name: string) =>
  invoke<void>("uninstall_library", { name });
export const sketchbookLibrariesDir = () =>
  invoke<string>("sketchbook_libraries_dir");
/**
 * Scaffold a new library in the sketchbook. When `sketchDir`/`profile` are
 * given the library is also pinned into that profile as an absolute `dir:`
 * entry, without which a profile build cannot see it.
 */
export const createLibrary = (
  spec: LibrarySpec,
  sketchDir?: string | null,
  profile?: string | null,
) =>
  invoke<CreatedLibrary>("create_library", {
    spec,
    sketchDir: sketchDir ?? null,
    profile: profile ?? null,
  });

// ---------- cores (platforms) ----------

export const listCores = () => invoke<CoreView[]>("list_cores");
/**
 * Search the indexes arduino-cli already knows about. Bancada never adds index
 * URLs of its own, so a platform with no configured index will not appear.
 */
export const searchCores = (query: string) =>
  invoke<CoreView[]>("search_cores", { query });
/**
 * Install or upgrade a platform. Progress arrives on `build://line`.
 *
 * With `sketchDir`/`profile` and a concrete `version`, the platform is also
 * pinned into that profile — a profile build is hermetic, so an unpinned
 * platform is invisible to it.
 */
export const installCore = (
  id: string,
  version?: string | null,
  sketchDir?: string | null,
  profile?: string | null,
) =>
  invoke<InstalledCore>("install_core", {
    id,
    version: version ?? null,
    sketchDir: sketchDir ?? null,
    profile: profile ?? null,
  });
export const uninstallCore = (id: string) =>
  invoke<RunResult>("uninstall_core", { id });
export const updateCoreIndex = () => invoke<RunResult>("update_core_index");
/** Pin an already-installed platform into a profile without reinstalling. */
export const addPlatformToProfile = (
  sketchDir: string,
  profile: string,
  id: string,
  version: string,
) =>
  invoke<SketchYaml>("add_platform_to_profile", {
    sketchDir,
    profile,
    id,
    version,
  });

// ---------- new projects ----------

/** Sketchbook root — the conventional Arduino home for sketches. */
export const sketchbookDir = () => invoke<string>("sketchbook_dir");
/** ~/Projects when it exists, else the home directory. */
export const defaultProjectParent = () => invoke<string>("default_project_parent");
/** Every board of every installed platform, grouped-ready for a picker. */
export const listAllBoards = () => invoke<BoardOption[]>("list_all_boards");
/**
 * Create a sketch with a profile for `fqbn` and the given registry libraries
 * pinned. `libraries` entries are `Name` or `Name@version`.
 */
export const createProject = (
  parent: string,
  name: string,
  fqbn: string,
  profile: string | null,
  libraries: string[],
) =>
  invoke<CreatedProject>("create_project", {
    parent,
    name,
    fqbn,
    profile: profile ?? null,
    libraries,
  });

// ---------- remote (git) libraries ----------

/** Tags for an alias, newest first, the library's own namespace first. */
export const ghListVersions = (alias: string) =>
  invoke<RemoteTag[]>("gh_list_versions", { alias });
export const ghManifest = (sketchDir: string) =>
  invoke<Manifest>("gh_manifest", { sketchDir });
/** Fetch, vendor, pin into the profile and record in bancada.yaml. */
export const ghAddLibrary = (
  sketchDir: string,
  profile: string | null,
  alias: string,
  ref: string,
) =>
  invoke<GhAdded>("gh_add_library", {
    sketchDir,
    profile: profile ?? null,
    alias,
    gitRef: ref,
  });
/** Re-materialise every manifest entry at its recorded commit. */
export const ghRestore = (sketchDir: string, profile: string | null) =>
  invoke<GhRestored>("gh_restore", { sketchDir, profile: profile ?? null });

export const compileSketch = (
  sketchDir: string,
  profile?: string,
  fqbn?: string,
) =>
  invoke<RunResult>("compile_sketch", {
    sketchDir,
    profile: profile ?? null,
    fqbn: fqbn ?? null,
  });
export const uploadSketch = (
  sketchDir: string,
  port: string,
  profile?: string,
  fqbn?: string,
) =>
  invoke<RunResult>("upload_sketch", {
    sketchDir,
    port,
    profile: profile ?? null,
    fqbn: fqbn ?? null,
  });

export const startMonitor = (port: string, baudrate: number) =>
  invoke<void>("start_monitor", { port, baudrate });
export const stopMonitor = () => invoke<void>("stop_monitor");
export const monitorSend = (data: string) =>
  invoke<void>("monitor_send", { data });

export const readBoardMac = (port: string) =>
  invoke<ChipInfo>("read_board_mac", { port });

// ---------- fleet (remembered physical boards) ----------

/** The Fleet panel's whole state, derived in Rust so the identity rules aren't
 * duplicated here. */
export interface FleetSnapshot {
  boards: FleetEntry[];
  /** Ids currently plugged in. */
  online: string[];
  /** Attached serial ports with no board-specific identity (bridges). */
  unidentified: DetectedPort[];
}

/**
 * Record every identifiable port from a scan and return the updated snapshot.
 * Ports with no board-specific identity (a bare USB-serial bridge) are skipped,
 * not recorded under the bridge's shared id.
 */
export const fleetSync = (ports: DetectedPort[]) =>
  invoke<FleetSnapshot>("fleet_sync", { ports });
export const setBoardNickname = (id: string, nickname?: string | null) =>
  invoke<FleetEntry[]>("set_board_nickname", { id, nickname: nickname ?? null });
/**
 * Record that the board on `port` was built for `fqbn`. The port is resolved to
 * a fleet id in Rust; an unknown or unidentifiable port is a silent no-op, so an
 * upload never fails over bookkeeping.
 */
export const noteBoardFqbn = (port: string, fqbn: string) =>
  invoke<void>("note_board_fqbn", { port, fqbn });
/**
 * Ask esptool for a board's real MAC and fold it into the registry. Takes the
 * serial port and resets the board. `previousId` migrates an existing
 * serial-keyed record — nickname and history included — onto the MAC.
 */
export const identifyBoard = (port: string, previousId?: string | null) =>
  invoke<FleetEntry[]>("identify_board", {
    port,
    previousId: previousId ?? null,
  });
export const forgetBoard = (id: string) =>
  invoke<FleetEntry[]>("forget_board", { id });

export const loadSettings = () => invoke<AppSettings>("load_settings");
export const saveSettings = (settings: AppSettings) =>
  invoke<void>("save_settings", { settings });

// ---------- scope ----------

export const scopeProbe = (port: string) =>
  invoke<ScopeCaps>("scope_probe", { port });

/** Starts ADC streaming; binary envelope messages arrive on `onMessage`. */
export const scopeStart = (
  port: string,
  baud: number,
  cfg: ScopeStreamCfg,
  onMessage: (data: ArrayBuffer | number[]) => void,
) => {
  const channel = new Channel<ArrayBuffer | number[]>();
  channel.onmessage = onMessage;
  return invoke<void>("scope_start", { port, baud, cfg, onMessage: channel });
};

export const scopeSingle = (cfg: ScopeSingleCfg) =>
  invoke<void>("scope_single", { cfg });
export const scopeSend = (line: string) => invoke<void>("scope_send", { line });
export const scopeStop = () => invoke<void>("scope_stop");
export const scopeInstallFirmware = (destDir: string) =>
  invoke<string>("scope_install_firmware", { destDir });

export const saveTextFile = (path: string, contents: string) =>
  invoke<void>("save_text_file", { path, contents });
export const saveBinaryFile = (path: string, contentsB64: string) =>
  invoke<void>("save_binary_file", { path, contentsB64 });

// ---------- mqtt (observability) ----------

export interface MqttBrokerCfg {
  name: string;
  url: string;
}

export interface MqttConfig {
  brokers: MqttBrokerCfg[];
}

export type MqttStage = "parse" | "tcp" | "connack" | "suback";

export type MqttEvent =
  | { ev: "stage"; stage: MqttStage; ok: boolean; detail: string; elapsed_ms: number }
  | {
      ev: "msg";
      topic: string;
      payload: string;
      b64: boolean;
      retain: boolean;
      qos: number;
      ts: number;
    }
  | { ev: "closed"; reason: string };

/** Connects to a broker; JSON envelope events arrive on `onEvent`. */
export const mqttConnect = (
  url: string,
  subscribeFilter: string | null,
  onEvent: (e: MqttEvent) => void,
) => {
  const channel = new Channel<MqttEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("mqtt_connect", {
    url,
    subscribeFilter,
    onMessage: channel,
  });
};

export const mqttPublish = (topic: string, payload: string, retain: boolean) =>
  invoke<void>("mqtt_publish", { topic, payload, retain });
export const mqttSubscribe = (filter: string) =>
  invoke<void>("mqtt_subscribe", { filter });
export const mqttUnsubscribe = (filter: string) =>
  invoke<void>("mqtt_unsubscribe", { filter });
export const mqttDisconnect = () => invoke<void>("mqtt_disconnect");
export const loadMqttConfig = () => invoke<MqttConfig>("load_mqtt_config");
export const saveMqttConfig = (cfg: MqttConfig) =>
  invoke<void>("save_mqtt_config", { cfg });

// ---------- agent (Assistant panel) ----------

/** `claude --version` + a min-version gate — whether the CLI is usable at all. */
export const agentProbe = () => invoke<AgentProbe>("agent_probe");
/**
 * Spawns the `claude` child for `sketchDir`, confined to it by a PreToolUse
 * guard hook the host generates (`write_agent_settings_file`).
 *
 * Resolves to the **child pid**. Every host event that can outlive its
 * session is stamped with it (`agent://closed`, `verify_started`,
 * `verify_done`, `security_alarm`), so the caller must hand it to
 * `agentStore.sessionStarted` — otherwise a stale event from a session the
 * user already stopped repaints the new one's panel.
 */
export const agentStart = (sketchDir: string, profile?: string, fqbn?: string) =>
  invoke<number>("agent_start", {
    sketchDir,
    profile: profile ?? null,
    fqbn: fqbn ?? null,
  });
/** Sends one user text message on the agent's stdin. */
export const agentSend = (text: string) => invoke<void>("agent_send", { text });
/** Best-effort control-protocol interrupt; the host kills the child if it doesn't land. */
export const agentInterrupt = () => invoke<void>("agent_interrupt");
/**
 * Kills the child (if any) and stops the loopback MCP listener.
 *
 * `pid`, when given, must match the *live* session's child pid or this is a
 * no-op — guards a stale `agent://closed` (from a session already
 * superseded by a newer `agentStart`) against killing the new one. Omit it
 * for an explicit user stop ("New session"), which always proceeds.
 */
export const agentStop = (pid?: number) =>
  invoke<void>("agent_stop", { pid: pid ?? null });

// ---------- events ----------

export const onBuildLine = (cb: (l: OutputLine) => void): Promise<UnlistenFn> =>
  listen<OutputLine>("build://line", (e) => cb(e.payload));
export const onSerialLine = (
  cb: (l: OutputLine) => void,
): Promise<UnlistenFn> =>
  listen<OutputLine>("serial://line", (e) => cb(e.payload));
export const onSerialClosed = (cb: () => void): Promise<UnlistenFn> =>
  listen("serial://closed", () => cb());
/** Fires when the set of serial ports on the machine changes (hotplug). */
export const onPortsChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("ports://changed", () => cb());

/** One parsed line from the agent child's stdout, plus bancada's synthetic
 * stderr/unparsed/verify/security_alarm events — see `src/agent/types.ts`. */
export const onAgentEvent = (cb: (ev: AgentEvent) => void): Promise<UnlistenFn> =>
  listen<AgentEvent>("agent://event", (e) => cb(e.payload));
/** Fires once on child EOF; the frontend should call `agentStop(p.pid)` on
 * receipt so the exited child is reaped — passing `pid` lets the backend
 * refuse to kill a newer session that has since superseded this one. */
export const onAgentClosed = (
  cb: (p: { reason: string; pid: number }) => void,
): Promise<UnlistenFn> =>
  listen<{ reason: string; pid: number }>("agent://closed", (e) => cb(e.payload));
