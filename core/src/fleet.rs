//! A persistent registry of the physical boards Bancada has seen.
//!
//! Ports are ephemeral — `/dev/ttyACM0` is whichever board enumerated first —
//! so the registry keys on something intrinsic to the board. On an ESP32 with
//! native USB that is free: arduino-cli reports the chip's MAC address as the
//! USB `serialNumber`, so a board is identified just by being plugged in, with
//! no esptool, no port takeover and no bootloader reset.
//!
//! Pure: no subprocess, no filesystem beyond an explicit path, and **no clock**.
//! `now` is always a parameter so every operation is deterministic under test;
//! the Tauri layer supplies `SystemTime`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::{DetectedPort, Port};
use crate::{Error, Result};

/// Bumped only for a breaking change to the on-disk shape. Additive fields ride
/// on `#[serde(default)]` instead, the same way `AppSettings` grows.
pub const FLEET_VERSION: u32 = 1;

/// How confidently a port identifies one specific physical board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardIdKind {
    /// A hardware MAC address — the strongest identity available.
    Mac,
    /// A per-unit USB serial that is not a MAC. Unique, but vendor-defined.
    Serial,
}

/// The identity of a detected port, when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardId {
    pub kind: BoardIdKind,
    pub value: String,
}

/// Six colon- or dash-separated hex pairs, e.g. `44:1B:F6:CE:A3:B8`.
///
/// Deliberately strict. A USB serial like `3477325620` or a bridge's `0001`
/// must not be mistaken for a hardware address, because the two are treated
/// differently: a MAC is stable across re-flashing and re-cabling, a vendor
/// serial is only as trustworthy as the vendor.
pub fn looks_like_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.trim().split([':', '-']).collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Canonical MAC form: lower case, colon-separated.
///
/// Stored normalised so the same board found via `board list` (upper case, from
/// the USB descriptor) and via esptool (lower case) is one record, not two.
pub fn normalize_mac(s: &str) -> String {
    s.trim().replace('-', ":").to_ascii_lowercase()
}

/// Identify a port, or `None` when it carries nothing board-specific.
///
/// `properties.serialNumber` is preferred over `hardware_id`: for a USB-serial
/// bridge, `hardware_id` is the *bridge's* serial (commonly `"0001"`) and would
/// collide across every board behind that bridge model. `serialNumber` is
/// absent in exactly that case, which is the signal we want.
pub fn identify(port: &Port) -> Option<BoardId> {
    let serial = port
        .properties
        .get("serialNumber")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())?;

    if looks_like_mac(serial) {
        Some(BoardId {
            kind: BoardIdKind::Mac,
            value: normalize_mac(serial),
        })
    } else {
        Some(BoardId {
            kind: BoardIdKind::Serial,
            value: serial.to_string(),
        })
    }
}

/// The board name to record, but only when arduino-cli actually identified
/// *this* board rather than its family.
///
/// A match set containing a hidden umbrella entry — `esp32:esp32:esp32_family`
/// — means the match was made on a family-wide USB vid/pid, not a unique board
/// id. Every ESP32-S3 with a native-USB bridge produces the same set, so the
/// non-hidden sibling arduino-cli offers alongside it is an arbitrary member of
/// that family: a plain dev board gets confidently labelled "Ozobot DRVKit".
/// Several non-hidden matches mean the same thing.
///
/// Being wrong here is worse than being silent, because the name is what the
/// user reads to tell one board from another. So an ambiguous match records no
/// name at all and [`FleetEntry::display_name`] falls back to the chip type or
/// the MAC — both of which are true.
pub fn board_name(dp: &DetectedPort) -> Option<&str> {
    if dp.matching_boards.iter().any(|b| b.is_hidden) {
        return None;
    }
    let mut named = dp.matching_boards.iter().filter(|b| !b.name.is_empty());
    let first = named.next()?;
    if named.next().is_some() {
        return None;
    }
    Some(first.name.as_str())
}

/// What was last flashed to a board, and from where.
///
/// The board itself cannot tell you what is running on it, so the registry
/// remembers: plugging a board in then leads back to the project that produced
/// its firmware. `tag` is the `flash/<timestamp>` tag written into that project
/// at flash time, so the exact tree is recoverable even after the branch moves
/// on; `commit` is kept alongside it because a tag can be deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlashRecord {
    /// Absolute path of the project directory that was flashed.
    pub project_dir: String,
    /// The git tag written at flash time, e.g. `flash/2026-08-12T1430`.
    pub tag: String,
    /// Absent on a detached HEAD, so optional — and defaulted, because a record
    /// written before this field existed must still parse.
    #[serde(default)]
    pub branch: Option<String>,
    /// HEAD sha at flash time.
    pub commit: String,
    /// Epoch seconds. Supplied by the caller, never read from a clock here.
    pub at: u64,
}

/// One remembered board. Every field beyond the id is optional or defaulted so
/// the file survives being written by an older build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetEntry {
    /// Normalised MAC, or the USB serial when no MAC is known.
    pub id: String,
    pub id_kind: BoardIdKind,
    /// What the user calls this board. The whole point of the registry.
    #[serde(default)]
    pub nickname: Option<String>,
    /// From esptool, once the user has run Identify.
    #[serde(default)]
    pub chip_type: Option<String>,
    /// Best name arduino-cli has offered for it.
    #[serde(default)]
    pub board_name: Option<String>,
    /// Every FQBN this board has been built for, in first-seen order.
    #[serde(default)]
    pub fqbns: Vec<String>,
    #[serde(default)]
    pub last_port: Option<String>,
    #[serde(default)]
    pub vid: Option<String>,
    #[serde(default)]
    pub pid: Option<String>,
    /// The most recent flash, and only that one: the `flash/*` tags in the
    /// project are the history, this is just the pointer back into it.
    #[serde(default)]
    pub last_flash: Option<FlashRecord>,
    /// Epoch seconds. Supplied by the caller, never read from a clock here.
    pub first_seen: u64,
    pub last_seen: u64,
}

