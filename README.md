# Bancada

An Arduino workbench: code editing, library management (registry **and**
local/proprietary libraries), compile/flash, serial monitor, and board
utilities (ESP MAC address reader) — built with **Tauri 2 + Rust + React**.

Bancada does not reimplement the toolchain. It drives the same engines the
official IDE uses, plus a few more, all resolved from `PATH`:

- **arduino-cli** — boards, cores, builds, uploads, library registry (all via `--json`)
- **esptool** — ESP-specific utilities (read MAC, chip info)
- **git** / **gh** — project version control and one-button repo creation
- **claude** — the AI Assistant panel
- **Mouser Search API** — optional live component lookup for stock, lifecycle,
  pricing, and datasheet links in the Circuit workspace

```
┌─────────────────────────── Tauri window ───────────────────────────┐
│ React UI — editor, file tree, library/board/fleet managers,        │
│ consoles, observability panels, oscilloscope, AI Assistant         │
└──────────────────────────────┬─────────────────────────────────────┘
          src/api.ts: 103 invoke commands · 7 events · 3 Channels
┌──────────────────────────────┴─────────────────────────────────────┐
│ src-tauri — commands, event streaming, threads, session state,     │
│             plus a loopback MCP server the Assistant calls into    │
├────────────────────────────────────────────────────────────────────┤
│ core (bancada-core) — 24 modules of pure Rust, no UI deps:         │
│   parsers · validators · policy · wire formats · argv builders     │
└──────────────────────────────┬─────────────────────────────────────┘
      subprocesses: arduino-cli · esptool · git · gh · claude
      outbound HTTPS: optional Mouser Search API
```

**Full architecture documentation: [docs/architecture/](docs/architecture/README.md)** —
layer map, the IPC contract, the runtime model, and seven end-to-end data flows.

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

# gh (GitHub CLI) — optional: powers the one-button "Create on GitHub"
# in the git pill; without it, paste any git remote URL instead.
sudo zypper install gh   # then: gh auth login

# claude CLI (for the AI Assistant panel) — resolved from PATH like the two
# above; install it, then run `claude` once and follow the login prompt.
# See https://claude.com/product/claude-code for the current install options
# (the native installer drops it in ~/.local/bin; npm is also supported).
curl -fsSL https://claude.ai/install.sh | bash

# Optional: request a Mouser Search API key at https://www.mouser.com/api-search/
# and save it in Hardware → Circuit. For managed/dev environments, Bancada also
# accepts MOUSER_API_KEY without copying the key into the webview or a project.
# Search results stay transient under Mouser's API terms and are not saved to
# circuit.yaml or the generated BOM.

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

- **The project menu** — `📁 <name> ▾` in the toolbar is the one project
  affordance: it names what is open and holds Open, Recent, New, Duplicate and
  Rename. Everything else on the bar is about the *target* (profile, port), the
  repository, or building
- **Rename project** — renames the folder *and* its main `.ino`, which have to
  move together, and carries across everything keyed to the old path: the
  Assistant chat history, the recorded token spend, and the recents entry. It
  is refused while an Assistant session is live, because the session pins the
  old path in places that cannot be rewritten after it starts
- **Duplicate project** — copies a project under a new name into a fresh git
  repository, retitling the main `.ino` and repointing local library paths.
  Never copies the source's history
- **New Project** — name it, pick a board from the installed platforms (the
  attached board is preselected), optionally tick registry libraries, and get a
  sketch with a `sketch.yaml` profile that compiles immediately. Driven entirely
  by `arduino-cli sketch new` + `profile create` + `profile lib add`, so the
  platform version is resolved from what is installed and library dependencies
  are resolved by the engine. The location defaults to the sketchbook and then
  remembers wherever you last created one
- **Starter sketches** — every new project begins as one of seven, not an
  empty file: **Blink**, **Waveforms** (three software-generated traces, no
  wiring — the fastest way to see the Scope draw), **Analog plot** (one ADC
  pin, raw against smoothed), **Serial echo** (proves the board listens, not
  just talks), plus the I2C, Wi-Fi and board-info probes. The first four
  compile unchanged on AVR, ESP32, ESP8266 and the UNO Q
