//! Bancada's Tauri layer: thin async commands over `bancada-core`, plus event
//! streaming for build output, the serial monitor, and the scope.
//!
//! Events emitted to the frontend:
//!   "build://line"      { stream: "stdout"|"stderr", line: string }
//!   "serial://line"     { stream, line }
//!   "serial://closed"   {}
//!   "ports://changed"   {}  (the set of serial ports on the machine changed)
//!   "agent://event"     the claude CLI's own stream-json event object,
//!                       verbatim, plus these synthetic ones the host adds:
//!                       { type: "stderr", line }        (child stderr)
//!                       { type: "unparsed", line }      (non-JSON stdout)
//!                       { type: "verify_started", pid }
//!                       { type: "verify_done", success, pid }
//!                       { type: "security_alarm", kind, detail, pid }
//!                       `pid` on all three is the session's child pid, so a
//!                       synthetic event from a session the user has already
//!                       stopped cannot be rendered into a *newer* session's
//!                       panel (same guard as `agent://closed`).
//!   "agent://closed"    { reason, pid }  (child stdout hit EOF; pid is the
//!                       child's, so the frontend's `agent_stop(pid)` call
//!                       is a no-op if a newer session has since superseded
//!                       this one — see `should_stop_agent`)
//!
//! Scope commands (`docs/scope-architecture.md` §3): `scope_probe`,
//! `scope_start`, `scope_single`, `scope_send`, `scope_stop`,
//! `scope_install_firmware`, plus `save_text_file` / `save_binary_file`.
//! `scope_start` streams binary envelopes (§2: kind 0x01 samples, 0x02 JSON
//! events) over a `tauri::ipc::Channel` instead of events. The serial port
//! has a single owner at a time — monitor child process or scope session —
//! and acquiring it for one evicts the other.
//!
//! Agent commands (Assistant panel): `agent_probe`, `agent_start`,
//! `agent_send`, `agent_interrupt`, `agent_stop`. One `claude` child per
//! session, driven over stdio stream-json, with four detached threads —
//! stdin writer (fed by an mpsc channel, so the `agent` mutex is never held
//! across a pipe write), stdout reader, stderr drain, and a loopback
//! `tiny_http` MCP listener serving the `verify` tool. The listener gets
//! owned clones at spawn time and never locks `agent`; shutdown breaks its
//! blocking `recv()` with `Server::unblock()`, which no atomic flag could
//! do. The child never restarts on its own (same philosophy as the MQTT
//! thread) — the panel shows "Session ended"; the stdout reader tears its
//! own session down at EOF, and the frontend's `agent_stop(pid)` on
//! `agent://closed` is then a belt-and-braces no-op. `pid` guards a stale
//! close from a superseded session against killing a newer one
//! (`should_stop_agent`). The `--mcp-config` bearer token rides a 0600 temp
//! file, not argv (`write_mcp_config_file`) — argv is readable by any local
//! process via `/proc/<pid>/cmdline` on Linux.
//!
//! ## Agent safety model — what is actually enforced
//!
//! The embedded session runs with the **user's own Claude Code
//! configuration loaded**: `--bare` and `--safe-mode`, the two flags that
//! would suppress it, respectively break keychain auth and disable
//! `--mcp-config` (so the `verify` tool disappears) — both probe-verified
//! against CLI 2.1.220, see `core::agent::agent_args`. That means the user's
//! hooks load, and **hooks are shell commands**. Composed with an
//! unconfined `Write`, that was a path to arbitrary command execution as
//! the user: the agent writes a `PreToolUse` hook into a settings file, the
//! CLI runs it as a shell command. Closing it needs the *write* leg, since
//! the *hooks* leg cannot be closed without losing auth or the compiler.
//!
//! Two enforcement layers, in order of strength:
//!
//! 1. **Refusal (the boundary).** `write_agent_settings_file` generates a
//!    `--settings` file whose `PreToolUse` hook is *this very binary*
//!    re-invoked as `bancada --agent-guard <sketch_dir>`
//!    (`run_agent_guard`), adjudicating every `Write`/`Edit`/`MultiEdit`/
//!    `NotebookEdit` with `core::agent::guard_decision`. An edit outside the
//!    sketch dir, or touching a `.claude/` component below it, is *denied* —
//!    the CLI reports it in `permission_denials` and nothing reaches disk.
//!    Probe-verified end to end, including that a permissive hook running
//!    alongside does not override the deny.
//! 2. **Detect-and-stop (the backstop).** The stdout reader independently
//!    re-checks every `Edit`/`Write` `tool_use` against the same
//!    `path_is_confined`, and checks the `system`/`init` `tools` array
//!    against `core::agent::EXPECTED_TOOLS`. Either check failing emits
//!    `{type:"security_alarm"}` and stops the session. This exists because
//!    layer 1 depends on the CLI honouring `--settings` hooks: if a future
//!    release changed that, or the guard binary could not run, the boundary
//!    would fail *open* with no other signal.
//!
//! What is **not** a boundary: `--disallowedTools` (a permission-layer
//! nudge — a session with it set still lists 25 built-in tools). What is:
//! `--tools`, which genuinely narrows the built-in set to
//! `Read,Edit,Write,Glob,Grep` while leaving the MCP `verify` tool intact.
//! Residual risk: a *pre-existing* hostile hook in the user's own config
//! still runs — Bancada stops the agent from installing one, it cannot stop
//! one that is already there.
//!
//! Build gate: `compile_sketch`, `upload_sketch` and the agent's MCP
//! `verify` tool all drive the same arduino-cli build cache, so they share
//! one `build_gate: Mutex<()>` in `AppState`. It is taken with `try_lock`,
//! never blocking — a contended build fails fast with "build already in
//! progress" instead of queueing behind a multi-minute platform build.
//! Before the gate, the only mutual exclusion was the frontend's `busy`
//! flag, which agent-initiated builds bypass entirely. Note the gate covers
//! exactly those three *compile/upload* entry points — `install_core`,
//! `uninstall_core`, `install_library` and the other arduino-cli commands
//! run outside it, so it is not "every arduino-cli invocation in the
//! process". Those neither read nor write the sketch build cache the gate
//! exists to protect.
//!
//! MQTT commands (Observability panel): `mqtt_connect` streams JSON envelopes
//! over a `tauri::ipc::Channel` — `{"ev":"stage"|"msg"|"closed"}` per the
//! `bancada_core::mqtt` contract — plus `mqtt_publish`, `mqtt_subscribe`,
//! `mqtt_unsubscribe`, `mqtt_disconnect` and `load_mqtt_config` /
//! `save_mqtt_config` (`mqtt.json`). The MQTT session lives in its own slot
//! beside the serial owner: monitor/scope never evict it and vice versa. The
//! connection thread never retries — on any error it emits `closed` and
//! exits; reconnecting is the frontend's job.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bancada_core::boards::{self, CoreView};
use bancada_core::cli::ArduinoCli;
use bancada_core::fleet::{self, Fleet};
use bancada_core::ghlib;
use bancada_core::scope::{self, serialport, FrameScanner, ScopeCaps, ScopeFrame};
use bancada_core::sketch::{PathStyle, SketchProject, SketchYaml};
use bancada_core::types::{DetectedPort, IndexedLibrary, InstalledLibrary, RunResult};
use base64::Engine as _;
use serialport::SerialPort;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

/// A running ADC-streaming session: the writer half of the port lives under
/// the state mutex; the reader half is owned by a dedicated thread that never
/// touches the mutex (same discipline as the monitor `Child`).
struct ScopeSession {
    writer: Box<dyn SerialPort>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

/// Whoever currently holds the serial port. Exactly one owner at a time.
enum SerialOwner {
    Monitor(Child),
    Scope(ScopeSession),
}

struct AppState {
    cli: ArduinoCli,
    serial: Mutex<Option<SerialOwner>>,
    /// MQTT broker session — a sibling of `serial`, never coupled to it.
    mqtt: Mutex<Option<MqttSession>>,
    /// Embedded `claude` session — another sibling slot, one at a time.
    agent: Mutex<Option<AgentSession>>,
    /// Serialises the three *sketch build* paths — user Verify
    /// (`compile_sketch`), user Upload (`upload_sketch`) and the agent's MCP
    /// `verify` tool — which share one arduino-cli build cache and were
    /// previously kept apart only by the frontend's `busy` flag (which
    /// agent-initiated builds bypass entirely).
    ///
    /// Deliberately *not* every arduino-cli invocation in the process:
    /// `install_core`, `uninstall_core`, `update_core_index`,
    /// `install_library` and friends run outside the gate. They touch the
    /// platform/library trees rather than a sketch's build cache, and
    /// serialising them behind a multi-minute compile would make the Boards
    /// and Libraries panels fail with "build already in progress" for no
    /// benefit.
    build_gate: Arc<Mutex<()>>,
}

/// What every build path reports when the gate is already held.
const BUILD_BUSY: &str = "build already in progress";

/// Take the build gate without waiting. `Err(BUILD_BUSY)` means another
/// build already holds it — callers report that rather than queueing, so a
/// second Verify click (or an agent `verify` during a user build) fails
/// fast instead of silently stacking up behind a multi-minute platform
/// build.
fn try_build_gate(gate: &Mutex<()>) -> Result<std::sync::MutexGuard<'_, ()>, String> {
    match gate.try_lock() {
        Ok(guard) => Ok(guard),
        // A panicking build poisons the gate. What it guards is `()` — there
        // is no state to have corrupted — so recover instead of wedging
        // every future build for the lifetime of the process.
        Err(std::sync::TryLockError::Poisoned(e)) => Ok(e.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => Err(BUILD_BUSY.to_string()),
    }
}

/// Convert any core error into the string Tauri sends to JS.
fn err_str(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Kill and reap a monitor child process.
fn kill_child(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Politely stop a scope session: ask the firmware to stop streaming, flag the
/// reader thread down, release the writer handle and join the thread.
fn stop_scope_session(mut session: ScopeSession) {
    let _ = session.writer.write_all(scope::cmd_stop().as_bytes());
    let _ = session.writer.flush();
    session.stop.store(true, Ordering::Relaxed);
    let join = session.join.take();
    drop(session.writer);
    if let Some(handle) = join {
        let _ = handle.join();
    }
}

/// Free the serial port from whichever owner holds it.
fn evict_owner(slot: &mut Option<SerialOwner>) {
    match slot.take() {
        Some(SerialOwner::Monitor(child)) => kill_child(child),
        Some(SerialOwner::Scope(session)) => stop_scope_session(session),
        None => {}
    }
}

// ---------- environment ----------

#[tauri::command]
async fn cli_version(state: State<'_, AppState>) -> Result<String, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || cli.version().map_err(err_str))
        .await
        .map_err(err_str)?
}

#[tauri::command]
async fn list_boards(state: State<'_, AppState>) -> Result<Vec<DetectedPort>, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || cli.board_list().map_err(err_str))
        .await
        .map_err(err_str)?
}

// ---------- sketch / files ----------

#[tauri::command]
fn list_sketch_files(sketch_dir: String) -> Result<Vec<bancada_core::sketch::SketchFile>, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.list_files().map_err(err_str)
}

/// Whether the sketch dir is under git — the Assistant panel warns when it
/// isn't, since auto-applied agent edits have no undo path without it (spec
/// Risk R4). A plain `.git` dir check, not a `git` invocation: cheap, and
/// good enough for the warning's purpose (a worktree/submodule's `.git` file
/// still counts as "under git").
#[tauri::command]
fn sketch_has_git(sketch_dir: String) -> bool {
    Path::new(&sketch_dir).join(".git").exists()
}

#[tauri::command]
fn read_sketch_file(sketch_dir: String, rel_path: String) -> Result<String, String> {
    let full = safe_join(&sketch_dir, &rel_path)?;
    std::fs::read_to_string(full).map_err(err_str)
}

#[tauri::command]
fn write_sketch_file(sketch_dir: String, rel_path: String, content: String) -> Result<(), String> {
    let full = safe_join(&sketch_dir, &rel_path)?;
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(err_str)?;
    }
    std::fs::write(full, content).map_err(err_str)
}

/// Join and refuse path traversal outside the sketch dir.
fn safe_join(base: &str, rel: &str) -> Result<std::path::PathBuf, String> {
    let rel_p = Path::new(rel);
    if rel_p.is_absolute()
        || rel_p
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!("invalid path: {rel}"));
    }
    Ok(Path::new(base).join(rel_p))
}

#[tauri::command]
fn load_sketch_yaml(sketch_dir: String) -> Result<SketchYaml, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.load_yaml().map_err(err_str)
}

#[tauri::command]
fn init_profile(sketch_dir: String, profile: String, fqbn: String) -> Result<SketchYaml, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.init_profile(&profile, &fqbn).map_err(err_str)
}

#[tauri::command]
fn add_local_library(
    sketch_dir: String,
    profile: String,
    lib_dir: String,
) -> Result<SketchYaml, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.add_local_library(&profile, Path::new(&lib_dir))
        .map_err(err_str)
}

/// Pin a registry library into a profile.
///
/// Delegates to `arduino-cli profile lib add`, which resolves the library's
/// dependencies — writing the YAML entry by hand did not. Signature unchanged so
/// the frontend is unaffected; the updated sketch.yaml is read back after.
#[tauri::command]
async fn add_registry_library_to_profile(
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: String,
    name: String,
    version: String,
) -> Result<SketchYaml, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = Path::new(&sketch_dir);
        let spec = if version.trim().is_empty() {
            name
        } else {
            format!("{name}@{version}")
        };
        cli.profile_lib_add(dir, &profile, &spec).map_err(err_str)?;
        SketchProject::open(dir)
            .and_then(|p| p.load_yaml())
            .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

// ---------- libraries (global sketchbook) ----------

#[tauri::command]
async fn search_libraries(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<IndexedLibrary>, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || cli.lib_search(&query).map_err(err_str))
        .await
        .map_err(err_str)?
}

#[tauri::command]
async fn list_installed_libraries(
    state: State<'_, AppState>,
) -> Result<Vec<InstalledLibrary>, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || cli.lib_list().map_err(err_str))
        .await
        .map_err(err_str)?
}

#[tauri::command]
async fn install_library(
    state: State<'_, AppState>,
    name: String,
    version: Option<String>,
) -> Result<(), String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cli.lib_install(&name, version.as_deref()).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn uninstall_library(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || cli.lib_uninstall(&name).map_err(err_str))
        .await
        .map_err(err_str)?
}

/// Where a newly created library will land, for the create form's preview.
#[tauri::command]
async fn sketchbook_libraries_dir(state: State<'_, AppState>) -> Result<String, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cli.sketchbook_libraries_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[derive(serde::Serialize)]
struct CreatedLibrary {
    dir: String,
    files: Vec<String>,
    warnings: Vec<String>,
    /// The rewritten sketch.yaml, when the `dir:` entry was added.
    yaml: Option<SketchYaml>,
    /// Set when the library was created but pinning it to the profile failed.
    profile_error: Option<String>,
}

/// Scaffold an empty library in `<sketchbook>/libraries/<Name>` and, when a
/// sketch profile is active, pin it there with an **absolute** `dir:` entry.
///
/// The pin is not optional bookkeeping: profile builds are hermetic — globally
/// installed libraries are excluded from them — so without it a sketchbook
/// library is invisible to every profile-based compile.
#[tauri::command]
async fn create_library(
    state: State<'_, AppState>,
    spec: bancada_core::library::LibrarySpec,
    sketch_dir: Option<String>,
    profile: Option<String>,
) -> Result<CreatedLibrary, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let libs_dir = cli.sketchbook_libraries_dir().map_err(err_str)?;
        let made = bancada_core::library::create_library(&libs_dir, &spec).map_err(err_str)?;

        let mut out = CreatedLibrary {
            dir: made.dir.to_string_lossy().into_owned(),
            files: made.files,
            warnings: made.warnings,
            yaml: None,
            profile_error: None,
        };

        // Deliberately non-fatal: the folder exists either way, and failing the
        // whole command would tell the user nothing was created.
        if let (Some(dir), Some(prof)) = (sketch_dir, profile) {
            match SketchProject::open(&dir)
                .and_then(|p| p.add_local_library_with(&prof, &made.dir, PathStyle::Absolute))
            {
                Ok(y) => out.yaml = Some(y),
                Err(e) => out.profile_error = Some(e.to_string()),
            }
        }
        Ok(out)
    })
    .await
    .map_err(err_str)?
}

// ---------- cores (platforms) ----------

/// Installed platforms. The view is derived in `bancada_core::boards` so the
/// frontend never re-implements version ordering.
#[tauri::command]
async fn list_cores(state: State<'_, AppState>) -> Result<Vec<CoreView>, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let platforms = cli.core_list().map_err(err_str)?;
        Ok(platforms.iter().map(boards::view).collect())
    })
    .await
    .map_err(err_str)?
}

/// Search every index arduino-cli already knows about.
///
/// Bancada does not add index URLs of its own, so a platform the user has not
/// configured an index for simply will not appear here.
#[tauri::command]
async fn search_cores(state: State<'_, AppState>, query: String) -> Result<Vec<CoreView>, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let platforms = cli.core_search(&query).map_err(err_str)?;
        Ok(platforms.iter().map(boards::view).collect())
    })
    .await
    .map_err(err_str)?
}

/// A core install that may have succeeded while failing to pin itself.
///
/// Mirrors `CreatedLibrary`: the platform is installed either way, so failing
/// the whole command would wrongly suggest nothing happened.
#[derive(serde::Serialize)]
struct InstalledCore {
    result: RunResult,
    /// The rewritten sketch.yaml, when the profile pin was updated.
    yaml: Option<SketchYaml>,
    /// Set when the platform installed but pinning it to the profile failed.
    profile_error: Option<String>,
}

