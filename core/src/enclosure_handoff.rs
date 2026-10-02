//! Hand-off to enclosure-maker: "bancada calls us" for the third pillar,
//! Enclosure, alongside the Software and Hardware ones this app already
//! covers.
//!
//! A project's BOM (`bom.yaml`, already carrying per-component description,
//! images, and wiring -- see [`crate::bom`]) plus its resolved board, written
//! as one JSON file that enclosure-maker's own process reads on startup. The
//! JSON shape is the contract, not any shared Rust type: enclosure-maker is a
//! separate binary in a separate repo, so it keeps its own copy of this
//! struct shape rather than depending on this crate. `#[serde(default)]`
//! throughout on the reading side is what actually keeps the two in sync as
//! either side's fields evolve, not a version bump on every change -- `version`
//! exists for a genuine breaking change, which this isn't expected to need.
//!
//! The optional `prompt` field is the seed chat message enclosure-maker feeds
//! its AI assistant. When present and non-blank it wins over enclosure-maker's
//! own fallback formatter. Bancada builds it from the BOM and board as a brief
//! that follows `.claude/skills/parametric-enclosures` (geometry, cutouts,
//! sensors, wiring, lid register) and `.claude/skills/3d-printing` (house
//! printer profile, process, fasteners, printability).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::bom::{Bom, BomEntry};
use crate::boardprofile::{Board, UsbKind, UsbPort};
use crate::project::BoardChoice;
use crate::Result;

const HANDOFF_FILE: &str = "enclosure_handoff.json";
const HANDOFF_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnclosureHandoff {
    pub version: u32,
    pub project_name: String,
    pub source_dir: String,
    /// `None` when the board isn't `Recorded`/`Inferred` (i.e.
    /// `BoardChoice::Unchosen`/`NoProfile`) -- the BOM alone is still useful
    /// to hand off even without a resolved board.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<BoardSummary>,
    #[serde(default)]
    pub components: Vec<BomEntry>,
    /// Seed chat message for enclosure-maker's assistant. Built by
    /// [`build_seed_prompt`]; callers may replace it with a user-edited
    /// version before [`EnclosureHandoff::write`]. Omitted when blank so an
    /// older enclosure-maker can fall back to generating its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoardSummary {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub usb_ports: Vec<UsbPortSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsbPortSummary {
    pub label: String,
    /// `"native"` or `"bridge"` -- flattened from [`UsbKind`] since the
    /// bridge chip name isn't something an enclosure needs to know.
    pub kind: &'static str,
}

impl From<&Board> for BoardSummary {
    fn from(board: &Board) -> Self {
        BoardSummary {
            id: board.id.to_string(),
            name: board.name.to_string(),
            vendor: board.vendor.to_string(),
            usb_ports: board.usb.iter().map(UsbPortSummary::from).collect(),
        }
    }
}

impl From<&UsbPort> for UsbPortSummary {
    fn from(port: &UsbPort) -> Self {
        UsbPortSummary {
            label: port.label.to_string(),
            kind: match port.kind {
                UsbKind::Native => "native",
                UsbKind::Bridge { .. } => "bridge",
            },
        }
    }
}