- Open a project (📁 menu) → file tree, main `.ino` auto-opens in the editor
  (CodeMirror 6, C++ mode, Ctrl+S to save)
- Board/port detection with live rescan (`arduino-cli board list`)
- Build profiles from `sketch.yaml` (default profile pre-selected)
- Verify / Flash with **live streaming build output** (compile falls back to
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
  fleet, circuit) groups, drag-resizable and collapsible to a rail
- **Circuit workspace** — guided ESP32 board/component/wire editing backed by
  `hardware/circuit.yaml`; generates a C++ pin header, SVG diagram, wiring
  guide, BOM and validation report. Verify and Flash refuse stale, unsafe or
  profile-incompatible circuit data. Optional Mouser Search API integration
  displays live manufacturer parts, stock, pricing, lifecycle, product and
  datasheet data without persisting the returned catalog content; exact parts
  and electrical verification remain explicit user-entered circuit data
- **Editor tabs** — multiple files open at once, dirty markers, and a
  close-again-to-discard step so unsaved work is never dropped by one click
- **Git pill** — repository state at a glance in the toolbar, with commit,
  sync, `git init`, and one-button repo creation through `gh` — private or
  public, with a description, initializing the repository first if the project
  has none. Warns before committing anything that looks like a tracked secret,
  and *refuses* to publish publicly when one is already tracked
- **Flash provenance** — every flash that changes the code checkpoints the
  project and writes an annotated `flash/<timestamp>` git tag recording the
  port, profile/FQBN and board, then pushes it. One tag per distinct code
  state, not per flash. Nothing in that path can fail a flash: it reports to
  the build console and gets out of the way
- **Scope** — a software oscilloscope in the Debugging tab, with two sources:
  a **plotter** that parses numeric values out of the existing serial stream
  (any board), and an **ADC** mode where companion firmware on an ESP32 streams
  raw 12-bit samples over serial. Trigger, timebase, cursors, measurements, FFT
  spectrum, and CSV/PNG export. The firmware ships in the binary — one button
  installs and flashes it. Protocol: [docs/scope-architecture.md](docs/scope-architecture.md)
- **Observability** tabs — an **MQTT** client and a **WebSocket** client for
  watching what your board publishes, with topic stats, filtering, pause, and
  pretty-printed JSON or hex payloads
- **Web** tab — browse a board's own HTTP interface in an embedded iframe,
  through a loopback reverse proxy that logs every request and response
- **Usage dashboard** — cumulative AI Assistant spend per project, expandable
  into individual sessions that replay inline
