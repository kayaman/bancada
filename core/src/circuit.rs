//! Project-local circuit manifests, electrical validation, and generated artifacts.
//!
//! `hardware/circuit.yaml` is the only hand-edited source of truth. Every
//! other circuit file is rendered deterministically from it and checked
//! before a firmware build or upload.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const SCHEMA_VERSION: u32 = 1;
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MANIFEST_PATH: &str = "hardware/circuit.yaml";
pub const HEADER_PATH: &str = "src/bancada_circuit_pins.h";
pub const SVG_PATH: &str = "hardware/circuit.svg";
pub const WIRING_PATH: &str = "hardware/wiring.md";
pub const BOM_PATH: &str = "hardware/bom.csv";
pub const VALIDATION_PATH: &str = "hardware/validation.json";

const GENERATED_PATHS: [&str; 5] = [
    HEADER_PATH,
    SVG_PATH,
    WIRING_PATH,
    BOM_PATH,
    VALIDATION_PATH,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CircuitManifest {
    pub version: u32,
    pub name: String,
    pub board: BoardSelection,
    #[serde(default)]
    pub components: Vec<Component>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoardSelection {
    pub catalog_id: String,
    pub fqbn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Component {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_v: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_ma: Option<u32>,
    /// Confirmation that the part's pinout and ratings were checked against
    /// its datasheet. Unknown parts intentionally block builds until checked.
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub pins: Vec<PartPin>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartPin {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub id: String,
    pub from: Endpoint,
    pub to: Endpoint,
    /// `signal`, `power`, `ground`, `i2c_sda`, `i2c_scl`, `spi`, `uart`, ...
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_v: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Endpoint {
    /// `board` for the selected development board; otherwise a component id.
    pub target: String,
    pub pin: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CircuitCatalog {
    pub version: u32,
    pub boards: Vec<BoardProfile>,
    pub component_kinds: Vec<ComponentTemplate>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BoardProfile {
    pub id: String,
    pub label: String,
    pub fqbn: String,
    pub logic_voltage_v: f32,
    pub max_3v3_ma: u32,
    pub source_url: String,
    pub pins: Vec<BoardPin>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BoardPin {
    pub id: String,
    pub label: String,
    pub gpio: Option<u8>,
    pub capabilities: Vec<String>,
    pub strapping: bool,
    pub reserved: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ComponentTemplate {
    pub kind: String,
    pub label: String,
    pub pins: Vec<String>,
    pub safety_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactStatus {
    pub path: String,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationReport {
    pub schema_version: u32,
    pub generator_version: String,
    pub manifest_digest: String,
    pub valid: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub artifacts: Vec<ArtifactStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CircuitSnapshot {
    pub manifest: CircuitManifest,
    pub validation: ValidationReport,
    pub svg: String,
    pub wiring_markdown: String,
    pub bom_csv: String,
    pub generated_header: String,
}

#[derive(Debug)]
struct RenderedArtifacts {
    header: String,
    svg: String,
    wiring: String,
    bom: String,
    validation: String,
}

impl RenderedArtifacts {
    fn get(&self, path: &str) -> &str {
        match path {
            HEADER_PATH => &self.header,
            SVG_PATH => &self.svg,
            WIRING_PATH => &self.wiring,
            BOM_PATH => &self.bom,
            VALIDATION_PATH => &self.validation,
            _ => unreachable!("generated path is fixed"),
        }
    }
}

pub fn catalog() -> CircuitCatalog {
    CircuitCatalog {
        version: SCHEMA_VERSION,
        boards: vec![
            board(
                "esp32-devkitc-v4",
                "ESP32-DevKitC V4",
                "esp32:esp32:esp32",
                "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32/esp32-devkitc/user_guide.html",
                &[0, 2, 4, 5, 12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23, 25, 26, 27, 32, 33, 34, 35, 36, 39],
                &[0, 2, 5, 12, 15],
                &[],
                &[34, 35, 36, 39],
                &[2, 4, 12, 13, 14, 15, 25, 26, 27, 32, 33, 34, 35, 36, 39],
            ),
            board(
                "esp32-s3-devkitc-1",
                "ESP32-S3-DevKitC-1",
                "esp32:esp32:esp32s3",
                "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32s3/esp32-s3-devkitc-1/user_guide.html",
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48],
                &[0, 3, 45, 46],
                &[19, 20],
                &[],
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
            ),
            board(
                "esp32-c3-devkitc-02",
                "ESP32-C3-DevKitC-02",
                "esp32:esp32:esp32c3",
                "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32c3/esp32-c3-devkitc-02/user_guide.html",
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 18, 19, 20, 21],
                &[2, 8, 9],
                &[18, 19],
                &[],
                &[0, 1, 2, 3, 4, 5],
            ),
            board(
                "esp32-c6-devkitc-1",
                "ESP32-C6-DevKitC-1",
                "esp32:esp32:esp32c6",
                "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32c6/esp32-c6-devkitc-1/user_guide.html",
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 25, 26, 27, 28, 30, 31],
                &[8, 9, 15],
                &[12, 13],
                &[],
                &[0, 1, 2, 3, 4, 5, 6],
            ),
        ],
        component_kinds: vec![
            template("led", "LED", &["anode", "cathode"], "Record series_resistor_ohms."),
            template("resistor", "Resistor", &["1", "2"], "Set part_number to the resistance/value."),
            template("button", "Push button", &["1", "2"], "Declare pull-up or pull-down in properties."),
            template("i2c_sensor", "I²C sensor", &["VCC", "GND", "SDA", "SCL"], "Record pullups as onboard or external."),
            template("spi_device", "SPI device", &["VCC", "GND", "SCK", "MOSI", "MISO", "CS"], "Check the device logic voltage."),
            template("uart_device", "UART device", &["VCC", "GND", "TX", "RX"], "Cross TX to RX and check logic voltage."),
            template("relay", "Relay / inductive load", &["VCC", "GND", "IN"], "Requires a driver and flyback protection."),
            template("motor", "Motor", &["+", "-"], "Use a rated driver and flyback protection."),
            template("mosfet", "MOSFET driver", &["G", "D", "S"], "Check gate drive, dissipation, and default state."),
            template("power_supply", "External power supply", &["V+", "GND"], "Tie grounds where signals cross supplies."),
            template("generic", "Generic component", &[], "Add datasheet pin names and explicitly verify the part."),
        ],
    }
}

fn template(kind: &str, label: &str, pins: &[&str], safety_hint: &str) -> ComponentTemplate {
    ComponentTemplate {
        kind: kind.into(),
        label: label.into(),
        pins: pins.iter().map(|p| (*p).into()).collect(),
        safety_hint: safety_hint.into(),
    }
}

fn board(
    id: &str,
    label: &str,
    fqbn: &str,
    source_url: &str,
    gpios: &[u8],
    strapping: &[u8],
    reserved: &[u8],
    input_only: &[u8],
    adc: &[u8],
) -> BoardProfile {
    let mut pins = vec![
        BoardPin {
            id: "3V3".into(),
            label: "3.3 V".into(),
            gpio: None,
            capabilities: vec!["power".into()],
            strapping: false,
            reserved: false,
        },
        BoardPin {
            id: "5V".into(),
            label: "5 V / VBUS".into(),
            gpio: None,
            capabilities: vec!["power".into()],
            strapping: false,
            reserved: false,
        },
        BoardPin {
            id: "GND".into(),
            label: "Ground".into(),
            gpio: None,
            capabilities: vec!["ground".into()],
            strapping: false,
            reserved: false,
        },
    ];
    pins.extend(gpios.iter().map(|gpio| {
        let mut capabilities = vec!["digital_input".into()];
        if !input_only.contains(gpio) {
            capabilities.push("digital_output".into());
            capabilities.push("pwm".into());
        }
        if adc.contains(gpio) {
            capabilities.push("adc".into());
        }
        BoardPin {
            id: format!("GPIO{gpio}"),
            label: format!("GPIO {gpio}"),
            gpio: Some(*gpio),
            capabilities,
            strapping: strapping.contains(gpio),
            reserved: reserved.contains(gpio),
        }
    }));
    BoardProfile {
        id: id.into(),
        label: label.into(),
        fqbn: fqbn.into(),
        logic_voltage_v: 3.3,
        max_3v3_ma: 500,
        source_url: source_url.into(),
        pins,
    }
}

pub fn default_manifest(board_id: Option<&str>, name: &str) -> CircuitManifest {
    let all = catalog();
    let selected = board_id
        .and_then(|id| all.boards.iter().find(|b| b.id == id))
        .unwrap_or(&all.boards[0]);
    CircuitManifest {
        version: SCHEMA_VERSION,
        name: if name.trim().is_empty() {
            "Circuit".into()
        } else {
            name.trim().into()
        },
        board: BoardSelection {
            catalog_id: selected.id.clone(),
            fqbn: selected.fqbn.clone(),
        },
        components: Vec::new(),
        connections: Vec::new(),
        notes: BTreeMap::new(),
    }
}

pub fn manifest_exists(project: &Path) -> bool {
    project.join(MANIFEST_PATH).is_file()
}

pub fn load_manifest(project: &Path) -> Result<Option<CircuitManifest>> {
    let path = project.join(MANIFEST_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    serde_yaml::from_str(&text)
        .map(Some)
        .map_err(|e| Error::Other(format!("invalid {}: {e}", path.display())))
}

pub fn load_project(
    project: &Path,
    selected_fqbn: Option<&str>,
) -> Result<Option<CircuitSnapshot>> {
    let Some(manifest) = load_manifest(project)? else {
        return Ok(None);
    };
    let (validation, rendered) = checked_render(project, &manifest, selected_fqbn)?;
    Ok(Some(snapshot(manifest, validation, rendered)))
}

pub fn save_project(
    project: &Path,
    manifest: &CircuitManifest,
    selected_fqbn: Option<&str>,
) -> Result<CircuitSnapshot> {
    fs::create_dir_all(project.join("hardware"))?;
    fs::create_dir_all(project.join("src"))?;
    let yaml = serde_yaml::to_string(manifest)?;
    atomic_write(&project.join(MANIFEST_PATH), yaml.as_bytes())?;
    sync_project(project, selected_fqbn)
}

pub fn init_project(
    project: &Path,
    board_id: Option<&str>,
    name: &str,
    selected_fqbn: Option<&str>,
) -> Result<CircuitSnapshot> {
    if manifest_exists(project) {
        return Err(Error::Other(format!("{MANIFEST_PATH} already exists")));
    }
    save_project(project, &default_manifest(board_id, name), selected_fqbn)
}

pub fn sync_project(project: &Path, selected_fqbn: Option<&str>) -> Result<CircuitSnapshot> {
    let manifest = load_manifest(project)?
        .ok_or_else(|| Error::Other(format!("{MANIFEST_PATH} does not exist")))?;
    let (validation, rendered) = render(project, &manifest, selected_fqbn)?;
    for path in GENERATED_PATHS {
        let full = project.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&full, rendered.get(path).as_bytes())?;
    }
    Ok(snapshot(manifest, validation, rendered))
}

pub fn validate_project(
    project: &Path,
    selected_fqbn: Option<&str>,
) -> Result<Option<ValidationReport>> {
    let Some(manifest) = load_manifest(project)? else {
        return Ok(None);
    };
    let (report, _) = checked_render(project, &manifest, selected_fqbn)?;
    Ok(Some(report))
}

/// Build/upload guard. Projects without a circuit manifest keep their
/// previous behavior; projects with one must be valid and fully synchronized.
pub fn require_build_ready(project: &Path, selected_fqbn: Option<&str>) -> Result<()> {
    let Some(report) = validate_project(project, selected_fqbn)? else {
        return Ok(());
    };
    if report.valid {
        return Ok(());
    }
    let reasons = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .take(6)
        .map(|d| format!("- {}", d.message))
        .collect::<Vec<_>>()
        .join("\n");
    Err(Error::Other(format!(
        "Circuit check blocked this build. Open Hardware → Circuit or run `cargo run -p bancada-core --bin bancada-circuit -- check --project .`.\n{reasons}"
    )))
}

fn checked_render(
    project: &Path,
    manifest: &CircuitManifest,
    selected_fqbn: Option<&str>,
) -> Result<(ValidationReport, RenderedArtifacts)> {
    let (mut report, rendered) = render(project, manifest, selected_fqbn)?;
    for status in &mut report.artifacts {
        let actual = fs::read(project.join(&status.path)).ok();
        status.current = actual.as_deref() == Some(rendered.get(&status.path).as_bytes());
        if !status.current {
            report.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "artifact.stale".into(),
                message: format!(
                    "{} is missing or stale; synchronize the circuit",
                    status.path
                ),
                subject: Some(status.path.clone()),
            });
        }
    }
    report.valid = !report
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    Ok((report, rendered))
}

fn render(
    project: &Path,
    manifest: &CircuitManifest,
    selected_fqbn: Option<&str>,
) -> Result<(ValidationReport, RenderedArtifacts)> {
    let canonical = serde_yaml::to_string(manifest)?;
    let digest = fnv1a(canonical.as_bytes());
    let diagnostics = validate_manifest(project, manifest, selected_fqbn);
    let report = ValidationReport {
        schema_version: SCHEMA_VERSION,
        generator_version: GENERATOR_VERSION.into(),
        manifest_digest: digest.clone(),
        valid: !diagnostics.iter().any(|d| d.severity == Severity::Error),
        diagnostics,
        artifacts: GENERATED_PATHS
            .iter()
            .map(|path| ArtifactStatus {
                path: (*path).into(),
                current: true,
            })
            .collect(),
    };
    let header = render_header(manifest, &digest);
    let svg = render_svg(manifest, &digest);
    let wiring = render_wiring(manifest, &report, &digest);
    let bom = render_bom(manifest);
    let validation = format!(
        "{}\n",
        serde_json::to_string_pretty(&report).map_err(|source| Error::Json {
            what: VALIDATION_PATH.into(),
            source,
        })?
    );
    Ok((
        report,
        RenderedArtifacts {
            header,
            svg,
            wiring,
            bom,
            validation,
        },
    ))
}

fn snapshot(
    manifest: CircuitManifest,
    validation: ValidationReport,
    rendered: RenderedArtifacts,
) -> CircuitSnapshot {
    CircuitSnapshot {
        manifest,
        validation,
        svg: rendered.svg,
        wiring_markdown: rendered.wiring,
        bom_csv: rendered.bom,
        generated_header: rendered.header,
    }
}

/// Validate a manifest that is not (yet) on disk.
///
/// The same rules [`sync_project`] runs, exposed so a *proposal* can show the
/// diagnostics acceptance will produce before anything is written. Sharing the
/// implementation is the point: a preview computed by a second, parallel rule
/// set would eventually disagree with the real one, and the disagreement would
/// surface only after the user had committed to it.
///
/// Artifact staleness is deliberately not included — nothing has been
/// rendered yet.
pub fn validate_manifest_for(
    project: &Path,
    m: &CircuitManifest,
    selected_fqbn: Option<&str>,
) -> Vec<Diagnostic> {
    validate_manifest(project, m, selected_fqbn)
}

fn validate_manifest(
    project: &Path,
    m: &CircuitManifest,
    selected_fqbn: Option<&str>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut error = |code: &str, message: String, subject: Option<String>| {
        out.push(Diagnostic {
            severity: Severity::Error,
            code: code.into(),
            message,
            subject,
        })
    };
    if m.version != SCHEMA_VERSION {
        error(
            "schema.version",
            format!(
                "circuit version {} is unsupported; expected {SCHEMA_VERSION}",
                m.version
            ),
            None,
        );
    }
    if m.name.trim().is_empty() {
        error("manifest.name", "the circuit needs a name".into(), None);
    }
    let all = catalog();
    let board = all.boards.iter().find(|b| b.id == m.board.catalog_id);
    if board.is_none() {
        error(
            "board.unknown",
            format!("unknown board catalog id `{}`", m.board.catalog_id),
            Some("board".into()),
        );
    }
    if let Some(board) = board {
        if fqbn_target(&m.board.fqbn) != fqbn_target(&board.fqbn) {
            error(
                "board.catalog_fqbn",
                format!(
                    "manifest FQBN `{}` does not match {} ({})",
                    m.board.fqbn, board.label, board.fqbn
                ),
                Some("board".into()),
            );
        }
    }
    if let Some(selected) = selected_fqbn.filter(|s| !s.trim().is_empty()) {
        if fqbn_target(selected) != fqbn_target(&m.board.fqbn) {
            error(
                "board.profile_mismatch",
                format!(
                    "selected build target `{selected}` does not match circuit board `{}`",
                    m.board.fqbn
                ),
                Some("board".into()),
            );
        }
    }

    let mut component_ids = BTreeSet::new();
    for c in &m.components {
        if c.id == "board" || !valid_id(&c.id) {
            error(
                "component.id",
                format!(
                    "component id `{}` must be a lowercase identifier and cannot be `board`",
                    c.id
                ),
                Some(c.id.clone()),
            );
        }
        if !component_ids.insert(c.id.clone()) {
            error(
                "component.duplicate",
                format!("duplicate component id `{}`", c.id),
                Some(c.id.clone()),
            );
        }
        if !c.verified {
            error(
                "component.unverified",
                format!("{} has not been verified against its datasheet", c.label),
                Some(c.id.clone()),
            );
        }
        let mut pins = BTreeSet::new();
        for pin in &c.pins {
            if pin.id.trim().is_empty() || !pins.insert(pin.id.clone()) {
                error(
                    "component.pin",
                    format!("{} has an empty or duplicate pin `{}`", c.label, pin.id),
                    Some(c.id.clone()),
                );
            }
        }
        let inductive = matches!(c.kind.as_str(), "relay" | "motor")
            || c.properties.get("inductive").is_some_and(|v| yes(v));
        if inductive && !c.properties.get("driver").is_some_and(|v| yes(v)) {
            error(
                "safety.driver",
                format!(
                    "{} is an inductive/load device but no rated driver is confirmed",
                    c.label
                ),
                Some(c.id.clone()),
            );
        }
        if inductive
            && !c
                .properties
                .get("flyback_protection")
                .is_some_and(|v| yes(v))
        {
            error(
                "safety.flyback",
                format!("{} needs confirmed flyback protection", c.label),
                Some(c.id.clone()),
            );
        }
        if c.kind == "led"
            && !c
                .properties
                .get("series_resistor_ohms")
                .is_some_and(|v| v.parse::<u32>().is_ok_and(|n| n > 0))
        {
            error(
                "safety.led_resistor",
                format!("{} needs a positive series_resistor_ohms value", c.label),
                Some(c.id.clone()),
            );
        }
        if c.kind == "i2c_sensor"
            && !matches!(
                c.properties.get("pullups").map(String::as_str),
                Some("onboard" | "external")
            )
        {
            error(
                "safety.i2c_pullups",
                format!(
                    "{} must record I²C pull-ups as `onboard` or `external`",
                    c.label
                ),
                Some(c.id.clone()),
            );
        }
    }

    let mut connection_ids = BTreeSet::new();
    let mut board_pin_uses: BTreeMap<String, Vec<&Connection>> = BTreeMap::new();
    let mut symbol_pins: BTreeMap<String, BTreeSet<u8>> = BTreeMap::new();
    for c in &m.connections {
        if !valid_id(&c.id) || !connection_ids.insert(c.id.clone()) {
            error(
                "connection.id",
                format!("connection id `{}` is invalid or duplicated", c.id),
                Some(c.id.clone()),
            );
        }
        for ep in [&c.from, &c.to] {
            if ep.target == "board" {
                if let Some(board) = board {
                    match board.pins.iter().find(|p| p.id == ep.pin) {
                        None => error(
                            "board.pin_unknown",
                            format!("{} references unknown board pin `{}`", c.id, ep.pin),
                            Some(c.id.clone()),
                        ),
                        Some(pin) => {
                            if pin.reserved {
                                error(
                                    "board.pin_reserved",
                                    format!("{} uses reserved {}", c.id, pin.label),
                                    Some(c.id.clone()),
                                );
                            }
                            if pin.strapping {
                                warnings.push(Diagnostic { severity: Severity::Warning, code: "board.pin_strapping".into(), message: format!("{} uses strapping pin {}; external levels can prevent boot", c.id, pin.label), subject: Some(c.id.clone()) });
                            }
                            let required = match c.role.as_str() {
                                "ground" => Some("ground"),
                                "power" => Some("power"),
                                "output" => Some("digital_output"),
                                "pwm" => Some("pwm"),
                                "input" => Some("digital_input"),
                                "adc" | "analog" => Some("adc"),
                                "i2c_sda" | "i2c_scl" => Some("digital_output"),
                                "signal" | "spi" | "uart" => Some("digital_input"),
                                _ => None,
                            };
                            if let Some(required) = required {
                                if !pin.capabilities.iter().any(|item| item == required) {
                                    error(
                                        "board.pin_capability",
                                        format!(
                                            "{} uses {} as `{}`, but that pin lacks the required `{required}` capability",
                                            c.id, pin.label, c.role
                                        ),
                                        Some(c.id.clone()),
                                    );
                                }
                            }
                        }
                    }
                }
                board_pin_uses.entry(ep.pin.clone()).or_default().push(c);
            } else if let Some(component) = m
                .components
                .iter()
                .find(|candidate| candidate.id == ep.target)
            {
                if !component.pins.is_empty() && !component.pins.iter().any(|p| p.id == ep.pin) {
                    error(
                        "component.pin_unknown",
                        format!(
                            "{} references unknown pin `{}` on {}",
                            c.id, ep.pin, component.label
                        ),
                        Some(c.id.clone()),
                    );
                }
            } else {
                error(
                    "connection.endpoint",
                    format!("{} references unknown component `{}`", c.id, ep.target),
                    Some(c.id.clone()),
                );
            }
        }
        if let Some(v) = c.voltage_v {
            let is_gpio = [&c.from, &c.to]
                .iter()
                .any(|ep| ep.target == "board" && ep.pin.starts_with("GPIO"));
            if is_gpio && v > 3.6 {
                error(
                    "voltage.gpio",
                    format!("{} applies {v:.2} V to a 3.3 V GPIO", c.id),
                    Some(c.id.clone()),
                );
            }
        }
        if let Some(symbol) = &c.firmware_symbol {
            if !valid_symbol(symbol) {
                error(
                    "firmware.symbol",
                    format!("firmware symbol `{symbol}` must be an uppercase C identifier"),
                    Some(c.id.clone()),
                );
            }
            if board_gpio(c).is_none() {
                error(
                    "firmware.pin",
                    format!("{} has a firmware symbol but no GPIO endpoint", c.id),
                    Some(c.id.clone()),
                );
            } else if let Some(gpio) = board_gpio(c) {
                symbol_pins.entry(symbol.clone()).or_default().insert(gpio);
            }
        }
    }
    for (symbol, pins) in symbol_pins {
        if pins.len() > 1 {
            error(
                "firmware.symbol_conflict",
                format!("firmware symbol `{symbol}` maps to multiple GPIOs: {pins:?}"),
                Some(symbol),
            );
        }
    }
    for (pin, uses) in board_pin_uses {
        if pin.starts_with("GPIO")
            && uses.len() > 1
            && !uses
                .iter()
                .all(|c| matches!(c.role.as_str(), "i2c_sda" | "i2c_scl"))
        {
            error(
                "board.pin_conflict",
                format!("{pin} is assigned to {} connections", uses.len()),
                Some(pin),
            );
        }
    }

    if let Some(board) = board {
        let board_powered: BTreeSet<&str> = m
            .connections
            .iter()
            .filter_map(|connection| {
                let (board_ep, other) = endpoints_with_board(connection)?;
                (board_ep.pin == "3V3").then_some(other.target.as_str())
            })
            .collect();
        let total: u32 = m
            .components
            .iter()
            .filter(|c| board_powered.contains(c.id.as_str()))
            .filter_map(|c| c.current_ma)
            .sum();
        if total > board.max_3v3_ma {
            error(
                "power.current_budget",
                format!(
                    "declared 3.3 V load is {total} mA; {} budget is {} mA",
                    board.label, board.max_3v3_ma
                ),
                Some("3V3".into()),
            );
        }
    }
    for component in &m.components {
        let powered = m.connections.iter().any(|c| {
            connects(c, "board", "3V3", &component.id) || connects(c, "board", "5V", &component.id)
        });
        let grounded = m
            .connections
            .iter()
            .any(|c| connects(c, "board", "GND", &component.id));
        if powered && !grounded {
            error(
                "power.missing_ground",
                format!(
                    "{} is powered from the board but has no board ground connection",
                    component.label
                ),
                Some(component.id.clone()),
            );
        }
        for connection in &m.connections {
            let Some((board_ep, other)) = endpoints_with_board(connection) else {
                continue;
            };
            if other.target != component.id || connection.role != "power" {
                continue;
            }
            let rail_voltage = match board_ep.pin.as_str() {
                "3V3" => Some(3.3),
                "5V" => Some(5.0),
                _ => None,
            };
            if let (Some(expected), Some(declared)) = (component.voltage_v, rail_voltage) {
                if (expected - declared).abs() > 0.35 {
                    error(
                        "power.voltage_mismatch",
                        format!(
                            "{} is declared for {expected:.2} V but is connected to the {declared:.1} V rail",
                            component.label
                        ),
                        Some(component.id.clone()),
                    );
                }
            }
        }
    }

    let symbols: Vec<&str> = m
        .connections
        .iter()
        .filter_map(|c| c.firmware_symbol.as_deref())
        .collect();
    if !symbols.is_empty() {
        let sources = source_text(project);
        if !sources.contains("bancada_circuit_pins.h") {
            error(
                "firmware.missing_include",
                format!(
                    "firmware must include `{HEADER_PATH}` when circuit pin symbols are present"
                ),
                Some(HEADER_PATH.into()),
            );
        }
        for symbol in symbols {
            if !sources.contains(symbol) {
                error(
                    "firmware.unused_symbol",
                    format!("firmware does not reference generated symbol `{symbol}`"),
                    Some(symbol.into()),
                );
            }
            if sources.lines().any(|line| {
                (line.contains("#define") || line.contains("constexpr"))
                    && line
                        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                        .any(|token| token == symbol)
            }) {
                error(
                    "firmware.duplicate_symbol",
                    format!("firmware redeclares generated symbol `{symbol}`"),
                    Some(symbol.into()),
                );
            }
        }
        for c in &m.connections {
            if let Some(gpio) = board_gpio(c) {
                for call in [
                    "pinMode",
                    "digitalWrite",
                    "digitalRead",
                    "analogRead",
                    "analogWrite",
                ] {
                    if call_uses_gpio_literal(&sources, call, gpio) {
                        error("firmware.raw_gpio", format!("firmware hard-codes GPIO {gpio} in {call}; use the generated symbol"), c.firmware_symbol.clone());
                    }
                }
            }
        }
    }
    out.extend(warnings);
    out
}

fn render_header(m: &CircuitManifest, digest: &str) -> String {
    let mut pins: Vec<(String, u8)> = m
        .connections
        .iter()
        .filter_map(|c| Some((c.firmware_symbol.clone()?, board_gpio(c)?)))
        .collect();
    pins.sort();
    pins.dedup();
    let mut out = format!("// Generated by Bancada {GENERATOR_VERSION}; manifest {digest}. DO NOT EDIT.\n#pragma once\n#include <stdint.h>\n\nnamespace bancada_circuit {{\n");
    for (symbol, gpio) in pins {
        out.push_str(&format!("inline constexpr uint8_t {symbol} = {gpio};\n"));
    }
    out.push_str("}  // namespace bancada_circuit\n");
    out
}

fn render_wiring(m: &CircuitManifest, report: &ValidationReport, digest: &str) -> String {
    let mut out = format!("# {} wiring\n\nGenerated by Bancada {} from manifest `{digest}`. Do not edit this file.\n\nBoard: **{}** (`{}`)\n\n## Connections\n\n| ID | From | To | Role | Voltage | Firmware | Notes |\n|---|---|---|---|---:|---|---|\n", m.name, GENERATOR_VERSION, m.board.catalog_id, m.board.fqbn);
    let mut connections: Vec<&Connection> = m.connections.iter().collect();
    connections.sort_by(|a, b| a.id.cmp(&b.id));
    for c in connections {
        out.push_str(&format!(
            "| {} | {}.{} | {}.{} | {} | {} | {} | {} |\n",
            md(&c.id),
            md(&c.from.target),
            md(&c.from.pin),
            md(&c.to.target),
            md(&c.to.pin),
            md(&c.role),
            c.voltage_v.map(|v| format!("{v:.2} V")).unwrap_or_default(),
            c.firmware_symbol.as_deref().map(md).unwrap_or_default(),
            c.notes.as_deref().map(md).unwrap_or_default()
        ));
    }
    out.push_str("\n## Validation\n\n");
    if report.diagnostics.is_empty() {
        out.push_str("No electrical or firmware-sync issues found.\n");
    }
    for d in &report.diagnostics {
        out.push_str(&format!(
            "- **{:?}** `{}` — {}\n",
            d.severity, d.code, d.message
        ));
    }
    out
}

fn render_bom(m: &CircuitManifest) -> String {
    let mut components: Vec<&Component> = m.components.iter().collect();
    components.sort_by(|a, b| a.id.cmp(&b.id));
    let mut out = "id,label,kind,part_number,voltage_v,current_ma,verified\n".to_string();
    for c in components {
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv(&c.id),
            csv(&c.label),
            csv(&c.kind),
            csv(c.part_number.as_deref().unwrap_or("")),
            c.voltage_v.map(|v| format!("{v:.2}")).unwrap_or_default(),
            c.current_ma.map(|v| v.to_string()).unwrap_or_default(),
            c.verified
        ));
    }
    out
}

fn render_svg(m: &CircuitManifest, digest: &str) -> String {
    let rows = m.connections.len().max(1);
    let height = 170 + rows * 52;
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="{height}" viewBox="0 0 1200 {height}" role="img" aria-label="{} circuit wiring">
<style>text{{font-family:ui-monospace,monospace;fill:#d8dee9}} .title{{font:700 22px system-ui}} .muted{{fill:#8f9bae}} .box{{fill:#1d2430;stroke:#5e81ac;stroke-width:2}} .wire{{stroke:#88c0d0;stroke-width:3}} .pin{{fill:#a3be8c}}</style>
<rect width="1200" height="{height}" fill="#111722"/><text x="32" y="38" class="title">{}</text><text x="32" y="64" class="muted">{} · manifest {digest}</text>
<rect x="32" y="92" rx="10" width="250" height="{}" class="box"/><text x="52" y="124" class="title">{}</text>
"##,
        xml(&m.name),
        xml(&m.name),
        xml(&m.board.fqbn),
        45 + rows * 52,
        xml(&m.board.catalog_id)
    );
    let mut connections: Vec<&Connection> = m.connections.iter().collect();
    connections.sort_by(|a, b| a.id.cmp(&b.id));
    for (idx, c) in connections.iter().enumerate() {
        let y = 158 + idx * 52;
        let (board_pin, other) = endpoints_with_board(c)
            .map(|(b, o)| (b.pin.as_str(), format!("{}.{}", o.target, o.pin)))
            .unwrap_or((
                "—",
                format!(
                    "{}.{} → {}.{}",
                    c.from.target, c.from.pin, c.to.target, c.to.pin
                ),
            ));
        out.push_str(&format!(r#"<circle cx="270" cy="{y}" r="5" class="pin"/><line x1="275" y1="{y}" x2="650" y2="{y}" class="wire"/><text x="52" y="{}">{}</text><text x="310" y="{}" class="muted">{} · {}</text><rect x="650" y="{}" rx="8" width="510" height="38" class="box"/><text x="670" y="{}">{}</text>
"#, y + 5, xml(board_pin), y - 8, xml(&c.id), xml(&c.role), y - 20, y + 5, xml(&other)));
    }
    if m.connections.is_empty() {
        out.push_str("<text x=\"52\" y=\"168\" class=\"muted\">No connections yet</text>\n");
    }
    out.push_str("</svg>\n");
    out
}

fn board_gpio(c: &Connection) -> Option<u8> {
    [&c.from, &c.to]
        .into_iter()
        .find(|e| e.target == "board")?
        .pin
        .strip_prefix("GPIO")?
        .parse()
        .ok()
}

fn endpoints_with_board(c: &Connection) -> Option<(&Endpoint, &Endpoint)> {
    if c.from.target == "board" {
        Some((&c.from, &c.to))
    } else if c.to.target == "board" {
        Some((&c.to, &c.from))
    } else {
        None
    }
}

fn connects(c: &Connection, a_target: &str, a_pin: &str, other_target: &str) -> bool {
    (c.from.target == a_target && c.from.pin == a_pin && c.to.target == other_target)
        || (c.to.target == a_target && c.to.pin == a_pin && c.from.target == other_target)
}

/// Where a source file sits relative to the sketch the user is writing.
///
/// Validation reads every file regardless (a `firmware_symbol` may legitimately
/// be used from a local library), but inference must not: a vendored
/// `Adafruit_BME280/examples/bme280test.ino` would otherwise manufacture a
/// sensor the user does not own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRole {
    /// The user's own code.
    Sketch,
    /// A library carried inside the project (`lib/`, `libraries/`, `.pio/`).
    Vendored,
    /// Example or test code, wherever it lives.
    Example,
}

/// One C++ source file of a project, with enough provenance to cite it.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Slash-separated, relative to the project root — quotable in a
    /// diagnostic as `rel_path:line`.
    pub rel_path: String,
    pub text: String,
    pub role: SourceRole,
}

/// Every C++ source file of a project, **sorted by `rel_path`**.
///
/// The sort is load-bearing rather than cosmetic: `read_dir` yields entries in
/// whatever order the filesystem hands back, and anything downstream that mints
/// ids or picks a "first" match would otherwise vary between runs on identical
/// input. Every existing consumer goes through `source_text`, whose checks are
/// order-independent, so imposing an order changes no current behaviour.
pub fn source_files(project: &Path) -> Vec<SourceFile> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<SourceFile>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if p.file_name()
                    .is_some_and(|n| n == "hardware" || n == ".git")
                {
                    continue;
                }
                walk(root, &p, out);
            } else if p.to_str().is_some_and(|s| {
                s.ends_with(".ino")
                    || s.ends_with(".cpp")
                    || s.ends_with(".c")
                    || s.ends_with(".h")
                    || s.ends_with(".hpp")
            }) && !p.ends_with(HEADER_PATH)
            {
                let Ok(text) = fs::read_to_string(&p) else {
                    continue;
                };
                let Ok(rel) = p.strip_prefix(root) else {
                    continue;
                };
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let dirs = &parts[..parts.len().saturating_sub(1)];
                let role = if dirs
                    .iter()
                    .any(|d| d == "examples" || d == "test" || d == "tests")
                {
                    SourceRole::Example
                } else if dirs
                    .iter()
                    .any(|d| d == "lib" || d == "libraries" || d == ".pio" || d == "build")
                {
                    SourceRole::Vendored
                } else {
                    SourceRole::Sketch
                };
                out.push(SourceFile {
                    rel_path: parts.join("/"),
                    text,
                    role,
                });
            }
        }
    }
    let mut out = Vec::new();
    walk(project, project, &mut out);
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out
}

/// Every C++ source of a project as one string, for the substring and
/// line-scanning checks `validate_manifest` runs.
///
/// Deliberately keeps reading *all* roles: a `firmware_symbol` used only from a
/// vendored library is still used, and narrowing this would turn
/// `firmware.unused_symbol` into a false alarm.
fn source_text(project: &Path) -> String {
    let mut out = String::new();
    for file in source_files(project) {
        out.push_str(&file.text);
        out.push('\n');
    }
    out
}

fn call_uses_gpio_literal(source: &str, call: &str, gpio: u8) -> bool {
    let mut remaining = source;
    while let Some(index) = remaining.find(call) {
        let boundary_ok = index == 0
            || remaining[..index]
                .chars()
                .next_back()
                .is_some_and(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        let after_name = &remaining[index + call.len()..];
        let Some(argument) = after_name.trim_start().strip_prefix('(') else {
            remaining = after_name;
            continue;
        };
        let argument = argument.trim_start();
        let digits: String = argument
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if boundary_ok && !digits.is_empty() && digits.parse::<u8>().ok() == Some(gpio) {
            return true;
        }
        remaining = after_name;
    }
    false
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_lowercase() || b.is_ascii_digit() && i > 0 || b == b'_' || b == b'-'
        })
}
fn valid_symbol(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_uppercase() || b.is_ascii_digit() && i > 0 || b == b'_')
}
fn yes(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "yes" | "true" | "1" | "confirmed"
    )
}
fn fqbn_target(s: &str) -> String {
    s.split(':').take(3).collect::<Vec<_>>().join(":")
}
fn fnv1a(bytes: &[u8]) -> String {
    let mut h = 0xcbf29ce484222325u64;
    for byte in bytes {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{h:016x}")
}
fn md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}
fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into()
    }
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(parent)?;
    let tmp: PathBuf = parent.join(format!(
        ".{}.bancada-tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn component(id: &str, kind: &str, pins: &[&str]) -> Component {
        Component {
            id: id.into(),
            label: id.into(),
            kind: kind.into(),
            part_number: None,
            voltage_v: Some(3.3),
            current_ma: Some(10),
            verified: true,
            pins: pins
                .iter()
                .map(|p| PartPin {
                    id: (*p).into(),
                    label: String::new(),
                })
                .collect(),
            properties: BTreeMap::new(),
        }
    }

    /// A project shaped like the ones the guess has to survive: sketch sources
    /// beside a vendored library and its bundled example, plus the two things
    /// the walk has always skipped.
    fn source_role_fixture() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let write = |rel: &str, text: &str| {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write("a.ino", "// sketch a\n");
        write("src/b.cpp", "// sketch b\n");
        write("libraries/Foo/Foo.cpp", "// vendored foo\n");
        write("examples/Bar/Bar.ino", "// example bar\n");
        write("hardware/x.h", "// generated, skipped\n");
        write(HEADER_PATH, "// generated header, skipped\n");
        write("notes.txt", "// not a source file\n");
        dir
    }

    #[test]
    fn source_files_are_sorted_and_carry_their_role() {
        let dir = source_role_fixture();
        let files = source_files(dir.path());
        let seen: Vec<(&str, SourceRole)> = files
            .iter()
            .map(|f| (f.rel_path.as_str(), f.role))
            .collect();
        // Sorted by rel_path, so the order is an assertion, not an accident:
        // `read_dir` order is nondeterministic and a guess built on it would
        // mint different component ids on different runs.
        assert_eq!(
            seen,
            vec![
                ("a.ino", SourceRole::Sketch),
                ("examples/Bar/Bar.ino", SourceRole::Example),
                ("libraries/Foo/Foo.cpp", SourceRole::Vendored),
                ("src/b.cpp", SourceRole::Sketch),
            ]
        );
    }

    #[test]
    fn source_text_still_sees_every_file_it_always_saw() {
        // Validation's semantics must not shift: `source_text` is what
        // firmware.* diagnostics read, and it has always included vendored and
        // example code. Only the guess narrows to SourceRole::Sketch.
        let dir = source_role_fixture();
        let text = source_text(dir.path());
        for expected in ["sketch a", "sketch b", "vendored foo", "example bar"] {
            assert!(text.contains(expected), "{expected} missing from {text:?}");
        }
        for skipped in [
            "generated, skipped",
            "generated header",
            "not a source file",
        ] {
            assert!(!text.contains(skipped), "{skipped} leaked into {text:?}");
        }
    }

    #[test]
    fn sync_is_deterministic_and_stale_files_block() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("demo.ino"),
            "#include \"src/bancada_circuit_pins.h\"\nvoid setup() { pinMode(bancada_circuit::PIN_SENSOR, INPUT); }\n",
        )
        .unwrap();
        let mut manifest = default_manifest(Some("esp32-c6-devkitc-1"), "Demo");
        manifest
            .components
            .push(component("sensor", "generic", &["SIG"]));
        manifest.connections.push(Connection {
            id: "sensor_signal".into(),
            from: Endpoint {
                target: "board".into(),
                pin: "GPIO4".into(),
            },
            to: Endpoint {
                target: "sensor".into(),
                pin: "SIG".into(),
            },
            role: "signal".into(),
            voltage_v: Some(3.3),
            firmware_symbol: Some("PIN_SENSOR".into()),
            notes: None,
        });
        let one = save_project(dir.path(), &manifest, Some("esp32:esp32:esp32c6")).unwrap();
        assert!(one.validation.valid);
        let header = fs::read(dir.path().join(HEADER_PATH)).unwrap();
        let two = sync_project(dir.path(), Some("esp32:esp32:esp32c6")).unwrap();
        assert!(two.validation.valid);
        assert_eq!(header, fs::read(dir.path().join(HEADER_PATH)).unwrap());
        fs::write(dir.path().join(HEADER_PATH), "edited").unwrap();
        let report = validate_project(dir.path(), Some("esp32:esp32:esp32c6"))
            .unwrap()
            .unwrap();
        assert!(!report.valid);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "artifact.stale"));
    }

    #[test]
    fn unsafe_voltage_and_profile_mismatch_are_errors() {
        let dir = tempdir().unwrap();
        let mut manifest = default_manifest(None, "Unsafe");
        manifest
            .components
            .push(component("sensor", "generic", &["SIG"]));
        manifest.connections.push(Connection {
            id: "bad".into(),
            from: Endpoint {
                target: "board".into(),
                pin: "GPIO21".into(),
            },
            to: Endpoint {
                target: "sensor".into(),
                pin: "SIG".into(),
            },
            role: "signal".into(),
            voltage_v: Some(5.0),
            firmware_symbol: None,
            notes: None,
        });
        let report = render(dir.path(), &manifest, Some("esp32:esp32:esp32c6"))
            .unwrap()
            .0;
        assert!(report.diagnostics.iter().any(|d| d.code == "voltage.gpio"));
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "board.profile_mismatch"));
    }

    #[test]
    fn pin_capability_and_power_voltage_are_enforced() {
        let dir = tempdir().unwrap();
        let mut manifest = default_manifest(Some("esp32-devkitc-v4"), "Capabilities");
        manifest
            .components
            .push(component("sensor", "generic", &["VCC", "OUT"]));
        manifest.components[0].voltage_v = Some(5.0);
        manifest.connections.push(Connection {
            id: "supply".into(),
            from: Endpoint {
                target: "board".into(),
                pin: "3V3".into(),
            },
            to: Endpoint {
                target: "sensor".into(),
                pin: "VCC".into(),
            },
            role: "power".into(),
            voltage_v: Some(3.3),
            firmware_symbol: None,
            notes: None,
        });
        manifest.connections.push(Connection {
            id: "drive".into(),
            from: Endpoint {
                target: "board".into(),
                pin: "GPIO34".into(),
            },
            to: Endpoint {
                target: "sensor".into(),
                pin: "OUT".into(),
            },
            role: "output".into(),
            voltage_v: Some(3.3),
            firmware_symbol: None,
            notes: None,
        });
        let report = render(dir.path(), &manifest, Some("esp32:esp32:esp32"))
            .unwrap()
            .0;
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "board.pin_capability"));
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "power.voltage_mismatch"));
    }

    #[test]
    fn projects_without_manifest_are_not_guarded() {
        let dir = tempdir().unwrap();
        require_build_ready(dir.path(), None).unwrap();
    }

    #[test]
    fn raw_gpio_scanner_handles_whitespace_without_prefix_false_positives() {
        assert!(call_uses_gpio_literal("pinMode( 4, OUTPUT);", "pinMode", 4));
        assert!(call_uses_gpio_literal("pinMode (4, OUTPUT);", "pinMode", 4));
        assert!(!call_uses_gpio_literal(
            "pinMode(42, OUTPUT);",
            "pinMode",
            4
        ));
        assert!(!call_uses_gpio_literal(
            "my_pinMode(4, OUTPUT);",
            "pinMode",
            4
        ));
        assert!(!call_uses_gpio_literal(
            "pinMode(PIN_LED, OUTPUT);",
            "pinMode",
            4
        ));
    }
}