impl EnclosureHandoff {
    /// Builds the hand-off from a project directory's own BOM and resolved
    /// board -- the same two facts [`crate::bom::Bom::load`] and
    /// [`crate::project::project_board`] already give the rest of the app.
    ///
    /// `prompt_override`, when `Some` and non-blank after trim, is stored as
    /// the seed message (the user edited it). Otherwise a fresh brief is
    /// generated from the board and BOM.
    pub fn build(
        project_dir: &Path,
        project_name: &str,
        bom: Option<Bom>,
        board_choice: &BoardChoice,
        prompt_override: Option<&str>,
    ) -> Self {
        let board_ref = match board_choice {
            BoardChoice::Recorded { board } | BoardChoice::Inferred { board } => Some(*board),
            BoardChoice::Unchosen { .. } | BoardChoice::NoProfile => None,
        };
        let components = bom.map(|b| b.components).unwrap_or_default();
        let prompt = prompt_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                let generated = build_seed_prompt(project_name, board_ref, &components);
                if generated.trim().is_empty() {
                    None
                } else {
                    Some(generated)
                }
            });
        EnclosureHandoff {
            version: HANDOFF_VERSION,
            project_name: project_name.to_string(),
            source_dir: project_dir.display().to_string(),
            board: board_ref.map(BoardSummary::from),
            components,
            prompt,
        }
    }

    /// Writes this hand-off to `<project_dir>/.bancada/enclosure_handoff.json`,
    /// creating the `.bancada` subdirectory if needed -- the same
    /// dotted-directory convention already used for vendored libraries
    /// ([`crate::ghlib::VENDOR_DIR`]).
    pub fn write(&self, project_dir: &Path) -> Result<PathBuf> {
        let dir = project_dir.join(".bancada");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(HANDOFF_FILE);
        let json = serde_json::to_string_pretty(self)
            .map_err(|source| crate::Error::Json { what: "enclosure handoff".to_string(), source })?;
        std::fs::write(&path, json)?;
        Ok(path)
    }
}

