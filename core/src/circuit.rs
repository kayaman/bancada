//! Simple electronic circuit validation against board pin profiles.
//!
//! Two progressive levels:
//! - **Level 1** — pin safety: every GPIO's caveats from `boardprofile`.
//! - **Level 2** — electrical rules: duplicate GPIO, I2C pull-ups, LED resistor.

use crate::boardprofile::{check_pin, Board, Caveat, PinVerdict};

// ── ConnectionRole ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionRole {
    DigitalOutput,
    DigitalInput,
    AnalogInput,
    I2cSda,
    I2cScl,
    SpiMosi,
    SpiMiso,
    SpiClk,
    SpiCs,
    UartTx,
    UartRx,
    Pwm,
    Neopixel,
}

impl ConnectionRole {
    /// Parse kebab-case (`"digital-output"`) or snake_case (`"digital_output"`).
    pub fn from_str(s: &str) -> Option<Self> {
        let norm = s.replace('-', "_");
        match norm.as_str() {
            "digital_output" => Some(Self::DigitalOutput),
            "digital_input" => Some(Self::DigitalInput),
            "analog_input" => Some(Self::AnalogInput),
            "i2c_sda" => Some(Self::I2cSda),
            "i2c_scl" => Some(Self::I2cScl),
            "spi_mosi" => Some(Self::SpiMosi),
            "spi_miso" => Some(Self::SpiMiso),
            "spi_clk" => Some(Self::SpiClk),
            "spi_cs" => Some(Self::SpiCs),
            "uart_tx" => Some(Self::UartTx),
            "uart_rx" => Some(Self::UartRx),
            "pwm" => Some(Self::Pwm),
            "neopixel" => Some(Self::Neopixel),
            _ => None,
        }
    }

    fn is_output(self) -> bool {
        matches!(
            self,
            Self::DigitalOutput
                | Self::I2cScl
                | Self::SpiMosi
                | Self::SpiClk
                | Self::SpiCs
                | Self::UartTx
                | Self::Pwm
                | Self::Neopixel
        )
    }

    fn is_analog_input(self) -> bool {
        matches!(self, Self::AnalogInput)
    }
}

// ── Connection / CircuitSpec ─────────────────────────────────────────────────

pub struct Connection {
    pub gpio: u8,
    pub role: ConnectionRole,
    pub component: Option<String>,
}

pub struct CircuitSpec {
    pub board: &'static Board,
    pub connections: Vec<Connection>,
}

// ── Issue / ValidationReport ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Error => "Error",
            Severity::Warn => "Warn",
            Severity::Info => "Info",
        }
    }
}

#[derive(Debug)]
pub struct Issue {
    pub severity: Severity,
    pub gpio: Option<u8>,
    pub message: String,
}

pub struct ValidationReport {
    pub board_name: &'static str,
    pub issues: Vec<Issue>,
    pub errors: u32,
    pub warnings: u32,
}

impl ValidationReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("## Circuit validation: {}\n\n", self.board_name));

        if self.issues.is_empty() {
            out.push_str("All connections look good.\n\n");
        } else {
            out.push_str("| Severity | GPIO | Message |\n");
            out.push_str("|----------|------|---------|\n");
            for issue in &self.issues {
                let gpio = issue
                    .gpio
                    .map(|g| g.to_string())
                    .unwrap_or_else(|| "—".to_string());
                out.push_str(&format!(
                    "| {} | {} | {} |\n",
                    issue.severity.label(),
                    gpio,
                    issue.message
                ));
            }
            out.push('\n');
        }

        out.push_str(&format!(
            "**Summary:** {} error(s), {} warning(s)\n",
            self.errors, self.warnings
        ));
        out
    }
}

// ── validate_circuit ─────────────────────────────────────────────────────────