/// Install (or upgrade) a platform, streaming progress to the build console.
///
/// When a sketch profile is active the platform is pinned into it afterwards,
/// mirroring how installing a library also pins it — a profile build is
/// hermetic, so an unpinned platform is invisible to it.
///
/// `version: None` installs the latest. The pin needs a concrete version, so it
/// is skipped unless one was named; the caller passes the version it chose.
#[tauri::command]
async fn install_core(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    version: Option<String>,
    sketch_dir: Option<String>,
    profile: Option<String>,
) -> Result<InstalledCore, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::boards::parse_core_id(&id).map_err(err_str)?;
        let result = cli
            .core_install(&id, version.as_deref(), |line| {
                let _ = app.emit("build://line", &line);
            })
            .map_err(err_str)?;

        let mut out = InstalledCore {
            result,
            yaml: None,
            profile_error: None,
        };
        if !out.result.success {
            return Ok(out);
        }

        if let (Some(dir), Some(prof), Some(v)) = (sketch_dir, profile, version) {
            match SketchProject::open(&dir).and_then(|p| p.add_platform(&prof, &id, &v)) {
                Ok(y) => out.yaml = Some(y),
                Err(e) => out.profile_error = Some(e.to_string()),
            }
        }
        Ok(out)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn uninstall_core(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<RunResult, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::boards::parse_core_id(&id).map_err(err_str)?;
        cli.core_uninstall(&id, |line| {
            let _ = app.emit("build://line", &line);
        })
        .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

/// Refresh the platform indexes so `search_cores` sees new releases.
#[tauri::command]
async fn update_core_index(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RunResult, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cli.core_update_index(|line| {
            let _ = app.emit("build://line", &line);
        })
        .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

/// Pin an already-installed platform into a profile, without reinstalling.
#[tauri::command]
fn add_platform_to_profile(
    sketch_dir: String,
    profile: String,
    id: String,
    version: String,
) -> Result<SketchYaml, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.add_platform(&profile, &id, &version).map_err(err_str)
}

// ---------- new projects ----------

/// Default location for a new project: `~/Projects` when present, else home.
#[tauri::command]
fn default_project_parent(app: AppHandle) -> Result<String, String> {
    let home = app.path().home_dir().map_err(err_str)?;
    Ok(bancada_core::project::default_project_parent(&home)
        .to_string_lossy()
        .into_owned())
}

/// Sketchbook root — the conventional Arduino home for sketches.
#[tauri::command]
async fn sketchbook_dir(state: State<'_, AppState>) -> Result<String, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cli.sketchbook_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

/// Every board of every installed platform, for the New Project picker.
#[tauri::command]
async fn list_all_boards(
    state: State<'_, AppState>,
) -> Result<Vec<bancada_core::types::BoardOption>, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || cli.board_listall().map_err(err_str))
        .await
        .map_err(err_str)?
}

#[derive(serde::Serialize)]
struct CreatedProject {
    dir: String,
    name: String,
    profile: String,
    /// Libraries that could not be added; the project is still usable.
    library_errors: Vec<String>,
}

/// Create a sketch, give it a profile for `fqbn`, and pin the requested
/// libraries — each step driven by arduino-cli rather than reimplemented.
#[tauri::command]
async fn create_project(
    state: State<'_, AppState>,
    parent: String,
    name: String,
    fqbn: String,
    profile: Option<String>,
    libraries: Vec<String>,
) -> Result<CreatedProject, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let name = bancada_core::project::validate_project_name(&name).map_err(err_str)?;
        if fqbn.trim().is_empty() {
            return Err("choose a board — the profile needs an FQBN".to_string());
        }
        let profile = profile
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| bancada_core::project::profile_name_for_fqbn(&fqbn));

        let parent_path = Path::new(&parent);
        if parent_path.exists() && !parent_path.is_dir() {
            return Err(format!("{parent} is not a directory"));
        }
        std::fs::create_dir_all(parent_path)
            .map_err(|e| format!("could not create {parent}: {e}"))?;
        let dir = parent_path.join(&name);
        // Never overwrite: `sketch new --overwrite` is deliberately not used.
        if dir.symlink_metadata().is_ok() {
            return Err(format!(
                "{} already exists — choose another name or location",
                dir.display()
            ));
        }

        cli.sketch_new(&dir).map_err(err_str)?;
        // Swap the empty setup/loop stub for a real Blink starter.
        bancada_core::project::write_main_ino(&dir, &name).map_err(err_str)?;
        cli.profile_create(&dir, &profile, &fqbn, true)
            .map_err(err_str)?;

        // Non-fatal: the project exists and builds without these, so report the
        // failures rather than abandoning a directory the user can already see.
        let mut library_errors = Vec::new();
        for spec in &libraries {
            let spec = spec.trim();
            if spec.is_empty() {
                continue;
            }
            if let Err(e) = cli.profile_lib_add(&dir, &profile, spec) {
                library_errors.push(format!("{spec}: {e}"));
            }
        }

        Ok(CreatedProject {
            dir: dir.to_string_lossy().into_owned(),
            name,
            profile,
            library_errors,
        })
    })
    .await
    .map_err(err_str)?
}

// ---------- remote (git) libraries ----------

/// Versions available for an alias, newest first, the library's own tag
/// namespace before anything else.
#[tauri::command]
async fn gh_list_versions(alias: String) -> Result<Vec<ghlib::RemoteTag>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let a = ghlib::parse_alias(&alias).map_err(err_str)?;
        ghlib::list_remote_tags(&a.url(), &a.name).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
fn gh_manifest(sketch_dir: String) -> Result<ghlib::Manifest, String> {
    ghlib::Manifest::load(Path::new(&sketch_dir)).map_err(err_str)
}

#[derive(serde::Serialize)]
struct GhAdded {
    alias: String,
    #[serde(rename = "ref")]
    git_ref: String,
    commit: String,
    vendor: String,
    gitignored: bool,
    /// Rewritten sketch.yaml, when the `dir:` entry was added.
    yaml: Option<SketchYaml>,
    /// Set when the library was vendored but pinning it to the profile failed.
    profile_error: Option<String>,
}

/// Fetch a library from a git repository at `git_ref`, vendor it into the
/// sketch, pin it into the active profile and record it in `bancada.yaml`.
#[tauri::command]
async fn gh_add_library(
    sketch_dir: String,
    profile: Option<String>,
    alias: String,
    git_ref: String,
) -> Result<GhAdded, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let a = ghlib::parse_alias(&alias).map_err(err_str)?;
        let sketch = Path::new(&sketch_dir);
        let vendor_rel = a.vendor_rel();
        let dest = sketch.join(&vendor_rel);

        let commit = ghlib::fetch_subtree(&a, &git_ref, &dest, None).map_err(err_str)?;
        let gitignored = ghlib::ensure_gitignored(sketch).unwrap_or(false);

        let mut manifest = ghlib::Manifest::load(sketch).map_err(err_str)?;
        manifest.upsert(ghlib::ManifestEntry {
            alias: a.canonical(),
            git_ref: git_ref.clone(),
            commit: commit.clone(),
            vendor: vendor_rel.clone(),
        });
        manifest.save(sketch).map_err(err_str)?;

        let mut out = GhAdded {
            alias: a.canonical(),
            git_ref,
            commit,
            vendor: vendor_rel,
            gitignored,
            yaml: None,
            profile_error: None,
        };

        // Non-fatal, as in create_library: the library is on disk and recorded
        // either way, so failing the whole call would misreport what happened.
        if let Some(prof) = profile {
            match SketchProject::open(sketch)
                .and_then(|p| p.add_local_library_with(&prof, &dest, PathStyle::Relative))
            {
                Ok(y) => out.yaml = Some(y),
                Err(e) => out.profile_error = Some(e.to_string()),
            }
        }
        Ok(out)
    })
    .await
    .map_err(err_str)?
}

#[derive(serde::Serialize)]
struct GhRestored {
    restored: Vec<String>,
    errors: Vec<String>,
    yaml: Option<SketchYaml>,
}

/// Re-materialise every manifest entry at its recorded commit. This is what
/// makes a fresh clone buildable, since the vendored bytes are gitignored.
#[tauri::command]
async fn gh_restore(sketch_dir: String, profile: Option<String>) -> Result<GhRestored, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let sketch = Path::new(&sketch_dir);
        let manifest = ghlib::Manifest::load(sketch).map_err(err_str)?;
        let mut out = GhRestored {
            restored: Vec::new(),
            errors: Vec::new(),
            yaml: None,
        };

        for entry in &manifest.libraries {
            // One bad entry must not stop the rest from being restored.
            let result = ghlib::parse_alias(&entry.alias).and_then(|a| {
                let dest = sketch.join(&entry.vendor);
                ghlib::fetch_subtree(&a, &entry.git_ref, &dest, Some(&entry.commit))
                    .map(|_| (a, dest))
            });
            match result {
                Ok((_, dest)) => {
                    out.restored.push(entry.alias.clone());
                    if let Some(prof) = profile.as_deref() {
                        if let Ok(p) = SketchProject::open(sketch) {
                            if let Ok(y) =
                                p.add_local_library_with(prof, &dest, PathStyle::Relative)
                            {
                                out.yaml = Some(y);
                            }
                        }
                    }
                }
                Err(e) => out.errors.push(format!("{}: {e}", entry.alias)),
            }
        }
        Ok(out)
    })
    .await
    .map_err(err_str)?
}

// ---------- build & flash ----------

#[tauri::command]
async fn compile_sketch(
    app: AppHandle,
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: Option<String>,
    fqbn: Option<String>,
) -> Result<RunResult, String> {
    let cli = state.cli.clone();
    let gate = state.build_gate.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _gate = try_build_gate(&gate)?;
        let result = cli
            .compile(
                &sketch_dir,
                profile.as_deref(),
                fqbn.as_deref(),
                &[],
                |line| {
                    let _ = app.emit("build://line", &line);
                },
            )
            .map_err(err_str)?;
        Ok(result)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn upload_sketch(
    app: AppHandle,
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: Option<String>,
    fqbn: Option<String>,
    port: String,
) -> Result<RunResult, String> {
    let cli = state.cli.clone();
    let gate = state.build_gate.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _gate = try_build_gate(&gate)?;
        let result = cli
            .upload(
                &sketch_dir,
                profile.as_deref(),
                fqbn.as_deref(),
                &port,
                |line| {
                    let _ = app.emit("build://line", &line);
                },
            )
            .map_err(err_str)?;
        Ok(result)
    })
    .await
    .map_err(err_str)?
}

// ---------- serial monitor ----------

#[tauri::command]
fn start_monitor(
    app: AppHandle,
    state: State<'_, AppState>,
    port: String,
    baudrate: u32,
) -> Result<(), String> {
    let mut guard = state.serial.lock().unwrap();
    evict_owner(&mut guard);

    let mut child = state.cli.monitor(&port, baudrate).map_err(err_str)?;
    let stdout = child.stdout.take().ok_or("monitor stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("monitor stderr unavailable")?;

    let app_out = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            let _ = app_out.emit(
                "serial://line",
                serde_json::json!({ "stream": "stdout", "line": line }),
            );
        }
        let _ = app_out.emit("serial://closed", serde_json::json!({}));
    });
    let app_err = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            let _ = app_err.emit(
                "serial://line",
                serde_json::json!({ "stream": "stderr", "line": line }),
            );
        }
    });

    *guard = Some(SerialOwner::Monitor(child));
    Ok(())
}

#[tauri::command]
fn stop_monitor(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.serial.lock().unwrap();
    if matches!(guard.as_ref(), Some(SerialOwner::Monitor(_))) {
        evict_owner(&mut guard);
    }
    Ok(())
}

/// Transmit a line to the board through the monitor's stdin.
#[tauri::command]
fn monitor_send(state: State<'_, AppState>, data: String) -> Result<(), String> {
    let mut guard = state.serial.lock().unwrap();
    let Some(SerialOwner::Monitor(child)) = guard.as_mut() else {
        return Err("serial monitor is not running".to_string());
    };
    let stdin = child.stdin.as_mut().ok_or("monitor stdin unavailable")?;
    writeln!(stdin, "{data}").map_err(err_str)?;
    stdin.flush().map_err(err_str)
}

// ---------- scope (ADC streaming firmware) ----------

/// Open `port` at `baud`, 8N1, with DTR asserted and RTS deasserted —
/// UART-bridge boards auto-reset on open; native CDC ports need DTR set
/// before they transmit.
fn open_scope_port(
    port: &str,
    baud: u32,
    timeout: Duration,
) -> Result<Box<dyn SerialPort>, String> {
    let mut sp = serialport::new(port, baud)
        .data_bits(serialport::DataBits::Eight)
        .parity(serialport::Parity::None)
        .stop_bits(serialport::StopBits::One)
        .timeout(timeout)
        .open()
        .map_err(err_str)?;
    let _ = sp.write_data_terminal_ready(true);
    let _ = sp.write_request_to_send(false);
    Ok(sp)
}

/// Pull complete lines out of an accumulating byte buffer, returning the
/// first `!BANCADA` banner found, if any.
fn banner_in_buffer(pending: &mut Vec<u8>) -> Option<ScopeCaps> {
    while let Some(nl) = pending.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = pending.drain(..=nl).collect();
        if let Some(caps) = scope::parse_banner(&String::from_utf8_lossy(&line)) {
            return Some(caps);
        }
    }
    None
}

/// Read from `sp` until `deadline`, feeding `pending`; returns early on banner.
fn read_for_banner(
    sp: &mut Box<dyn SerialPort>,
    pending: &mut Vec<u8>,
    deadline: Instant,
) -> Option<ScopeCaps> {
    let mut chunk = [0u8; 512];
    while Instant::now() < deadline {
        match sp.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                if let Some(caps) = banner_in_buffer(pending) {
                    return Some(caps);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None, // port vanished — caller reports
        }
    }
    None
}

/// Probe one baud rate: tolerate boot garbage (the open may have reset the
/// board — the boot banner itself may answer us), then ask `{"c":"id"}` up to
/// three times.
fn probe_at_baud(port: &str, baud: u32) -> Result<ScopeCaps, String> {
    let mut sp = open_scope_port(port, baud, Duration::from_millis(200))?;
    let mut pending: Vec<u8> = Vec::new();

    // Boot window: a board that auto-reset on open prints its banner ~1 s in.
    if let Some(caps) = read_for_banner(
        &mut sp,
        &mut pending,
        Instant::now() + Duration::from_millis(1500),
    ) {
        return Ok(caps);
    }

    for _ in 0..3 {
        sp.write_all(scope::cmd_id().as_bytes()).map_err(err_str)?;
        sp.flush().map_err(err_str)?;
        if let Some(caps) = read_for_banner(
            &mut sp,
            &mut pending,
            Instant::now() + Duration::from_millis(400),
        ) {
            return Ok(caps);
        }
    }
    Err(format!("no !BANCADA banner at {baud} baud"))
}

/// Detect the Bancada scope firmware on `port` and return its capabilities.
#[tauri::command]
async fn scope_probe(state: State<'_, AppState>, port: String) -> Result<ScopeCaps, String> {
    {
        let guard = state.serial.lock().unwrap();
        match guard.as_ref() {
            Some(SerialOwner::Scope(_)) => return Err("stop the scope first".to_string()),
            Some(SerialOwner::Monitor(_)) => {
                return Err("stop the serial monitor before probing the scope".to_string())
            }
            None => {}
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut last_err = String::new();
        for baud in [921_600u32, 115_200] {
            match probe_at_baud(&port, baud) {
                Ok(mut caps) => {
                    if caps.proto != 1 {
                        return Err(format!(
                            "scope firmware speaks protocol {} (need 1) — reflash the companion firmware",
                            caps.proto
                        ));
                    }
                    caps.baud = baud;
                    return Ok(caps); // port handle already dropped by probe_at_baud
                }
                Err(e) => last_err = e,
            }
        }
        Err(format!(
            "no Bancada scope firmware detected on {port} ({last_err})"
        ))
    })
    .await
    .map_err(err_str)?
}

/// Send one channel message, reporting whether the frontend is still there.
fn channel_send(on_message: &Channel<InvokeResponseBody>, bytes: Vec<u8>) -> bool {
    on_message.send(InvokeResponseBody::Raw(bytes)).is_ok()
}