- **AI Assistant** panel — a bottom-panel **Assistant** tab where a `claude`
  CLI session (spawned per project, scoped to the open sketch) reads/edits
  your sketch's files, runs Verify, reads the compiler errors, and iterates
  until the build passes — and, since 0.12.0, can **flash the board** (behind
  a per-session "Allow uploads" switch), **watch and type to the serial
  monitor**, and **search/fetch the web**. Edits show up as diff cards and
  **auto-apply immediately** — no per-edit approval step. Writes are confined
  to the sketch directory as in-process CLI policy (not an OS sandbox); reads
  are not confined, and web access means what is read can leave the machine.
  See [AI Assistant panel](#ai-assistant-panel) below

## AI Assistant panel

A bottom-panel **Assistant** tab where you chat with a **Claude** agent that
can read and edit the open sketch's files, run **Verify** through a
bancada-provided tool, read the compiler errors, and iterate until the build
passes — then **flash the board**, watch the **serial monitor** for the boot
output, and type commands to the running sketch. The full
edit → verify → flash → watch-serial loop runs without leaving the panel.

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
narrowed to `Read`, `Edit`, `Write`, `Glob`, `Grep`, `WebFetch`, `WebSearch`
(via `--tools`, which genuinely removes tools from the CLI, unlike
`--disallowedTools` alone), plus six bancada MCP tools:

- **`verify`** — the same compile path as the **Verify** button; compiles
  only, never flashes.
- **`upload`** — the same `compile -u` path as the **Flash** button,
  gated by the **Allow uploads** switch (below). It takes **no port
  argument**: it flashes the board selected in the UI, with the
  profile/FQBN the session was started with — the agent cannot pick a
  different target. It shares the build gate with your own Verify/Upload,
  stops the monitor for the flash (like the manual flow), and does not
  restart it — the agent's next `serial_read` does.
- **`serial_read`** — reads monitor output the session hasn't seen yet
  (bounded scrollback; a session never replays output from before it
  started), auto-starting the monitor on the UI-selected port/baud when
  the port is free. The Monitor tab stays in sync.
- **`serial_send`** — types one line to the board through the running
  monitor, exactly like the Monitor tab's send box.
- **`circuit_status`** — checks the project circuit, active FQBN, generated
  artifacts and Arduino pin usage without writing.
- **`circuit_sync`** — regenerates the project pin header, SVG, wiring guide,
  BOM and validation report from `hardware/circuit.yaml`.

It still cannot run shell commands (`Bash` stays out of the tool set —
capabilities are scoped by what the tools can express, not by prompt
instructions), and it can never touch the oscilloscope's serial session.

**Allow uploads (arm switch)** — flashing is the one physically-consequential
action, so it starts **off** every session: the `upload` tool is refused with
"ask the user" until you click **🔒 Allow uploads** in the panel footer
(it turns **🔓 Uploads armed**). While armed, the agent flashes without
per-flash prompts, so the autonomous fix → verify → flash → watch-boot loop
stays unbroken; disarm at any time, and a **New session** always starts
disarmed again.

**Cost** — each completed turn's cost and cumulative turn count appear in
the panel footer (`$0.0123 · 4 turns`), pulled straight from what the
`claude` CLI reports.

**Safety** — edits **auto-apply**, with no per-edit approval prompt. If the
open project has no `.git`, the panel shows a persistent warning (`⚠ not
under git`), because there is no undo path for an agent's edit without
version control; commit or initialize git before trusting it with anything
you can't easily retype.

Writes are confined to the sketch directory, and refused for the project's
`.claude/`, `.git/` and `.mcp.json` as well as your own `~/.claude/`, by four
enforcement layers — deny rules, a `PreToolUse` guard hook, a pre-flight
refusal, and an independent runtime backstop that stops the session if an edit
or the tool list drifts from what Bancada expects.

Three limits are worth stating plainly before you use it:

- This is **in-process policy inside the `claude` CLI's own permission engine —
  not an OS-level sandbox or container.**
- **Reads are not confined at all.** The agent can read anything your account
  can, including SSH keys and credential files. Only writes are policed.
- Since 0.12.0 the session has **web access** (`WebFetch`/`WebSearch`) — a
  deliberate egress trade-off. Combined with unconfined reads, data the agent
  reads on your machine *can leave it*. If that is wrong for your environment,
  don't chat with the Assistant on machines holding secrets you wouldn't paste
  into a browser.


**The complete model — all four layers, what each one can and cannot express,
and everything that is *not* enforced — is documented in
[docs/architecture/agent-safety.md](docs/architecture/agent-safety.md).**

## Roadmap ideas

- `clangd` integration for real C++ completion/diagnostics (CodeMirror LSP client)
- Profile editor UI (create/edit `sketch.yaml` platforms visually)
- More esptool utilities: flash erase, flash size, filesystem image upload
- Board options in the FQBN (`CDCOnBoot=cdc`) chosen from a picker
- Multi-project workspaces

## Repo layout

```
bancada/
├── core/            # bancada-core: 24 modules of pure Rust (no Tauri, unit-tested)
├── src-tauri/       # Tauri app: commands, events, session state, window config
├── src/             # React frontend (Vite + TypeScript + CodeMirror)
├── firmware/        # bancada_scope: companion ESP32 sketch for the ADC scope
├── docs/            # architecture, install, wire contracts, release notes
├── package.json
└── Cargo.toml       # workspace: core + src-tauri
```

See [docs/](docs/README.md) for the documentation index, and
[docs/architecture/](docs/architecture/README.md) for how the pieces fit
together.

## Notes

- The serial port can only be held by one process: Bancada automatically stops
  the monitor before uploading or reading the MAC.
- `esptool read_mac` talks to the ROM bootloader, so the running sketch is
  briefly interrupted, then the board resets back into it.
- Everything the UI does can be reproduced by hand with `arduino-cli` — the
  `sketch.yaml` written by Bancada is plain Arduino tooling, no lock-in.
