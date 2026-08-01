//! Bancada's Tauri layer: thin async commands over `bancada-core`, plus event
//! streaming for build output, the serial monitor, and the scope.
//!
//! Events emitted to the frontend:
//!   "build://line"      { stream: "stdout"|"stderr", line: string }
//!   "serial://line"     { stream, line }
//!   "serial://closed"   {}
//!   "ports://changed"   {}  (the set of serial ports on the machine changed)
//!
//! Scope commands (`docs/scope-architecture.md` §3): `scope_probe`,
//! `scope_start`, `scope_single`, `scope_send`, `scope_stop`,
//! `scope_install_firmware`, plus `save_text_file` / `save_binary_file`.
//! `scope_start` streams binary envelopes (§2: kind 0x01 samples, 0x02 JSON
//! events) over a `tauri::ipc::Channel` instead of events. The serial port
//! has a single owner at a time — monitor child process or scope session —
//! and acquiring it for one evicts the other.
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

/// Sketchbook root — the default place to create a new project.
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
        if !parent_path.is_dir() {
            return Err(format!("{parent} is not a directory"));
        }
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
    tauri::async_runtime::spawn_blocking(move || {
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
    tauri::async_runtime::spawn_blocking(move || {
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
            save_mqtt_config
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
            }
        });
}
