//! The third pillar, Enclosure, now in-process and in-pane: starts
//! enclosure-maker's own preview server (3D render, code pane, chat)
//! directly inside bancada's Tauri app and hands the frontend a URL to
//! embed in an `<iframe>` filling the Enclosure division's main pane --
//! instead of spawning a separate `enclosure-maker-app` process (retired
//! last session) or even a separate native window (retired this one, per
//! "same window" -- see `src/components/EnclosurePanel.tsx`).
//! `em_preview::spawn_preview` has no Tauri dependency of its own -- it's a
//! plain axum/tokio server built to be "embedded in another process" (its
//! own doc comment) -- which is what makes this possible. Ported from
//! enclosure-maker's own (now retired) standalone `src-tauri/src/main.rs`;
//! see `enclosure-maker/README.md`.
//!
//! The one feature this costs: `enclosure-maker/static/preview.js`'s Export
//! STL button normally calls `save_stl_to_downloads` via Tauri's injected
//! `window.__TAURI__` IPC bridge, which Tauri does not extend into a nested
//! iframe's own `window`. `preview.js` falls back to `postMessage`ing its
//! parent when `window.__TAURI__` is absent; `src/App.tsx` listens for that
//! message and calls the command on the iframe's behalf (see the
//! `message` event listener there).

use std::path::{Path, PathBuf};

use base64::Engine as _;
use tauri::{AppHandle, Manager};

use bancada_core::enclosure_handoff::EnclosureHandoff;
use em_projects::bancada_import::{BancadaImport, BoardSummary, ComponentSummary, UsbPortSummary, WiringEntry};

// Nouveau-GPU WebKit crash avoidance lives in `setup::ensure_webkit_renderer_works`
// now -- bancada already had its own equivalent for its main window (one env
// var), extended there to cover the other two enclosure-maker's own fix
// needed, rather than keeping two separate GPU-detection code paths in one
// binary. The embedded preview iframe is the same WebKitGTK process as the
// main window, so that one process-wide, startup-time fix covers it too.

/// `<sketch_dir>/main.rhai` -- the one fact that distinguishes "this project
/// already has an enclosure" from "nothing sent yet". Nesting directly in
/// the bancada project's own directory (not some separate projects root) is
/// `em_projects::bancada_import`'s own established behavior whenever
/// `source_dir` exists, which it always does here.
fn main_rhai_path(sketch_dir: &str) -> PathBuf {
    Path::new(sketch_dir).join(em_projects::projects::ENTRY_FILE)
}

/// Whether an enclosure project already exists for this sketch -- lets the
/// Enclosure tab skip straight to the embedded viewer instead of the
/// compose box when there's already something to look at.
#[tauri::command]
pub fn has_enclosure_project(sketch_dir: String) -> bool {
    main_rhai_path(&sketch_dir).is_file()
}

/// Starts the preview server for an *already-existing* enclosure project and
/// returns its URL, with no seed message -- entering the Enclosure division
/// again later should not inject a new chat message, just resume looking at
/// what is already there.
#[tauri::command]
pub async fn resume_enclosure_preview(sketch_dir: String) -> Result<String, String> {
    let main_path = main_rhai_path(&sketch_dir);
    if !main_path.is_file() {
        return Err("no enclosure project exists for this sketch yet".to_string());
    }
    let (addr, _state) = em_preview::spawn_preview(main_path, None).await.map_err(|e| e.to_string())?;
    Ok(format!("http://{addr}"))
}

fn to_bancada_import(handoff: &EnclosureHandoff) -> BancadaImport {
    BancadaImport {
        version: handoff.version,
        project_name: handoff.project_name.clone(),
        source_dir: handoff.source_dir.clone(),
        board: handoff.board.as_ref().map(|b| BoardSummary {
            id: b.id.clone(),
            name: b.name.clone(),
            vendor: b.vendor.clone(),
            usb_ports: b
                .usb_ports
                .iter()
                .map(|p| UsbPortSummary { label: p.label.clone(), kind: p.kind.to_string() })
                .collect(),
        }),
        components: handoff
            .components
            .iter()
            .map(|c| ComponentSummary {
                qty: c.qty,
                ref_: c.ref_.clone(),
                value: c.value.clone(),
                package: c.package.clone(),
                description: c.description.clone(),
                wiring: c
                    .wiring
                    .iter()
                    .map(|w| WiringEntry { pin: w.pin.clone(), gpio: w.gpio, rail: w.rail.clone(), notes: w.notes.clone() })
                    .collect(),
            })
            .collect(),
        prompt: handoff.prompt.clone(),
    }
}

