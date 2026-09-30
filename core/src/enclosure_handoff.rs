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
    pub fn build(project_dir: &Path, project_name: &str, bom: Option<Bom>, board_choice: &BoardChoice) -> Self {
        let board = match board_choice {
            BoardChoice::Recorded { board } | BoardChoice::Inferred { board } => Some(BoardSummary::from(*board)),
            BoardChoice::Unchosen { .. } | BoardChoice::NoProfile => None,
        };
        EnclosureHandoff {
            version: HANDOFF_VERSION,
            project_name: project_name.to_string(),
            source_dir: project_dir.display().to_string(),
            board,
            components: bom.map(|b| b.components).unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bom::WiringEntry;
    use tempfile::tempdir;

    fn sample_bom() -> Bom {
        Bom {
            components: vec![BomEntry {
                qty: 1,
                ref_: "U1".into(),
                value: "ESP32-C6-WROOM-1".into(),
                package: Some("SMD".into()),
                description: Some("main MCU".into()),
                wiring: vec![WiringEntry { pin: "IO4".into(), gpio: Some(4), notes: Some("I2C SDA".into()), ..Default::default() }],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn build_with_no_profile_board_has_no_board_but_keeps_components() {
        let handoff = EnclosureHandoff::build(
            Path::new("/tmp/soil-sensor"),
            "soil-sensor",
            Some(sample_bom()),
            &BoardChoice::NoProfile,
        );
        assert_eq!(handoff.board, None);
        assert_eq!(handoff.components.len(), 1);
        assert_eq!(handoff.components[0].ref_, "U1");
    }

    #[test]
    fn build_with_unchosen_board_has_no_board() {
        let handoff = EnclosureHandoff::build(
            Path::new("/tmp/soil-sensor"),
            "soil-sensor",
            None,
            &BoardChoice::Unchosen { candidates: Vec::new() },
        );
        assert_eq!(handoff.board, None);
        assert!(handoff.components.is_empty());
    }

    #[test]
    fn write_produces_valid_json_with_expected_shape() {
        let tmp = tempdir().unwrap();
        let handoff = EnclosureHandoff::build(tmp.path(), "soil-sensor", Some(sample_bom()), &BoardChoice::NoProfile);
        let path = handoff.write(tmp.path()).unwrap();
        assert_eq!(path, tmp.path().join(".bancada").join("enclosure_handoff.json"));

        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["project_name"], "soil-sensor");
        assert_eq!(value["components"][0]["ref"], "U1");
        assert_eq!(value["components"][0]["wiring"][0]["gpio"], 4);
        assert!(value.get("board").is_none(), "board should be omitted, not null, when there's no profile");
    }
}
