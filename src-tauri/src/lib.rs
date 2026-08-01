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
//!                       { type: "verify_started" }
//!                       { type: "verify_done", success }
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
//! thread) — the panel shows "Session ended" and the frontend calls
//! `agent_stop(pid)` on `agent://closed` so the child is reaped; `pid`
//! guards a stale close from a superseded session against killing a newer
//! one (`should_stop_agent`). The `--mcp-config` bearer token rides a 0600
//! temp file, not argv (`write_mcp_config_file`) — argv is readable by any
//! local process via `/proc/<pid>/cmdline` on Linux.
//!
//! Build gate: `compile_sketch`, `upload_sketch` and the agent's MCP
//! `verify` tool all drive the same arduino-cli build cache, so they share
//! one `build_gate: Mutex<()>` in `AppState`. It is taken with `try_lock`,
//! never blocking — a contended build fails fast with "build already in
//! progress" instead of queueing behind a multi-minute platform build.
//! Before the gate, the only mutual exclusion was the frontend's `busy`
//! flag, which agent-initiated builds bypass entirely.
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
    /// Serialises every arduino-cli build in the process — user Verify,
    /// user Upload, and the agent's MCP `verify` tool all share one build
    /// cache, and nothing but the frontend's `busy` flag used to keep them
    /// apart (which agent-initiated builds bypass entirely).
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
    let stale = f
        .boards
        .iter()
        .any(|b| a.online.contains(&b.id) && now.saturating_sub(b.last_seen) > LAST_SEEN_RESOLUTION);
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
    Client as MqttClient, Connection as MqttConnection, ConnectReturnCode, Event as MqttNetEvent,
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
/// pool directly and fall back to a clock/ASLR mix where that file does not
/// exist. The listener is bound to `127.0.0.1`, so this only has to keep
/// *other local processes* from driving the user's compiler.
fn random_token() -> String {
    let mut bytes = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_ok()
    {
        return bytes.iter().map(|b| format!("{b:02x}")).collect();
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let heap = Box::new(0u8);
    let addr = &*heap as *const u8 as usize;
    format!("{nanos:032x}{addr:016x}")
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
    let path = std::env::temp_dir().join(format!("bancada-agent-mcp-{}.json", random_token()));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("could not create the agent's MCP config file: {e}"))?;
        if let Err(e) = file.write_all(mcp_config.as_bytes()) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(format!("could not write the agent's MCP config file: {e}"));
        }
    }
    #[cfg(not(unix))]
    {
        // No equivalent atomic-mode create outside unix (no Windows dev/CI
        // target exists in this workspace yet) — plain write, no ACL
        // narrowing. Revisit if/when one does.
        std::fs::write(&path, &mcp_config)
            .map_err(|e| format!("could not write the agent's MCP config file: {e}"))?;
    }

    Ok(path)
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

        // Size cap *before* parsing: a request body is attacker-controlled
        // in the sense that any local process can POST here, and
        // `read_to_string` on an unbounded reader would otherwise let one
        // of them push the whole app into swap.
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