/// Formats the board + BOM as a brief for enclosure-maker's assistant.
///
/// Structure follows the parametric-enclosures skill brief (PCB, interfaces,
/// sensors, power/wiring, thermal, context, mounting, fasteners) and pins the
/// house print process from the 3d-printing skill. Assumptions the user did
/// not specify are tagged `ASSUMPTION:` so the assistant can correct them
/// later instead of inventing silently.
pub fn build_seed_prompt(project_name: &str, board: Option<&Board>, components: &[BomEntry]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Design a 3D-printable electronics enclosure for project \"{project_name}\".\n\n\
         Follow the parametric-enclosures and 3d-printing skills. Produce a \
         parametric `main.rhai` (base + lid) that prints without supports on \
         the house profile. Tag every value I did not specify with ASSUMPTION:\n\n"
    ));

    out.push_str("## PCB\n");
    match board {
        Some(b) => {
            out.push_str(&format!("- Board: {} ({}) — id `{}`\n", b.name, b.vendor, b.id));
            out.push_str(&format!("- Module: {}\n", b.module));
            if let Some(hint) = footprint_hint(b.id) {
                out.push_str(&format!("- {hint}\n"));
            } else {
                out.push_str(
                    "- ASSUMPTION: outline and mounting holes unknown — measure or \
                     use a known footprint table and tag ASSUMPTION.\n",
                );
            }
            out.push_str("- ASSUMPTION: PCB thickness 1.6 mm unless the footprint says otherwise.\n");
            if b.led.is_some() {
                out.push_str("- Onboard LED present — leave a light pipe or a small window if useful.\n");
            }
            if b.boot_button.is_some() {
                out.push_str("- Onboard BOOT button present — decide if it needs an external presser.\n");
            }
            if !b.notes.is_empty() {
                out.push_str("- Board notes:\n");
                for n in b.notes {
                    out.push_str(&format!("  - {n}\n"));
                }
            }
        }
        None => {
            out.push_str(
                "- Board: not identified. Infer a sensible enclosure from the \
                 components below; tag every geometric guess ASSUMPTION.\n",
            );
        }
    }

    out.push_str("\n## Interfaces\n");
    if let Some(b) = board {
        if b.usb.is_empty() {
            out.push_str("- No USB ports listed on the board profile.\n");
        } else {
            out.push_str("- Board USB ports (need wall cutouts a plug can reach; recess for overmold):\n");
            for p in b.usb {
                let kind = match p.kind {
                    UsbKind::Native => "native",
                    UsbKind::Bridge { chip } => chip,
                };
                out.push_str(&format!("  - {} ({kind})\n", p.label));
            }
            out.push_str(
                "- ASSUMPTION: USB sits on a short edge; put the board against that \
                 wall and move screw columns to the other sides.\n",
            );
        }
    }
    let interface_lines = classify_interfaces(components);
    if interface_lines.is_empty() && board.map(|b| b.usb.is_empty()).unwrap_or(true) {
        out.push_str("- No connectors, buttons, displays, or cable exits named in the BOM.\n");
    } else {
        for line in interface_lines {
            out.push_str(&format!("- {line}\n"));
        }
    }

    out.push_str("\n## Sensors\n");
    let sensors = classify_sensors(components);
    if sensors.is_empty() {
        out.push_str("- None named. Skip sensor chambers.\n");
    } else {
        out.push_str(
            "- Isolate each in its own vented chamber, thermally decoupled from heat sources:\n",
        );
        for line in sensors {
            out.push_str(&format!("  - {line}\n"));
        }
    }

    out.push_str("\n## Power and wiring\n");
    let has_external_cable = components.iter().any(looks_like_external_cable);
    let has_battery = components.iter().any(looks_like_battery);
    let has_switched_load = components.iter().any(looks_like_switched_load);
    let multi_module = components.len() > 1;
    if components.is_empty() {
        out.push_str("- No BOM. ASSUMPTION: USB 5 V, one board, no cables leaving the box.\n");
    } else {
        out.push_str("- Components and pin map:\n");
        for c in components {
            out.push_str(&format!("  - {} ×{}: {}", c.ref_, c.qty, c.value));
            if let Some(pkg) = &c.package {
                out.push_str(&format!(" [{pkg}]"));
            }
            if let Some(desc) = &c.description {
                out.push_str(&format!(" — {desc}"));
            } else if let Some(notes) = &c.notes {
                out.push_str(&format!(" — {notes}"));
            }
            out.push('\n');
            for w in &c.wiring {
                out.push_str(&format!("    - {}", w.pin));
                if let Some(gpio) = w.gpio {
                    out.push_str(&format!(" → GPIO{gpio}"));
                }
                if let Some(rail) = &w.rail {
                    out.push_str(&format!(" → {rail}"));
                }
                if let Some(notes) = &w.notes {
                    out.push_str(&format!(" ({notes})"));
                }
                out.push('\n');
            }
        }
        if multi_module || has_external_cable || has_battery || has_switched_load {
            out.push_str(
                "- More than one module / battery / switched load / cable out: include a \
                 power budget, fuse at the source, wiring bay on a free long side, \
                 zip-tie anchor at each exit, and an anti-chafe cable hole.\n",
            );
        } else {
            out.push_str(
                "- ASSUMPTION: USB 5 V supply, no cables leaving the box beyond the board USB.\n",
            );
        }
        if has_battery {
            out.push_str("- Battery present: tool-required compartment; no detachable small parts.\n");
        }
    }

    out.push_str("\n## Thermal\n");
    if components.iter().any(looks_like_heat_source) {
        out.push_str(
            "- Heat source named in the BOM. Add a chimney: inlets in the lowest 25% of \
             one wall, outlets in the top 25% of the opposite wall or lid. Slot vents \
             1.5–2.5 mm wide.\n",
        );
    } else {
        out.push_str("- ASSUMPTION: dissipation < 0.3 W — vents optional unless a heat source appears.\n");
    }

    out.push_str("\n## Context\n");
    out.push_str(
        "- ASSUMPTION: indoor, dry, desk/shelf use; not reachable by children or pets. \
         If that is wrong, tighten openings (≤ 4 mm, no straight line to the PCB) and \
         revisit battery rules.\n",
    );

    out.push_str("\n## Mounting\n");
    out.push_str("- ASSUMPTION: desk feet (Ø 8 × 1 mm recesses). No wall/DIN mount unless asked.\n");

    out.push_str("\n## Fasteners\n");
    out.push_str(
        "- Default: M3 heat-set inserts in gusseted, filleted columns that run floor to \
         mating face and avoid the PCB footprint. Lid clearance holes + counterbore. \
         No snap-fits on the house material.\n",
    );

    out.push_str("\n## Manufacturing (house profile)\n");
    out.push_str(
        "- Process: FDM on Bambu Lab A1 Mini + PETG-CF (do not ask to confirm).\n\
         - Design envelope: 170 × 170 × 170 mm.\n\
         - Wall = 2.52 mm (6 × 0.42 ew); floor/ceiling 3.0 mm; max bridge 5 mm; \
         overhang ≤ 45° from vertical.\n\
         - Supports are a defect: every part prints on a flat face with nothing under it.\n\
         - Body and lid locate each other with tongue-and-groove (or internal lip) and \
         slip clearance (~0.25 mm per side). A rim-to-rim plate is not a fit.\n\
         - RF: 20 mm antenna keepout from inserts/columns/ribs; keep plastic thin in \
         front of the antenna. PETG-CF attenuates 2.4 GHz — note an RSSI check.\n\
         - No elastic features (snaps, living hinges) in PETG-CF.\n\
         - Chamfer bed-parallel edges; fillet vertical and loaded internal corners R ≥ 1 mm.\n\
         - Nothing solid thicker than ~4 mm — hollow and rib.\n",
    );

    out.push_str("\n## Deliverable\n");
    out.push_str(
        "- Write `main.rhai` with `emit(\"base\", …)` and `emit(\"lid\", …)`, plus \
         preview `view(\"assembly\"|\"exploded\"|\"section\", …)`.\n\
         - Expose key dimensions with `param(...)`.\n\
         - Prefer `screw_boss`, `heat_set_bore`, `pcb_standoff`, `vent_slot_row` from \
         the hardware library.\n\
         - Cutouts referenced from the PCB origin / board edge so plugs reach their \
         sockets (overmold recess when the wall is thick).\n\
         - Reply with ASSUMPTIONS and a self-check: (a) no supports (b) overhangs ≤ 45° \
         (c) gusseted columns clear of the board (d) sensor chambers if any (e) antenna \
         keepout (f) plug reach (g) lid register with slip clearance (h) fits 170³ mm.\n\
         - Ask only if PCB outline is missing and not a known board; otherwise pick \
         defensible defaults and tag them.\n",
    );

    out
}

