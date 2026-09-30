//! Bill of materials: `bom.yaml` in the sketch directory.
//!
//! The file travels with the project in git and is readable by the Agent.
//! Absence is not an error — projects that were created before this feature
//! (or that simply have no BOM) return `Ok(None)` from `load`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

pub const BOM_FILE: &str = "bom.yaml";

fn bom_path(sketch_dir: &Path) -> PathBuf {
    sketch_dir.join(BOM_FILE)
}

/// The full bill of materials for a project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Bom {
    #[serde(default)]
    pub components: Vec<BomEntry>,
}

/// One line item in the BOM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BomEntry {
    pub qty: u32,
    /// Reference designator(s), e.g. `"R1"` or `"R1,R2,R3"`.
    #[serde(rename = "ref")]
    pub ref_: String,
    /// Component value or name, e.g. `"10k"` or `"ESP32-S3-WROOM-1"`.
    pub value: String,
    /// PCB package / footprint, e.g. `"0402"` or `"DIP-8"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Supplier name, e.g. `"LCSC"` or `"Mouser"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supplier: Option<String>,
    /// Supplier part number, e.g. `"C25804"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_no: Option<String>,
    /// Free-text notes for this line item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Bom {
    /// Read `bom.yaml` from `sketch_dir`.
    ///
    /// Returns `Ok(None)` when the file does not exist — a project that has
    /// never had a BOM is valid, not an error.
    pub fn load(sketch_dir: &Path) -> Result<Option<Self>> {
        let p = bom_path(sketch_dir);
        match std::fs::read_to_string(&p) {
            Ok(s) => Ok(Some(serde_yaml::from_str(&s)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Write `bom.yaml` into `sketch_dir`, creating the file if absent.
    pub fn save(&self, sketch_dir: &Path) -> Result<()> {
        std::fs::write(bom_path(sketch_dir), serde_yaml::to_string(self)?)?;
        Ok(())
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> Bom {
        Bom {
            components: vec![
                BomEntry {
                    qty: 1,
                    ref_: "U1".into(),
                    value: "ESP32-S3-WROOM-1".into(),
                    package: Some("SMD".into()),
                    supplier: Some("LCSC".into()),
                    part_no: Some("C528945".into()),
                    notes: Some("main MCU".into()),
                },
                BomEntry {
                    qty: 3,
                    ref_: "R1,R2,R3".into(),
                    value: "10k".into(),
                    package: Some("0402".into()),
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn round_trip_through_save_and_load() {
        let tmp = tempdir().unwrap();
        let bom = sample();
        bom.save(tmp.path()).unwrap();
        let back = Bom::load(tmp.path()).unwrap().expect("should exist");
        assert_eq!(back, bom);
    }

    #[test]
    fn missing_file_returns_none() {
        let tmp = tempdir().unwrap();
        assert_eq!(Bom::load(tmp.path()).unwrap(), None);
    }

    #[test]
    fn optional_fields_absent_still_parse() {
        let tmp = tempdir().unwrap();
        let yaml = "components:\n  - qty: 2\n    ref: C1\n    value: 100nF\n";
        std::fs::write(tmp.path().join(BOM_FILE), yaml).unwrap();
        let bom = Bom::load(tmp.path()).unwrap().unwrap();
        assert_eq!(bom.components.len(), 1);
        assert_eq!(bom.components[0].package, None);
        assert_eq!(bom.components[0].supplier, None);
    }

    #[test]
    fn empty_components_list_round_trips() {
        let tmp = tempdir().unwrap();
        let bom = Bom::default();
        bom.save(tmp.path()).unwrap();
        let back = Bom::load(tmp.path()).unwrap().unwrap();
        assert!(back.components.is_empty());
    }
}
