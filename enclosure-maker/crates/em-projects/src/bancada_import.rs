//! Receiving side of "bancada calls us": a project's BOM and resolved board,
//! handed off as a JSON file. This is this repo's own copy of the contract
//! bancada's `core::enclosure_handoff::EnclosureHandoff` produces -- the two
//! are independent binaries in independent repos, so the JSON shape is the
//! contract, not a shared Rust type. `#[serde(default)]` throughout is what
//! actually keeps the two in sync as either side's fields evolve.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BancadaImport {
    #[serde(default)]
    pub version: u32,
    pub project_name: String,
    #[serde(default)]
    pub source_dir: String,
    #[serde(default)]
    pub board: Option<BoardSummary>,
    #[serde(default)]
    pub components: Vec<ComponentSummary>,
    /// The seed chat message, already reviewed and (maybe) edited by the
    /// user in bancada's own Enclosure tab. `None` means an older bancada
    /// that never offered a compose step -- `build_seed_message` below
    /// still generates one in that case.
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSummary {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub usb_ports: Vec<UsbPortSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbPortSummary {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentSummary {
    #[serde(default)]
    pub qty: u32,
    #[serde(rename = "ref", default)]
    pub ref_: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub wiring: Vec<WiringEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiringEntry {
    #[serde(default)]
    pub pin: String,
    #[serde(default)]
    pub gpio: Option<u32>,
    #[serde(default)]
    pub rail: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

pub fn load(path: &std::path::Path) -> Result<BancadaImport, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("could not parse {}: {e}", path.display()))
}

const PROVENANCE_FILE: &str = ".enclosure-maker/import.json";

/// The project name to create/reuse: bancada's own `project_name`, falling
/// back to `source_dir`'s basename if that's empty (defends against a future
/// hand-off producer that leaves it blank). Only used for the `root`-based
/// fallback in [`project_dir_for`], when there's no `source_dir` to nest
/// under -- otherwise unused, but kept as that fallback's naming.
fn project_name_for(import: &BancadaImport) -> String {
    if !import.project_name.trim().is_empty() {
        return import.project_name.clone();
    }
    std::path::Path::new(&import.source_dir)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("bancada-import")
        .to_string()
}

/// Where this hand-off's enclosure project lives: `source_dir` itself,
/// alongside bancada's own `.bancada/` -- Software, Hardware and Enclosure
/// as one project in one folder, not a same-named sibling in some separate
/// enclosure-maker projects root (which collided with bancada's own project
/// directory whenever `projects_root()` and bancada's own projects folder
/// happened to be the same place, as they are by default). Falls back to
/// `root/<name>` when `source_dir` is blank or doesn't exist, so a hand-off
/// producer that can't supply a real project directory still works.
fn project_dir_for(root: &std::path::Path, name: &str, import: &BancadaImport) -> std::path::PathBuf {
    let source = std::path::Path::new(&import.source_dir);
    if !import.source_dir.trim().is_empty() && source.is_dir() {
        source.to_path_buf()
    } else {
        root.join(name)
    }
}

/// Creates a fresh project from `import`, or reuses one from a prior import
/// under the same name, leaving `main.rhai` untouched either way once it
/// exists -- re-sending from bancada must not clobber work already done on
/// the enclosure. Refuses a name collision with an unrelated, non-imported
/// project rather than overwriting it silently. Returns the project's
/// `main.rhai` path and the seed chat message to open it with.
///
/// Branches on `provenance`/`main.rhai`, not on whether `dir` itself
/// exists: `dir` is routinely a bancada project's own directory, already
/// populated with its sketch, `.bancada/`, `bom.yaml` and so on well before
/// the first import ever runs, so "the directory exists" can't be the
/// fresh-vs-collision signal the way it is for the launcher's own
/// `create_project`.
pub fn create_or_reuse_project(root: &std::path::Path, import: &BancadaImport) -> Result<(std::path::PathBuf, String), String> {
    let name = crate::projects::sanitize_name(&project_name_for(import))?;
    let dir = project_dir_for(root, &name, import);
    let provenance = dir.join(PROVENANCE_FILE);
    let raw_json = serde_json::to_string_pretty(import).unwrap_or_default();

    let main_path = if provenance.is_file() {
        let main_path = dir.join(crate::projects::ENTRY_FILE);
        if !main_path.is_file() {
            return Err(format!("{} is missing its {} despite having prior import provenance", dir.display(), crate::projects::ENTRY_FILE));
        }
        std::fs::write(&provenance, &raw_json).map_err(|e| e.to_string())?;
        main_path
    } else {
        let main_path = crate::projects::create_project_at(&dir).map_err(|_| {
            format!(
                "{} already exists and wasn't created from a bancada import -- move or rename it and try again",
                dir.join(crate::projects::ENTRY_FILE).display()
            )
        })?;
        std::fs::create_dir_all(provenance.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&provenance, &raw_json).map_err(|e| e.to_string())?;
        main_path
    };

    // The user already reviewed and could edit this in bancada's Enclosure
    // tab before it was sent -- it wins over generating a fresh one here.
    let seed = import
        .prompt
        .clone()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| build_seed_message(import));
    Ok((main_path, seed))
}

/// Formats the hand-off into a readable first request for the AI assistant
/// -- the same channel a user's own typed chat message goes through, so no
/// new capability is needed on the assistant side, just better input.
pub fn build_seed_message(import: &BancadaImport) -> String {
    let mut out = String::from("Design an enclosure for this project.\n\n");

    match &import.board {
        Some(board) if !board.name.is_empty() => {
            out.push_str(&format!("Board: {}", board.name));
            if !board.vendor.is_empty() {
                out.push_str(&format!(" ({})", board.vendor));
            }
            out.push('\n');
            if !board.usb_ports.is_empty() {
                let ports: Vec<String> = board
                    .usb_ports
                    .iter()
                    .map(|p| {
                        if p.label.is_empty() {
                            p.kind.clone()
                        } else {
                            format!("{} ({})", p.label, p.kind)
                        }
                    })
                    .collect();
                out.push_str(&format!("USB: {}\n", ports.join(", ")));
            }
        }
        _ => out.push_str("Board: not identified -- infer a reasonable enclosure from the components below.\n"),
    }

    if import.components.is_empty() {
        out.push_str("\nNo bill of materials was provided.\n");
    } else {
        out.push_str("\nComponents:\n");
        for c in &import.components {
            out.push_str(&format!("- {} x{}: {}", c.ref_, c.qty, c.value));
            if let Some(pkg) = &c.package {
                out.push_str(&format!(" [{pkg}]"));
            }
            if let Some(desc) = &c.description {
                out.push_str(&format!(" -- {desc}"));
            }
            out.push('\n');
            for w in &c.wiring {
                out.push_str(&format!("    {}", w.pin));
                if let Some(gpio) = w.gpio {
                    out.push_str(&format!(" -> GPIO{gpio}"));
                }
                if let Some(rail) = &w.rail {
                    out.push_str(&format!(" -> {rail}"));
                }
                if let Some(notes) = &w.notes {
                    out.push_str(&format!(" ({notes})"));
                }
                out.push('\n');
            }
        }
    }

    out.push_str(
        "\nUse this to decide which faces need cutouts (connectors, buttons, \
         displays), which components need mounting or clearance, and a \
         sensible overall size. Ask if anything critical is missing.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> BancadaImport {
        serde_json::from_str(
            r#"{
                "version": 1,
                "project_name": "soil-sensor",
                "source_dir": "/home/user/Projects/soil-sensor",
                "board": {
                    "id": "esp32-c6-devkitc-1",
                    "name": "ESP32-C6-DevKitC-1",
                    "vendor": "Espressif",
                    "usb_ports": [{"label": "USB", "kind": "native"}]
                },
                "components": [
                    {
                        "ref": "U1", "value": "ESP32-C6-WROOM-1", "package": "SMD",
                        "qty": 1, "description": "main MCU",
                        "wiring": [{"pin": "IO4", "gpio": 4, "rail": null, "notes": "I2C SDA"}]
                    }
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn loads_the_full_fixture_shape() {
        let import = fixture();
        assert_eq!(import.project_name, "soil-sensor");
        assert_eq!(import.board.as_ref().unwrap().name, "ESP32-C6-DevKitC-1");
        assert_eq!(import.components[0].ref_, "U1");
        assert_eq!(import.components[0].wiring[0].gpio, Some(4));
    }

    #[test]
    fn tolerates_missing_optional_fields() {
        let import: BancadaImport = serde_json::from_str(r#"{"project_name": "bare"}"#).unwrap();
        assert_eq!(import.project_name, "bare");
        assert!(import.board.is_none());
        assert!(import.components.is_empty());
    }

    #[test]
    fn seed_message_mentions_board_name_and_component_details() {
        let msg = build_seed_message(&fixture());
        assert!(msg.contains("ESP32-C6-DevKitC-1"));
        assert!(msg.contains("Espressif"));
        assert!(msg.contains("main MCU"));
        assert!(msg.contains("GPIO4"));
        assert!(msg.contains("I2C SDA"));
    }

    #[test]
    fn seed_message_handles_no_board_and_no_components() {
        let import: BancadaImport = serde_json::from_str(r#"{"project_name": "bare"}"#).unwrap();
        let msg = build_seed_message(&import);
        assert!(msg.contains("not identified"));
        assert!(msg.contains("No bill of materials"));
    }

    fn tempdir(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("em-bancada-import-test-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture_named(name: &str) -> BancadaImport {
        let mut import = fixture();
        import.project_name = name.to_string();
        import
    }

    #[test]
    fn creates_a_fresh_project_with_provenance() {
        let root = tempdir("fresh");
        let import = fixture_named("soil-sensor");

        let (main_path, seed) = create_or_reuse_project(&root, &import).unwrap();

        assert!(main_path.ends_with("soil-sensor/main.rhai"));
        assert!(main_path.is_file());
        assert!(root.join("soil-sensor/.enclosure-maker/import.json").is_file());
        assert!(seed.contains("ESP32-C6-DevKitC-1"));
    }

    #[test]
    fn uses_the_provided_prompt_instead_of_generating_one() {
        let root = tempdir("edited-prompt");
        let mut import = fixture_named("soil-sensor");
        import.prompt = Some("Make it a round enclosure, please.".to_string());

        let (_, seed) = create_or_reuse_project(&root, &import).unwrap();
        assert_eq!(seed, "Make it a round enclosure, please.");
    }

    #[test]
    fn falls_back_to_generating_a_prompt_when_the_provided_one_is_blank() {
        let root = tempdir("blank-prompt");
        let mut import = fixture_named("soil-sensor");
        import.prompt = Some("   ".to_string());

        let (_, seed) = create_or_reuse_project(&root, &import).unwrap();
        assert!(seed.contains("ESP32-C6-DevKitC-1"));
    }

    #[test]
    fn re_importing_refreshes_provenance_but_leaves_main_rhai_alone() {
        let root = tempdir("reimport");
        let import = fixture_named("soil-sensor");
        let (main_path, _) = create_or_reuse_project(&root, &import).unwrap();

        // Simulate the assistant (or the user) having customized the script.
        std::fs::write(&main_path, "// customized by hand\nemit(cuboid(1.0, 1.0, 1.0));\n").unwrap();

        let (main_path_2, _) = create_or_reuse_project(&root, &import).unwrap();
        assert_eq!(main_path, main_path_2);
        let contents = std::fs::read_to_string(&main_path).unwrap();
        assert!(contents.contains("customized by hand"), "re-import must not overwrite main.rhai");
    }

    #[test]
    fn refuses_to_collide_with_an_unrelated_project_of_the_same_name() {
        let root = tempdir("collision");
        crate::projects::create_project(&root, "soil-sensor").unwrap();
        let import = fixture_named("soil-sensor");

        let result = create_or_reuse_project(&root, &import);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wasn't created from a bancada import"));
    }

    fn fixture_with_source_dir(source_dir: &std::path::Path) -> BancadaImport {
        let mut import = fixture();
        import.source_dir = source_dir.to_string_lossy().into_owned();
        import
    }

    /// The bug this pins: bancada's own project directory and this app's
    /// `projects_root()` are `~/Projects` by default, so a same-named
    /// bancada project and enclosure-maker project used to collide on the
    /// very first import. Placing the enclosure files directly inside the
    /// bancada project's own directory instead of `root` sidesteps that
    /// regardless of what `root` is -- and regardless of `dir` already
    /// being a populated, pre-existing directory, which is the normal case
    /// here (it's bancada's own sketch folder).
    #[test]
    fn places_the_enclosure_inside_an_existing_bancada_project_directory() {
        let root = tempdir("nest-root");
        let source = tempdir("nest-source");
        std::fs::write(source.join("sketch.ino"), "void setup(){}\nvoid loop(){}\n").unwrap();
        std::fs::create_dir_all(source.join(".bancada")).unwrap();
        let import = fixture_with_source_dir(&source);

        let (main_path, _) = create_or_reuse_project(&root, &import).unwrap();

        assert_eq!(main_path, source.join("main.rhai"));
        assert!(source.join(".enclosure-maker/import.json").is_file());
        assert!(source.join("lib").is_dir());
        assert!(source.join("sketch.ino").is_file(), "the bancada project's own files must be untouched");
        assert!(!root.join(&import.project_name).exists(), "must not also create a same-named project under root");
    }

    #[test]
    fn re_importing_into_a_bancada_project_directory_leaves_main_rhai_alone() {
        let root = tempdir("nest-reimport-root");
        let source = tempdir("nest-reimport-source");
        let import = fixture_with_source_dir(&source);
        let (main_path, _) = create_or_reuse_project(&root, &import).unwrap();

        std::fs::write(&main_path, "// customized by hand\nemit(cuboid(1.0, 1.0, 1.0));\n").unwrap();

        let (main_path_2, _) = create_or_reuse_project(&root, &import).unwrap();
        assert_eq!(main_path, main_path_2);
        let contents = std::fs::read_to_string(&main_path).unwrap();
        assert!(contents.contains("customized by hand"), "re-import must not overwrite main.rhai");
    }

    /// A narrower collision than before: it's no longer "the bancada
    /// project directory already exists" (normal -- it's bancada's own
    /// sketch folder), but "it already has a `main.rhai` that isn't ours".
    #[test]
    fn refuses_a_bancada_directory_with_an_unrelated_main_rhai() {
        let root = tempdir("nest-collision-root");
        let source = tempdir("nest-collision-source");
        std::fs::write(source.join("main.rhai"), "// not from us\n").unwrap();
        let import = fixture_with_source_dir(&source);

        let result = create_or_reuse_project(&root, &import);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wasn't created from a bancada import"));
        assert_eq!(std::fs::read_to_string(source.join("main.rhai")).unwrap(), "// not from us\n");
    }

    /// `source_dir` pointing at a path that doesn't exist (a stale or
    /// malformed hand-off) falls back to `root`, the same as a blank
    /// `source_dir` -- there's nothing real to nest under.
    #[test]
    fn falls_back_to_root_when_source_dir_does_not_exist() {
        let root = tempdir("nest-fallback-root");
        let mut import = fixture_named("soil-sensor");
        import.source_dir = "/does/not/exist/soil-sensor".to_string();

        let (main_path, _) = create_or_reuse_project(&root, &import).unwrap();
        assert_eq!(main_path, root.join("soil-sensor/main.rhai"));
    }
}