/// Footprint hints aligned with parametric-enclosures `pcb-and-components.md` §4.
fn footprint_hint(board_id: &str) -> Option<&'static str> {
    match board_id {
        "esp32-s3-devkitc-1" | "esp32-c6-devkitc-1" => Some(
            "ASSUMPTION: ESP32 DevKitC-class outline ≈ 54–55 × 28 mm, no mounting holes \
             (rails + stops); USB on a short edge; chip antenna on the opposite short \
             end — verify with calipers, clones vary",
        ),
        "esp32-doit-devkit-v1" => Some(
            "ASSUMPTION: DOIT ESP32 30-pin DevKit ≈ 51–52 × 28 mm, usually no holes \
             (some clones have 4 × Ø2.5); USB on a short edge; antenna opposite — \
             verify with calipers",
        ),
        _ => None,
    }
}

fn component_blob(c: &BomEntry) -> String {
    let mut s = format!("{} {} ", c.ref_, c.value);
    if let Some(d) = &c.description {
        s.push_str(d);
        s.push(' ');
    }
    if let Some(n) = &c.notes {
        s.push_str(n);
    }
    s.to_ascii_lowercase()
}

fn classify_sensors(components: &[BomEntry]) -> Vec<String> {
    const KEYS: &[&str] = &[
        "bme280", "bme680", "bmp280", "dht11", "dht22", "sht31", "sht40", "htu21",
        "ds18b20", "pir", "hc-sr501", "hc-sr04", "vl53", "mpu6050", "mpu9250",
        "soil", "moisture", "mq-", "mq135", "ccs811", "sgp30", "pms5003", "sds011",
        "microphone", "ics-43434", "inmp441", "ambient light", "bh1750", "tsl2561",
        "sensor",
    ];
    components
        .iter()
        .filter(|c| {
            let blob = component_blob(c);
            KEYS.iter().any(|k| blob.contains(k))
        })
        .map(|c| {
            let mut line = format!("{}: {}", c.ref_, c.value);
            if let Some(d) = &c.description {
                line.push_str(&format!(" — {d}"));
            }
            line.push_str(
                " (ASSUMPTION: sense room/air/subject outside the electronics bay; \
                 give the chamber the aperture that sensor needs)",
            );
            line
        })
        .collect()
}