/// Reader-thread body: raw bytes → FrameScanner → channel envelopes.
fn scope_reader_loop(
    mut reader: Box<dyn SerialPort>,
    stop: Arc<AtomicBool>,
    on_message: Channel<InvokeResponseBody>,
) {
    let mut scanner = FrameScanner::new();
    let mut buf = [0u8; 4096];
    let mut crc_reported: u32 = 0;

    while !stop.load(Ordering::Relaxed) {
        let n = match reader.read(&mut buf) {
            Ok(0) => {
                // Not a normal timeout; avoid a hot spin if the driver
                // reports EOF-ish reads on a dying port.
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break, // fatal: port unplugged, permissions, ...
        };

        for frame in scanner.push(&buf[..n]) {
            let sent = match frame.frame_type {
                ScopeFrame::TYPE_SAMPLES => channel_send(
                    &on_message,
                    scope::envelope_samples(frame.flags, frame.first_sample_index, &frame.payload),
                ),
                // META / RECORD_HDR / ERROR payloads are already JSON events.
                _ => channel_send(
                    &on_message,
                    scope::envelope_json(&String::from_utf8_lossy(&frame.payload)),
                ),
            };
            if !sent {
                return; // frontend gone; die quietly
            }
        }

        let dropped = scanner.take_dropped();
        if dropped > 0 {
            channel_send(
                &on_message,
                scope::envelope_json(&format!("{{\"ev\":\"drop\",\"frames\":{dropped}}}")),
            );
        }
        if scanner.crc_errors >= crc_reported + 50 {
            crc_reported = scanner.crc_errors;
            channel_send(
                &on_message,
                scope::envelope_json(&format!("{{\"ev\":\"crc\",\"count\":{crc_reported}}}")),
            );
        }
    }

    channel_send(&on_message, scope::envelope_json("{\"ev\":\"closed\"}"));
}

/// Start continuous ADC streaming; binary envelopes flow on `on_message`.
#[tauri::command]
fn scope_start(
    state: State<'_, AppState>,
    port: String,
    baud: u32,
    cfg: scope::ScopeStreamCfg,
    on_message: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let mut guard = state.serial.lock().unwrap();
    evict_owner(&mut guard);

    let mut writer = open_scope_port(&port, baud, Duration::from_millis(100))?;
    writer
        .write_all(scope::cmd_start(cfg.sps, &cfg.pins, cfg.atten).as_bytes())
        .map_err(err_str)?;
    writer.flush().map_err(err_str)?;

    let reader = writer.try_clone().map_err(err_str)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = stop.clone();
    let join = std::thread::spawn(move || scope_reader_loop(reader, stop_reader, on_message));

    *guard = Some(SerialOwner::Scope(ScopeSession {
        writer,
        stop,
        join: Some(join),
    }));
    Ok(())
}

/// Arm a device-triggered single-shot capture on the running scope session.
#[tauri::command]
fn scope_single(state: State<'_, AppState>, cfg: scope::ScopeSingleCfg) -> Result<(), String> {
    let mut guard = state.serial.lock().unwrap();
    let Some(SerialOwner::Scope(session)) = guard.as_mut() else {
        return Err("scope is not running".to_string());
    };
    session
        .writer
        .write_all(scope::cmd_single(&cfg).as_bytes())
        .map_err(err_str)?;
    session.writer.flush().map_err(err_str)
}

/// Escape hatch: send a raw control line to the scope port.
#[tauri::command]
fn scope_send(state: State<'_, AppState>, line: String) -> Result<(), String> {
    let mut guard = state.serial.lock().unwrap();
    let Some(SerialOwner::Scope(session)) = guard.as_mut() else {
        return Err("scope is not running".to_string());
    };
    session
        .writer
        .write_all(format!("{line}\n").as_bytes())
        .map_err(err_str)?;
    session.writer.flush().map_err(err_str)
}

/// Stop the scope session, if one is running. No-op for any other owner.
#[tauri::command]
fn scope_stop(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.serial.lock().unwrap();
    if matches!(guard.as_ref(), Some(SerialOwner::Scope(_))) {
        evict_owner(&mut guard);
    }
    Ok(())
}

/// Materialize the embedded companion firmware as a compilable sketch dir;
/// returns the sketch directory path (feed it to compile/upload).
#[tauri::command]
fn scope_install_firmware(dest_dir: String) -> Result<String, String> {
    let base = if dest_dir.is_empty() {
        std::env::temp_dir().join("bancada_scope_fw")
    } else {
        PathBuf::from(dest_dir)
    };
    let sketch_dir = base.join("bancada_scope");
    std::fs::create_dir_all(&sketch_dir).map_err(err_str)?;
    std::fs::write(
        sketch_dir.join("bancada_scope.ino"),
        include_str!("../../firmware/bancada_scope/bancada_scope.ino"),
    )
    .map_err(err_str)?;
    std::fs::write(
        sketch_dir.join("sketch.yaml"),
        include_str!("../../firmware/bancada_scope/sketch.yaml"),
    )
    .map_err(err_str)?;
    Ok(sketch_dir.to_string_lossy().into_owned())
}

// ---------- exports (paths come from the OS save dialog) ----------

#[tauri::command]
fn save_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(path, contents).map_err(err_str)
}

#[tauri::command]
fn save_binary_file(path: String, contents_b64: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(contents_b64.as_bytes())
        .map_err(err_str)?;
    std::fs::write(path, bytes).map_err(err_str)
}

// ---------- app settings ----------

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("settings.json"))
        .map_err(err_str)
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Result<bancada_core::settings::AppSettings, String> {
    Ok(bancada_core::settings::load(&settings_path(&app)?))
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    settings: bancada_core::settings::AppSettings,
) -> Result<(), String> {
    bancada_core::settings::save(&settings_path(&app)?, &settings).map_err(err_str)
}

// ---------- board utilities ----------

/// Read a board's MAC with esptool.
///
/// Takes the serial port from whoever holds it first: esptool drives the ROM
/// bootloader, so a running monitor or scope session would both fight it for the
/// device and be left reading a rebooting chip.
#[tauri::command]
async fn read_board_mac(
    state: State<'_, AppState>,
    port: String,
) -> Result<bancada_core::esptool::ChipInfo, String> {
    evict_owner(&mut state.serial.lock().unwrap());
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::esptool::read_mac(&port).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

// ---------- fleet (remembered physical boards) ----------

/// How stale a board's `last_seen` may get before a rescan rewrites the file.
/// "When did I last have this board plugged in" needs minutes, not seconds.
const LAST_SEEN_RESOLUTION: u64 = 60;

fn fleet_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("fleet.json"))
        .map_err(err_str)
}

/// Epoch seconds. The `fleet` module takes `now` as a parameter so it stays
/// pure and deterministic under test; this is the only place a clock is read.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_fleet(app: &AppHandle) -> Result<(PathBuf, Fleet), String> {
    let path = fleet_path(app)?;
    let fleet = Fleet::load(&path).map_err(err_str)?;
    Ok((path, fleet))
}

/// The whole panel's data in one round trip: the remembered boards, which of
/// them are plugged in, and which attached ports we cannot name.
#[derive(serde::Serialize)]
struct FleetSnapshot {
    boards: Vec<fleet::FleetEntry>,
    online: Vec<String>,
    unidentified: Vec<DetectedPort>,
}

/// Record every identifiable port from a scan and return the updated snapshot.
///
/// Called when the Fleet panel opens and after each port rescan, so plugging a
/// board in enrols it. Ports with no board-specific identity (a bare USB-serial
/// bridge) are skipped rather than recorded under the bridge's shared id.
#[tauri::command]
fn fleet_sync(app: AppHandle, ports: Vec<DetectedPort>) -> Result<FleetSnapshot, String> {
    let (path, mut f) = load_fleet(&app)?;
    let a = fleet::attached(&ports);
    let now = now_secs();

    // Ports are rescanned often — on mount, on every manual refresh, and each
    // time the Fleet panel re-renders. Writing on every one of those would churn
    // the file for no new information and widen the window in which a concurrent
    // rename could be lost to this load-modify-save. So only write when a board
    // is genuinely new or its `last_seen` has gone stale.
    let stale = f.boards.iter().any(|b| {
        a.online.contains(&b.id) && now.saturating_sub(b.last_seen) > LAST_SEEN_RESOLUTION
    });
    let added = fleet::sight_all(&mut f, &ports, now);
    if added > 0 || stale {
        f.save(&path).map_err(err_str)?;
    }
    Ok(FleetSnapshot {
        boards: f.boards,
        online: a.online,
        unidentified: a.unidentified,
    })
}

#[tauri::command]
fn set_board_nickname(
    app: AppHandle,
    id: String,
    nickname: Option<String>,
) -> Result<Vec<fleet::FleetEntry>, String> {
    let (path, mut f) = load_fleet(&app)?;
    f.set_nickname(&id, nickname.as_deref()).map_err(err_str)?;
    f.save(&path).map_err(err_str)?;
    Ok(f.boards)
}

/// Record that the board on `port` was built for `fqbn`.
///
/// Resolving the port to a fleet id happens here so the frontend never needs a
/// copy of the identity rules. Called opportunistically after a successful
/// upload, so anything unknown — an unidentifiable bridge, a port that has since
/// vanished — is a silent no-op: bookkeeping must not turn a good flash into an
/// error.
#[tauri::command]
async fn note_board_fqbn(
    app: AppHandle,
    state: State<'_, AppState>,
    port: String,
    fqbn: String,
) -> Result<(), String> {
    let cli = state.cli.clone();
    let ports = tauri::async_runtime::spawn_blocking(move || cli.board_list().map_err(err_str))
        .await
        .map_err(err_str)??;

    let Some(id) = ports
        .iter()
        .find(|dp| dp.port.address == port)
        .and_then(|dp| fleet::identify(&dp.port))
    else {
        return Ok(());
    };

    let (path, mut f) = load_fleet(&app)?;
    if f.get(&id.value).is_none() {
        return Ok(());
    }
    f.note_fqbn(&id.value, &fqbn).map_err(err_str)?;
    f.save(&path).map_err(err_str)
}

/// Ask esptool for a board's real MAC and fold it into the registry.
///
/// `previous_id` is the id the board is currently filed under, if any, so a
/// serial-keyed record — nickname and history included — is migrated to the MAC
/// rather than orphaned beside it.
#[tauri::command]
async fn identify_board(
    app: AppHandle,
    state: State<'_, AppState>,
    port: String,
    previous_id: Option<String>,
) -> Result<Vec<fleet::FleetEntry>, String> {
    evict_owner(&mut state.serial.lock().unwrap());
    let info = tauri::async_runtime::spawn_blocking(move || {
        bancada_core::esptool::read_mac(&port).map_err(err_str)
    })
    .await
    .map_err(err_str)??;

    let (path, mut f) = load_fleet(&app)?;
    f.merge_identified(
        previous_id.as_deref(),
        &info.mac,
        info.chip_type.as_deref(),
        now_secs(),
    )
    .map_err(err_str)?;
    f.save(&path).map_err(err_str)?;
    Ok(f.boards)
}

#[tauri::command]
fn forget_board(app: AppHandle, id: String) -> Result<Vec<fleet::FleetEntry>, String> {
    let (path, mut f) = load_fleet(&app)?;
    f.forget(&id);
    f.save(&path).map_err(err_str)?;
    Ok(f.boards)
}

// ---------- mqtt ----------

use bancada_core::mqtt;
use bancada_core::mqtt::rumqttc::{
    Client as MqttClient, ConnectReturnCode, Connection as MqttConnection, Event as MqttNetEvent,
    MqttOptions, Packet as MqttPacket, QoS, SubscribeReasonCode,
};

/// A live broker connection: the `Client` request handle lives under the
/// state mutex; the `Connection` is owned by a dedicated thread that never
/// touches the mutex (same discipline as the scope reader).
struct MqttSession {
    client: MqttClient,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

/// Stop an MQTT session: flag the thread down, then `disconnect()` — which
/// unblocks the connection iterator — and join.
fn stop_mqtt_session(mut session: MqttSession) {
    session.stop.store(true, Ordering::Relaxed);
    let _ = session.client.disconnect();
    if let Some(handle) = session.join.take() {
        let _ = handle.join();
    }
}

/// Send one JSON envelope, reporting whether the frontend is still there.
fn mqtt_send(on_message: &Channel<InvokeResponseBody>, event: &mqtt::MqttEvent) -> bool {
    let json = serde_json::to_string(event).expect("MqttEvent always serializes");
    on_message.send(InvokeResponseBody::Json(json)).is_ok()
}

/// Epoch milliseconds for `msg` envelope timestamps.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Connection-thread body: rumqttc events → JSON envelopes on the channel.
///
/// Never retries (decision D4): rumqttc's iterator would happily reconnect if
/// kept looping, so any `Err` — and any refused CONNACK — emits `closed` and
/// breaks; the frontend owns the backoff/reconnect policy.
fn mqtt_reader_loop(
    mut connection: MqttConnection,
    stop: Arc<AtomicBool>,
    on_message: Channel<InvokeResponseBody>,
    started: Instant,
) {
    let mut reason = "disconnected".to_string();
    for event in connection.iter() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match event {
            Ok(MqttNetEvent::Incoming(MqttPacket::ConnAck(ack))) => {
                let detail = mqtt::connack_text(ack.code).to_string();
                if ack.code == ConnectReturnCode::Success {
                    // The socket demonstrably connected for a CONNACK to
                    // arrive, so the tcp stage folds into this moment.
                    let tcp = mqtt::MqttEvent::Stage {
                        stage: mqtt::MqttStage::Tcp,
                        ok: true,
                        detail: "socket connected".to_string(),
                        elapsed_ms,
                    };
                    let connack = mqtt::MqttEvent::Stage {
                        stage: mqtt::MqttStage::Connack,
                        ok: true,
                        detail,
                        elapsed_ms,
                    };
                    if !mqtt_send(&on_message, &tcp) || !mqtt_send(&on_message, &connack) {
                        return; // frontend gone; die quietly
                    }
                } else {
                    let _ = mqtt_send(
                        &on_message,
                        &mqtt::MqttEvent::Stage {
                            stage: mqtt::MqttStage::Connack,
                            ok: false,
                            detail: detail.clone(),
                            elapsed_ms,
                        },
                    );
                    reason = format!("connection refused: {detail}");
                    break;
                }
            }
            Ok(MqttNetEvent::Incoming(MqttPacket::SubAck(ack))) => {
                let rejected = ack
                    .return_codes
                    .iter()
                    .any(|c| matches!(c, SubscribeReasonCode::Failure));
                let ev = mqtt::MqttEvent::Stage {
                    stage: mqtt::MqttStage::Suback,
                    ok: !rejected,
                    detail: if rejected {
                        "broker rejected the subscription".to_string()
                    } else {
                        "subscription acknowledged".to_string()
                    },
                    elapsed_ms,
                };
                if !mqtt_send(&on_message, &ev) {
                    return;
                }
            }
            Ok(MqttNetEvent::Incoming(MqttPacket::Publish(p))) => {
                let ev = mqtt::MqttEvent::msg_from_parts(
                    &p.topic,
                    &p.payload,
                    p.retain,
                    p.qos as u8,
                    now_millis(),
                );
                if !mqtt_send(&on_message, &ev) {
                    return;
                }
            }
            Ok(_) => {} // other incoming packets and all outgoing echoes
            Err(e) => {
                reason = e.to_string();
                break;
            }
        }
    }
    let _ = mqtt_send(&on_message, &mqtt::MqttEvent::Closed { reason });
}

/// Connect to a broker; JSON envelopes flow on `on_message`. Passing a
/// `subscribe_filter` subscribes immediately (QoS 0); `None` observes nothing
/// until `mqtt_subscribe` is called. An existing session is replaced.
#[tauri::command]
fn mqtt_connect(
    state: State<'_, AppState>,
    url: String,
    subscribe_filter: Option<String>,
    on_message: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let mut guard = state.mqtt.lock().unwrap();
    if let Some(old) = guard.take() {
        stop_mqtt_session(old);
    }

    let started = Instant::now();
    let addr = match mqtt::parse_url(&url) {
        Ok(addr) => {
            let _ = mqtt_send(
                &on_message,
                &mqtt::MqttEvent::Stage {
                    stage: mqtt::MqttStage::Parse,
                    ok: true,
                    detail: mqtt::redact_password(&url),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
            );
            addr
        }
        Err(e) => {
            let _ = mqtt_send(
                &on_message,
                &mqtt::MqttEvent::Stage {
                    stage: mqtt::MqttStage::Parse,
                    ok: false,
                    detail: e.clone(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
            );
            let _ = mqtt_send(&on_message, &mqtt::MqttEvent::Closed { reason: e.clone() });
            return Err(e);
        }
    };

    let mut opts = MqttOptions::new(mqtt::client_id(), addr.host, addr.port);
    opts.set_keep_alive(Duration::from_secs(15));
    if let Some(user) = addr.username {
        opts.set_credentials(user, addr.password.unwrap_or_default());
    }

    let (client, connection) = MqttClient::new(opts, 100);
    // Queued now, delivered after CONNACK — the SUBACK stage confirms it.
    if let Some(filter) = subscribe_filter.filter(|f| !f.is_empty()) {
        client.subscribe(filter, QoS::AtMostOnce).map_err(err_str)?;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = stop.clone();
    let join =
        std::thread::spawn(move || mqtt_reader_loop(connection, stop_reader, on_message, started));

    *guard = Some(MqttSession {
        client,
        stop,
        join: Some(join),
    });
    Ok(())
}

#[tauri::command]
fn mqtt_publish(
    state: State<'_, AppState>,
    topic: String,
    payload: String,
    retain: bool,
) -> Result<(), String> {
    let guard = state.mqtt.lock().unwrap();
    let Some(session) = guard.as_ref() else {
        return Err("mqtt is not connected".to_string());
    };
    session
        .client
        .publish(topic, QoS::AtMostOnce, retain, payload.into_bytes())
        .map_err(err_str)
}

#[tauri::command]
fn mqtt_subscribe(state: State<'_, AppState>, filter: String) -> Result<(), String> {
    let guard = state.mqtt.lock().unwrap();
    let Some(session) = guard.as_ref() else {
        return Err("mqtt is not connected".to_string());
    };
    session
        .client
        .subscribe(filter, QoS::AtMostOnce)
        .map_err(err_str)
}

#[tauri::command]
fn mqtt_unsubscribe(state: State<'_, AppState>, filter: String) -> Result<(), String> {
    let guard = state.mqtt.lock().unwrap();
    let Some(session) = guard.as_ref() else {
        return Err("mqtt is not connected".to_string());
    };
    session.client.unsubscribe(filter).map_err(err_str)
}

#[tauri::command]
fn mqtt_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.mqtt.lock().unwrap();
    let Some(session) = guard.take() else {
        return Err("mqtt is not connected".to_string());
    };
    stop_mqtt_session(session);
    Ok(())
}

fn mqtt_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("mqtt.json"))
        .map_err(err_str)
}

#[tauri::command]
fn load_mqtt_config(app: AppHandle) -> Result<mqtt::MqttConfig, String> {
    Ok(mqtt::load(&mqtt_config_path(&app)?))
}

#[tauri::command]
fn save_mqtt_config(app: AppHandle, cfg: mqtt::MqttConfig) -> Result<(), String> {
    mqtt::save(&mqtt_config_path(&app)?, &cfg).map_err(err_str)
}

// ---------- agent (Assistant panel) ----------

use bancada_core::agent::{self, AgentCfg};
use bancada_core::mcp::{self, McpReply};
use bancada_core::types::OutputLine;

/// A running embedded `claude` session.
///
/// Thread discipline mirrors the monitor/scope/MQTT sessions: the handles
/// that must be reachable from a command live under the `agent` mutex, and
/// everything that *reads* or *blocks* lives on a detached thread that never
/// touches that mutex. In particular `stdin_tx` is an mpsc sender rather
/// than the child's `ChildStdin`: writing to the pipe under the mutex would
/// deadlock the moment a message exceeds the 64 KB pipe buffer while the
/// child is itself blocked waiting on an MCP reply.
struct AgentSession {
    child: Child,
    /// Writer-thread channel. Commands clone this out of the mutex, drop
    /// the guard, and only then send — the mutex is never held across a
    /// write to the child.
    stdin_tx: std::sync::mpsc::Sender<String>,
    /// Kept solely so shutdown can call `unblock()`: the listener thread
    /// parks in `incoming_requests()`, and no flag can break that recv.
    mcp_server: Arc<tiny_http::Server>,
    sketch_dir: String,
    /// The 0600 temp file backing `--mcp-config` (F5: keeps the bearer token
    /// off argv) — deleted when the session stops.
    mcp_config_path: PathBuf,
    /// The 0600 temp file backing `--settings`: the A1 `PreToolUse`
    /// confinement hook. Also deleted when the session stops.
    settings_path: PathBuf,
    /// Set by `stop_agent_session`. `Server::unblock()` only wakes a thread
    /// parked in `incoming_requests()` — a listener that is *inside*
    /// `run_verify` (a multi-minute build) does not see it, and used to run
    /// on holding the build gate and emitting `verify_done` through the live
    /// `AppHandle` into whatever session the panel was showing by then. This
    /// flag is what `run_verify` checks before taking the gate and before
    /// every emit (C1).
    verify_cancel: Arc<AtomicBool>,
}

/// Lock the agent slot, recovering from a poisoned mutex.
///
/// Same policy as `try_build_gate`, and for the same reason: what the mutex
/// guards is an `Option<AgentSession>` whose invariants do not survive a
/// panic *anyway* (the panicking command has already returned), and
/// `.unwrap()`ing here meant one panic under the lock bricked every agent
/// command for the process lifetime — including the `RunEvent::Exit`
/// teardown, which would then panic during shutdown and orphan the child
/// plus its 0600 temp files (I4).
fn lock_agent(state: &AppState) -> std::sync::MutexGuard<'_, Option<AgentSession>> {
    state.agent.lock().unwrap_or_else(|e| e.into_inner())
}

