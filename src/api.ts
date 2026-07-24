// Typed wrappers around Bancada's Tauri commands and events.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ---------- types mirrored from Rust ----------

export interface Port {
  address: string;
  label: string;
  protocol: string;
  protocol_label: string;
}

export interface MatchingBoard {
  name: string;
  fqbn: string;
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
export const readSketchFile = (sketchDir: string, relPath: string) =>
  invoke<string>("read_sketch_file", { sketchDir, relPath });
export const writeSketchFile = (
  sketchDir: string,
  relPath: string,
  content: string,
) => invoke<void>("write_sketch_file", { sketchDir, relPath, content });

export const loadSketchYaml = (sketchDir: string) =>
  invoke<SketchYaml>("load_sketch_yaml", { sketchDir });
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

// ---------- events ----------

export const onBuildLine = (cb: (l: OutputLine) => void): Promise<UnlistenFn> =>
  listen<OutputLine>("build://line", (e) => cb(e.payload));
export const onSerialLine = (
  cb: (l: OutputLine) => void,
): Promise<UnlistenFn> =>
  listen<OutputLine>("serial://line", (e) => cb(e.payload));
export const onSerialClosed = (cb: () => void): Promise<UnlistenFn> =>
  listen("serial://closed", () => cb());