/// Hands this project's BOM, resolved board, and the user-approved `prompt`
/// from bancada's Enclosure tab to enclosure-maker's own project-creation
/// logic (in-process, a function call -- not a spawned binary and a JSON
/// file anymore), starts its preview server, and returns the URL with that
/// seed message queued as a `?seed=` query param --
/// `enclosure-maker/static/preview.js` sends it as the first chat message
/// on initial connect, exactly as if the user had typed it themselves.
/// `EnclosurePanel` embeds the returned URL in an iframe filling the
/// Enclosure division's main pane. Still writes the `.bancada/
/// enclosure_handoff.json` hand-off file alongside, for provenance/
/// debugging parity with the old cross-process path.
#[tauri::command]
pub async fn open_enclosure_preview(sketch_dir: String, prompt: String) -> Result<String, String> {
    let dir = Path::new(&sketch_dir);
    let mut handoff = crate::load_enclosure_handoff(&sketch_dir)?;
    handoff.prompt = Some(prompt);
    handoff.write(dir).map_err(crate::err_str)?;

    let import = to_bancada_import(&handoff);
    let (main_path, seed_message) = em_projects::bancada_import::create_or_reuse_project(&em_projects::projects::projects_root(), &import)
        .map_err(|e| e)?;

    let (addr, _state) = em_preview::spawn_preview(main_path, None).await.map_err(|e| e.to_string())?;
    let mut url: tauri::Url = format!("http://{addr}").parse().map_err(|_| "bad preview URL")?;
    url.query_pairs_mut().append_pair("seed", &seed_message);
    Ok(url.to_string())
}

/// Keeps only a single safe file name, always ending in `.stl`. Directory
/// components and characters that are not part of a plain name are dropped
/// so a download cannot be steered outside the Downloads folder.
fn sanitize_download_name(filename: &str) -> String {
    let base = Path::new(filename).file_name().and_then(|n| n.to_str()).unwrap_or("model.stl");
    let cleaned: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    let cleaned = if cleaned.is_empty() { "model.stl".to_string() } else { cleaned };
    if cleaned.to_ascii_lowercase().ends_with(".stl") {
        cleaned
    } else {
        format!("{cleaned}.stl")
    }
}

fn unique_download_path(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let stem = Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("model");
    for n in 1..10_000 {
        let candidate = dir.join(format!("{stem} ({n}).stl"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}.stl", std::process::id()))
}

fn write_stl_download(dir: &Path, filename: &str, contents_b64: &str) -> Result<PathBuf, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(contents_b64)
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = unique_download_path(dir, &sanitize_download_name(filename));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Writes the exported STL straight into the OS Downloads folder. The
/// preview WebView does not reliably honor `<a download>`, so Export STL
/// fetches the bytes and hands them here instead of opening a save dialog.
/// Ported verbatim from enclosure-maker's own (retired) standalone app --
/// `enclosure-maker/static/preview.js`'s Export STL button calls this exact
/// command name, now routed to bancada's own process instead.
#[tauri::command]
pub fn save_stl_to_downloads(app: AppHandle, filename: String, contents_b64: String) -> Result<String, String> {
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    write_stl_download(&dir, &filename, &contents_b64).map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_download_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bancada-enclosure-preview-test-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_stl_download_decodes_and_writes_the_exact_bytes() {
        let dir = temp_download_dir("bytes");
        let original = vec![0u8, 1, 2, 253, 254, 255, b'h', b'i'];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&original);

        let path = write_stl_download(&dir, "out.stl", &b64).unwrap();

        assert_eq!(path, dir.join("out.stl"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn write_stl_download_adds_a_suffix_when_the_name_is_taken() {
        let dir = temp_download_dir("suffix");
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"one");
        let first = write_stl_download(&dir, "model.stl", &b64).unwrap();
        let second = write_stl_download(&dir, "model.stl", &b64).unwrap();

        assert_eq!(first, dir.join("model.stl"));
        assert_eq!(second, dir.join("model (1).stl"));
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
    }

    #[test]
    fn write_stl_download_keeps_the_file_inside_the_download_dir() {
        let dir = temp_download_dir("sanitize");
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"stl");

        let path = write_stl_download(&dir, "../../etc/passwd", &b64).unwrap();

        assert_eq!(path, dir.join("passwd.stl"));
        assert!(path.starts_with(&dir));
    }

    #[test]
    fn write_stl_download_rejects_invalid_base64() {
        let dir = temp_download_dir("invalid");
        assert!(write_stl_download(&dir, "out.stl", "not valid base64 !!").is_err());
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
    }
}