/// A cold ESP32 platform build runs for minutes — far longer than the
/// default MCP tool timeout, which would abort `verify` mid-compile.
const MCP_TIMEOUT_MS: &str = "600000";

/// Hard cap on an MCP request body, enforced before anything is parsed.
const MCP_MAX_BODY: usize = 1024 * 1024;

/// Caps for the build summary handed back to the agent (spec decision #7).
const VERIFY_MAX_LINES: usize = 200;
const VERIFY_MAX_BYTES: usize = 50_000;

/// Everything the MCP listener thread needs, owned outright.
///
/// Deliberately a set of **clones taken at `agent_start`**: the listener
/// must never lock `state.agent`, or a `verify` arriving while a command
/// holds that mutex would deadlock the compile against the UI. The
/// trade-off is that a session keeps the profile/fqbn it was started with
/// even if the user switches boards mid-session.
struct McpVerifyCtx {
    token: String,
    cli: ArduinoCli,
    sketch_dir: String,
    profile: Option<String>,
    fqbn: Option<String>,
    build_gate: Arc<Mutex<()>>,
    /// The session's child pid, stamped onto every synthetic event this
    /// listener emits so the frontend can discard one that outlived its
    /// session (C1) — the same identity guard `agent://closed` already had.
    session_pid: u32,
    /// Shared with `AgentSession::verify_cancel`; see its doc.
    cancelled: Arc<AtomicBool>,
}

impl McpVerifyCtx {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Bind the MCP listener on a kernel-assigned loopback port and read the
/// real port back — no collision roulette over a hardcoded number.
fn bind_mcp_server() -> Result<(Arc<tiny_http::Server>, u16), String> {
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|e| format!("could not start the agent's MCP listener: {e}"))?;
    let port = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
        other => return Err(format!("MCP listener bound to a non-IP address: {other:?}")),
    };
    Ok((Arc::new(server), port))
}

/// A per-session bearer token for the loopback MCP listener.
///
/// The workspace has no `rand` dependency (`mqtt::client_id` solves its own
/// uniqueness problem the same way), so read 16 bytes from the OS entropy
/// pool directly. The listener is bound to `127.0.0.1`, so this only has to
/// keep *other local processes* from driving the user's compiler.
///
/// **No fallback.** This used to degrade to a `nanos + heap address` mix when
/// `/dev/urandom` could not be read. That is a uniqueness device, not an
/// unguessability one: a local attacker knows roughly when the session
/// started and ASLR gives few bits, so the "token" would be searchable —
/// while the caller went on believing the listener was protected. A session
/// that cannot be given a real secret must fail to start instead.
fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| {
            format!(
                "could not read 16 bytes of entropy for the agent's MCP token \
                 (/dev/urandom: {e}) — refusing to start a session whose \
                 loopback listener would be guessable"
            )
        })?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes the child's `--mcp-config` JSON (the loopback server's URL plus
/// its bearer token) to a private temp file, instead of the CLI argv (F5,
/// post-review fix): argv is visible to any local process via
/// `/proc/<pid>/cmdline` on Linux, which would hand out the one thing
/// gating the `verify` listener to whoever asked. `agent_start` deletes the
/// file on every exit path (spawn failure, stdio-pipe failure, and the
/// normal `stop_agent_session`).
///
/// The filename carries a fresh random nonce, not the token: a temp-dir
/// *listing* is typically world-readable even when an individual file's
/// contents aren't, so the secret has no business appearing in the name.
///
/// On unix the file is *created* at 0600 (`OpenOptionsExt::mode` +
/// `create_new`), not written-then-chmodded (post-review fix): the latter
/// leaves a real window — however brief — where the file exists at the
/// process umask (often 022, i.e. world-readable) before the permission
/// tightens. `create_new` is also `O_EXCL`, so this refuses to write
/// through a pre-existing file or symlink at the same path rather than
/// silently following it.
fn write_mcp_config_file(port: u16, token: &str) -> Result<PathBuf, String> {
    let mcp_config = serde_json::json!({
        "mcpServers": {
            "bancada": {
                "type": "http",
                "url": format!("http://127.0.0.1:{port}/mcp"),
                "headers": {
                    "Authorization": format!("Bearer {token}")
                }
            }
        }
    })
    .to_string();
    let path = std::env::temp_dir().join(format!("bancada-agent-mcp-{}.json", random_token()?));
    write_private_file(&path, &mcp_config, "MCP config")
}

/// The `--settings` payload: Bancada's write-confinement policy (A1).
///
/// **Two layers, and the order between them is the whole point.**
///
/// 1. `permissions.deny` rules (`core::agent::deny_rules`) protecting the
///    project's `.claude/`, `.git/`, `.mcp.json` and the user's own
///    `~/.claude/**`. Deny rules are evaluated *before* hooks and are
///    unaffected by `disableAllHooks`.
/// 2. A `PreToolUse` hook doing the subtree containment deny rules cannot
///    express — a denylist has no "everything except this directory" form.
///
/// A hook alone is **not** sufficient, and this was the red-team finding
/// against the first version of this code: a project `.claude/settings.json`
/// containing `{"disableAllHooks": true}` makes a `--settings`-supplied
/// `PreToolUse` hook never fire at all (verified against 2.1.220 —
/// instrumented hook log empty, out-of-tree write landed). A hook that is
/// the only thing protecting `.claude/` is one write away from being
/// switched off, so the deny rules anchor the hook rather than the reverse.
///
/// `--managed-settings` (a hidden "policy-tier settings JSON from a spawning
/// parent process, SDK use only" flag) would be the theoretically stronger
/// tier, being the one `disableAllHooks` cannot override. It is not used:
/// probed clean against 2.1.220 with no hostile settings anywhere, a
/// `PreToolUse` hook supplied through it **never fired** and the write went
/// straight through. It does not carry hooks.
///
/// The hook's `command` is **this very binary**, re-invoked as
/// `bancada --agent-guard <sketch_dir>` (see `run_agent_guard`). Using
/// `current_exe()` rather than generating a shell script means the policy is
/// the same tested Rust function (`core::agent::guard_decision`) the unit
/// tests exercise, with no dependency on a `sh`/`python3` being present and
/// no second copy of the rule to drift.
///
/// The command string is shell-parsed by the CLI, so both the executable
/// path and the sketch dir are single-quoted with POSIX escaping
/// (`shell_quote`) — a project directory containing a space or a quote must
/// not turn into two arguments, and must certainly not turn into a second
/// command.
///
/// **The file is written to the temp dir, deliberately outside the project
/// tree**: the policy must not live somewhere the thing it constrains can
/// edit, or an agent that got past the hook could rewrite its own rules.
/// Same 0600 `create_new` discipline as the MCP config, and for a sharper
/// reason — this file names a command the CLI will *execute*, so a
/// world-writable file at a predictable path would be a way to make Bancada
/// run someone else's command.
fn write_agent_settings_file(sketch_dir: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("could not resolve Bancada's own executable path: {e}"))?;
    write_agent_settings_file_with_exe(sketch_dir, &exe)
}

/// The body of [`write_agent_settings_file`], with the guard executable
/// passed in.
///
/// Split out for the live tests only: under `cargo test`, `current_exe()` is
/// the libtest harness, which has no `--agent-guard` entry point — so a live
/// test that used it would exercise a hook that does nothing and prove the
/// opposite of what it claims. They pass the real `bancada` binary instead.
/// Production has exactly one caller, immediately above.
fn write_agent_settings_file_with_exe(sketch_dir: &str, exe: &Path) -> Result<PathBuf, String> {
    let command = format!(
        "{} --agent-guard {}",
        shell_quote(&exe.to_string_lossy()),
        shell_quote(sketch_dir)
    );
    // Deny patterns are matched against resolved paths, so they must be
    // built from the *canonical* project dir: a project opened through a
    // symlink would otherwise get rules that never match anything.
    let canonical = std::fs::canonicalize(sketch_dir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| sketch_dir.to_string());
    let settings = serde_json::json!({
        "permissions": { "deny": agent::deny_rules(&canonical) },
        "hooks": {
            "PreToolUse": [{
                "matcher": agent::guard_hook_matcher(),
                "hooks": [{ "type": "command", "command": command, "timeout": 15 }]
            }]
        }
    })
    .to_string();
    let path =
        std::env::temp_dir().join(format!("bancada-agent-settings-{}.json", random_token()?));
    write_private_file(&path, &settings, "settings")
}

/// Refuse to start when a settings file already switches hooks off (A1).
///
/// `{"disableAllHooks": true}` in any settings source stops the confinement
/// hook firing — verified live, twice, including against this exact
/// production configuration: the `permissions.deny` anchor still held, but
/// the *subtree containment* was gone and an out-of-project write landed.
///
/// The agent cannot create such a file (the deny rules cover `.claude/**`),
/// so this only arises for a project — or a user config — that already had
/// it before Bancada opened it. That is checkable before spawning, and a
/// session whose boundary is known in advance not to hold must not start.
/// Returns the offending path in the error so the user can act on it rather
/// than face an unexplained refusal.
fn check_hooks_are_enabled(sketch_dir: &str) -> Result<(), String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    for path in agent::hook_disabling_settings_paths(Path::new(sketch_dir), home.as_deref()) {
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue; // absent or unreadable — cannot disable anything
        };
        if agent::settings_disables_hooks(&body) {
            return Err(format!(
                "The Assistant cannot start safely: {} sets \"disableAllHooks\", \
                 which stops Bancada's file-confinement hook from running at all. \
                 Remove that setting (or open a project that does not carry it) \
                 and try again.",
                path.display()
            ));
        }
    }
    Ok(())
}

/// POSIX single-quoting: wrap in `'...'` and replace each embedded `'` with
/// `'\''`. The result is one shell word whatever the input contains.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Create `path` at 0600 and write `body`, never following or truncating an
/// existing file. `what` names the file in error messages.
fn write_private_file(path: &Path, body: &str, what: &str) -> Result<PathBuf, String> {
    let path = path.to_path_buf();

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("could not create the agent's {what} file: {e}"))?;
        if let Err(e) = file.write_all(body.as_bytes()) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(format!("could not write the agent's {what} file: {e}"));
        }
    }
    #[cfg(not(unix))]
    {
        // No equivalent atomic-mode create outside unix (no Windows dev/CI
        // target exists in this workspace yet) — plain write, no ACL
        // narrowing. Revisit if/when one does.
        std::fs::write(&path, body)
            .map_err(|e| format!("could not write the agent's {what} file: {e}"))?;
    }

    Ok(path)
}

/// The `bancada --agent-guard <sketch_dir>` entry point: the `PreToolUse`
/// hook the embedded session is started with (A1).
///
/// Reads the CLI's hook JSON from stdin, adjudicates it with
/// `core::agent::guard_decision`, and prints a deny payload (or nothing) on
/// stdout. **Always exits 0**: a non-zero exit is how a hook reports *its own*
/// failure, and the CLI's handling of that is "log and continue" — i.e. fail
/// open, which is exactly what this must never do. The refusal is carried by
/// the printed JSON, not by the exit code.
///
/// Reading stdin cannot block the session for long: the CLI writes one small
/// JSON object and closes the pipe.
fn run_agent_guard(sketch_dir: &str) {
    let mut stdin_body = String::new();
    // A read error yields an empty body, which `guard_decision` denies —
    // failing closed, same as unparseable input.
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin_body);
    if let Some(deny) = agent::guard_decision(Path::new(sketch_dir), &stdin_body) {
        println!("{deny}");
    }
}

/// Intercept `--agent-guard <sketch_dir>` before Tauri ever starts.
///
/// Called first thing from `run()`. When Bancada is invoked as its own
/// `PreToolUse` hook it must behave as a plain stdin/stdout filter — building
/// a windowed app for every tool call would be absurd, and on a headless
/// machine would fail outright.
///
/// Returns `true` when it handled the invocation and the caller must return.
fn handle_agent_guard_argv() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(idx) = args.iter().position(|a| a == "--agent-guard") else {
        return false;
    };
    match args.get(idx + 1) {
        Some(sketch_dir) => run_agent_guard(sketch_dir),
        // No dir to confine against — deny everything rather than start a GUI.
        None => println!(
            "{}",
            agent::guard_decision(Path::new(""), "").unwrap_or_default()
        ),
    }
    true
}

/// The MCP listener thread body: JSON-RPC over loopback HTTP, plus the one
/// tool that actually does work.
///
/// `emit` is the only way out of this thread — production passes
/// `AppHandle::emit`, tests pass a collector, which is what makes the whole
/// listener testable without standing up a Tauri app.
///
/// Returns when `unblock()` is called on the server (see [`AgentSession`]).
fn mcp_listener_loop(
    server: Arc<tiny_http::Server>,
    ctx: McpVerifyCtx,
    emit: impl Fn(&str, serde_json::Value),
) {
    let tools = vec![mcp::verify_tool_def()];

    for mut request in server.incoming_requests() {
        // The CLI's MCP client opens a `GET /mcp` server->client SSE stream
        // alongside its POSTs. This server offers no such stream, and the
        // spec's answer for that is 405 — feeding the empty GET body through
        // `handle_request` instead returns a JSON-RPC parse error, which the
        // client treats as a broken stream and reconnects from in a tight
        // busy-loop (observed during the Task 5 prototype).
        if request.method() != &tiny_http::Method::Post {
            let _ = request.respond(tiny_http::Response::empty(405));
            continue;
        }

        let auth = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.as_str().to_string());
        if !mcp::check_bearer(auth.as_deref(), &ctx.token) {
            let _ = request.respond(tiny_http::Response::empty(401));
            continue;
        }

        // Size cap *before* any byte is read (I2). Checking the declared
        // Content-Length first is what keeps teardown responsive: reading
        // even a capped 1 MB from a client that dribbles one byte a minute
        // parks this thread for as long as that client likes, and
        // `unblock()` cannot interrupt a read — so the whole session's
        // listener thread leaks, holding its `Arc<Server>`, `ArduinoCli` and
        // `AppHandle` alive with it.
        //
        // Residual: a client that declares a small (or chunked, hence
        // `None`) length and then dribbles those bytes still parks the
        // thread. Bounding *that* needs a read timeout on the socket, which
        // `tiny_http` does not expose on a `Request`. The realistic case
        // this closes is a huge declared body; the residual case is
        // loopback-only and needs a local process actively trying.
        if request
            .body_length()
            .is_some_and(|declared| declared > MCP_MAX_BODY)
        {
            let _ = request.respond(tiny_http::Response::empty(413));
            continue;
        }
        let mut body = String::new();
        let read = request
            .as_reader()
            .take(MCP_MAX_BODY as u64 + 1)
            .read_to_string(&mut body);
        if read.is_err() || body.len() > MCP_MAX_BODY {
            let _ = request.respond(tiny_http::Response::empty(413));
            continue;
        }

        match mcp::handle_request(&body, &tools) {
            // Spec-required: a JSON-RPC notification has no id to correlate
            // a reply with, so it gets 202 and an empty body, never JSON.
            McpReply::NoContent => {
                let _ = request.respond(tiny_http::Response::empty(202));
            }
            McpReply::Json(json) => {
                let _ = request.respond(json_response(json));
            }
            McpReply::CallTool { id, name, .. } => {
                let (text, is_error) = if name == "verify" {
                    run_verify(&ctx, &emit)
                } else {
                    // Unreachable via `handle_request`, which rejects tools
                    // outside `tools` — belt and braces.
                    (format!("unknown tool: {name}"), true)
                };
                let _ = request.respond(json_response(mcp::tool_result_json(&id, &text, is_error)));
            }
        }
    }
}

/// What `verify` reports when its session was stopped before it could start.
const VERIFY_CANCELLED: &str = "the agent session was stopped before this build could start";

