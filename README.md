# Bancada

An Arduino workbench: code editing, library management (registry **and**
local/proprietary libraries), compile/flash, serial monitor, and board
utilities (ESP MAC address reader) — built with **Tauri 2 + Rust + React**.

Bancada does not reimplement the toolchain. It drives the same engines the
official IDE uses:

- **arduino-cli** — boards, cores, builds, uploads, library registry (all via `--json`)
- **esptool** — ESP-specific utilities (read MAC, chip info)

```
┌─────────────────────────── Tauri window ───────────────────────────┐
│ React UI (CodeMirror editor, file tree, library manager, consoles) │
└──────────────────────────────┬─────────────────────────────────────┘
                    invoke / events (build://line, serial://line)
┌──────────────────────────────┴─────────────────────────────────────┐
│ src-tauri  — commands, event streaming, serial monitor process     │
│ core (bancada-core) — pure Rust, no UI deps, unit-tested:          │
│   cli.rs      arduino-cli wrapper (JSON parsing, line streaming)   │
│   sketch.rs   sketch.yaml profiles, dir: local libraries           │
│   esptool.rs  MAC / chip info reader                               │
└──────────────────────────────┬─────────────────────────────────────┘
                     subprocesses: arduino-cli, esptool
```

## Prerequisites (openSUSE Tumbleweed)

```bash
# Tauri build dependencies
sudo zypper install -t pattern devel_basis
sudo zypper install webkit2gtk3-devel libopenssl-devel curl wget file librsvg-devel
# (Tauri 2 needs webkit2gtk 4.1 — check https://tauri.app/start/prerequisites/
#  for the current openSUSE package names if this one has been renamed.)

# Rust + Node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Node 20+ via zypper or nvm

# The engines Bancada drives
curl -fsSL https://raw.githubusercontent.com/arduino/arduino-cli/master/install.sh | sh
pip install --user esptool

# Serial port access
sudo usermod -aG dialout $USER   # check group with: ls -l /dev/ttyACM0
```

## Run in development

```bash
npm install
npm run tauri dev
```

## Build a release bundle (deb/rpm/AppImage)

```bash
npm run tauri build
```

## What works in this scaffold

- Open a sketch folder → file tree, main `.ino` auto-opens in the editor
  (CodeMirror 6, C++ mode, Ctrl+S to save)
- Board/port detection with live rescan (`arduino-cli board list`)
- Build profiles from `sketch.yaml` (default profile pre-selected)
- Verify / Upload with **live streaming build output** (compile falls back to
  nothing-selected errors gracefully; upload requires a port)
- Library manager:
  - search the Arduino registry, install/remove (global sketchbook)
  - installing while a project is open **also pins** the library into the
    active `sketch.yaml` profile
  - **“+ Local…”** adds a local/proprietary library folder as a `dir:` entry
    in the active profile — per-project local libraries, which the official
    IDE cannot do
  - **“New”** scaffolds a complete library in the sketchbook —
    `library.properties`, `src/<Name>.{h,cpp}` with a stub class, `keywords.txt`
    and an example that compiles as-is — then pins it into the active profile
    as an absolute `dir:` entry. That pin matters: profile builds are
    *hermetic*, so globally installed libraries are excluded from them and a
    sketchbook library would otherwise be invisible to the build. The flip
    side is that such an entry names a path on this machine, so a `sketch.yaml`
    carrying one is not portable to a collaborator
- Serial monitor via `arduino-cli monitor` (start/stop, baudrate, TX input)
- Utilities: **read board MAC address** via esptool (shows chip type too)

## Roadmap ideas

- `clangd` integration for real C++ completion/diagnostics (CodeMirror LSP client)
- Profile editor UI (create/edit `sketch.yaml` platforms visually)
- More esptool utilities: flash erase, flash size, filesystem image upload
- Core/board manager panel (`arduino-cli core install`)
- Git integration; multi-sketch workspaces

## Repo layout

```
bancada/
├── core/            # bancada-core: pure Rust bridge (unit-tested, no Tauri)
├── src-tauri/       # Tauri app: commands, events, window config
├── src/             # React frontend (Vite + TypeScript + CodeMirror)
├── package.json
└── Cargo.toml       # workspace: core + src-tauri
```

## Notes

- The serial port can only be held by one process: Bancada automatically stops
  the monitor before uploading or reading the MAC.
- `esptool read_mac` talks to the ROM bootloader, so the running sketch is
  briefly interrupted, then the board resets back into it.
- Everything the UI does can be reproduced by hand with `arduino-cli` — the
  `sketch.yaml` written by Bancada is plain Arduino tooling, no lock-in.