impl FleetEntry {
    /// What to show as the board's name, in decreasing order of usefulness.
    pub fn display_name(&self) -> &str {
        self.nickname
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(self.board_name.as_deref().filter(|s| !s.is_empty()))
            .or(self.chip_type.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or(&self.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fleet {
    #[serde(default = "fleet_version")]
    pub version: u32,
    #[serde(default)]
    pub boards: Vec<FleetEntry>,
}

fn fleet_version() -> u32 {
    FLEET_VERSION
}

impl Default for Fleet {
    fn default() -> Self {
        Self {
            version: FLEET_VERSION,
            boards: Vec::new(),
        }
    }
}

impl Fleet {
    /// A missing file is an empty fleet; a corrupt one is an **error**.
    ///
    /// Deliberately unlike `settings::load`, which swallows corruption to keep
    /// startup unblockable. Here the file *is* the user's accumulated record of
    /// their hardware, and silently replacing it with an empty list would
    /// destroy it on the next save. Erroring is safe because the fleet is read
    /// when its panel opens, not during startup.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|source| Error::Json {
            what: format!("the board fleet file at {}", path.display()),
            source,
        })
    }

    /// Atomic write (temp file + rename), matching `settings::save`.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|source| Error::Json {
            what: "the board fleet".to_string(),
            source,
        })?;
        crate::replace_file_atomically(path, &text)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&FleetEntry> {
        self.boards.iter().find(|b| b.id == id)
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.boards.iter().position(|b| b.id == id)
    }

    /// Record that a board was seen. Returns true when it was new.
    ///
    /// Updates in place, never reorders — a re-sighting should read as an edit
    /// rather than jumping the board to the end of the list. `first_seen` is
    /// preserved; only `last_seen` and the mutable details move.
    pub fn sight(&mut self, id: &BoardId, port: &Port, name: Option<&str>, now: u64) -> bool {
        let vid = port.properties.get("vid").cloned();
        let pid = port.properties.get("pid").cloned();
        let address = (!port.address.is_empty()).then(|| port.address.clone());

        match self.index_of(&id.value) {
            Some(i) => {
                let e = &mut self.boards[i];
                e.last_seen = now;
                if address.is_some() {
                    e.last_port = address;
                }
                // Never downgrade a known name to nothing.
                if let Some(n) = name.filter(|n| !n.is_empty()) {
                    e.board_name = Some(n.to_string());
                }
                if vid.is_some() {
                    e.vid = vid;
                }
                if pid.is_some() {
                    e.pid = pid;
                }
                false
            }
            None => {
                self.boards.push(FleetEntry {
                    id: id.value.clone(),
                    id_kind: id.kind,
                    nickname: None,
                    chip_type: None,
                    board_name: name.filter(|n| !n.is_empty()).map(str::to_string),
                    fqbns: Vec::new(),
                    last_port: address,
                    vid,
                    pid,
                    // A sighting learns nothing about firmware; only a flash
                    // fills this in, and re-sighting must never clear it.
                    last_flash: None,
                    first_seen: now,
                    last_seen: now,
                });
                true
            }
        }
    }

    /// Set or clear a nickname. An empty string clears it rather than storing
    /// a blank, so `display_name` falls back cleanly.
    pub fn set_nickname(&mut self, id: &str, nickname: Option<&str>) -> Result<()> {
        let i = self
            .index_of(id)
            .ok_or_else(|| Error::Other(format!("no board with id `{id}` in the fleet")))?;
        self.boards[i].nickname = nickname
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Ok(())
    }

    /// Record an FQBN this board has been built for. Deduped, order preserved.
    pub fn note_fqbn(&mut self, id: &str, fqbn: &str) -> Result<()> {
        let fqbn = fqbn.trim();
        if fqbn.is_empty() {
            return Ok(());
        }
        let i = self
            .index_of(id)
            .ok_or_else(|| Error::Other(format!("no board with id `{id}` in the fleet")))?;
        let e = &mut self.boards[i];
        if !e.fqbns.iter().any(|f| f == fqbn) {
            e.fqbns.push(fqbn.to_string());
        }
        Ok(())
    }

    /// Record what was just flashed to a board, replacing any previous record.
    ///
    /// Only the latest is kept. The history lives in the project as `flash/*`
    /// tags, which are durable and shareable; duplicating it per board would be
    /// an unbounded log inside a file that is rewritten on every hotplug.
    pub fn note_flash(&mut self, id: &str, rec: FlashRecord) -> Result<()> {
        let i = self
            .index_of(id)
            .ok_or_else(|| Error::Other(format!("no board with id `{id}` in the fleet")))?;
        self.boards[i].last_flash = Some(rec);
        Ok(())
    }

    /// Follow a project that moved. Returns how many boards were repointed.
    ///
    /// Without this, renaming a project leaves every board it was flashed from
    /// pointing at a path that no longer exists, and the way back to the source
    /// of the running firmware is lost.
    ///
    /// Matching is **exact**. Rewriting descendants would mean guessing where
    /// one project ends and a nested sketch begins, and guessing wrong points a
    /// board at a directory that was never flashed — worse than a stale path,
    /// which at least fails visibly.
    pub fn repoint_project(&mut self, old_dir: &str, new_dir: &str) -> usize {
        let mut changed = 0;
        for e in &mut self.boards {
            if let Some(rec) = e.last_flash.as_mut() {
                if rec.project_dir == old_dir {
                    rec.project_dir = new_dir.to_string();
                    changed += 1;
                }
            }
        }
        changed
    }

    /// Fold what esptool learned into the registry.
    ///
    /// When the board was already known only by a vendor serial, its record is
    /// *migrated* to the MAC rather than duplicated: the nickname and history
    /// the user built up must survive learning a better identity. If a MAC
    /// record already exists, the two are merged into it and the serial record
    /// is dropped.
    pub fn merge_identified(
        &mut self,
        previous_id: Option<&str>,
        mac: &str,
        chip_type: Option<&str>,
        now: u64,
    ) -> Result<String> {
        if !looks_like_mac(mac) {
            return Err(Error::Other(format!(
                "`{mac}` is not a MAC address, so it cannot identify a board"
            )));
        }
        let mac = normalize_mac(mac);

        // Lift the old record out, if it was a different id.
        let old = previous_id
            .filter(|p| *p != mac)
            .and_then(|p| self.index_of(p))
            .map(|i| self.boards.remove(i));

        match self.index_of(&mac) {
            Some(i) => {
                let e = &mut self.boards[i];
                e.last_seen = now;
                if let Some(c) = chip_type.filter(|c| !c.is_empty()) {
                    e.chip_type = Some(c.to_string());
                }
                if let Some(old) = old {
                    // Prefer what is already on the MAC record; fill its gaps.
                    e.nickname = e.nickname.take().or(old.nickname);
                    e.board_name = e.board_name.take().or(old.board_name);
                    e.last_port = e.last_port.take().or(old.last_port);
                    e.vid = e.vid.take().or(old.vid);
                    e.pid = e.pid.take().or(old.pid);
                    e.last_flash = e.last_flash.take().or(old.last_flash);
                    e.first_seen = e.first_seen.min(old.first_seen);
                    for f in old.fqbns {
                        if !e.fqbns.contains(&f) {
                            e.fqbns.push(f);
                        }
                    }
                }
            }
            None => {
                let mut e = old.unwrap_or(FleetEntry {
                    id: mac.clone(),
                    id_kind: BoardIdKind::Mac,
                    nickname: None,
                    chip_type: None,
                    board_name: None,
                    fqbns: Vec::new(),
                    last_port: None,
                    vid: None,
                    pid: None,
                    last_flash: None,
                    first_seen: now,
                    last_seen: now,
                });
                e.id = mac.clone();
                e.id_kind = BoardIdKind::Mac;
                e.last_seen = now;
                if let Some(c) = chip_type.filter(|c| !c.is_empty()) {
                    e.chip_type = Some(c.to_string());
                }
                self.boards.push(e);
            }
        }
        Ok(mac)
    }

    /// Remove a board. Returns whether anything was removed.
    pub fn forget(&mut self, id: &str) -> bool {
        match self.index_of(id) {
            Some(i) => {
                self.boards.remove(i);
                true
            }
            None => false,
        }
    }
}