/// The `verify` tool: the same `cli.compile` path the toolbar's Verify
/// button runs, with the same `build://line` stream, so agent builds show up
/// in the Console for free.
///
/// ## Cancellation (C1)
///
/// `stop_agent_session` tears a session down with `Server::unblock()`, which
/// only wakes a listener thread parked in `incoming_requests()`. A listener
/// *inside* this function does not see it: before the fix it went on holding
/// the `build_gate` (blocking every build in the app, user Verify included)
/// and then emitted `verify_done` through the still-live `AppHandle`, into
/// whatever session the panel was showing by then.
///
/// So: the cancel flag is checked **before taking the gate** and **before
/// every emit**, and every synthetic event carries `session_pid` so the
/// frontend can discard one that outlived its session even if it slips
/// through the flag check by a hair.
///
/// Residual, and deliberately not fixed here: a compile that has *already
/// started* runs to completion and keeps the gate until it does.
/// `ArduinoCli::compile` is a blocking call with no abort handle, and giving
/// it one means changing a signature `compile_sketch`/`upload_sketch` share.
/// The window that used to be unbounded (a whole cold platform build, with
/// leaking events at the end) is now "one in-flight build, silent".
fn run_verify(ctx: &McpVerifyCtx, emit: &impl Fn(&str, serde_json::Value)) -> (String, bool) {
    // Stamp every synthetic event with the session it belongs to, and drop
    // it entirely once that session is gone.
    let emit_agent = |mut payload: serde_json::Value| {
        if ctx.is_cancelled() {
            return;
        }
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("pid".to_string(), serde_json::json!(ctx.session_pid));
        }
        emit("agent://event", payload);
    };

    // Before the gate, not after: a cancelled session must not take a lock
    // that blocks every other build in the app for the length of a compile.
    if ctx.is_cancelled() {
        return (VERIFY_CANCELLED.to_string(), true);
    }

    // Shared with compile_sketch/upload_sketch: an agent build must not race
    // a user build through the same arduino-cli build cache (risk R5).
    let _gate = match try_build_gate(&ctx.build_gate) {
        Ok(guard) => guard,
        Err(busy) => return (busy, true),
    };

    // Re-checked after the gate: `try_build_gate` is non-blocking, but the
    // stop could have landed between the two lines.
    if ctx.is_cancelled() {
        return (VERIFY_CANCELLED.to_string(), true);
    }

    emit_agent(serde_json::json!({ "type": "verify_started" }));

    let mut collected: Vec<OutputLine> = Vec::new();
    let run = ctx.cli.compile(
        &ctx.sketch_dir,
        ctx.profile.as_deref(),
        ctx.fqbn.as_deref(),
        &[],
        |line| {
            // Console lines are session-agnostic, so they get the same
            // cancellation check rather than a pid stamp: a stopped
            // session's build output has no business scrolling past in the
            // Console under a new session's Verify.
            if !ctx.is_cancelled() {
                if let Ok(value) = serde_json::to_value(&line) {
                    emit("build://line", value);
                }
            }
            collected.push(line);
        },
    );

    match run {
        Ok(result) => {
            emit_agent(serde_json::json!({
                "type": "verify_done", "success": result.success
            }));
            let summary =
                agent::summarize_build_output(&collected, VERIFY_MAX_LINES, VERIFY_MAX_BYTES);
            let text = format!(
                "success: {}\nexit_code: {}\n\n{summary}",
                result.success, result.exit_code
            );
            // `isError` is false even for a *failed* build: the tool itself
            // ran fine, and the whole point of the session is to iterate on
            // compiler errors. Flagging the normal path of a fix-the-build
            // loop as a tool failure invites the model to stop calling the
            // tool. `isError` is reserved for "the tool could not run".
            (text, false)
        }
        Err(e) => {
            emit_agent(serde_json::json!({ "type": "verify_done", "success": false }));
            (format!("verify could not run: {e}"), true)
        }
    }
}

fn json_response(json: String) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header is valid");
    tiny_http::Response::from_string(json).with_header(header)
}

/// The project context appended to the agent's system prompt.
fn system_prompt_extra(sketch_dir: &str, profile: Option<&str>, fqbn: Option<&str>) -> String {
    let mut out = format!(
        "You are embedded in Bancada, an Arduino workbench. The Arduino \
         project you are working on is at {sketch_dir}, which is also your \
         working directory."
    );
    if let Some(profile) = profile {
        out.push_str(&format!(" The active sketch.yaml profile is {profile}."));
    }
    if let Some(fqbn) = fqbn {
        out.push_str(&format!(" The active board FQBN is {fqbn}."));
    }
    out.push_str(
        " To compile, use the mcp__bancada__verify tool — never shell out to \
         arduino-cli, and never try to upload to the board. After every edit, \
         run mcp__bancada__verify and iterate until the build passes.",
    );
    out
}

/// Tear a session down: cancel any in-flight verify, close the writer
/// channel, stop the listener, then kill and reap the child so no zombie is
/// left behind.
fn stop_agent_session(session: AgentSession) {
    let AgentSession {
        child,
        stdin_tx,
        mcp_server,
        mcp_config_path,
        settings_path,
        verify_cancel,
        ..
    } = session;
    // First, before anything else: `unblock()` below cannot reach a listener
    // that is inside `run_verify`, and this flag can (C1).
    verify_cancel.store(true, Ordering::SeqCst);
    drop(stdin_tx); // the writer thread's recv ends
    mcp_server.unblock(); // the listener's blocking incoming_requests() ends
    kill_child(child);
    // Best-effort (F5 cleanup): leftover temp files after a hard crash
    // aren't worth failing shutdown over, but every normal stop/exit path
    // reaches here and removes them.
    let _ = std::fs::remove_file(&mcp_config_path);
    let _ = std::fs::remove_file(&settings_path);
}

/// Take and stop the live session **only if it is the one `pid` names**.
///
/// The teardown half of `should_stop_agent`, for callers that already know
/// which session they are talking about: the stdout reader at EOF (D1) and
/// the security backstop (A1/A2). Both run on a per-session thread that may
/// well be outlived by a newer session, and neither must take that newer
/// one down.
fn stop_agent_session_by_pid(app: &AppHandle, pid: u32) {
    let state = app.state::<AppState>();
    let session = {
        let mut guard = lock_agent(&state);
        match guard.as_ref() {
            Some(session) if session.child.id() == pid => guard.take(),
            _ => None,
        }
    }; // guard dropped before the blocking kill/wait below
    if let Some(session) = session {
        stop_agent_session(session);
    }
}

/// Emit a `security_alarm` for `pid`'s session and stop it (A1 backstop, A2).
///
/// `kind` is a stable machine-readable tag the panel switches on; `detail` is
/// the human sentence naming the offending path or tool. The alarm is emitted
/// *before* the teardown so it is on the wire before `agent://closed`.
fn raise_agent_alarm(app: &AppHandle, pid: u32, kind: &str, detail: String) {
    let _ = app.emit(
        "agent://event",
        serde_json::json!({
            "type": "security_alarm",
            "kind": kind,
            "detail": detail,
            "pid": pid,
        }),
    );
    stop_agent_session_by_pid(app, pid);
}

/// The security backstop applied to one parsed stdout event (A1 layer 2, A2).
///
/// Returns `Some((kind, detail))` when the session must be stopped. Split out
/// as a pure function so the policy is unit-testable without a Tauri app or a
/// live CLI — the thread that calls it only knows how to emit and tear down.
fn agent_event_alarm(
    event: &agent::AgentEvent,
    sketch_dir: &str,
) -> Option<(&'static str, String)> {
    match event {
        // A2: the session must have exactly the tools this argv asked for.
        agent::AgentEvent::System(system) if system.subtype == "init" => {
            let extra = agent::unexpected_tools(&system.tools);
            if extra.is_empty() {
                return None;
            }
            Some((
                "unexpected_tools",
                format!(
                    "This session was given tools Bancada did not ask for: {}. \
                     Bancada's safety model only covers {}, so the session was stopped.",
                    extra.join(", "),
                    agent::EXPECTED_TOOLS.join(", ")
                ),
            ))
        }
        // A1 layer 2: an edit outside the project should already have been
        // *refused* by the PreToolUse guard, so seeing one attempted here
        // means either the guard did not run or the CLI stopped honouring
        // it. Either way the boundary is not holding and the session ends.
        agent::AgentEvent::Assistant(assistant) => {
            for block in &assistant.message.content {
                let agent::ContentBlock::ToolUse { name, input, .. } = block else {
                    continue;
                };
                if !matches!(
                    name.as_str(),
                    "Write" | "Edit" | "MultiEdit" | "NotebookEdit"
                ) {
                    continue;
                }
                let file_path = input.get("file_path").and_then(|v| v.as_str());
                let escapes = match file_path {
                    Some(path) => !agent::path_is_confined(Path::new(sketch_dir), path),
                    // A guarded tool with no readable file_path: the guard
                    // hook denies it, and so does this.
                    None => true,
                };
                if escapes {
                    return Some((
                        "path_escape",
                        format!(
                            "The assistant tried to {name} {} — outside the project \
                             directory {sketch_dir}. The edit was refused and the \
                             session was stopped.",
                            file_path.unwrap_or("a file it did not name")
                        ),
                    ));
                }
            }
            None
        }
        _ => None,
    }
}

/// Mirrors `AgentProbe` in `src/agent/types.ts`, where `version` and `error`
/// are genuinely optional — hence `skip_serializing_if` rather than letting
/// serde emit `null` for a field the frontend types as absent.
#[derive(serde::Serialize)]
struct AgentProbe {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Is the `claude` CLI available? Resolved from PATH, like arduino-cli and
/// esptool; a settings override can follow later if users need one.
#[tauri::command]
async fn agent_probe() -> Result<AgentProbe, String> {
    tauri::async_runtime::spawn_blocking(|| {
        match std::process::Command::new("claude")
            .arg("--version")
            .output()
        {
            Ok(out) if out.status.success() => AgentProbe {
                ok: true,
                version: Some(String::from_utf8_lossy(&out.stdout).trim().to_string()),
                error: None,
            },
            Ok(out) => AgentProbe {
                ok: false,
                version: None,
                error: Some(format!(
                    "`claude --version` failed ({}): {}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).trim()
                )),
            },
            Err(e) => AgentProbe {
                ok: false,
                version: None,
                error: Some(claude_spawn_error(e)),
            },
        }
    })
    .await
    .map_err(err_str)
}

fn claude_spawn_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "Claude Code is not installed (no `claude` on PATH) — install it and \
         log in with `claude` once, then try again"
            .to_string()
    } else {
        format!("could not start Claude Code: {e}")
    }
}

/// Start an embedded agent session for `sketch_dir`.
///
/// Order matters: bind the MCP listener first (its real port goes into the
/// child's `--mcp-config`), write the two 0600 temp files, spawn the
/// `claude` child, and only *then* start the four threads — the listener now
/// needs the child's pid to stamp its synthetic events with (C1), which does
/// not exist until the spawn returns. Binding before spawning is what makes
/// that safe: the socket is already listening, so the child's first MCP
/// connection queues in the kernel backlog until the listener thread accepts
/// it a few microseconds later.
///
/// Returns the child pid so the frontend can tag the session it started and
/// ignore events from a superseded one (FE-C1). Nothing here blocks, so this
/// stays a sync command like `start_monitor`.
#[tauri::command]
fn agent_start(
    app: AppHandle,
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: Option<String>,
    fqbn: Option<String>,
) -> Result<u32, String> {
    let mut guard = lock_agent(&state);
    if let Some(existing) = guard.as_ref() {
        return Err(format!(
            "an agent session is already running for {}",
            existing.sketch_dir
        ));
    }

    // Before anything is bound or spawned: if the confinement hook is
    // already switched off by a settings file we do not control, there is no
    // safe session to start (A1 pre-flight).
    check_hooks_are_enabled(&sketch_dir)?;

    let (server, port) = bind_mcp_server()?;
    let token = match random_token() {
        Ok(token) => token,
        Err(e) => {
            server.unblock();
            return Err(e);
        }
    };

    // F5: the bearer token goes into a 0600 temp file, not argv (see
    // write_mcp_config_file's doc comment) — write it before spawning so its
    // path can go straight into the child's --mcp-config.
    let mcp_config_path = match write_mcp_config_file(port, &token) {
        Ok(path) => path,
        Err(e) => {
            server.unblock();
            return Err(e);
        }
    };
    // A1: the PreToolUse confinement hook. A session that cannot be given
    // its boundary must not start — this is the release-blocking guarantee,
    // so failing to write the file is a hard error, not a downgrade.
    let settings_path = match write_agent_settings_file(&sketch_dir) {
        Ok(path) => path,
        Err(e) => {
            server.unblock();
            let _ = std::fs::remove_file(&mcp_config_path);
            return Err(e);
        }
    };

    let cfg = AgentCfg {
        mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
        settings_path: settings_path.to_string_lossy().into_owned(),
        system_prompt_extra: system_prompt_extra(&sketch_dir, profile.as_deref(), fqbn.as_deref()),
    };
    let cleanup = |server: &Arc<tiny_http::Server>| {
        server.unblock(); // never leave the listener thread parked
        let _ = std::fs::remove_file(&mcp_config_path);
        let _ = std::fs::remove_file(&settings_path);
    };
    let mut child = match std::process::Command::new("claude")
        .args(agent::agent_args(&cfg))
        .current_dir(&sketch_dir)
        .env("MCP_TOOL_TIMEOUT", MCP_TIMEOUT_MS)
        .env("MCP_TIMEOUT", MCP_TIMEOUT_MS)
        // Auto-memory is written with ordinary Write/Edit and loaded into
        // the system prompt of every *later* session of this project. It is
        // not code execution, so the confinement hook has no reason to refuse
        // it — but it is prompt injection that outlives the session, which a
        // write boundary alone does not address. The embedded agent has no
        // use for it either way.
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            cleanup(&server);
            return Err(claude_spawn_error(e));
        }
    };
    // Captured before stdin/stdout/stderr are taken: every thread below
    // stamps its events with it, and the frontend uses it to tell this
    // session's events from a superseded one's (F4, C1, FE-C1).
    let child_pid = child.id();

    // All three were just piped, so this cannot realistically fail — but if
    // it ever did, bailing with `?` would strand a live child and a parked
    // listener thread with no handle left to stop either.
    let (mut child_stdin, stdout, stderr) =
        match (child.stdin.take(), child.stdout.take(), child.stderr.take()) {
            (Some(stdin), Some(stdout), Some(stderr)) => (stdin, stdout, stderr),
            _ => {
                cleanup(&server);
                kill_child(child);
                return Err("the agent's stdio pipes were unavailable".to_string());
            }
        };

    // ---------- thread 1 of 4: MCP listener (owned clones, never locks agent)
    let verify_cancel = Arc::new(AtomicBool::new(false));
    let ctx = McpVerifyCtx {
        token,
        cli: state.cli.clone(),
        sketch_dir: sketch_dir.clone(),
        profile,
        fqbn,
        build_gate: state.build_gate.clone(),
        session_pid: child_pid,
        cancelled: verify_cancel.clone(),
    };
    let listener_server = server.clone();
    let listener_app = app.clone();
    std::thread::spawn(move || {
        mcp_listener_loop(listener_server, ctx, move |event, payload| {
            let _ = listener_app.emit(event, payload);
        });
    });

    // ---------- thread 2 of 4: stdin writer
    // The mutex is never held across a pipe write; commands hand lines here.
    let (stdin_tx, stdin_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in stdin_rx {
            if writeln!(child_stdin, "{line}").is_err() || child_stdin.flush().is_err() {
                break; // child gone; agent_stop reaps it
            }
        }
    });

    // ---------- thread 3 of 4: stdout reader (stream-json)
    let app_out = app.clone();
    let guard_dir = sketch_dir.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            // `parse_event` is the validity gate, but what reaches the
            // frontend is the CLI's own event object verbatim: the panel's
            // contract is the wire shape, not a re-modelled subset that
            // would silently drop fields core doesn't happen to name.
            match agent::parse_event(&line) {
                Ok(event) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                        let _ = app_out.emit("agent://event", value);
                    }
                    // Security backstop (A1 layer 2, A2), *after* the event
                    // itself is emitted so the panel can show what triggered
                    // the alarm right above it.
                    if let Some((kind, detail)) = agent_event_alarm(&event, &guard_dir) {
                        raise_agent_alarm(&app_out, child_pid, kind, detail);
                        break; // the child is gone; stop reading its pipe
                    }
                }
                // The only `Err` case is a line that isn't JSON at all.
                Err(_) => {
                    let _ = app_out.emit(
                        "agent://event",
                        serde_json::json!({ "type": "unparsed", "line": line }),
                    );
                }
            }
        }
        let _ = app_out.emit(
            "agent://closed",
            serde_json::json!({ "reason": "the agent process ended", "pid": child_pid }),
        );
        // D1: reap our own session here rather than relying on the frontend
        // round-tripping `agent_stop(pid)` back — that call was
        // fire-and-forget (`.catch(() => {})`), so a closed window, a
        // navigation, or a rejected invoke used to leave the listener
        // thread, the MCP server and both 0600 temp files behind for the
        // lifetime of the app. Pid-scoped, so a session that has already
        // been superseded takes nothing with it.
        stop_agent_session_by_pid(&app_out, child_pid);
    });

    // ---------- thread 4 of 4: stderr drain
    // Not optional: an undrained pipe fills and wedges the child, the same
    // reason `start_monitor` runs two reader threads.
    let app_err = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            let _ = app_err.emit(
                "agent://event",
                serde_json::json!({ "type": "stderr", "line": line }),
            );
        }
    });

    *guard = Some(AgentSession {
        child,
        stdin_tx,
        mcp_server: server,
        sketch_dir,
        mcp_config_path,
        settings_path,
        verify_cancel,
    });
    Ok(child_pid)
}

/// Queue a user message for the child's stdin.
#[tauri::command]
fn agent_send(state: State<'_, AppState>, text: String) -> Result<(), String> {
    // Clone the sender out and drop the guard *before* sending: no write to
    // the child ever happens while the agent mutex is held.
    let tx = {
        let guard = lock_agent(&state);
        let Some(session) = guard.as_ref() else {
            return Err("the agent is not running".to_string());
        };
        session.stdin_tx.clone()
    };
    tx.send(agent::user_message_json(&text))
        .map_err(|_| "the agent's stdin writer has stopped".to_string())
}

