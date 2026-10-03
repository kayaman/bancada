//! The third pillar, Enclosure, now in-process and in-pane: starts
//! enclosure-maker's own chat server directly inside bancada's Tauri app and
//! hands the frontend a URL to embed in an `<iframe>` filling the Enclosure
//! division's main pane -- instead of spawning a separate
//! `enclosure-maker-app` process (retired long ago) or even a separate
//! native window (retired after that, per "same window" -- see
//! `src/components/EnclosurePanel.tsx`). `em_preview::spawn_preview` has no
//! Tauri dependency of its own -- it's a plain axum/tokio server built to be
//! "embedded in another process" (its own doc comment) -- which is what
//! makes this possible.
//!
//! Enclosure-maker no longer hand-rolls geometry: the embedded chat session
//! drives FreeCAD directly through FreeCAD's own MCP server, and FreeCAD's
//! own GUI window is the live model view -- there is no more 3D canvas, code
//! pane, or model-controls UI inside the iframe, and no more STL export
//! bridge through Tauri (FreeCAD exports straight to a filesystem path
//! itself via `Mesh.export`/`Import.export`, called from the session's own
//! Python). The iframe now only carries the chat UI.

use std::path::{Path, PathBuf};

use bancada_core::enclosure_handoff::EnclosureHandoff;
use em_projects::bancada_import::{BancadaImport, BoardSummary, ComponentSummary, UsbPortSummary, WiringEntry};

// Nouveau-GPU WebKit crash avoidance lives in `setup::ensure_webkit_renderer_works`
// now -- bancada already had its own equivalent for its main window (one env
// var), extended there to cover the other two enclosure-maker's own fix
// needed, rather than keeping two separate GPU-detection code paths in one
// binary. The embedded preview iframe is the same WebKitGTK process as the
// main window, so that one process-wide, startup-time fix covers it too.

/// `<sketch_dir>/model.FCStd` -- the one fact that distinguishes "this
/// project already has an enclosure" from "nothing sent yet". Nesting
/// directly in the bancada project's own directory (not some separate
/// projects root) is `em_projects::bancada_import`'s own established
/// behavior whenever `source_dir` exists, which it always does here.
fn project_entry_path(sketch_dir: &str) -> PathBuf {
    Path::new(sketch_dir).join(em_projects::projects::ENTRY_FILE)
}

/// Whether an enclosure project already exists for this sketch -- lets the
/// Enclosure tab skip straight to the embedded chat view instead of the
/// compose box when there's already something to look at.
#[tauri::command]
pub fn has_enclosure_project(sketch_dir: String) -> bool {
    project_entry_path(&sketch_dir).is_file()
}

/// Starts the chat server for an *already-existing* enclosure project and
/// returns its URL, with no seed message -- entering the Enclosure division
/// again later should not inject a new chat message, just resume the
/// conversation that is already there.
#[tauri::command]
pub async fn resume_enclosure_preview(sketch_dir: String) -> Result<String, String> {
    let entry_path = project_entry_path(&sketch_dir);
    if !entry_path.is_file() {
        return Err("no enclosure project exists for this sketch yet".to_string());
    }
    let (addr, _state) = em_preview::spawn_preview(entry_path, None).await.map_err(|e| e.to_string())?;
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
/// file), starts its chat server, and returns the URL with that seed
/// message queued as a `?seed=` query param -- the chat page sends it as
/// the first message on initial connect, exactly as if the user had typed
/// it themselves. `EnclosurePanel` embeds the returned URL in an iframe
/// filling the Enclosure division's main pane. Still writes the `.bancada/
/// enclosure_handoff.json` hand-off file alongside, for provenance/
/// debugging.
#[tauri::command]
pub async fn open_enclosure_preview(sketch_dir: String, prompt: String) -> Result<String, String> {
    let dir = Path::new(&sketch_dir);
    let mut handoff = crate::load_enclosure_handoff(&sketch_dir)?;
    handoff.prompt = Some(prompt);
    handoff.write(dir).map_err(crate::err_str)?;

    let import = to_bancada_import(&handoff);
    let (entry_path, seed_message) = em_projects::bancada_import::create_or_reuse_project(&em_projects::projects::projects_root(), &import)
        .map_err(|e| e)?;

    let (addr, _state) = em_preview::spawn_preview(entry_path, None).await.map_err(|e| e.to_string())?;
    let mut url: tauri::Url = format!("http://{addr}").parse().map_err(|_| "bad preview URL")?;
    url.query_pairs_mut().append_pair("seed", &seed_message);
    Ok(url.to_string())
}