/// Record every identifiable port in one pass. Returns how many were new.
pub fn sight_all(fleet: &mut Fleet, ports: &[DetectedPort], now: u64) -> usize {
    let mut added = 0;
    for dp in ports {
        if let Some(id) = identify(&dp.port) {
            if fleet.sight(&id, &dp.port, board_name(dp), now) {
                added += 1;
            }
        }
    }
    added
}

/// What is physically attached right now, split by whether we can name it.
///
/// Derived here rather than in the frontend so the identity rules — which
/// `serialNumber` counts, and when `hardware_id` must be ignored — have one
/// implementation instead of a copy in TypeScript.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Attached {
    /// Fleet ids currently plugged in, so the panel can mark them online.
    pub online: Vec<String>,
    /// Attached serial ports carrying no board-specific identity. These are the
    /// ones worth offering an esptool "Identify" for.
    pub unidentified: Vec<DetectedPort>,
}

pub fn attached(ports: &[DetectedPort]) -> Attached {
    let mut out = Attached::default();
    for dp in ports.iter().filter(|dp| dp.port.protocol == "serial") {
        match identify(&dp.port) {
            Some(id) => out.online.push(id.value),
            None => out.unidentified.push(dp.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MatchingBoard;
    use std::collections::BTreeMap;

    /// The real ESP32 shape: MAC in `serialNumber`.
    fn esp_port() -> Port {
        let mut properties = BTreeMap::new();
        properties.insert("vid".to_string(), "0x303A".to_string());
        properties.insert("pid".to_string(), "0x1001".to_string());
        properties.insert("serialNumber".to_string(), "44:1B:F6:CE:A3:B8".to_string());
        Port {
            address: "/dev/ttyACM0".into(),
            protocol: "serial".into(),
            properties,
            hardware_id: "44:1B:F6:CE:A3:B8".into(),
            ..Default::default()
        }
    }

    /// The UNO Q shape: a per-unit serial that is not a MAC.
    fn serial_port() -> Port {
        let mut properties = BTreeMap::new();
        properties.insert("vid".to_string(), "0x2341".to_string());
        properties.insert("pid".to_string(), "0x0078".to_string());
        properties.insert("serialNumber".to_string(), "3477325620".to_string());
        Port {
            address: "/dev/ttyACM1".into(),
            protocol: "serial".into(),
            properties,
            hardware_id: "3477325620".into(),
            ..Default::default()
        }
    }

    /// The CP210x bridge shape: no serialNumber, bridge-owned hardware_id.
    fn bridge_port() -> Port {
        let mut properties = BTreeMap::new();
        properties.insert("vid".to_string(), "0x10C4".to_string());
        properties.insert("pid".to_string(), "0xEA60".to_string());
        Port {
            address: "/dev/ttyUSB0".into(),
            protocol: "serial".into(),
            properties,
            hardware_id: "0001".into(),
            ..Default::default()
        }
    }

    // ---------- identity ----------

    #[test]
    fn a_mac_is_six_hex_pairs() {
        assert!(looks_like_mac("44:1B:F6:CE:A3:B8"));
        assert!(looks_like_mac("44-1b-f6-ce-a3-b8"));
        assert!(looks_like_mac("  44:1b:f6:ce:a3:b8  "));
    }

    #[test]
    fn a_usb_serial_is_not_mistaken_for_a_mac() {
        // The distinction the whole registry rests on.
        assert!(!looks_like_mac("3477325620"));
        assert!(!looks_like_mac("0001"));
        assert!(!looks_like_mac(""));
        assert!(!looks_like_mac("44:1b:f6:ce:a3")); // five pairs
        assert!(!looks_like_mac("44:1b:f6:ce:a3:b8:c0")); // seven
        assert!(!looks_like_mac("44:1b:f6:ce:a3:zz")); // not hex
        assert!(!looks_like_mac("441bf6cea3b8")); // no separators
    }

    #[test]
    fn macs_normalise_to_one_canonical_form() {
        // board list gives upper case, esptool lower — they must be one record.
        assert_eq!(normalize_mac("44:1B:F6:CE:A3:B8"), "44:1b:f6:ce:a3:b8");
        assert_eq!(normalize_mac("44-1B-F6-CE-A3-B8"), "44:1b:f6:ce:a3:b8");
        assert_eq!(normalize_mac(" 44:1b:f6:ce:a3:b8 "), "44:1b:f6:ce:a3:b8");
    }

    #[test]
    fn identifies_an_esp32_by_mac_without_touching_it() {
        let id = identify(&esp_port()).expect("esp32 is identifiable");
        assert_eq!(id.kind, BoardIdKind::Mac);
        assert_eq!(id.value, "44:1b:f6:ce:a3:b8");
    }

    #[test]
    fn identifies_a_plain_serial_as_a_weaker_identity() {
        let id = identify(&serial_port()).expect("uno q is identifiable");
        assert_eq!(id.kind, BoardIdKind::Serial);
        assert_eq!(id.value, "3477325620");
    }

    #[test]
    fn a_bridge_is_not_identifiable() {
        // hardware_id is "0001" — the bridge's, shared by every board behind
        // that model, so keying on it would merge unrelated boards.
        assert!(identify(&bridge_port()).is_none());
    }

    #[test]
    fn an_empty_serial_number_is_not_an_identity() {
        let mut port = esp_port();
        port.properties
            .insert("serialNumber".to_string(), "   ".to_string());
        assert!(identify(&port).is_none());
    }

    /// Real capture from a native-USB ESP32: a hidden family umbrella plus one
    /// arbitrary non-hidden sibling. This exact set is what a plain ESP32-S3
    /// dev board produces.
    fn esp_family_match() -> Vec<MatchingBoard> {
        vec![
            MatchingBoard {
                name: "ESP32 Family Device".into(),
                fqbn: "esp32:esp32:esp32_family".into(),
                is_hidden: true,
            },
            MatchingBoard {
                name: "Ozobot DRVKit".into(),
                fqbn: "esp32:esp32:ozobot_drvkit".into(),
                is_hidden: false,
            },
        ]
    }

    #[test]
    fn a_family_level_match_records_no_board_name() {
        // The regression: naming this board "Ozobot DRVKit" is confidently
        // wrong, and the name is what the user reads to tell boards apart.
        let dp = DetectedPort {
            port: esp_port(),
            matching_boards: esp_family_match(),
        };
        assert_eq!(board_name(&dp), None);
    }

    #[test]
    fn a_hidden_only_match_records_no_board_name() {
        let dp = DetectedPort {
            port: esp_port(),
            matching_boards: vec![MatchingBoard {
                name: "ESP32 Family Device".into(),
                fqbn: "esp32:esp32:esp32_family".into(),
                is_hidden: true,
            }],
        };
        assert_eq!(board_name(&dp), None);
    }

    #[test]
    fn a_single_unambiguous_match_is_named() {
        // An Arduino UNO or UNO Q matches exactly one board and no umbrella —
        // that name is trustworthy and must still be recorded.
        let dp = DetectedPort {
            port: serial_port(),
            matching_boards: vec![MatchingBoard {
                name: "Arduino UNO Q".into(),
                fqbn: "arduino:zephyr:unoq".into(),
                is_hidden: false,
            }],
        };
        assert_eq!(board_name(&dp), Some("Arduino UNO Q"));
    }

    #[test]
    fn several_candidate_matches_are_too_ambiguous_to_name() {
        let dp = DetectedPort {
            port: esp_port(),
            matching_boards: vec![
                MatchingBoard {
                    name: "Board A".into(),
                    fqbn: "a:b:c".into(),
                    is_hidden: false,
                },
                MatchingBoard {
                    name: "Board B".into(),
                    fqbn: "a:b:d".into(),
                    is_hidden: false,
                },
            ],
        };
        assert_eq!(board_name(&dp), None);
    }

    #[test]
    fn no_matches_at_all_records_no_name() {
        let dp = DetectedPort {
            port: bridge_port(),
            matching_boards: vec![],
        };
        assert_eq!(board_name(&dp), None);
    }

    // ---------- sighting ----------

    fn fleet_with_esp(now: u64) -> (Fleet, BoardId) {
        let mut f = Fleet::default();
        let port = esp_port();
        let id = identify(&port).unwrap();
        assert!(f.sight(&id, &port, Some("Ozobot DRVKit"), now));
        (f, id)
    }

    #[test]
    fn a_first_sighting_creates_a_record() {
        let (f, _) = fleet_with_esp(1000);
        assert_eq!(f.boards.len(), 1);
        let e = &f.boards[0];
        assert_eq!(e.id, "44:1b:f6:ce:a3:b8");
        assert_eq!(e.id_kind, BoardIdKind::Mac);
        assert_eq!(e.board_name.as_deref(), Some("Ozobot DRVKit"));
        assert_eq!(e.last_port.as_deref(), Some("/dev/ttyACM0"));
        assert_eq!(e.vid.as_deref(), Some("0x303A"));
        assert_eq!(e.first_seen, 1000);
        assert_eq!(e.last_seen, 1000);
    }

    #[test]
    fn a_second_sighting_updates_without_duplicating() {
        let (mut f, id) = fleet_with_esp(1000);
        let mut port = esp_port();
        port.address = "/dev/ttyACM2".into(); // replugged elsewhere
        assert!(!f.sight(&id, &port, Some("Ozobot DRVKit"), 2000));

        assert_eq!(f.boards.len(), 1);
        let e = &f.boards[0];
        assert_eq!(e.first_seen, 1000, "first_seen must never move");
        assert_eq!(e.last_seen, 2000);
        assert_eq!(e.last_port.as_deref(), Some("/dev/ttyACM2"));
    }

    #[test]
    fn re_sighting_does_not_reorder_the_list() {
        let mut f = Fleet::default();
        let esp = esp_port();
        let ser = serial_port();
        let esp_id = identify(&esp).unwrap();
        let ser_id = identify(&ser).unwrap();
        f.sight(&esp_id, &esp, None, 10);
        f.sight(&ser_id, &ser, None, 20);
        // Touch the first one again; it must stay first.
        f.sight(&esp_id, &esp, None, 30);
        let ids: Vec<&str> = f.boards.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["44:1b:f6:ce:a3:b8", "3477325620"]);
    }

    #[test]
    fn a_sighting_never_erases_a_known_name() {
        let (mut f, id) = fleet_with_esp(1000);
        f.sight(&id, &esp_port(), None, 2000);
        assert_eq!(f.boards[0].board_name.as_deref(), Some("Ozobot DRVKit"));
    }

    #[test]
    fn sight_all_skips_unidentifiable_ports() {
        let mut f = Fleet::default();
        let ports = vec![
            DetectedPort {
                port: esp_port(),
                matching_boards: vec![],
            },
            DetectedPort {
                port: bridge_port(),
                matching_boards: vec![],
            },
            DetectedPort {
                port: serial_port(),
                matching_boards: vec![],
            },
        ];
        assert_eq!(sight_all(&mut f, &ports, 500), 2);
        assert_eq!(f.boards.len(), 2, "the bridge must not be recorded");
        // Idempotent: a second scan of the same ports adds nothing.
        assert_eq!(sight_all(&mut f, &ports, 600), 0);
        assert_eq!(f.boards.len(), 2);
    }

    #[test]
    fn attached_splits_identifiable_from_bridge_ports() {
        let ports = vec![
            DetectedPort {
                port: esp_port(),
                matching_boards: vec![],
            },
            DetectedPort {
                port: bridge_port(),
                matching_boards: vec![],
            },
        ];
        let a = attached(&ports);
        assert_eq!(a.online, ["44:1b:f6:ce:a3:b8"]);
        assert_eq!(a.unidentified.len(), 1);
        assert_eq!(a.unidentified[0].port.address, "/dev/ttyUSB0");
    }

    #[test]
    fn attached_ignores_non_serial_ports() {
        // The UNO Q also advertises itself over the network via mDNS; a network
        // port is the same board counted twice, and has no USB identity.
        let ports = vec![DetectedPort {
            port: Port {
                address: "2804:7f0::1".into(),
                protocol: "network".into(),
                ..Default::default()
            },
            matching_boards: vec![],
        }];
        let a = attached(&ports);
        assert!(a.online.is_empty());
        assert!(a.unidentified.is_empty(), "a network port is not a bridge");
    }

    // ---------- nickname, fqbns, display ----------

    #[test]
    fn nicknames_round_trip_and_can_be_cleared() {
        let (mut f, _) = fleet_with_esp(1);
        f.set_nickname("44:1b:f6:ce:a3:b8", Some("  bench probe  "))
            .unwrap();
        assert_eq!(f.boards[0].nickname.as_deref(), Some("bench probe"));
        f.set_nickname("44:1b:f6:ce:a3:b8", Some("")).unwrap();
        assert_eq!(
            f.boards[0].nickname, None,
            "a blank clears rather than stores"
        );
        f.set_nickname("44:1b:f6:ce:a3:b8", None).unwrap();
        assert_eq!(f.boards[0].nickname, None);
    }

    #[test]
    fn nicknaming_an_unknown_board_is_an_error() {
        let (mut f, _) = fleet_with_esp(1);
        let err = f.set_nickname("nope", Some("x")).unwrap_err().to_string();
        assert!(err.contains("no board with id `nope`"), "got: {err}");
    }

    #[test]
    fn fqbns_accumulate_without_duplicates() {
        let (mut f, _) = fleet_with_esp(1);
        let id = "44:1b:f6:ce:a3:b8";
        f.note_fqbn(id, "esp32:esp32:esp32s3").unwrap();
        f.note_fqbn(id, "esp32:esp32:esp32s3").unwrap();
        f.note_fqbn(id, "esp32:esp32:esp32c3").unwrap();
        f.note_fqbn(id, "   ").unwrap(); // ignored
        assert_eq!(
            f.boards[0].fqbns,
            ["esp32:esp32:esp32s3", "esp32:esp32:esp32c3"]
        );
    }

    #[test]
    fn display_name_prefers_nickname_then_board_name_then_id() {
        let (mut f, _) = fleet_with_esp(1);
        assert_eq!(f.boards[0].display_name(), "Ozobot DRVKit");
        f.set_nickname("44:1b:f6:ce:a3:b8", Some("bench probe"))
            .unwrap();
        assert_eq!(f.boards[0].display_name(), "bench probe");
        f.boards[0].nickname = None;
        f.boards[0].board_name = None;
        assert_eq!(f.boards[0].display_name(), "44:1b:f6:ce:a3:b8");
    }

    // ---------- identification / merging ----------

    #[test]
    fn identifying_migrates_a_serial_record_and_keeps_its_history() {
        // The case that matters: a bridge board nicknamed by the user, later
        // identified by esptool. Losing the nickname would be a data loss bug.
        let mut f = Fleet::default();
        let port = serial_port();
        let id = identify(&port).unwrap();
        f.sight(&id, &port, Some("Some Board"), 100);
        f.set_nickname("3477325620", Some("mesh node 3")).unwrap();
        f.note_fqbn("3477325620", "esp32:esp32:esp32").unwrap();

        let new_id = f
            .merge_identified(
                Some("3477325620"),
                "44:1B:F6:CE:A3:B8",
                Some("ESP32-S3"),
                200,
            )
            .unwrap();

        assert_eq!(new_id, "44:1b:f6:ce:a3:b8");
        assert_eq!(f.boards.len(), 1, "the serial record must not linger");
        let e = &f.boards[0];
        assert_eq!(e.id, "44:1b:f6:ce:a3:b8");
        assert_eq!(e.id_kind, BoardIdKind::Mac);
        assert_eq!(e.nickname.as_deref(), Some("mesh node 3"));
        assert_eq!(e.chip_type.as_deref(), Some("ESP32-S3"));
        assert_eq!(e.fqbns, ["esp32:esp32:esp32"]);
        assert_eq!(e.first_seen, 100, "history predates the identification");
        assert_eq!(e.last_seen, 200);
    }

    #[test]
    fn identifying_merges_into_an_existing_mac_record() {
        let mut f = Fleet::default();
        // Already known by MAC, with a nickname.
        let esp = esp_port();
        let esp_id = identify(&esp).unwrap();
        f.sight(&esp_id, &esp, Some("Ozobot DRVKit"), 50);
        f.set_nickname("44:1b:f6:ce:a3:b8", Some("the good one"))
            .unwrap();
        // Also known by a serial, with its own fqbn history.
        let ser = serial_port();
        let ser_id = identify(&ser).unwrap();
        f.sight(&ser_id, &ser, None, 60);
        f.note_fqbn("3477325620", "esp32:esp32:esp32c3").unwrap();

        f.merge_identified(Some("3477325620"), "44:1b:f6:ce:a3:b8", None, 70)
            .unwrap();

        assert_eq!(f.boards.len(), 1);
        let e = &f.boards[0];
        assert_eq!(
            e.nickname.as_deref(),
            Some("the good one"),
            "MAC record wins"
        );
        assert_eq!(e.fqbns, ["esp32:esp32:esp32c3"], "history is folded in");
        assert_eq!(e.first_seen, 50);
    }

    #[test]
    fn identifying_a_brand_new_board_creates_a_mac_record() {
        let mut f = Fleet::default();
        let id = f
            .merge_identified(None, "44:1b:f6:ce:a3:b8", Some("ESP32-C6"), 900)
            .unwrap();
        assert_eq!(id, "44:1b:f6:ce:a3:b8");
        assert_eq!(f.boards.len(), 1);
        assert_eq!(f.boards[0].chip_type.as_deref(), Some("ESP32-C6"));
        assert_eq!(f.boards[0].first_seen, 900);
    }

    #[test]
    fn identifying_with_a_non_mac_is_refused() {
        let mut f = Fleet::default();
        let err = f
            .merge_identified(None, "3477325620", None, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a MAC address"), "got: {err}");
        assert!(f.boards.is_empty());
    }

    #[test]
    fn re_identifying_the_same_board_is_idempotent() {
        let (mut f, _) = fleet_with_esp(100);
        f.merge_identified(
            Some("44:1b:f6:ce:a3:b8"),
            "44:1B:F6:CE:A3:B8",
            Some("ESP32-S3"),
            200,
        )
        .unwrap();
        f.merge_identified(
            Some("44:1b:f6:ce:a3:b8"),
            "44:1B:F6:CE:A3:B8",
            Some("ESP32-S3"),
            300,
        )
        .unwrap();
        assert_eq!(f.boards.len(), 1);
        assert_eq!(f.boards[0].first_seen, 100);
        assert_eq!(f.boards[0].last_seen, 300);
    }

    // ---------- what was last flashed ----------

    fn flash_from(project_dir: &str) -> FlashRecord {
        FlashRecord {
            project_dir: project_dir.to_string(),
            tag: "flash/2026-08-12T1430".into(),
            branch: Some("main".into()),
            commit: "9f2c1ab".into(),
            at: 1_500,
        }
    }

    #[test]
    fn note_flash_overwrites_the_previous_record() {
        // Only the last flash is kept; the `flash/*` tags are the history.
        let (mut f, _) = fleet_with_esp(1);
        let id = "44:1b:f6:ce:a3:b8";
        f.note_flash(id, flash_from("/home/m/blink")).unwrap();
        assert_eq!(
            f.boards[0].last_flash.as_ref().unwrap().project_dir,
            "/home/m/blink"
        );

        let mut later = flash_from("/home/m/sonar");
        later.tag = "flash/2026-08-12T1901".into();
        later.commit = "0ddba11".into();
        later.at = 1_900;
        f.note_flash(id, later).unwrap();

        let rec = f.boards[0].last_flash.as_ref().unwrap();
        assert_eq!(rec.project_dir, "/home/m/sonar");
        assert_eq!(rec.tag, "flash/2026-08-12T1901");
        assert_eq!(rec.commit, "0ddba11");
        assert_eq!(rec.at, 1_900);
    }

    #[test]
    fn noting_a_flash_on_an_unknown_board_is_an_error() {
        let (mut f, _) = fleet_with_esp(1);
        let err = f
            .note_flash("nope", flash_from("/home/m/blink"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no board with id `nope`"), "got: {err}");
    }

    #[test]
    fn a_detached_head_flash_records_no_branch() {
        let (mut f, _) = fleet_with_esp(1);
        let mut rec = flash_from("/home/m/blink");
        rec.branch = None;
        f.note_flash("44:1b:f6:ce:a3:b8", rec).unwrap();
        assert_eq!(f.boards[0].last_flash.as_ref().unwrap().branch, None);
    }

    #[test]
    fn sighting_a_board_does_not_disturb_its_last_flash() {
        // The scan path runs on a 2 s hotplug poll and must not touch this.
        let (mut f, id) = fleet_with_esp(1000);
        f.note_flash("44:1b:f6:ce:a3:b8", flash_from("/home/m/blink"))
            .unwrap();
        let before = f.boards[0].last_flash.clone();

        f.sight(&id, &esp_port(), Some("Ozobot DRVKit"), 2000);
        assert_eq!(f.boards[0].last_flash, before);

        let ports = vec![DetectedPort {
            port: esp_port(),
            matching_boards: vec![],
        }];
        sight_all(&mut f, &ports, 3000);
        assert_eq!(f.boards[0].last_flash, before);
    }

    // ---------- following a renamed project ----------

    #[test]
    fn repoint_project_rewrites_only_exact_matches() {
        let mut f = Fleet::default();
        let esp = esp_port();
        let ser = serial_port();
        let esp_id = identify(&esp).unwrap();
        let ser_id = identify(&ser).unwrap();
        f.sight(&esp_id, &esp, None, 10);
        f.sight(&ser_id, &ser, None, 20);
        f.note_flash("44:1b:f6:ce:a3:b8", flash_from("/home/m/blink"))
            .unwrap();
        f.note_flash("3477325620", flash_from("/home/m/blink-old"))
            .unwrap();

        assert_eq!(f.repoint_project("/home/m/blink", "/home/m/beacon"), 1);
        assert_eq!(
            f.boards[0].last_flash.as_ref().unwrap().project_dir,
            "/home/m/beacon"
        );
        assert_eq!(
            f.boards[1].last_flash.as_ref().unwrap().project_dir,
            "/home/m/blink-old",
            "a longer path that merely starts with the old one is untouched"
        );
    }

    #[test]
    fn repoint_project_does_not_follow_descendants() {
        // Exact match only: a nested sketch is its own project, and rewriting
        // it would point the board at a directory that was never flashed.
        let (mut f, _) = fleet_with_esp(1);
        f.note_flash("44:1b:f6:ce:a3:b8", flash_from("/home/m/blink/nested"))
            .unwrap();
        assert_eq!(f.repoint_project("/home/m/blink", "/home/m/beacon"), 0);
        assert_eq!(
            f.boards[0].last_flash.as_ref().unwrap().project_dir,
            "/home/m/blink/nested"
        );
    }

    #[test]
    fn repoint_project_counts_every_board_it_changed() {
        let mut f = Fleet::default();
        let esp = esp_port();
        let ser = serial_port();
        let esp_id = identify(&esp).unwrap();
        let ser_id = identify(&ser).unwrap();
        f.sight(&esp_id, &esp, None, 10);
        f.sight(&ser_id, &ser, None, 20);
        f.note_flash("44:1b:f6:ce:a3:b8", flash_from("/home/m/blink"))
            .unwrap();
        f.note_flash("3477325620", flash_from("/home/m/blink"))
            .unwrap();
        assert_eq!(f.repoint_project("/home/m/blink", "/home/m/beacon"), 2);
    }

    #[test]
    fn repoint_project_leaves_boards_with_no_flash_record_alone() {
        let (mut f, _) = fleet_with_esp(1);
        assert_eq!(f.repoint_project("/home/m/blink", "/home/m/beacon"), 0);
        assert_eq!(f.boards[0].last_flash, None);
    }

    #[test]
    fn identifying_carries_a_last_flash_onto_a_new_mac_record() {
        // The serial record is migrated wholesale; anything not named in the
        // merge is silently dropped, and losing this loses the way back to the
        // project running on the board.
        let mut f = Fleet::default();
        let port = serial_port();
        let id = identify(&port).unwrap();
        f.sight(&id, &port, None, 100);
        f.note_flash("3477325620", flash_from("/home/m/blink"))
            .unwrap();

        f.merge_identified(Some("3477325620"), "44:1B:F6:CE:A3:B8", None, 200)
            .unwrap();

        assert_eq!(f.boards.len(), 1);
        assert_eq!(
            f.boards[0].last_flash.as_ref().unwrap().project_dir,
            "/home/m/blink"
        );
    }

    #[test]
    fn identifying_fills_a_missing_last_flash_from_the_serial_record() {
        // The MAC record exists but has never been flashed: fill its gap.
        let mut f = Fleet::default();
        let esp = esp_port();
        let esp_id = identify(&esp).unwrap();
        f.sight(&esp_id, &esp, None, 50);
        let ser = serial_port();
        let ser_id = identify(&ser).unwrap();
        f.sight(&ser_id, &ser, None, 60);
        f.note_flash("3477325620", flash_from("/home/m/blink"))
            .unwrap();

        f.merge_identified(Some("3477325620"), "44:1b:f6:ce:a3:b8", None, 70)
            .unwrap();

        assert_eq!(f.boards.len(), 1);
        assert_eq!(
            f.boards[0].last_flash.as_ref().unwrap().project_dir,
            "/home/m/blink"
        );
    }

    #[test]
    fn identifying_prefers_the_mac_records_own_last_flash() {
        // Both have one: the MAC record is the stronger identity and its record
        // is the more recent truth about what is actually running.
        let mut f = Fleet::default();
        let esp = esp_port();
        let esp_id = identify(&esp).unwrap();
        f.sight(&esp_id, &esp, None, 50);
        f.note_flash("44:1b:f6:ce:a3:b8", flash_from("/home/m/beacon"))
            .unwrap();
        let ser = serial_port();
        let ser_id = identify(&ser).unwrap();
        f.sight(&ser_id, &ser, None, 60);
        f.note_flash("3477325620", flash_from("/home/m/blink"))
            .unwrap();

        f.merge_identified(Some("3477325620"), "44:1b:f6:ce:a3:b8", None, 70)
            .unwrap();

        assert_eq!(f.boards.len(), 1);
        assert_eq!(
            f.boards[0].last_flash.as_ref().unwrap().project_dir,
            "/home/m/beacon",
            "MAC record wins"
        );
    }

    // ---------- forget ----------

    #[test]
    fn forget_removes_once_and_is_idempotent() {
        let (mut f, _) = fleet_with_esp(1);
        assert!(f.forget("44:1b:f6:ce:a3:b8"));
        assert!(f.boards.is_empty());
        assert!(!f.forget("44:1b:f6:ce:a3:b8"));
    }

    // ---------- persistence ----------

    #[test]
    fn a_missing_file_is_an_empty_fleet_at_the_current_version() {
        let tmp = tempfile::tempdir().unwrap();
        let f = Fleet::load(&tmp.path().join("nope.json")).unwrap();
        assert_eq!(f.version, FLEET_VERSION);
        assert!(f.boards.is_empty());
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_an_empty_fleet() {
        // Unlike settings.json: silently returning an empty fleet would destroy
        // the user's whole board history on the next save.
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("fleet.json");
        std::fs::write(&p, "{not json").unwrap();
        assert!(Fleet::load(&p).is_err());
    }

    #[test]
    fn a_round_trip_preserves_every_field() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("deep/nested/fleet.json");

        let (mut f, _) = fleet_with_esp(1000);
        f.set_nickname("44:1b:f6:ce:a3:b8", Some("bench probe"))
            .unwrap();
        f.note_fqbn("44:1b:f6:ce:a3:b8", "esp32:esp32:esp32s3")
            .unwrap();
        f.boards[0].chip_type = Some("ESP32-S3".into());
        f.save(&p).unwrap();
        assert!(p.is_file(), "save must create parent dirs");

        let back = Fleet::load(&p).unwrap();
        assert_eq!(back.version, FLEET_VERSION);
        assert_eq!(back.boards, f.boards);
    }

    #[test]
    fn a_file_without_a_version_loads_at_the_current_version() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("fleet.json");
        std::fs::write(&p, r#"{"boards":[]}"#).unwrap();
        assert_eq!(Fleet::load(&p).unwrap().version, FLEET_VERSION);
    }

    #[test]
    fn a_flash_record_round_trips_through_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("fleet.json");

        let (mut f, _) = fleet_with_esp(1000);
        f.note_flash("44:1b:f6:ce:a3:b8", flash_from("/home/m/blink"))
            .unwrap();
        f.save(&p).unwrap();

        let back = Fleet::load(&p).unwrap();
        assert_eq!(back.boards, f.boards);
        let rec = back.boards[0].last_flash.as_ref().unwrap();
        assert_eq!(rec.project_dir, "/home/m/blink");
        assert_eq!(rec.tag, "flash/2026-08-12T1430");
        assert_eq!(rec.branch.as_deref(), Some("main"));
        assert_eq!(rec.commit, "9f2c1ab");
        assert_eq!(rec.at, 1_500);
    }

    #[test]
    fn an_entry_without_a_last_flash_key_still_loads() {
        // Every fleet.json written before this field existed looks like this.
        // Without `#[serde(default)]` the whole file would be refused and the
        // panel would show an error instead of the user's boards.
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("fleet.json");
        std::fs::write(
            &p,
            r#"{"version":1,"boards":[{"id":"44:1b:f6:ce:a3:b8","id_kind":"mac",
               "first_seen":10,"last_seen":20}]}"#,
        )
        .unwrap();
        let f = Fleet::load(&p).unwrap();
        assert_eq!(f.boards.len(), 1);
        assert_eq!(f.boards[0].last_flash, None);
    }

    #[test]
    fn a_flash_record_without_a_branch_key_still_loads() {
        // Same rule one level down: an optional field inside FlashRecord must
        // default too, or an older record refuses the whole file.
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("fleet.json");
        std::fs::write(
            &p,
            r#"{"version":1,"boards":[{"id":"44:1b:f6:ce:a3:b8","id_kind":"mac",
               "first_seen":10,"last_seen":20,
               "last_flash":{"project_dir":"/home/m/blink",
                 "tag":"flash/2026-08-12T1430","commit":"9f2c1ab","at":1500}}]}"#,
        )
        .unwrap();
        let f = Fleet::load(&p).unwrap();
        let rec = f.boards[0].last_flash.as_ref().unwrap();
        assert_eq!(rec.branch, None);
        assert_eq!(rec.project_dir, "/home/m/blink");
    }
}