/// Interrupt the current turn.
///
/// Sends the CLI's undocumented `control_request` interrupt line
/// best-effort, then kills the child after a short grace period. The kill is
/// **unconditional** — the trade-off is deliberate: the control protocol is
/// unverified, so the only reliable stop is the one the OS guarantees, and
/// the cost is that interrupting ends the session (the stdout reader emits
/// `agent://closed`, and the next message needs a fresh `agent_start`)
/// rather than merely ending the turn.
#[tauri::command]
fn agent_interrupt(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let (tx, pid) = {
        let guard = lock_agent(&state);
        let Some(session) = guard.as_ref() else {
            return Err("the agent is not running".to_string());
        };
        (session.stdin_tx.clone(), session.child.id())
    };
    let _ = tx.send(agent::interrupt_json(&format!("int-{}", now_millis())));

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        // Only kill the session this interrupt was aimed at: two seconds is
        // long enough for the user to have stopped it and started a new one,
        // which must not be collateral damage.
        stop_agent_session_by_pid(&app, pid);
    });
    Ok(())
}

/// Whether an `agent_stop` call for `requested_pid` should tear down
/// `live_pid` (the pid of whatever session is actually running, if any).
///
/// `None` means "no pid given" — the user's explicit "stop"/"new session"
/// action — and always proceeds, same as before this existed. `Some` means
/// the call is *about* a specific child: `agent://closed`'s stdout-EOF
/// notification carries the pid of the session that just exited, and the
/// session that's live by the time the frontend's handler runs might already
/// be a *different, newer* one (session A's `agent://closed` racing behind
/// `agent_start` already having stored session B) — reaping session A must
/// never kill session B.
fn should_stop_agent(requested_pid: Option<u32>, live_pid: Option<u32>) -> bool {
    match requested_pid {
        None => true,
        Some(pid) => live_pid == Some(pid),
    }
}

/// Stop the session and reap the child. Idempotent: the frontend calls this
/// on `agent://closed` to reap an already-exited child, so "not running" is
/// success, not an error.
///
/// `pid`, when given, must match the *live* session's child pid or this is a
/// no-op (`should_stop_agent` — guards the race above).
#[tauri::command]
fn agent_stop(state: State<'_, AppState>, pid: Option<u32>) -> Result<(), String> {
    let session = {
        let mut guard = lock_agent(&state);
        let live_pid = guard.as_ref().map(|s| s.child.id());
        if !should_stop_agent(pid, live_pid) {
            return Ok(());
        }
        guard.take()
    };
    if let Some(session) = session {
        stop_agent_session(session);
    }
    Ok(())
}

// ---------- entry point ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before anything else: Bancada re-invokes *itself* as the embedded
    // agent's `PreToolUse` confinement hook (A1). In that role it is a plain
    // stdin/stdout filter, not a windowed app.
    if handle_agent_guard_argv() {
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(AppState {
                cli: ArduinoCli::default(),
                serial: Mutex::new(None),
                mqtt: Mutex::new(None),
                agent: Mutex::new(None),
                build_gate: Arc::new(Mutex::new(())),
            });
            // Hotplug watcher: enumeration (does the port exist?) is orders of
            // magnitude cheaper than identification (arduino-cli), so poll the
            // former and let the frontend run the latter only on a change. The
            // first tick only seeds `prev` — the frontend does its own initial
            // scan, and boards present at launch are not arrivals.
            let watcher = app.handle().clone();
            std::thread::spawn(move || {
                let mut prev: Option<std::collections::BTreeSet<String>> = None;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    // A failed enumeration keeps the previous set: never emit
                    // on error, never die.
                    let Ok(ports) = serialport::available_ports() else {
                        continue;
                    };
                    let names: std::collections::BTreeSet<String> =
                        ports.into_iter().map(|p| p.port_name).collect();
                    if prev.as_ref().is_some_and(|p| *p != names) {
                        let _ = watcher.emit("ports://changed", ());
                    }
                    prev = Some(names);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cli_version,
            list_boards,
            list_sketch_files,
            sketch_has_git,
            read_sketch_file,
            write_sketch_file,
            load_sketch_yaml,
            init_profile,
            add_local_library,
            add_registry_library_to_profile,
            search_libraries,
            list_installed_libraries,
            install_library,
            uninstall_library,
            sketchbook_libraries_dir,
            create_library,
            list_cores,
            search_cores,
            install_core,
            uninstall_core,
            update_core_index,
            add_platform_to_profile,
            sketchbook_dir,
            default_project_parent,
            list_all_boards,
            create_project,
            gh_list_versions,
            gh_manifest,
            gh_add_library,
            gh_restore,
            compile_sketch,
            upload_sketch,
            start_monitor,
            stop_monitor,
            monitor_send,
            scope_probe,
            scope_start,
            scope_single,
            scope_send,
            scope_stop,
            scope_install_firmware,
            save_text_file,
            save_binary_file,
            load_settings,
            save_settings,
            read_board_mac,
            fleet_sync,
            set_board_nickname,
            note_board_fqbn,
            identify_board,
            forget_board,
            mqtt_connect,
            mqtt_publish,
            mqtt_subscribe,
            mqtt_unsubscribe,
            mqtt_disconnect,
            load_mqtt_config,
            save_mqtt_config,
            agent_probe,
            agent_start,
            agent_send,
            agent_interrupt,
            agent_stop
        ])
        .build(tauri::generate_context!())
        .expect("error while building Bancada")
        .run(|app, event| {
            // Whoever owns the serial port holds it exclusively; release it on
            // exit so the port is immediately free for other tools.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                evict_owner(&mut state.serial.lock().unwrap());
                // ---------- mqtt ---------- send the broker a clean DISCONNECT.
                let mqtt_session = state.mqtt.lock().unwrap().take();
                if let Some(session) = mqtt_session {
                    stop_mqtt_session(session);
                }
                // ---------- agent ---------- same teardown as `agent_stop`:
                // kill and reap the child rather than orphan it.
                let agent_session = lock_agent(&state).take();
                if let Some(session) = agent_session {
                    stop_agent_session(session);
                }
            }
        });
}

// ---------- tests ----------