fn classify_interfaces(components: &[BomEntry]) -> Vec<String> {
    const KEYS: &[&str] = &[
        "oled", "lcd", "tft", "display", "ssd1306", "st7789",
        "button", "switch", "encoder", "potentiometer", "keypad",
        "usb", "jack", "barrel", "connector", "header", "jst", "xh-",
        "led", "neopixel", "ws2812", "relay", "servo", "buzzer",
    ];
    components
        .iter()
        .filter(|c| {
            let blob = component_blob(c);
            // Skip pure MCU modules already covered by the board section.
            if blob.contains("esp32") || blob.contains("wroom") || blob.contains("mcu") {
                return false;
            }
            KEYS.iter().any(|k| blob.contains(k))
        })
        .map(|c| {
            let mut line = format!("{} ×{}: {}", c.ref_, c.qty, c.value);
            if let Some(d) = &c.description {
                line.push_str(&format!(" — {d}"));
            }
            line.push_str(" — needs a cutout, window, presser, or clearance");
            line
        })
        .collect()
}

fn looks_like_battery(c: &BomEntry) -> bool {
    let blob = component_blob(c);
    ["battery", "lipo", "li-ion", "18650", "21700", "powerbank"]
        .iter()
        .any(|k| blob.contains(k))
}

fn looks_like_switched_load(c: &BomEntry) -> bool {
    let blob = component_blob(c);
    ["relay", "mosfet", "ssr", "motor", "pump", "solenoid", "led strip", "ws2812b strip"]
        .iter()
        .any(|k| blob.contains(k))
}

fn looks_like_external_cable(c: &BomEntry) -> bool {
    let blob = component_blob(c);
    blob.contains("cable")
        || blob.contains("lead")
        || blob.contains("probe")
        || c.notes.as_deref().is_some_and(|n| n.to_ascii_lowercase().contains("leave"))
        || c.description
            .as_deref()
            .is_some_and(|d| d.to_ascii_lowercase().contains("external"))
}