pub fn validate_circuit(spec: &CircuitSpec) -> ValidationReport {
    let mut issues: Vec<Issue> = Vec::new();
    let mut seen_gpios: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();

    for (idx, conn) in spec.connections.iter().enumerate() {
        // Level 2a: duplicate GPIO
        if let Some(&first) = seen_gpios.get(&conn.gpio) {
            issues.push(Issue {
                severity: Severity::Error,
                gpio: Some(conn.gpio),
                message: format!(
                    "GPIO{} is assigned twice (connections {} and {})",
                    conn.gpio,
                    first,
                    idx
                ),
            });
        } else {
            seen_gpios.insert(conn.gpio, idx);
        }

        // Level 1: pin safety
        match check_pin(spec.board, conn.gpio) {
            PinVerdict::NotBrokenOut => {
                issues.push(Issue {
                    severity: Severity::Warn,
                    gpio: Some(conn.gpio),
                    message: format!(
                        "GPIO{} is not broken out on this board — you cannot wire to it",
                        conn.gpio
                    ),
                });
            }
            PinVerdict::Free => {}
            PinVerdict::Caution { caveats } => {
                for caveat in caveats {
                    let severity = caveat_severity(caveat, conn.role);
                    if let Some(sev) = severity {
                        issues.push(Issue {
                            severity: sev,
                            gpio: Some(conn.gpio),
                            message: format!(
                                "GPIO{}: {} — {}",
                                conn.gpio,
                                caveat.label(),
                                caveat.advice()
                            ),
                        });
                    }
                }
            }
        }

        // Level 2c: LED without current-limiting resistor
        if conn.role == ConnectionRole::DigitalOutput {
            if let Some(comp) = &conn.component {
                let comp_lower = comp.to_lowercase();
                let has_led = comp_lower.contains("led");
                // Check Ω on original (to_lowercase maps Ω→ω)
                let has_resistor = comp.contains('Ω')
                    || comp_lower.contains("ohm")
                    || comp_lower.contains("resistor");
                if has_led && !has_resistor {
                    issues.push(Issue {
                        severity: Severity::Warn,
                        gpio: Some(conn.gpio),
                        message: format!(
                            "GPIO{}: LED without current-limiting resistor — add a series resistor (e.g. 220Ω) to prevent damage",
                            conn.gpio
                        ),
                    });
                }
            }
        }
    }

    // Level 2b: I2C without pull-up resistors
    let has_i2c = spec.connections.iter().any(|c| {
        matches!(c.role, ConnectionRole::I2cSda | ConnectionRole::I2cScl)
    });
    if has_i2c {
        let has_pullup = spec.connections.iter().any(|c| {
            c.component.as_ref().map_or(false, |comp| {
                let lower = comp.to_lowercase();
                lower.contains("pull")
                    || comp.contains("kΩ")   // Ω→ω after to_lowercase
                    || lower.contains("kohm")
                    || lower.contains("resistor")
            })
        });
        if !has_pullup {
            issues.push(Issue {
                severity: Severity::Warn,
                gpio: None,
                message: "I2C bus: no pull-up resistor found in connection list — add 4.7kΩ pull-ups on SDA and SCL"
                    .to_string(),
            });
        }
    }

    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count() as u32;
    let warnings = issues
        .iter()
        .filter(|i| i.severity == Severity::Warn)
        .count() as u32;

    ValidationReport {
        board_name: spec.board.name,
        issues,
        errors,
        warnings,
    }
}