/// The agent panel's host-side plumbing, exercised over real loopback HTTP
/// against a stub `arduino-cli` (the `with_stub` script trick from
/// `core/src/cli.rs`'s tests). None of this needs Tauri: `mcp_listener_loop`
/// takes its emitter as a closure precisely so a test can collect events
/// where production calls `AppHandle::emit`.
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;
    use std::sync::mpsc;

    // ---------- harness ----------

    type Events = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

    /// A listener thread plus the handles a test needs to talk to it. Dropping
    /// it unblocks and joins the thread, so no test leaks a listener.
    struct TestListener {
        port: u16,
        token: String,
        server: Arc<tiny_http::Server>,
        events: Events,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl TestListener {
        fn auth(&self) -> String {
            format!("Bearer {}", self.token)
        }

        fn event_types(&self) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|(name, _)| name == "agent://event")
                .filter_map(|(_, v)| v.get("type").and_then(|t| t.as_str()).map(String::from))
                .collect()
        }

        fn build_lines(&self) -> Vec<serde_json::Value> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|(name, _)| name == "build://line")
                .map(|(_, v)| v.clone())
                .collect()
        }
    }

    impl Drop for TestListener {
        fn drop(&mut self) {
            self.server.unblock();
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    /// The pid every test listener stamps its synthetic events with, unless
    /// the test cares about a specific one.
    const TEST_PID: u32 = 4242;

    fn start_listener(cli: ArduinoCli, sketch_dir: &str, gate: Arc<Mutex<()>>) -> TestListener {
        start_listener_cancellable(cli, sketch_dir, gate, Arc::new(AtomicBool::new(false)))
    }

    fn start_listener_cancellable(
        cli: ArduinoCli,
        sketch_dir: &str,
        gate: Arc<Mutex<()>>,
        cancelled: Arc<AtomicBool>,
    ) -> TestListener {
        let (server, port) = bind_mcp_server().expect("bind loopback");
        let token = random_token().expect("entropy");
        let events: Events = Arc::new(Mutex::new(Vec::new()));

        let ctx = McpVerifyCtx {
            token: token.clone(),
            cli,
            sketch_dir: sketch_dir.to_string(),
            profile: None,
            fqbn: None,
            build_gate: gate,
            session_pid: TEST_PID,
            cancelled,
        };
        let thread_server = server.clone();
        let thread_events = events.clone();
        let join = std::thread::spawn(move || {
            mcp_listener_loop(thread_server, ctx, move |name, payload| {
                thread_events
                    .lock()
                    .unwrap()
                    .push((name.to_string(), payload));
            });
        });

        TestListener {
            port,
            token,
            server,
            events,
            join: Some(join),
        }
    }

    /// One HTTP round trip, hand-rolled: the workspace has no HTTP client and
    /// this needs to send exactly what it says it sends (including a missing
    /// Authorization header, and a body far past the size cap).
    fn http(
        port: u16,
        method: &str,
        auth: Option<&str>,
        body: Option<&str>,
    ) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .unwrap();

        let mut head =
            format!("{method} /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
        if let Some(auth) = auth {
            head.push_str(&format!("Authorization: {auth}\r\n"));
        }
        if let Some(body) = body {
            head.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        head.push_str("\r\n");

        stream.write_all(head.as_bytes()).expect("write head");
        if let Some(body) = body {
            stream.write_all(body.as_bytes()).expect("write body");
        }
        stream.flush().unwrap();

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (headers, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
        let status: u16 = headers
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        (status, headers.to_lowercase(), body.to_string())
    }

    fn post(listener: &TestListener, body: &str) -> (u16, String, String) {
        http(listener.port, "POST", Some(&listener.auth()), Some(body))
    }

    /// A fake `arduino-cli` whose body is `sh` source, in its own tempdir.
    #[cfg(unix)]
    fn stub_cli(dir: &tempfile::TempDir, body: &str) -> ArduinoCli {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.path().join("fake-arduino-cli");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        ArduinoCli::new(script.to_string_lossy().into_owned())
    }

    fn gate() -> Arc<Mutex<()>> {
        Arc::new(Mutex::new(()))
    }

    // ---------- JSON-RPC over HTTP ----------

    /// Task 5 Step-0 finding (a): the CLI advertises `Accept:
    /// application/json, text/event-stream` but is perfectly happy with a
    /// plain JSON body — no SSE framing needed.
    #[cfg(unix)]
    #[test]
    fn initialize_answers_200_with_plain_application_json() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());

        let (status, headers, body) = post(
            &l,
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
        );
        assert_eq!(status, 200);
        assert!(
            headers.contains("content-type: application/json"),
            "{headers}"
        );
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["result"]["serverInfo"]["name"], "bancada");
        assert_eq!(value["id"], 0);
    }

    /// Spec-required: a notification has no id to correlate a reply with, so
    /// it must get an empty body, never JSON.
    #[cfg(unix)]
    #[test]
    fn a_notification_answers_202_with_an_empty_body() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());

        let (status, _, body) = post(
            &l,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert_eq!(status, 202);
        assert!(body.is_empty(), "expected an empty body, got {body:?}");
    }

    #[cfg(unix)]
    #[test]
    fn tools_list_advertises_the_verify_tool() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());

        let (status, _, body) = post(&l, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        assert_eq!(status, 200);
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["result"]["tools"][0]["name"], "verify");
    }

    // ---------- auth ----------

    #[cfg(unix)]
    #[test]
    fn a_missing_bearer_token_is_401() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());

        let (status, _, _) = http(
            l.port,
            "POST",
            None,
            Some(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#),
        );
        assert_eq!(status, 401);
    }

    #[cfg(unix)]
    #[test]
    fn a_wrong_bearer_token_is_401() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());

        let (status, _, _) = http(
            l.port,
            "POST",
            Some("Bearer not-the-token"),
            Some(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#),
        );
        assert_eq!(status, 401);
    }

    // ---------- request-shape guards ----------

    /// Regression for a Task 5 Step-0 observation: the CLI opens a
    /// `GET /mcp` server->client SSE stream. Answering it with a JSON-RPC
    /// parse error (what happens if GETs are routed through
    /// `handle_request`) made the client reconnect in a tight busy-loop —
    /// hundreds of requests per turn. 405 is the correct answer for a
    /// server that offers no such stream.
    #[cfg(unix)]
    #[test]
    fn a_get_is_405_and_never_a_jsonrpc_body() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());

        let (status, _, body) = http(l.port, "GET", Some(&l.auth()), None);
        assert_eq!(status, 405);
        assert!(!body.contains("jsonrpc"), "{body}");
    }

    /// Any local process can POST here, so the body is capped before it is
    /// parsed — `read_to_string` on an unbounded reader would let one of
    /// them push the whole app into swap.
    #[cfg(unix)]
    #[test]
    fn an_oversized_body_is_rejected_before_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());

        let huge = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{"pad":"{}"}}}}"#,
            "x".repeat(MCP_MAX_BODY + 1024)
        );
        let (status, _, _) = post(&l, &huge);
        assert_eq!(status, 413);
    }

    // ---------- the verify tool ----------

    const CALL_VERIFY: &str = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"verify","arguments":{}}}"#;

    fn tool_text(body: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        value["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    fn tool_is_error(body: &str) -> bool {
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        value["result"]["isError"].as_bool().unwrap_or(true)
    }

    /// The point of the whole tool: compiler errors reach the agent. The
    /// stub writes to stderr, which `summarize_build_output` never drops.
    #[cfg(unix)]
    #[test]
    fn verify_runs_the_compile_and_keeps_every_stderr_line() {
        let dir = tempfile::tempdir().unwrap();
        let cli = stub_cli(
            &dir,
            "echo 'Compiling sketch...'\n\
             echo \"Blink.ino:3:1: error: expected ';' before '}' token\" >&2\n\
             exit 1",
        );
        let l = start_listener(cli, "/nowhere", gate());

        let (status, _, body) = post(&l, CALL_VERIFY);
        assert_eq!(status, 200);

        let text = tool_text(&body);
        assert!(text.contains("success: false"), "{text}");
        assert!(text.contains("exit_code: 1"), "{text}");
        assert!(text.contains("expected ';'"), "{text}");
        assert!(text.contains("Compiling sketch..."), "{text}");
        // A failed build is a valid tool *result*, not a tool failure —
        // flagging it would invite the model to give up on the tool.
        assert!(!tool_is_error(&body));

        assert_eq!(
            l.event_types(),
            vec!["verify_started".to_string(), "verify_done".to_string()]
        );
        let lines = l.build_lines();
        assert!(
            lines
                .iter()
                .any(|v| v["stream"] == "stderr" && v["line"].as_str().unwrap().contains("error:")),
            "the console stream must carry the stderr line too: {lines:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn verify_reports_a_successful_build() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(
            stub_cli(&dir, "echo 'Sketch uses 1234 bytes'\nexit 0"),
            "/nowhere",
            gate(),
        );

        let (_, _, body) = post(&l, CALL_VERIFY);
        let text = tool_text(&body);
        assert!(text.contains("success: true"), "{text}");
        assert!(text.contains("exit_code: 0"), "{text}");
        assert!(text.contains("Sketch uses 1234 bytes"), "{text}");
        assert!(!tool_is_error(&body));

        let done = l
            .events
            .lock()
            .unwrap()
            .iter()
            .find(|(_, v)| v["type"] == "verify_done")
            .map(|(_, v)| v.clone())
            .expect("a verify_done event");
        assert_eq!(done["success"], true);
    }

    /// Risk R5: the agent must not race a user Verify/Upload through the
    /// same build cache. Contention fails fast instead of queueing.
    #[cfg(unix)]
    #[test]
    fn verify_is_refused_while_the_build_gate_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let gate = gate();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate.clone());

        let held = gate.lock().unwrap();
        let (status, _, body) = post(&l, CALL_VERIFY);
        drop(held);

        assert_eq!(status, 200);
        assert!(tool_is_error(&body), "{body}");
        assert!(tool_text(&body).contains(BUILD_BUSY), "{body}");
        // The compile never started, so neither did its events.
        assert!(l.event_types().is_empty(), "{:?}", l.event_types());
    }

    /// A missing arduino-cli is the tool genuinely failing to run — that is
    /// what `isError` is for.
    #[test]
    fn verify_reports_a_tool_error_when_arduino_cli_is_missing() {
        let l = start_listener(
            ArduinoCli::new("bancada-definitely-no-such-binary"),
            "/nowhere",
            gate(),
        );

        let (_, _, body) = post(&l, CALL_VERIFY);
        assert!(tool_is_error(&body), "{body}");
        assert!(tool_text(&body).contains("verify could not run"), "{body}");
        assert_eq!(
            l.event_types(),
            vec!["verify_started".to_string(), "verify_done".to_string()]
        );
    }

    // ---------- C1: a stopped session's verify ----------

    /// The core of C1: `Server::unblock()` cannot reach a listener thread
    /// that is *inside* `run_verify`, so a stopped session used to go on
    /// holding the build gate for a whole compile and then emit `verify_done`
    /// into the next session's UI. A cancelled context must not emit at all.
    #[cfg(unix)]
    #[test]
    fn a_cancelled_context_never_emits_and_never_takes_the_build_gate() {
        let dir = tempfile::tempdir().unwrap();
        let gate = gate();
        let cancelled = Arc::new(AtomicBool::new(true)); // stopped before the call
        let l = start_listener_cancellable(
            stub_cli(&dir, "echo 'should never run'\nexit 0"),
            "/nowhere",
            gate.clone(),
            cancelled,
        );

        let (status, _, body) = post(&l, CALL_VERIFY);
        assert_eq!(status, 200);
        assert!(tool_is_error(&body), "{body}");
        assert!(tool_text(&body).contains(VERIFY_CANCELLED), "{body}");
        assert!(
            l.event_types().is_empty(),
            "a cancelled session must emit nothing: {:?}",
            l.event_types()
        );
        assert!(
            l.build_lines().is_empty(),
            "nor may its build output scroll past in the Console"
        );
        // The gate is the app-wide one: a cancelled verify holding it would
        // block the user's own Verify button for the length of a compile.
        assert!(
            gate.try_lock().is_ok(),
            "a cancelled verify must not be holding the build gate"
        );
    }

    /// The other half of C1: an event that *does* get emitted carries the
    /// session pid, so the frontend can discard one that outlived its
    /// session even if it slips past the flag check.
    #[cfg(unix)]
    #[test]
    fn verify_events_are_stamped_with_the_session_pid() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());
        let (_, _, _) = post(&l, CALL_VERIFY);

        let events = l.events.lock().unwrap();
        let synthetic: Vec<&serde_json::Value> = events
            .iter()
            .filter(|(name, _)| name == "agent://event")
            .map(|(_, v)| v)
            .collect();
        assert!(!synthetic.is_empty());
        for event in synthetic {
            assert_eq!(
                event["pid"], TEST_PID,
                "every synthetic agent event must name its session: {event}"
            );
        }
    }

    /// Cancellation arriving mid-compile: the compile itself cannot be
    /// aborted (documented residual in `run_verify`), but nothing from it
    /// may reach the UI once the session is gone.
    #[cfg(unix)]
    #[test]
    fn a_verify_cancelled_during_the_compile_emits_no_done_event() {
        let dir = tempfile::tempdir().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        // The stub sleeps long enough for this test to cancel mid-build.
        let l = start_listener_cancellable(
            stub_cli(&dir, "sleep 1\necho done\nexit 0"),
            "/nowhere",
            gate(),
            cancelled.clone(),
        );

        let port = l.port;
        let auth = l.auth();
        let caller = std::thread::spawn(move || http(port, "POST", Some(&auth), Some(CALL_VERIFY)));
        // Let verify_started land, then stop the session mid-compile.
        std::thread::sleep(Duration::from_millis(300));
        cancelled.store(true, Ordering::SeqCst);
        let (status, _, _) = caller.join().unwrap();
        assert_eq!(status, 200);

        let types = l.event_types();
        assert!(
            types.contains(&"verify_started".to_string()),
            "the build had already started: {types:?}"
        );
        assert!(
            !types.contains(&"verify_done".to_string()),
            "a stopped session's verify_done must not reach the next session's \
             panel: {types:?}"
        );
    }

    // ---------- build gate ----------

    #[test]
    fn the_build_gate_reports_busy_when_it_is_already_held() {
        let gate = Mutex::new(());
        let held = gate.lock().unwrap();
        assert_eq!(try_build_gate(&gate).unwrap_err(), BUILD_BUSY);
        drop(held);
        assert!(try_build_gate(&gate).is_ok());
    }

    #[test]
    fn a_poisoned_build_gate_still_lets_the_next_build_through() {
        // A build that panicked must not wedge every later build: the gate
        // guards `()`, so there is no corrupt state to protect.
        let gate = Arc::new(Mutex::new(()));
        let poisoner = gate.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("a build panicked");
        })
        .join();
        assert!(gate.is_poisoned());
        assert!(try_build_gate(&gate).is_ok());
    }

    // ---------- misc ----------

    #[test]
    fn the_system_prompt_names_the_project_the_profile_and_the_verify_tool() {
        let prompt = system_prompt_extra("/home/me/Blink", Some("esp32s3"), Some("esp32:esp32:x"));
        assert!(prompt.contains("/home/me/Blink"), "{prompt}");
        assert!(prompt.contains("esp32s3"), "{prompt}");
        assert!(prompt.contains("esp32:esp32:x"), "{prompt}");
        assert!(prompt.contains("mcp__bancada__verify"), "{prompt}");
        // No profile/fqbn must not produce dangling text.
        let bare = system_prompt_extra("/home/me/Blink", None, None);
        assert!(!bare.contains("profile is"), "{bare}");
        assert!(!bare.contains("FQBN"), "{bare}");
    }

    // F4: a stale `agent://closed` for a superseded session must not kill a
    // newer one that `agent_start` has since stored.
    #[test]
    fn should_stop_agent_with_no_pid_always_proceeds() {
        assert!(should_stop_agent(None, None));
        assert!(should_stop_agent(None, Some(42)));
    }

    #[test]
    fn should_stop_agent_matches_the_live_pid() {
        assert!(should_stop_agent(Some(42), Some(42)));
    }

    #[test]
    fn should_stop_agent_refuses_a_pid_mismatch_or_no_live_session() {
        // A newer session (a different live pid) must survive a stale close.
        assert!(!should_stop_agent(Some(1), Some(2)));
        // Nothing live at all — also not this call's session to reap.
        assert!(!should_stop_agent(Some(1), None));
    }

    #[test]
    fn tokens_are_hex_and_do_not_repeat() {
        let a = random_token().expect("entropy");
        let b = random_token().expect("entropy");
        assert_ne!(a, b);
        assert!(a.len() >= 32, "{a}");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
    }

    // F5: the bearer token moved from argv into this file — pin its
    // content/permissions/lifecycle directly.
    #[test]
    fn mcp_config_file_has_the_right_url_and_bearer_token() {
        let path = write_mcp_config_file(54321, "sekrit").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        let server = &parsed["mcpServers"]["bancada"];
        assert_eq!(server["type"], "http");
        assert_eq!(server["url"], "http://127.0.0.1:54321/mcp");
        assert_eq!(server["headers"]["Authorization"], "Bearer sekrit");
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn mcp_config_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let path = write_mcp_config_file(1, "t").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        // Mask down to the permission bits; the type bits vary by platform.
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
        let _ = std::fs::remove_file(&path);
    }

    /// Pins the property `mcp_config_file_is_0600` alone can't: this file is
    /// never briefly world-readable. A single post-hoc `stat()` (as above)
    /// can't observe a window that's already closed by the time the test
    /// runs it, so this instead pins the *mechanism* the atomicity actually
    /// comes from — `create_new` is `O_EXCL`, so it must refuse a path that
    /// already exists rather than open (and so implicitly widen, the way
    /// the old write-then-chmod's plain `std::fs::write` would have) it.
    /// Regressing back to write-then-chmod would fail this deterministically
    /// (`fs::write` truncates and overwrites a pre-existing path instead of
    /// refusing), where a permissions-only assertion would not.
    #[cfg(unix)]
    #[test]
    fn mcp_config_file_creation_is_exclusive_and_never_touches_a_pre_existing_path() {
        use std::os::unix::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!(
            "bancada-agent-mcp-preexisting-{}.json",
            random_token().expect("entropy")
        ));
        std::fs::write(&path, "not agent config").unwrap();

        let result = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path);
        assert!(result.is_err(), "create_new must refuse an existing path");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "not agent config",
            "a pre-existing file at the target path must be left untouched"
        );
        let _ = std::fs::remove_file(&path);
    }

    // ---------- A1: the --settings confinement hook ----------

    /// The `permissions.deny` half of the policy — the layer that survives
    /// `disableAllHooks` and is therefore what actually keeps
    /// `.claude/settings.json` out of reach.
    #[test]
    fn the_settings_file_carries_the_deny_rule_anchor() {
        let sketch = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(sketch.path()).unwrap();
        let path = write_agent_settings_file(&sketch.path().to_string_lossy()).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);

        let deny: Vec<String> = parsed["permissions"]["deny"]
            .as_array()
            .expect("a deny array")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        // `//` is itself the filesystem-root anchor, so the canonical path
        // contributes its components without a second leading slash.
        let anchored = canonical
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string();
        assert!(
            deny.contains(&format!("Edit(//{anchored}/.claude/**)")),
            "the project's own .claude must be denied by *rule*, not only by \
             the hook — a hook cannot protect the file that can disable it: \
             {deny:?}"
        );
        assert!(
            deny.iter().any(|r| r.contains(".git/**")),
            ".git/hooks/* is code the next git operation runs: {deny:?}"
        );
        // Verified against 2.1.220: Write(...) patterns are accepted and
        // never evaluated, so one here would be an invisible no-op.
        assert!(
            !deny.iter().any(|r| r.starts_with("Write(")),
            "Write(...) rules are silently never consulted: {deny:?}"
        );
    }

    /// The pre-flight is what turns the one live failure this design has —
    /// a project that arrives with hooks already disabled — from a silent
    /// bypass into a refusal the user can act on.
    #[test]
    fn a_project_that_disables_hooks_refuses_to_start() {
        let sketch = tempfile::tempdir().unwrap();
        assert!(
            check_hooks_are_enabled(&sketch.path().to_string_lossy()).is_ok(),
            "a clean project must start"
        );

        std::fs::create_dir_all(sketch.path().join(".claude")).unwrap();
        std::fs::write(
            sketch.path().join(".claude/settings.json"),
            r#"{"disableAllHooks": true}"#,
        )
        .unwrap();
        let err = check_hooks_are_enabled(&sketch.path().to_string_lossy())
            .expect_err("must refuse: the confinement hook would never fire");
        assert!(err.contains("disableAllHooks"), "{err}");
        assert!(
            err.contains(".claude/settings.json"),
            "the error must name the offending file: {err}"
        );
    }

    #[test]
    fn a_settings_local_json_disabling_hooks_also_refuses() {
        let sketch = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(sketch.path().join(".claude")).unwrap();
        std::fs::write(
            sketch.path().join(".claude/settings.local.json"),
            r#"{"disableAllHooks": true}"#,
        )
        .unwrap();
        assert!(check_hooks_are_enabled(&sketch.path().to_string_lossy()).is_err());
    }

    /// The policy file must not be reachable by the thing it constrains: if
    /// it lived in the sketch dir, an agent that got past the hook could
    /// rewrite its own rules (and the hook allows writes there by design).
    #[test]
    fn the_settings_file_lives_outside_the_project_tree() {
        let sketch = tempfile::tempdir().unwrap();
        let path = write_agent_settings_file(&sketch.path().to_string_lossy()).unwrap();
        let inside = agent::path_is_confined(sketch.path(), &path.to_string_lossy());
        let _ = std::fs::remove_file(&path);
        assert!(
            !inside,
            "the policy file must be somewhere the agent cannot write: {path:?}"
        );
    }

    /// The hook file is the boundary. If it stops naming this binary, or
    /// stops matching the write tools, the session silently loses its
    /// confinement — with nothing else in the app to notice.
    #[test]
    fn the_settings_file_registers_this_binary_as_the_pretooluse_guard() {
        let path = write_agent_settings_file("/home/me/Blink").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");

        let entries = parsed["hooks"]["PreToolUse"].as_array().expect("an array");
        assert_eq!(entries.len(), 1);
        let matcher = entries[0]["matcher"].as_str().unwrap();
        for tool in ["Write", "Edit", "MultiEdit", "NotebookEdit"] {
            assert!(matcher.contains(tool), "{matcher} must match {tool}");
        }
        let command = entries[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(entries[0]["hooks"][0]["type"], "command");
        assert!(
            command.contains("--agent-guard"),
            "the hook must re-invoke Bancada's own guard: {command}"
        );
        assert!(
            command.contains("/home/me/Blink"),
            "the guard must be told which directory to confine to: {command}"
        );
        let exe = std::env::current_exe().unwrap();
        assert!(
            command.contains(exe.to_str().unwrap()),
            "the hook command must be this very binary, not a generated \
             script: {command}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn the_settings_file_is_0600() {
        // It names a command the CLI will *execute*: a world-writable file at
        // a predictable path would be a way to make Bancada run someone
        // else's command.
        use std::os::unix::fs::PermissionsExt;
        let path = write_agent_settings_file("/nowhere").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
        let _ = std::fs::remove_file(&path);
    }

    /// The `command` string is shell-parsed by the CLI. A project called
    /// `My Sketch` must not become two arguments, and one called
    /// `'; rm -rf ~; '` must not become a second command. `sh` itself is the
    /// ground truth here rather than a hand-written expectation.
    #[cfg(unix)]
    #[test]
    fn a_sketch_dir_with_shell_metacharacters_stays_one_argument() {
        let nasty = "/tmp/My Sketch's; touch /tmp/bancada-quoting-pwned; dir";
        let round_trip = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf '%s' {}", shell_quote(nasty)))
            .output()
            .expect("run sh");
        assert_eq!(
            String::from_utf8_lossy(&round_trip.stdout),
            nasty,
            "shell_quote must round-trip a path containing quotes and \
             semicolons as exactly one word"
        );
        assert!(
            !Path::new("/tmp/bancada-quoting-pwned").exists(),
            "the embedded `touch` must never have run"
        );
        // ... and the real settings file must use that quoting.
        let path = write_agent_settings_file(nasty).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let command = parsed["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_string();
        let _ = std::fs::remove_file(&path);
        assert!(command.contains(r"'\''"), "unquoted sketch dir: {command}");
    }

    #[test]
    fn shell_quote_neutralises_quotes_and_separators() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("with space"), "'with space'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(
            shell_quote("a'; touch pwned; '"),
            r"'a'\''; touch pwned; '\'''"
        );
    }

    /// The `bancada` binary as built by this workspace, which is what the
    /// hook command actually names — *not* `current_exe()`, which under
    /// `cargo test` is the libtest harness and would never reach `run()`.
    /// `cargo test` builds the package's bin target, so this normally
    /// exists; the test skips rather than fails if a future layout moves it.
    fn bancada_binary() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        // target/debug/deps/bancada-<hash> -> target/debug/bancada
        let path = exe.parent()?.parent()?.join("bancada");
        path.exists().then_some(path)
    }

    /// The guard entry point, exercised the way the CLI drives it: JSON on
    /// stdin, a decision on stdout, always exit 0. Run as a real subprocess
    /// so this covers `handle_agent_guard_argv` and the process contract —
    /// including that the binary does *not* try to open a window — and not
    /// just `guard_decision`, which core already unit-tests.
    #[cfg(unix)]
    #[test]
    fn the_guard_subprocess_denies_an_out_of_project_write_and_exits_zero() {
        let Some(exe) = bancada_binary() else {
            eprintln!("skipped: no built `bancada` binary next to the test harness");
            return;
        };
        let sketch = tempfile::tempdir().unwrap();

        let deny = run_guard_subprocess(
            &exe,
            &sketch.path().to_string_lossy(),
            r#"{"tool_name":"Write","tool_input":{"file_path":"/etc/passwd"}}"#,
        );
        let parsed: serde_json::Value =
            serde_json::from_str(deny.trim()).expect("a JSON decision on stdout");
        assert_eq!(
            parsed["hookSpecificOutput"]["permissionDecision"], "deny",
            "{deny}"
        );

        let allow = run_guard_subprocess(
            &exe,
            &sketch.path().to_string_lossy(),
            r#"{"tool_name":"Write","tool_input":{"file_path":"Blink.ino"}}"#,
        );
        assert!(
            allow.trim().is_empty(),
            "an in-project edit must fall through silently, got {allow:?}"
        );
    }

    /// A hook that fails is "log and continue" to the CLI — i.e. fail *open*.
    /// So the guard must never exit non-zero, even on garbage input.
    #[cfg(unix)]
    #[test]
    fn the_guard_subprocess_denies_garbage_rather_than_failing() {
        let Some(exe) = bancada_binary() else {
            eprintln!("skipped: no built `bancada` binary next to the test harness");
            return;
        };
        let sketch = tempfile::tempdir().unwrap();
        let out = run_guard_subprocess(&exe, &sketch.path().to_string_lossy(), "not json");
        assert!(out.contains("\"deny\""), "{out}");
    }

    /// Run `<exe> --agent-guard <dir>`, feeding `stdin`, and return its
    /// stdout. Asserts the exit status is 0.
    #[cfg(unix)]
    fn run_guard_subprocess(exe: &Path, sketch_dir: &str, stdin_body: &str) -> String {
        let mut child = std::process::Command::new(exe)
            .arg("--agent-guard")
            .arg(sketch_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn the guard");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin_body.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("wait on the guard");
        assert!(
            out.status.success(),
            "the guard must always exit 0 — a failing hook is 'log and \
             continue' to the CLI, i.e. fail open. status: {:?}",
            out.status
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    // ---------- A1 layer 2 / A2: the stdout backstop ----------

    fn event(line: &str) -> agent::AgentEvent {
        agent::parse_event(line).unwrap()
    }

    #[test]
    fn the_backstop_stops_a_session_offered_tools_bancada_did_not_ask_for() {
        let line = r#"{"type":"system","subtype":"init","tools":["Read","Write","Bash","Skill"]}"#;
        let (kind, detail) = agent_event_alarm(&event(line), "/s").expect("an alarm");
        assert_eq!(kind, "unexpected_tools");
        assert!(detail.contains("Bash"), "{detail}");
        assert!(detail.contains("Skill"), "{detail}");
    }

    #[test]
    fn the_backstop_is_quiet_for_the_expected_tool_set() {
        let line = r#"{"type":"system","subtype":"init","tools":["Read","Edit","Write","Glob","Grep","mcp__bancada__verify"]}"#;
        assert!(agent_event_alarm(&event(line), "/s").is_none());
        // A non-init system line carries no tools and must not alarm.
        let status = r#"{"type":"system","subtype":"status"}"#;
        assert!(agent_event_alarm(&event(status), "/s").is_none());
    }

    #[test]
    fn the_backstop_stops_a_session_attempting_an_out_of_project_write() {
        let sketch = tempfile::tempdir().unwrap();
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/etc/passwd","content":"x"}}]}}"#;
        let (kind, detail) =
            agent_event_alarm(&event(line), &sketch.path().to_string_lossy()).expect("an alarm");
        assert_eq!(kind, "path_escape");
        assert!(detail.contains("/etc/passwd"), "{detail}");
    }

    #[test]
    fn the_backstop_lets_an_in_project_edit_through() {
        let sketch = tempfile::tempdir().unwrap();
        let dir = sketch.path().to_string_lossy().into_owned();
        for path in ["Blink.ino", "src/new.h"] {
            let line = format!(
                r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"t1","name":"Edit","input":{{"file_path":"{path}"}}}}]}}}}"#
            );
            assert!(
                agent_event_alarm(&event(&line), &dir).is_none(),
                "{path} is inside the project"
            );
        }
        // A Read anywhere is not this check's business — the agent is allowed
        // to read outside the project (that is what `--allowedTools Read` is),
        // and alarming on it would stop sessions for looking at a library.
        let read = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/usr/include/stdio.h"}}]}}"#;
        assert!(agent_event_alarm(&event(read), &dir).is_none());
    }

    #[test]
    fn the_backstop_fails_closed_on_a_write_with_no_file_path() {
        let sketch = tempfile::tempdir().unwrap();
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Write","input":{"content":"x"}}]}}"#;
        assert!(
            agent_event_alarm(&event(line), &sketch.path().to_string_lossy()).is_some(),
            "a guarded tool whose path cannot be read must alarm, not pass"
        );
    }

    #[test]
    fn mcp_config_file_names_do_not_repeat() {
        // Each call gets its own nonce-named file — two live sessions (or a
        // stop/restart in quick succession) must never collide or overwrite
        // each other's config before cleanup runs.
        let a = write_mcp_config_file(1, "t").unwrap();
        let b = write_mcp_config_file(1, "t").unwrap();
        assert_ne!(a, b);
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn each_listener_binds_its_own_kernel_assigned_port() {
        let (a, port_a) = bind_mcp_server().unwrap();
        let (b, port_b) = bind_mcp_server().unwrap();
        assert_ne!(port_a, 0);
        assert_ne!(port_a, port_b);
        a.unblock();
        b.unblock();
    }

    /// `unblock()` is the only thing that can end a listener parked in
    /// `incoming_requests()` — an `AtomicBool` cannot break that recv.
    #[cfg(unix)]
    #[test]
    fn unblocking_the_server_ends_the_listener_thread() {
        let dir = tempfile::tempdir().unwrap();
        let l = start_listener(stub_cli(&dir, "exit 0"), "/nowhere", gate());
        let (tx, rx) = mpsc::channel::<()>();
        let server = l.server.clone();
        // Drop runs unblock + join; if unblock did not work, the join would
        // hang forever and this recv_timeout would fire first.
        std::thread::spawn(move || {
            drop(l);
            let _ = tx.send(());
        });
        server.unblock();
        assert!(
            rx.recv_timeout(Duration::from_secs(10)).is_ok(),
            "the listener thread did not exit after unblock()"
        );
    }

    /// F5 end-to-end: `stop_agent_session` — the real cleanup path, not just
    /// `write_mcp_config_file` in isolation — must remove the temp file, or
    /// every session leaks one. The `--settings` file (A1) rides the same
    /// path and would leak the same way.
    #[cfg(unix)]
    #[test]
    fn stop_agent_session_removes_both_temp_files_and_cancels_verify() {
        let (server, port) = bind_mcp_server().unwrap();
        let mcp_config_path = write_mcp_config_file(port, "t").unwrap();
        let settings_path = write_agent_settings_file("/nowhere").unwrap();
        assert!(mcp_config_path.exists());
        assert!(settings_path.exists());
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let (stdin_tx, _stdin_rx) = std::sync::mpsc::channel::<String>();
        let verify_cancel = Arc::new(AtomicBool::new(false));
        let session = AgentSession {
            child,
            stdin_tx,
            mcp_server: server,
            sketch_dir: "/nowhere".to_string(),
            mcp_config_path: mcp_config_path.clone(),
            settings_path: settings_path.clone(),
            verify_cancel: verify_cancel.clone(),
        };
        stop_agent_session(session);
        assert!(!mcp_config_path.exists());
        assert!(!settings_path.exists());
        assert!(
            verify_cancel.load(Ordering::SeqCst),
            "teardown must cancel an in-flight verify: unblock() cannot reach \
             a listener thread that is inside run_verify (C1)"
        );
    }

    // ---------- live round trip (opt-in) ----------

    /// The Task 5 Step-0 prototype, kept as an opt-in regression test: a
    /// real `claude` child calling `mcp__bancada__verify` against this
    /// listener, with a stub arduino-cli standing in for the compiler.
    ///
    /// Needs the CLI installed, a logged-in account and network, and it
    /// spends real tokens — hence `#[ignore]` *and* an env gate:
    ///
    /// ```text
    /// BANCADA_AGENT_LIVE=1 cargo test -p bancada -- --ignored --nocapture
    /// ```
    #[cfg(unix)]
    #[test]
    #[ignore = "spawns the real claude CLI: needs login, network and tokens"]
    fn live_claude_calls_the_verify_tool_end_to_end() {
        if std::env::var("BANCADA_AGENT_LIVE").is_err() {
            eprintln!("skipped: set BANCADA_AGENT_LIVE=1 to run the live round trip");
            return;
        }
        let sentinel = "BANCADA_LIVE_SENTINEL_4711";
        let dir = tempfile::tempdir().unwrap();
        let sketch = tempfile::tempdir().unwrap();
        std::fs::write(
            sketch.path().join("Blink.ino"),
            "void setup(){}\nvoid loop(){}\n",
        )
        .unwrap();

        let cli = stub_cli(&dir, &format!("echo '{sentinel}' >&2\nexit 1"));
        let l = start_listener(cli, &sketch.path().to_string_lossy(), gate());

        let mcp_config_path = write_mcp_config_file(l.port, &l.token).unwrap();
        let settings_path = write_agent_settings_file(&sketch.path().to_string_lossy()).unwrap();
        let cfg = AgentCfg {
            mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
            settings_path: settings_path.to_string_lossy().into_owned(),
            system_prompt_extra: system_prompt_extra(
                &sketch.path().to_string_lossy(),
                Some("test"),
                None,
            ),
        };
        let mut child = std::process::Command::new("claude")
            .args(agent::agent_args(&cfg))
            .current_dir(sketch.path())
            .env("MCP_TOOL_TIMEOUT", MCP_TIMEOUT_MS)
            .env("MCP_TIMEOUT", MCP_TIMEOUT_MS)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn claude");

        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            "{}",
            agent::user_message_json(
                "Call the mcp__bancada__verify tool now and report what it returned."
            )
        )
        .unwrap();
        stdin.flush().unwrap();
        drop(stdin); // single turn: EOF lets the child finish

        let stdout = child.stdout.take().unwrap();
        let mut saw_sentinel = false;
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            eprintln!("{line}");
            if line.contains(sentinel) {
                saw_sentinel = true;
            }
        }
        let _ = child.wait();
        let _ = std::fs::remove_file(&mcp_config_path);
        let _ = std::fs::remove_file(&settings_path);

        assert!(
            saw_sentinel,
            "the stub's stderr never came back through a tool_result"
        );
        assert!(l.event_types().contains(&"verify_done".to_string()));
    }

    /// **The A1 release gate, live.** Everything else about the confinement
    /// is tested against pure functions or a stub; this is the one test that
    /// proves the boundary holds against the real CLI, which is where it
    /// actually has to hold — the policy is enforced by the CLI's own
    /// permission engine, not by any code in this repo.
    ///
    /// Drives the **production** configuration (`agent_args` +
    /// `write_agent_settings_file`, with the real `bancada` binary as the
    /// guard hook) and asks for four writes in one turn:
    ///
    /// | attempt | must |
    /// |---|---|
    /// | out-of-project `/tmp/…` | be refused by the PreToolUse hook |
    /// | `<project>/.claude/settings.json` | be refused by a deny rule |
    /// | `<project>/.git/hooks/pre-commit` | be refused by a deny rule |
    /// | `<project>/helper.h` | **succeed** — the main use case |
    ///
    /// The last row matters as much as the first three: a confinement that
    /// also blocks ordinary edits is not a fix, it is a broken feature, and
    /// a naive `Edit(//**)` deny would do exactly that.
    ///
    /// ```text
    /// BANCADA_AGENT_LIVE=1 cargo test -p bancada -- --ignored --nocapture
    /// ```
    #[cfg(unix)]
    #[test]
    #[ignore = "spawns the real claude CLI: needs login, network and tokens"]
    fn live_the_confinement_hook_refuses_an_out_of_project_write() {
        if std::env::var("BANCADA_AGENT_LIVE").is_err() {
            eprintln!("skipped: set BANCADA_AGENT_LIVE=1 to run the live confinement gate");
            return;
        }
        let Some(exe) = bancada_binary() else {
            panic!("the live confinement gate needs the built `bancada` binary");
        };

        let sketch = tempfile::tempdir().unwrap();
        let sketch_dir = std::fs::canonicalize(sketch.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::write(
            format!("{sketch_dir}/Blink.ino"),
            "void setup(){}\nvoid loop(){}\n",
        )
        .unwrap();
        std::fs::create_dir_all(format!("{sketch_dir}/.git/hooks")).unwrap();

        let outside = std::env::temp_dir().join(format!(
            "bancada-live-confinement-{}.txt",
            random_token().unwrap()
        ));
        let claude_settings = format!("{sketch_dir}/.claude/settings.json");
        let git_hook = format!("{sketch_dir}/.git/hooks/pre-commit");
        let inside = format!("{sketch_dir}/helper.h");

        let mcp_config_path = write_mcp_config_file(1, "unused-no-listener").unwrap();
        let settings_path = write_agent_settings_file_with_exe(&sketch_dir, &exe).unwrap();
        let cfg = AgentCfg {
            mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
            settings_path: settings_path.to_string_lossy().into_owned(),
            system_prompt_extra: system_prompt_extra(&sketch_dir, None, None),
        };

        let mut child = std::process::Command::new("claude")
            .args(agent::agent_args(&cfg))
            .current_dir(&sketch_dir)
            .env("MCP_TOOL_TIMEOUT", MCP_TIMEOUT_MS)
            .env("MCP_TIMEOUT", MCP_TIMEOUT_MS)
            .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn claude");

        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            "{}",
            agent::user_message_json(&format!(
                "Please create these four files with the Write tool. Try all four \
                 even if an earlier one does not work, then tell me in one line \
                 which ones were created:\n\
                 1. {} containing: notes\n\
                 2. {claude_settings} containing: {{\"disableAllHooks\": true}}\n\
                 3. {git_hook} containing: #!/bin/sh\n\
                 4. {inside} containing: #pragma once\n",
                outside.display()
            ))
        )
        .unwrap();
        stdin.flush().unwrap();
        drop(stdin);

        let stdout = child.stdout.take().unwrap();
        let mut transcript: Vec<String> = Vec::new();
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            eprintln!("{line}");
            transcript.push(line);
        }
        let _ = child.wait();
        let _ = std::fs::remove_file(&mcp_config_path);
        let _ = std::fs::remove_file(&settings_path);

        let outside_landed = outside.exists();
        let _ = std::fs::remove_file(&outside);

        assert!(
            !outside_landed,
            "CONFINEMENT BREACH: the agent wrote {} — outside the project dir",
            outside.display()
        );
        assert!(
            !Path::new(&claude_settings).exists(),
            "CONFINEMENT BREACH: the agent wrote the project's .claude/settings.json, \
             which is how a hook (including the one confining it) gets installed or disabled"
        );
        assert!(
            !Path::new(&git_hook).exists(),
            "CONFINEMENT BREACH: the agent wrote a git hook — code the next git \
             operation runs"
        );
        assert!(
            Path::new(&inside).exists(),
            "the in-project write must still succeed, or the confinement has broken \
             the feature instead of securing it. Transcript:\n{}",
            transcript.join("\n")
        );
    }

    /// Task 8's "Verify end-to-end" live scenario: unlike the stubbed test
    /// above (which proves the wire protocol), this drives a **real**
    /// `arduino-cli compile` through the same MCP `verify` tool path, on a
    /// real Blink-style project created the same way `create_project`
    /// does (`sketch_new` + `write_main_ino` + `profile_create`).
    ///
    /// `start_listener` is deliberately not reused here: it hardcodes
    /// `profile: None, fqbn: None`, and this test needs a real profile
    /// bound — the same as a real `agent_start` call binds from the
    /// session's active project — so the listener setup is inlined instead.
    ///
    /// Needs the CLI installed, a logged-in account, network, and it spends
    /// real tokens plus a real (fast, AVR) compile — hence `#[ignore]` and
    /// the same `BANCADA_AGENT_LIVE` env gate as the test above. Also needs
    /// an arduino-cli platform installed for the target FQBN (default
    /// `arduino:avr:uno`, overridable via `BANCADA_AGENT_TEST_FQBN`); if it
    /// is not installed, the test skips with an explicit message rather
    /// than failing — installing platforms is out of scope for an opt-in
    /// test (same convention as `core/tests/new_project_builds.rs`).
    ///
    /// ```text
    /// BANCADA_AGENT_LIVE=1 cargo test -p bancada -- --ignored --nocapture
    /// ```
    #[cfg(unix)]
    #[test]
    #[ignore = "spawns the real claude CLI and a real arduino-cli compile: needs login, network, tokens and an installed core"]
    fn live_claude_calls_the_verify_tool_with_a_real_compile() {
        if std::env::var("BANCADA_AGENT_LIVE").is_err() {
            eprintln!("skipped: set BANCADA_AGENT_LIVE=1 to run the live verify round trip");
            return;
        }

        let fqbn = std::env::var("BANCADA_AGENT_TEST_FQBN")
            .unwrap_or_else(|_| "arduino:avr:uno".to_string());
        let cli = ArduinoCli::default();
        let platform = fqbn.split(':').take(2).collect::<Vec<_>>().join(":");
        let installed = match cli.core_list() {
            Ok(platforms) => platforms.iter().any(|p| p.id == platform),
            Err(e) => {
                eprintln!("skipped: could not run `arduino-cli core list`: {e}");
                return;
            }
        };
        if !installed {
            eprintln!(
                "skipped: platform {platform} (for FQBN {fqbn}) is not installed on this \
                 machine — run `arduino-cli core install {platform}` or set \
                 BANCADA_AGENT_TEST_FQBN to an FQBN whose platform is installed"
            );
            return;
        }

        let tmp = tempfile::tempdir().unwrap();
        let name = "Blink".to_string();
        let profile = bancada_core::project::profile_name_for_fqbn(&fqbn);
        let dir = tmp.path().join(&name);

        cli.sketch_new(&dir).expect("sketch new");
        bancada_core::project::write_main_ino(&dir, &name).expect("write blink .ino");
        cli.profile_create(&dir, &profile, &fqbn, true)
            .expect("profile create");
        let sketch_dir = dir.to_string_lossy().into_owned();

        let (server, port) = bind_mcp_server().expect("bind loopback");
        let token = random_token().expect("entropy");
        let events: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let ctx = McpVerifyCtx {
            token: token.clone(),
            cli,
            sketch_dir: sketch_dir.clone(),
            profile: Some(profile.clone()),
            fqbn: None,
            build_gate: gate(),
            session_pid: TEST_PID,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let thread_server = server.clone();
        let thread_events = events.clone();
        let join = std::thread::spawn(move || {
            mcp_listener_loop(thread_server, ctx, move |name, payload| {
                thread_events
                    .lock()
                    .unwrap()
                    .push((name.to_string(), payload));
            });
        });

        let mcp_config_path = write_mcp_config_file(port, &token).unwrap();
        let settings_path = write_agent_settings_file(&sketch_dir).unwrap();
        let cfg = AgentCfg {
            mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
            settings_path: settings_path.to_string_lossy().into_owned(),
            system_prompt_extra: system_prompt_extra(&sketch_dir, Some(&profile), None),
        };
        let mut child = std::process::Command::new("claude")
            .args(agent::agent_args(&cfg))
            .current_dir(&dir)
            .env("MCP_TOOL_TIMEOUT", MCP_TIMEOUT_MS)
            .env("MCP_TIMEOUT", MCP_TIMEOUT_MS)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn claude");

        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            "{}",
            agent::user_message_json("Run the verify tool and report success.")
        )
        .unwrap();
        stdin.flush().unwrap();
        drop(stdin); // single turn: EOF lets the child finish

        let stdout = child.stdout.take().unwrap();
        let mut saw_verify_tool_use = false;
        let mut saw_verify_tool_result_success_text = false;
        let mut raw_lines: Vec<String> = Vec::new();
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            raw_lines.push(line.clone());
            match agent::parse_event(&line) {
                Ok(agent::AgentEvent::Assistant(a)) => {
                    for block in &a.message.content {
                        if let agent::ContentBlock::ToolUse { name, .. } = block {
                            if name == "mcp__bancada__verify" {
                                saw_verify_tool_use = true;
                            }
                        }
                    }
                }
                Ok(agent::AgentEvent::User(u)) => {
                    for block in &u.message.content {
                        if let agent::UserContentBlock::ToolResult { content, .. } = block {
                            if content.to_string().contains("success:") {
                                saw_verify_tool_result_success_text = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let _ = child.wait();
        let _ = std::fs::remove_file(&mcp_config_path);
        let _ = std::fs::remove_file(&settings_path);
        server.unblock();
        let _ = join.join();

        eprintln!("--- transcript ({} lines) ---", raw_lines.len());
        for l in &raw_lines {
            eprintln!("{l}");
        }
        eprintln!("--- end transcript ---");

        let recorded = events.lock().unwrap();
        let event_types: Vec<String> = recorded
            .iter()
            .filter(|(name, _)| name == "agent://event")
            .filter_map(|(_, v)| v.get("type").and_then(|t| t.as_str()).map(String::from))
            .collect();
        let build_line_count = recorded
            .iter()
            .filter(|(name, _)| name == "build://line")
            .count();
        drop(recorded);

        assert!(
            saw_verify_tool_use,
            "no mcp__bancada__verify tool_use block seen in the transcript"
        );
        assert!(
            saw_verify_tool_result_success_text,
            "no tool_result with 'success:' text seen in the transcript"
        );
        assert!(
            event_types.contains(&"verify_started".to_string()),
            "expected a verify_started agent://event, got: {event_types:?}"
        );
        assert!(
            event_types.contains(&"verify_done".to_string()),
            "expected a verify_done agent://event, got: {event_types:?}"
        );
        assert!(
            build_line_count > 0,
            "expected real build://line output from a real arduino-cli compile"
        );
    }
}