fn looks_like_heat_source(c: &BomEntry) -> bool {
    let blob = component_blob(c);
    ["regulator", "buck", "boost", "ldo", "heater", "h-bridge", "motor driver", "dissipat"]
        .iter()
        .any(|k| blob.contains(k))
        || c.notes
            .as_deref()
            .is_some_and(|n| n.to_ascii_lowercase().contains("watt") || n.contains('W'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bom::WiringEntry;
    use crate::boardprofile::Board;
    use tempfile::tempdir;

    fn sample_bom() -> Bom {
        Bom {
            components: vec![BomEntry {
                qty: 1,
                ref_: "U1".into(),
                value: "ESP32-C6-WROOM-1".into(),
                package: Some("SMD".into()),
                description: Some("main MCU".into()),
                wiring: vec![WiringEntry {
                    pin: "IO4".into(),
                    gpio: Some(4),
                    notes: Some("I2C SDA".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    fn sensor_bom() -> Bom {
        Bom {
            components: vec![
                BomEntry {
                    qty: 1,
                    ref_: "U1".into(),
                    value: "ESP32-C6-WROOM-1".into(),
                    description: Some("main MCU".into()),
                    ..Default::default()
                },
                BomEntry {
                    qty: 1,
                    ref_: "U2".into(),
                    value: "BME280".into(),
                    description: Some("temp/humidity/pressure".into()),
                    wiring: vec![
                        WiringEntry {
                            pin: "SDA".into(),
                            gpio: Some(4),
                            ..Default::default()
                        },
                        WiringEntry {
                            pin: "VCC".into(),
                            rail: Some("3V3".into()),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
            ],
        }
    }

    fn known_c6() -> &'static Board {
        Board::from_id("esp32-c6-devkitc-1").expect("c6 profile")
    }

    #[test]
    fn build_with_no_profile_board_has_no_board_but_keeps_components() {
        let handoff = EnclosureHandoff::build(
            Path::new("/tmp/soil-sensor"),
            "soil-sensor",
            Some(sample_bom()),
            &BoardChoice::NoProfile,
            None,
        );
        assert_eq!(handoff.board, None);
        assert_eq!(handoff.components.len(), 1);
        assert_eq!(handoff.components[0].ref_, "U1");
        assert!(handoff.prompt.as_ref().unwrap().contains("soil-sensor"));
    }

    #[test]
    fn build_with_unchosen_board_has_no_board() {
        let handoff = EnclosureHandoff::build(
            Path::new("/tmp/soil-sensor"),
            "soil-sensor",
            None,
            &BoardChoice::Unchosen { candidates: Vec::new() },
            None,
        );
        assert_eq!(handoff.board, None);
        assert!(handoff.components.is_empty());
    }

    #[test]
    fn write_produces_valid_json_with_expected_shape() {
        let tmp = tempdir().unwrap();
        let handoff = EnclosureHandoff::build(
            tmp.path(),
            "soil-sensor",
            Some(sample_bom()),
            &BoardChoice::NoProfile,
            None,
        );
        let path = handoff.write(tmp.path()).unwrap();
        assert_eq!(path, tmp.path().join(".bancada").join("enclosure_handoff.json"));

        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["project_name"], "soil-sensor");
        assert_eq!(value["components"][0]["ref"], "U1");
        assert_eq!(value["components"][0]["wiring"][0]["gpio"], 4);
        assert!(value.get("board").is_none(), "board should be omitted, not null, when there's no profile");
        assert!(
            value["prompt"].as_str().unwrap().contains("Bambu Lab A1 Mini"),
            "prompt must name the house print profile"
        );
    }

    #[test]
    fn seed_prompt_covers_brief_sections_and_house_profile() {
        let prompt = build_seed_prompt("soil-sensor", Some(known_c6()), &sensor_bom().components);
        for needle in [
            "## PCB",
            "## Interfaces",
            "## Sensors",
            "## Power and wiring",
            "## Thermal",
            "## Context",
            "## Mounting",
            "## Fasteners",
            "## Manufacturing (house profile)",
            "## Deliverable",
            "ESP32-C6-DevKitC-1",
            "BME280",
            "sensor chambers",
            "PETG-CF",
            "tongue-and-groove",
            "heat-set",
            "ASSUMPTION:",
            "main.rhai",
            "170 × 170 × 170",
        ] {
            assert!(prompt.contains(needle), "missing `{needle}` in:\n{prompt}");
        }
    }

    #[test]
    fn seed_prompt_uses_devkit_footprint_assumption_for_known_boards() {
        let prompt = build_seed_prompt("x", Some(known_c6()), &[]);
        assert!(prompt.contains("DevKitC-class") || prompt.contains("54–55"));
        assert!(prompt.contains("no mounting holes") || prompt.contains("rails"));
    }

    #[test]
    fn prompt_override_wins_over_generated_brief() {
        let handoff = EnclosureHandoff::build(
            Path::new("/tmp/soil-sensor"),
            "soil-sensor",
            Some(sample_bom()),
            &BoardChoice::NoProfile,
            Some("  Make it round.  "),
        );
        assert_eq!(handoff.prompt.as_deref(), Some("Make it round."));
    }

    #[test]
    fn blank_prompt_override_falls_back_to_generated() {
        let handoff = EnclosureHandoff::build(
            Path::new("/tmp/soil-sensor"),
            "soil-sensor",
            Some(sample_bom()),
            &BoardChoice::NoProfile,
            Some("   "),
        );
        assert!(handoff.prompt.as_ref().unwrap().contains("## PCB"));
    }
}