fn caveat_severity(caveat: Caveat, role: ConnectionRole) -> Option<Severity> {
    match caveat {
        Caveat::InputOnly if role.is_output() => Some(Severity::Error),
        Caveat::InputOnly => None,
        Caveat::FlashOrPsram => Some(Severity::Error),
        Caveat::UsbSerialJtag => Some(Severity::Warn),
        Caveat::Strapping if role.is_output() => Some(Severity::Warn),
        Caveat::Strapping => None,
        Caveat::Adc2WifiConflict if role.is_analog_input() => Some(Severity::Warn),
        Caveat::Adc2WifiConflict => None,
        _ => Some(Severity::Info),
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boardprofile::Board;

    fn find_board(id: &str) -> &'static Board {
        Board::from_id(id).unwrap_or_else(|| panic!("board {id} not found"))
    }

    #[test]
    fn from_str_kebab_and_snake() {
        assert_eq!(
            ConnectionRole::from_str("digital-output"),
            Some(ConnectionRole::DigitalOutput)
        );
        assert_eq!(
            ConnectionRole::from_str("digital_output"),
            Some(ConnectionRole::DigitalOutput)
        );
        assert_eq!(
            ConnectionRole::from_str("i2c-sda"),
            Some(ConnectionRole::I2cSda)
        );
        assert_eq!(
            ConnectionRole::from_str("i2c_scl"),
            Some(ConnectionRole::I2cScl)
        );
        assert_eq!(ConnectionRole::from_str("pwm"), Some(ConnectionRole::Pwm));
        assert_eq!(
            ConnectionRole::from_str("neopixel"),
            Some(ConnectionRole::Neopixel)
        );
    }

    #[test]
    fn from_str_all_variants() {
        let cases = [
            "digital-output",
            "digital-input",
            "analog-input",
            "i2c-sda",
            "i2c-scl",
            "spi-mosi",
            "spi-miso",
            "spi-clk",
            "spi-cs",
            "uart-tx",
            "uart-rx",
            "pwm",
            "neopixel",
        ];
        for s in cases {
            assert!(
                ConnectionRole::from_str(s).is_some(),
                "from_str failed for {s}"
            );
        }
    }

    #[test]
    fn from_str_unknown_returns_none() {
        assert_eq!(ConnectionRole::from_str(""), None);
        assert_eq!(ConnectionRole::from_str("led"), None);
        assert_eq!(ConnectionRole::from_str("gpio"), None);
    }

    #[test]
    fn known_good_circuit_has_no_issues() {
        let board = find_board("esp32-s3-devkitc-1");
        // GPIO 4 is a free general-purpose pin on most ESP32-S3 boards
        let spec = CircuitSpec {
            board,
            connections: vec![Connection {
                gpio: 4,
                role: ConnectionRole::DigitalOutput,
                component: Some("LED + 220Ω".to_string()),
            }],
        };
        let report = validate_circuit(&spec);
        assert_eq!(report.errors, 0);
        assert_eq!(report.warnings, 0);
    }

    #[test]
    fn duplicate_gpio_is_error() {
        let board = find_board("esp32-s3-devkitc-1");
        let spec = CircuitSpec {
            board,
            connections: vec![
                Connection {
                    gpio: 4,
                    role: ConnectionRole::DigitalOutput,
                    component: None,
                },
                Connection {
                    gpio: 4,
                    role: ConnectionRole::DigitalInput,
                    component: None,
                },
            ],
        };
        let report = validate_circuit(&spec);
        assert!(
            report.errors >= 1,
            "expected at least one error for duplicate GPIO"
        );
        assert!(report
            .issues
            .iter()
            .any(|i| i.severity == Severity::Error && i.gpio == Some(4)));
    }

    #[test]
    fn led_without_resistor_warns() {
        let board = find_board("esp32-s3-devkitc-1");
        let spec = CircuitSpec {
            board,
            connections: vec![Connection {
                gpio: 4,
                role: ConnectionRole::DigitalOutput,
                component: Some("LED".to_string()),
            }],
        };
        let report = validate_circuit(&spec);
        assert!(
            report.warnings >= 1,
            "expected warning for LED without resistor"
        );
        let has_led_warn = report.issues.iter().any(|i| {
            i.severity == Severity::Warn
                && i.gpio == Some(4)
                && i.message.contains("resistor")
        });
        assert!(has_led_warn, "LED resistor warning not found");
    }

    #[test]
    fn led_with_resistor_no_warn() {
        let board = find_board("esp32-s3-devkitc-1");
        let spec = CircuitSpec {
            board,
            connections: vec![Connection {
                gpio: 4,
                role: ConnectionRole::DigitalOutput,
                component: Some("LED + 220Ω".to_string()),
            }],
        };
        let report = validate_circuit(&spec);
        let has_led_warn = report.issues.iter().any(|i| {
            i.severity == Severity::Warn && i.message.contains("resistor")
        });
        assert!(!has_led_warn, "unexpected LED resistor warning");
    }

    #[test]
    fn i2c_without_pullup_warns() {
        let board = find_board("esp32-s3-devkitc-1");
        let spec = CircuitSpec {
            board,
            connections: vec![
                Connection {
                    gpio: 8,
                    role: ConnectionRole::I2cSda,
                    component: Some("SHT31".to_string()),
                },
                Connection {
                    gpio: 9,
                    role: ConnectionRole::I2cScl,
                    component: Some("SHT31".to_string()),
                },
            ],
        };
        let report = validate_circuit(&spec);
        let has_pullup_warn = report
            .issues
            .iter()
            .any(|i| i.severity == Severity::Warn && i.message.contains("pull-up"));
        assert!(has_pullup_warn, "I2C pull-up warning not found");
    }

    #[test]
    fn i2c_with_pullup_component_no_warn() {
        let board = find_board("esp32-s3-devkitc-1");
        let spec = CircuitSpec {
            board,
            connections: vec![
                Connection {
                    gpio: 8,
                    role: ConnectionRole::I2cSda,
                    component: Some("SHT31".to_string()),
                },
                Connection {
                    gpio: 9,
                    role: ConnectionRole::I2cScl,
                    component: Some("SHT31".to_string()),
                },
                Connection {
                    gpio: 8,
                    role: ConnectionRole::DigitalInput,
                    component: Some("4.7kohm pull-up".to_string()),
                },
            ],
        };
        let report = validate_circuit(&spec);
        let has_pullup_warn = report
            .issues
            .iter()
            .any(|i| i.severity == Severity::Warn && i.message.contains("pull-up"));
        assert!(!has_pullup_warn, "unexpected I2C pull-up warning");
    }

    #[test]
    fn to_markdown_has_summary_line() {
        let board = find_board("esp32-s3-devkitc-1");
        let spec = CircuitSpec {
            board,
            connections: vec![Connection {
                gpio: 4,
                role: ConnectionRole::DigitalOutput,
                component: Some("LED".to_string()),
            }],
        };
        let md = validate_circuit(&spec).to_markdown();
        assert!(md.contains("Summary:"), "markdown missing Summary line");
        assert!(md.contains("warning"), "markdown missing warning count");
    }
}
