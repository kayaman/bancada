//! Bancada's Tauri layer: thin async commands over `bancada-core`, plus event
//! streaming for build output and the serial monitor.
//!
//! Events emitted to the frontend:
//!   "build://line"      { stream: "stdout"|"stderr", line: string }
//!   "serial://line"     { stream, line }
//!   "serial://closed"   {}

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Child;
use std::sync::Mutex;

use bancada_core::cli::ArduinoCli;
use bancada_core::sketch::{SketchProject, SketchYaml};
use bancada_core::types::{DetectedPort, IndexedLibrary, InstalledLibrary, RunResult};
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    cli: ArduinoCli,
    monitor: Mutex<Option<Child>>,
}

/// Convert any core error into the string Tauri sends to JS.
fn err_str(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Kill and reap a child process, if one is running.
fn kill_child(slot: &mut Option<Child>) {
    if let Some(mut child) = slot.take() {
        let _ = child.kill();
        let _ = child.wait();
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
fn list_sketch_files(
    sketch_dir: String,
) -> Result<Vec<bancada_core::sketch::SketchFile>, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.list_files().map_err(err_str)
}

#[tauri::command]
fn read_sketch_file(sketch_dir: String, rel_path: String) -> Result<String, String> {
    let full = safe_join(&sketch_dir, &rel_path)?;
    std::fs::read_to_string(full).map_err(err_str)
}

#[tauri::command]
fn write_sketch_file(
    sketch_dir: String,
    rel_path: String,
    content: String,
) -> Result<(), String> {
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
fn add_local_library(
    sketch_dir: String,
    profile: String,
    lib_dir: String,
) -> Result<SketchYaml, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.add_local_library(&profile, Path::new(&lib_dir))
        .map_err(err_str)
}

#[tauri::command]
fn add_registry_library_to_profile(
    sketch_dir: String,
    profile: String,
    name: String,
    version: String,
) -> Result<SketchYaml, String> {
    let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
    proj.add_registry_library(&profile, &name, &version)
        .map_err(err_str)
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
            .upload(&sketch_dir, profile.as_deref(), fqbn.as_deref(), &port, |line| {
                let _ = app.emit("build://line", &line);
            })
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
    let mut guard = state.monitor.lock().unwrap();
    kill_child(&mut guard);

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

    *guard = Some(child);
    Ok(())
}

#[tauri::command]
fn stop_monitor(state: State<'_, AppState>) -> Result<(), String> {
    kill_child(&mut state.monitor.lock().unwrap());
    Ok(())
}

/// Transmit a line to the board through the monitor's stdin.
#[tauri::command]
fn monitor_send(state: State<'_, AppState>, data: String) -> Result<(), String> {
    let mut guard = state.monitor.lock().unwrap();
    let child = guard.as_mut().ok_or("serial monitor is not running")?;
    let stdin = child.stdin.as_mut().ok_or("monitor stdin unavailable")?;
    writeln!(stdin, "{data}").map_err(err_str)?;
    stdin.flush().map_err(err_str)
}

// ---------- board utilities ----------

#[tauri::command]
async fn read_board_mac(port: String) -> Result<bancada_core::esptool::ChipInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        bancada_core::esptool::read_mac(&port).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

// ---------- entry point ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(AppState {
                cli: ArduinoCli::default(),
                monitor: Mutex::new(None),
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
            add_local_library,
            add_registry_library_to_profile,
            search_libraries,
            list_installed_libraries,
            install_library,
            uninstall_library,
            compile_sketch,
            upload_sketch,
            start_monitor,
            stop_monitor,
            monitor_send,
            read_board_mac
        ])
        .build(tauri::generate_context!())
        .expect("error while building Bancada")
        .run(|app, event| {
            // The monitor child holds the serial port exclusively; kill it on
            // exit so the port is immediately free for other tools.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                kill_child(&mut state.monitor.lock().unwrap());
            }
        });
}
