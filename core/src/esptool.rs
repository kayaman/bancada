//! Board utilities powered by `esptool` (ESP8266/ESP32 family).
//!
//! This is the "utilities" drawer of Bancada: things the official IDE hides.
//! First tool: read the board's factory MAC address — handy for MQTT client
//! IDs, AWS IoT thing names, and DHCP reservations.

use std::process::{Command, Stdio};

use serde::Serialize;

use crate::{Error, Result};

#[derive(Debug, Clone, Serialize)]
pub struct ChipInfo {
    pub mac: String,
    pub chip_type: Option<String>,
    /// Raw esptool output, for the expandable "details" view in the UI.
    pub raw_output: String,
}

/// Locate a working esptool entry point. Modern installs ship `esptool`,
/// older pip installs ship `esptool.py`.
fn find_esptool() -> Result<String> {
    for candidate in ["esptool", "esptool.py"] {
        let ok = Command::new(candidate)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Ok(candidate.to_string());
        }
    }
    Err(Error::ToolMissing(
        "esptool (install with: pip install esptool)".to_string(),
    ))
}

/// Read the MAC address (and chip type) of the ESP on `port`.
///
/// Note: esptool talks to the ROM bootloader, so the running sketch is
/// interrupted; the chip resets back into the app afterwards.
pub fn read_mac(port: &str) -> Result<ChipInfo> {
    let bin = find_esptool()?;
    let out = Command::new(&bin)
        .args(["--port", port, "read_mac"])
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(Error::ToolFailed {
            tool: format!("{bin} read_mac"),
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }

    let mac = parse_mac(&stdout).ok_or_else(|| {
        Error::Other(format!("no MAC address found in esptool output:\n{stdout}"))
    })?;
    let chip_type = parse_chip_type(&stdout);

    Ok(ChipInfo {
        mac,
        chip_type,
        raw_output: stdout,
    })
}

fn parse_mac(output: &str) -> Option<String> {
    output.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("MAC:").map(|m| m.trim().to_string())
    })
}

fn parse_chip_type(output: &str) -> Option<String> {
    output.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("Chip is").map(|c| c.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
esptool.py v4.7.0
Serial port /dev/ttyACM0
Connecting...
Chip is ESP32-S3 (QFN56) (revision v0.2)
Features: WiFi, BLE, Embedded PSRAM 8MB (AP_3v3)
Crystal is 40MHz
MAC: 68:b6:b3:2d:f0:1c
Hard resetting via RTS pin...
";

    #[test]
    fn parses_mac_and_chip() {
        assert_eq!(parse_mac(SAMPLE).as_deref(), Some("68:b6:b3:2d:f0:1c"));
        assert_eq!(
            parse_chip_type(SAMPLE).as_deref(),
            Some("ESP32-S3 (QFN56) (revision v0.2)")
        );
    }

    #[test]
    fn no_mac_returns_none() {
        assert_eq!(parse_mac("Connecting...\nerror"), None);
    }
}