/// The `verify` tool: the same `cli.compile` path the toolbar's Verify
/// button runs, with the same `build://line` stream, so agent builds show up
/// in the Console for free.
fn run_verify(ctx: &McpVerifyCtx, emit: &impl Fn(&str, serde_json::Value)) -> (String, bool) {
    // Shared with compile_sketch/upload_sketch: an agent build must not race
    // a user build through the same arduino-cli build cache (risk R5).
    let _gate = match try_build_gate(&ctx.build_gate) {
        Ok(guard) => guard,
        Err(busy) => return (busy, true),
    };

    emit(
        "agent://event",
        serde_json::json!({ "type": "verify_started" }),
    );

    let mut collected: Vec<OutputLine> = Vec::new();
    let run = ctx.cli.compile(
        &ctx.sketch_dir,
        ctx.profile.as_deref(),
        ctx.fqbn.as_deref(),
        &[],
        |line| {
            if let Ok(value) = serde_json::to_value(&line) {
                emit("build://line", value);
            }
            collected.push(line);
        },
    );

    match run {
        Ok(result) => {
            emit(
                "agent://event",
                serde_json::json!({ "type": "verify_done", "success": result.success }),
            );
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
            emit(
                "agent://event",
                serde_json::json!({ "type": "verify_done", "success": false }),
            );
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

/// Tear a session down: close the writer channel, stop the listener, then
/// kill and reap the child so no zombie is left behind.
fn stop_agent_session(session: AgentSession) {
    let AgentSession {
        child,
        stdin_tx,
        mcp_server,
        mcp_config_path,
        ..
    } = session;
    drop(stdin_tx); // the writer thread's recv ends
    mcp_server.unblock(); // the listener's blocking incoming_requests() ends
    kill_child(child);
    // Best-effort (F5 cleanup): a leftover temp file after a hard crash
    // isn't worth failing shutdown over, but every normal stop/exit path
    // reaches here and removes it.
    let _ = std::fs::remove_file(&mcp_config_path);
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
/// Spawns, in order: the loopback MCP listener (bound first, because its
/// real port goes into the child's `--mcp-config`), the `claude` child with
/// cwd = the sketch dir, and the three pipe threads. Nothing here blocks,
/// so this stays a sync command like `start_monitor`.
#[tauri::command]
fn agent_start(
    app: AppHandle,
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: Option<String>,
    fqbn: Option<String>,
) -> Result<(), String> {
    let mut guard = state.agent.lock().unwrap();
    if let Some(existing) = guard.as_ref() {
        return Err(format!(
            "an agent session is already running for {}",
            existing.sketch_dir
        ));
    }

    let (server, port) = bind_mcp_server()?;
    let token = random_token();

    // ---------- thread 1 of 4: MCP listener (owned clones, never locks agent)
    let ctx = McpVerifyCtx {
        token: token.clone(),
        cli: state.cli.clone(),
        sketch_dir: sketch_dir.clone(),
        profile: profile.clone(),
        fqbn: fqbn.clone(),
        build_gate: state.build_gate.clone(),
    };
    let listener_server = server.clone();
    let listener_app = app.clone();
    std::thread::spawn(move || {
        mcp_listener_loop(listener_server, ctx, move |event, payload| {
            let _ = listener_app.emit(event, payload);
        });
    });

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

    let cfg = AgentCfg {
        mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
        system_prompt_extra: system_prompt_extra(&sketch_dir, profile.as_deref(), fqbn.as_deref()),
    };
    let mut child = match std::process::Command::new("claude")
        .args(agent::agent_args(&cfg))
        .current_dir(&sketch_dir)
        .env("MCP_TOOL_TIMEOUT", MCP_TIMEOUT_MS)
        .env("MCP_TIMEOUT", MCP_TIMEOUT_MS)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            server.unblock(); // never leave the listener thread parked
            let _ = std::fs::remove_file(&mcp_config_path);
            return Err(claude_spawn_error(e));
        }
    };
    // Captured before stdin/stdout/stderr are taken so the stdout-reader
    // thread (below) can stamp `agent://closed` with the pid it's about,
    // for the frontend to relay to `agent_stop` (F4 guard above).
    let child_pid = child.id();

    // All three were just piped, so this cannot realistically fail — but if
    // it ever did, bailing with `?` would strand a live child and a parked
    // listener thread with no handle left to stop either.
    let (mut child_stdin, stdout, stderr) =
        match (child.stdin.take(), child.stdout.take(), child.stderr.take()) {
            (Some(stdin), Some(stdout), Some(stderr)) => (stdin, stdout, stderr),
            _ => {
                server.unblock();
                kill_child(child);
                let _ = std::fs::remove_file(&mcp_config_path);
                return Err("the agent's stdio pipes were unavailable".to_string());
            }
        };

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
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            // `parse_event` is the validity gate, but what reaches the
            // frontend is the CLI's own event object verbatim: the panel's
            // contract is the wire shape, not a re-modelled subset that
            // would silently drop fields core doesn't happen to name.
            match agent::parse_event(&line) {
                Ok(_) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                        let _ = app_out.emit("agent://event", value);
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
    });
    Ok(())
}

/// Queue a user message for the child's stdin.
#[tauri::command]
fn agent_send(state: State<'_, AppState>, text: String) -> Result<(), String> {
    // Clone the sender out and drop the guard *before* sending: no write to
    // the child ever happens while the agent mutex is held.
    let tx = {
        let guard = state.agent.lock().unwrap();
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
        let guard = state.agent.lock().unwrap();
        let Some(session) = guard.as_ref() else {
            return Err("the agent is not running".to_string());
        };
        (session.stdin_tx.clone(), session.child.id())
    };
    let _ = tx.send(agent::interrupt_json(&format!("int-{}", now_millis())));

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        let state = app.state::<AppState>();
        let session = {
            let mut guard = state.agent.lock().unwrap();
            // Only kill the session this interrupt was aimed at: two seconds
            // is long enough for the user to have stopped it and started a
            // new one, which must not be collateral damage.
            match guard.as_ref() {
                Some(session) if session.child.id() == pid => guard.take(),
                _ => None,
            }
        }; // guard dropped before the blocking kill/wait below
        if let Some(session) = session {
            stop_agent_session(session);
        }
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
        let mut guard = state.agent.lock().unwrap();
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
                let agent_session = state.agent.lock().unwrap().take();
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

    fn start_listener(cli: ArduinoCli, sketch_dir: &str, gate: Arc<Mutex<()>>) -> TestListener {
        let (server, port) = bind_mcp_server().expect("bind loopback");
        let token = random_token();
        let events: Events = Arc::new(Mutex::new(Vec::new()));

        let ctx = McpVerifyCtx {
            token: token.clone(),
            cli,
            sketch_dir: sketch_dir.to_string(),
            profile: None,
            fqbn: None,
            build_gate: gate,
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
        let a = random_token();
        let b = random_token();
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
            random_token()
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
    /// every session leaks one.
    #[cfg(unix)]
    #[test]
    fn stop_agent_session_removes_the_mcp_config_file() {
        let (server, port) = bind_mcp_server().unwrap();
        let mcp_config_path = write_mcp_config_file(port, "t").unwrap();
        assert!(mcp_config_path.exists());
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let (stdin_tx, _stdin_rx) = std::sync::mpsc::channel::<String>();
        let session = AgentSession {
            child,
            stdin_tx,
            mcp_server: server,
            sketch_dir: "/nowhere".to_string(),
            mcp_config_path: mcp_config_path.clone(),
        };
        stop_agent_session(session);
        assert!(!mcp_config_path.exists());
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
        let cfg = AgentCfg {
            mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
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

        assert!(
            saw_sentinel,
            "the stub's stderr never came back through a tool_result"
        );
        assert!(l.event_types().contains(&"verify_done".to_string()));
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
        let token = random_token();
        let events: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let ctx = McpVerifyCtx {
            token: token.clone(),
            cli,
            sketch_dir: sketch_dir.clone(),
            profile: Some(profile.clone()),
            fqbn: None,
            build_gate: gate(),
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
        let cfg = AgentCfg {
            mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
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
