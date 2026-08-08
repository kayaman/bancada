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
# git (>= 2.25 for sparse checkout) — used to fetch pinned libraries from a repo
sudo zypper install git

# claude CLI (for the AI Assistant panel) — resolved from PATH like the two
# above; install it, then run `claude` once and follow the login prompt.
# See https://claude.com/product/claude-code for the current install options
# (the native installer drops it in ~/.local/bin; npm is also supported).
curl -fsSL https://claude.ai/install.sh | bash

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

Bundles land in `target/release/bundle/{rpm,deb,appimage}/` with the
desktop entry and hicolor icon set included. **Installing them (and the
serial-port/ModemManager setup an end user needs) is documented in
[docs/INSTALL.md](docs/INSTALL.md).**

## Tests and coverage

```bash
cargo test --workspace     # Rust unit tests
npm test                   # frontend (vitest)
npm run coverage           # Rust line coverage, per file
npm run coverage:html      # same, as a browsable report
```

Coverage needs `cargo-llvm-cov` and the LLVM tools:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov   # or grab the prebuilt binary from its releases
```

Some tests are **opt-in** because they need network, `git`, an installed core, or
a board, so a plain `cargo test` skips them:

```bash
cargo test -p bancada-core --test gh_fetch            -- --ignored  # network + git
cargo test -p bancada-core --test scaffold_compiles   -- --ignored  # installed core
cargo test -p bancada-core --test new_project_builds  -- --ignored  # installed core
npm run coverage:full                                              # coverage incl. those
```

They are worth running before a release: they are the only tests that prove a
scaffolded library, a fetched library and a newly created project actually
*compile*, rather than merely producing the expected text.

## What works in this scaffold

- **New Project** — name it, pick a board from the installed platforms (the
  attached board is preselected), optionally tick registry libraries, and get a
  sketch with a `sketch.yaml` profile that compiles immediately. Driven entirely
  by `arduino-cli sketch new` + `profile create` + `profile lib add`, so the
  platform version is resolved from what is installed and library dependencies
  are resolved by the engine. The location defaults to the sketchbook and then
  remembers wherever you last created one
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
  - **“GitHub”** references a library from a git repo by alias, pinned to a
    version: `@kayaman/Arduino/libraries/HomeNode`. Versions come from
    `git ls-remote --tags`, so no API token is needed and any git host works.
    The chosen tag is fetched with a shallow sparse checkout, vendored into
    `.bancada/libs/<Name>` (a copy — no nested `.git`), pinned into the active
    profile as a relative `dir:` entry, and recorded in `bancada.yaml` with
    **both the tag and the commit it resolved to**, because a tag can move and a
    commit cannot. `.bancada/` is gitignored and `bancada.yaml` is committed, so
    a fresh clone carries the pins and **Restore** re-fetches the bytes
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
- **Boards** panel — the platform (core) manager: search every index arduino-cli
  already knows, install/update/remove a platform with live progress in the
  build console, pick a specific version, and pin the installed version into the
  active profile's `platforms:` list. Bancada never edits your global
  `board_manager.additional_urls`; a platform you have no index for simply will
  not appear, and the empty state names the `arduino-cli config add` command
- **Fleet** panel — the physical boards Bancada has seen, remembered across runs
  in `fleet.json` and keyed by MAC address. A native-USB ESP32 reports its MAC as
  its USB serial number, so plugging one in enrols it with no esptool, no port
  takeover and no reset. Boards behind a USB-serial bridge expose only the
  bridge's serial, so they are listed as unidentified with an **Identify** button
  that reads the real MAC via esptool and migrates the record — nickname and
  history included. Each board carries a nickname you set, its chip type, the
  FQBNs it has been built for, and when it was first and last seen
- Sidebar split into **Software** (files, libraries) and **Hardware** (boards,
  fleet) groups, drag-resizable and collapsible to a rail
- **AI Assistant** panel — a bottom-panel **Assistant** tab where a `claude`
  CLI session (spawned per project, scoped to the open sketch) reads/edits
  your sketch's files, runs Verify, reads the compiler errors, and iterates
  until the build passes. Edits show up as diff cards and **auto-apply
  immediately** — no per-edit approval step. Writes are confined to the
  sketch directory as in-process CLI policy (not an OS sandbox); reads are
  not confined. See [AI Assistant panel](#ai-assistant-panel) below

## AI Assistant panel

A bottom-panel **Assistant** tab where you chat with a **Claude** agent that
can read and edit the open sketch's files, run **Verify** through a
bancada-provided tool, read the compiler errors, and iterate until the build
passes. First slice of an AI-assisted-IDE direction — no upload/serial tools
yet (roadmap).

Bancada drives this the same way it drives its other engines: it spawns the
**`claude` CLI** as a supervised child process (`--input-format stream-json
--output-format stream-json`) and talks to it over stdio. No bundled SDK, no
sidecar.

**Prerequisites** — the **`claude` CLI** on PATH, signed in (`claude` once,
same login as Claude Code). Resolved from PATH exactly like arduino-cli and
esptool — no bundled copy, no separate API key to configure. If it isn't
found, the panel shows install/login guidance instead of a raw error.

**Using it** — open a sketch, switch to the **Assistant** tab, and send a
message ("make this build", "add a debounce to the button read", …). The
agent replies with streamed text plus cards for what it does: **Edit/Write**
cards show a unified diff of the change **after it has already been
applied** (no approval step in between), and **Verify** cards mirror
`arduino-cli compile` (pass/fail, exit code), with the same output also
streaming to the existing **Build** console. The agent keeps editing and
re-verifying on its own until the build passes or it gives up. **Stop**
interrupts the current turn; **New session** ends the child process and
clears the transcript.

**What it can and can't do** — the embedded session's built-in tools are
narrowed to `Read`, `Edit`, `Write`, `Glob`, `Grep` (via `--tools`, which
genuinely removes tools from the CLI, unlike `--disallowedTools` alone),
plus one MCP tool, `verify`, that runs the same compile path as the
**Verify** button and never runs `-u` (upload) — it only compiles. It
cannot run shell commands, fetch URLs, search the web, or touch
upload/serial.

**Cost** — each completed turn's cost and cumulative turn count appear in
the panel footer (`$0.0123 · 4 turns`), pulled straight from what the
`claude` CLI reports.

**Safety** — edits **auto-apply**, with no per-edit approval prompt. If the
open project has no `.git`, the panel shows a persistent warning (`⚠ not
under git`), because there is no undo path for an agent's edit without
version control; commit or initialize git before trusting it with anything
you can't easily retype.

Writes are refused outside the sketch directory, and for the project's
`.claude/`, `.git/`, `.mcp.json` (plus your own `~/.claude/`, shell rc
files, `/etc/`, and similar), by `permissions.deny` rules in a `--settings`
policy file Bancada writes **outside the project tree** — a policy the
agent cannot edit — anchoring a `PreToolUse` guard hook that adds the
subtree-containment check a denylist alone can't express. If a project's
own settings already disable hooks (`disableAllHooks`), Bancada refuses to
*start* the session rather than run one whose guard hook is known in
advance not to fire. A second, independent check re-inspects every edit the
agent reports and the session's tool list, and stops the session outright
if either drifted from what Bancada expects.

This is **in-process policy enforced by the `claude` CLI's own permission
engine — not an OS-level sandbox or container**. **Reads are not confined
at all**: the agent can read anything your account can, including SSH keys
and credential files; only writes are policed. The embedded session also
still loads your own Claude Code configuration (hooks, plugins, skills) —
the flags that would suppress that also break login or disable the
`verify` tool — so a hostile hook already present in your personal
configuration before the session starts is out of scope for this to catch.
Bancada can only stop the agent from *installing* a new one.

## Roadmap ideas

- `clangd` integration for real C++ completion/diagnostics (CodeMirror LSP client)
- Profile editor UI (create/edit `sketch.yaml` platforms visually)
- More esptool utilities: flash erase, flash size, filesystem image upload
- Board options in the FQBN (`CDCOnBoot=cdc`) chosen from a picker
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
