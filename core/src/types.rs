//! Typed views over `arduino-cli --json` output.
//!
//! All structs are tolerant (`#[serde(default)]` everywhere) because the CLI's
//! JSON schema gains fields between releases; we only fail on missing data we
//! actually need.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ---------- `arduino-cli board list --json` ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BoardListResponse {
    #[serde(default)]
    pub detected_ports: Vec<DetectedPort>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DetectedPort {
    #[serde(default)]
    pub port: Port,
    #[serde(default)]
    pub matching_boards: Vec<MatchingBoard>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Port {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub protocol_label: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MatchingBoard {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fqbn: String,
}

// ---------- `arduino-cli lib search <q> --json` ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibSearchResponse {
    #[serde(default)]
    pub libraries: Vec<IndexedLibrary>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct IndexedLibrary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub latest: LibraryRelease,
    /// Every published version, newest last (map keys in the raw JSON).
    #[serde(default)]
    pub available_versions: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibraryRelease {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub paragraph: String,
    #[serde(default)]
    pub website: String,
}

// ---------- `arduino-cli lib list --json` ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibListResponse {
    #[serde(default)]
    pub installed_libraries: Vec<InstalledLibraryEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InstalledLibraryEntry {
    #[serde(default)]
    pub library: InstalledLibrary,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InstalledLibrary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub sentence: String,
    /// "user" (sketchbook), "platform", or "ide" — lets the UI badge
    /// personal/local libraries differently from bundled ones.
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub install_dir: String,
}

// ---------- `arduino-cli core list --json` / `core search <q> --json` ----------
//
// Both commands return the same shape, so one set of types serves both. `search`
// reports platforms that are merely known to an index, which is why
// `installed_version` is routinely empty rather than always present.

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CoreListResponse {
    #[serde(default)]
    pub platforms: Vec<Platform>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Platform {
    /// `packager:architecture`, e.g. `esp32:esp32`.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub maintainer: String,
    #[serde(default)]
    pub website: String,
    /// Empty when the platform is known to an index but not installed.
    #[serde(default)]
    pub installed_version: String,
    #[serde(default)]
    pub latest_version: String,
    /// Every published release, keyed by version string — this is a JSON object
    /// keyed by version, not an array, so the keys are the version list.
    #[serde(default)]
    pub releases: BTreeMap<String, PlatformRelease>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PlatformRelease {
    /// Human name, e.g. `Arduino AVR Boards`.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// Board names only — `core list`/`core search` do **not** carry FQBNs here.
    /// FQBNs come from `board listall` (see [`BoardOption`]), which covers
    /// installed platforms only.
    #[serde(default)]
    pub boards: Vec<PlatformBoard>,
    /// False when arduino-cli has no toolchain for this host; installing such a
    /// release fails, so the UI must not offer it.
    ///
    /// Defaults to true: older CLI releases omit the key entirely, and treating
    /// "absent" as incompatible would hide every version.
    #[serde(default = "yes")]
    pub compatible: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PlatformBoard {
    #[serde(default)]
    pub name: String,
}

// ---------- Bancada's own composite types ----------

/// A line of process output, tagged so the UI can colour stderr.
#[derive(Debug, Clone, Serialize)]
pub struct OutputLine {
    pub stream: OutputStream,
    pub line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Result of a compile/upload run.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub success: bool,
    pub exit_code: i32,
}

// ---------- board listall ----------
//
// `board listall --json` nests the entire platform — including every one of its
// boards — inside each board entry. Only the fields below are read; the rest is
// ignored, and `ArduinoCli::board_listall` flattens the result into
// [`BoardOption`] before it reaches the frontend.

#[derive(Debug, Clone, Deserialize)]
pub struct BoardListAllResponse {
    #[serde(default)]
    pub boards: Vec<BoardListAllEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoardListAllEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fqbn: String,
    #[serde(default)]
    pub platform: BoardListAllPlatform,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BoardListAllPlatform {
    #[serde(default)]
    pub metadata: BoardListAllPlatformMeta,
    #[serde(default)]
    pub release: BoardListAllPlatformRelease,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BoardListAllPlatformMeta {
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BoardListAllPlatformRelease {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub installed: bool,
}

/// One selectable board, flattened for the New Project picker.
#[derive(Debug, Clone, Serialize)]
pub struct BoardOption {
    pub fqbn: String,
    pub name: String,
    /// e.g. `esp32:esp32`
    pub platform_id: String,
    /// e.g. `esp32` — used to group the picker.
    pub platform_name: String,
}

// ---------- tests ----------

/// These pin the contract with an *externally versioned* tool: arduino-cli's
/// JSON gains and renames fields between releases, and a mismatch surfaces at
/// runtime as an opaque parse error rather than at compile time.
///
/// Every fixture below is real output captured from arduino-cli 1.5.0, trimmed
/// only by removing sibling array entries — never by reshaping an object.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_list_without_matching_boards() {
        // A CP210x bridge is not identifiable, so arduino-cli omits
        // `matching_boards` entirely rather than sending an empty array.
        let json = r#"{
          "detected_ports": [
            {
              "port": {
                "address": "/dev/ttyUSB0",
                "label": "/dev/ttyUSB0",
                "protocol": "serial",
                "protocol_label": "Serial Port (USB)",
                "properties": { "pid": "0xEA60", "vid": "0x10C4" },
                "hardware_id": "0001"
              }
            }
          ]
        }"#;
        let r: BoardListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.detected_ports.len(), 1);
        let p = &r.detected_ports[0];
        assert_eq!(p.port.address, "/dev/ttyUSB0");
        assert_eq!(p.port.protocol, "serial");
        assert_eq!(p.port.protocol_label, "Serial Port (USB)");
        assert!(
            p.matching_boards.is_empty(),
            "a missing key must not fail the parse"
        );
    }

    #[test]
    fn board_list_with_an_identified_board() {
        let json = r#"{
          "detected_ports": [
            {
              "port": { "address": "/dev/ttyACM0", "protocol": "serial" },
              "matching_boards": [
                { "name": "Arduino UNO", "fqbn": "arduino:avr:uno" }
              ]
            }
          ]
        }"#;
        let r: BoardListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.detected_ports[0].matching_boards[0].fqbn, "arduino:avr:uno");
    }

    #[test]
    fn board_list_empty_when_nothing_attached() {
        let r: BoardListResponse = serde_json::from_str("{}").unwrap();
        assert!(r.detected_ports.is_empty());
    }

    #[test]
    fn lib_search_response() {
        let json = r#"{"libraries":[{"name":"ArduinoJson","latest":{"version":"7.4.3","author":"Benoit Blanchon <blog.benoitblanchon.fr>","sentence":"A simple and efficient JSON library for embedded C++."},"available_versions":["4.0.0","5.0.0"]}]}"#;
        let r: LibSearchResponse = serde_json::from_str(json).unwrap();
        let lib = &r.libraries[0];
        assert_eq!(lib.name, "ArduinoJson");
        assert_eq!(lib.latest.version, "7.4.3");
        assert!(lib.latest.author.contains("Benoit"));
        assert_eq!(lib.available_versions.len(), 2);
    }

    #[test]
    fn lib_list_response_and_location() {
        // `location` drives the Remove button and the badge in the UI, so its
        // exact spelling matters.
        let json = r#"{"installed_libraries":[{"library":{"name":"Adafruit GFX Library","version":"1.12.6","author":"Adafruit","sentence":"core class","location":"user","install_dir":"/home/kayaman/Arduino/libraries/Adafruit_GFX_Library"}}]}"#;
        let r: LibListResponse = serde_json::from_str(json).unwrap();
        let lib = &r.installed_libraries[0].library;
        assert_eq!(lib.name, "Adafruit GFX Library");
        assert_eq!(lib.location, "user");
        assert!(lib.install_dir.ends_with("Adafruit_GFX_Library"));
    }

    #[test]
    fn core_list_response() {
        // `releases` is an object keyed by version, not an array — the keys are
        // where the version list comes from.
        let json = r#"{
          "platforms": [
            {
              "id": "esp32:esp32",
              "maintainer": "Espressif Systems",
              "website": "https://github.com/espressif/arduino-esp32",
              "email": "hristo@espressif.com",
              "indexed": true,
              "releases": {
                "3.2.0": { "name": "esp32", "version": "3.2.0", "compatible": true },
                "3.3.11": {
                  "name": "esp32",
                  "version": "3.3.11",
                  "types": ["Contributed"],
                  "boards": [
                    { "name": "ESP32 Dev Module" },
                    { "name": "ESP32S3 Dev Module" }
                  ],
                  "compatible": true
                }
              },
              "installed_version": "3.3.11",
              "latest_version": "3.3.11"
            }
          ]
        }"#;
        let r: CoreListResponse = serde_json::from_str(json).unwrap();
        let p = &r.platforms[0];
        assert_eq!(p.id, "esp32:esp32");
        assert_eq!(p.maintainer, "Espressif Systems");
        assert_eq!(p.installed_version, "3.3.11");
        assert_eq!(p.releases.len(), 2);
        let rel = p.releases.get("3.3.11").expect("release keyed by version");
        assert_eq!(rel.boards.len(), 2);
        assert_eq!(rel.boards[0].name, "ESP32 Dev Module");
        // A release carries board *names* only; FQBNs are not available here.
        assert!(rel.compatible);
    }

    #[test]
    fn core_search_reports_an_uninstalled_platform() {
        // `core search` omits `installed_version` for anything not installed;
        // the empty string is what marks a platform as available-but-absent.
        let json = r#"{
          "platforms": [
            {
              "id": "arduino:esp32",
              "maintainer": "Arduino",
              "latest_version": "2.0.18-arduino.5",
              "releases": {
                "2.0.18-arduino.5": {
                  "name": "Arduino ESP32 Boards",
                  "version": "2.0.18-arduino.5",
                  "boards": [{ "name": "Arduino Nano ESP32" }],
                  "missing_metadata": true,
                  "compatible": true
                }
              }
            }
          ]
        }"#;
        let r: CoreListResponse = serde_json::from_str(json).unwrap();
        let p = &r.platforms[0];
        assert!(p.installed_version.is_empty());
        assert_eq!(p.latest_version, "2.0.18-arduino.5");
        assert_eq!(p.website, "", "a missing key must not fail the parse");
    }

    #[test]
    fn platform_release_is_compatible_when_the_key_is_absent() {
        // Treating "absent" as incompatible would hide every version from the UI.
        let json = r#"{"platforms":[{"id":"a:b","releases":{"1.0.0":{"version":"1.0.0"}}}]}"#;
        let r: CoreListResponse = serde_json::from_str(json).unwrap();
        assert!(r.platforms[0].releases["1.0.0"].compatible);
    }

    #[test]
    fn board_listall_nests_the_whole_platform_per_board() {
        // The real shape: each board repeats its platform, including a `boards`
        // array of all its siblings. Only the outer name/fqbn and the platform's
        // id/name/installed flag are read.
        let json = r#"{
          "boards": [
            {
              "name": "Arduino Yún",
              "fqbn": "arduino:avr:yun",
              "platform": {
                "metadata": { "id": "arduino:avr", "maintainer": "Arduino" },
                "release": {
                  "name": "Arduino AVR Boards",
                  "version": "1.8.8",
                  "installed": true,
                  "boards": [ { "name": "Arduino BT", "fqbn": "arduino:avr:bt" } ]
                }
              }
            }
          ]
        }"#;
        let r: BoardListAllResponse = serde_json::from_str(json).unwrap();
        let b = &r.boards[0];
        assert_eq!(b.fqbn, "arduino:avr:yun");
        assert_eq!(b.platform.metadata.id, "arduino:avr");
        assert_eq!(b.platform.release.name, "Arduino AVR Boards");
        assert!(b.platform.release.installed);
    }

    #[test]
    fn board_listall_tolerates_a_missing_platform() {
        let r: BoardListAllResponse =
            serde_json::from_str(r#"{"boards":[{"name":"X","fqbn":"a:b:c"}]}"#).unwrap();
        assert_eq!(r.boards[0].fqbn, "a:b:c");
        assert!(!r.boards[0].platform.release.installed, "defaults to false");
    }

    #[test]
    fn unknown_fields_are_ignored_across_every_response() {
        // The whole reason these types are tolerant: a new arduino-cli release
        // adding a field must not break Bancada.
        let with_extra = r#"{"detected_ports":[{"port":{"address":"/dev/x","brand_new_field":42},"future_key":{"a":1}}],"another_new_top_level":true}"#;
        let r: BoardListResponse = serde_json::from_str(with_extra).unwrap();
        assert_eq!(r.detected_ports[0].port.address, "/dev/x");
    }

    #[test]
    fn output_stream_serialises_lowercase_for_the_ui() {
        // The frontend switches on these strings to colour stderr.
        let line = OutputLine {
            stream: OutputStream::Stderr,
            line: "boom".into(),
        };
        let json = serde_json::to_string(&line).unwrap();
        assert!(json.contains(r#""stream":"stderr""#), "{json}");
        let out = serde_json::to_string(&OutputStream::Stdout).unwrap();
        assert_eq!(out, r#""stdout""#);
    }
}
