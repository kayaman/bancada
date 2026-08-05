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
    find_esptool_among(&["esptool", "esptool.py"])
}

/// Probe `candidates` in order; the first that answers `version` wins.
/// Split from `find_esptool` so tests can probe absolute paths instead of
/// mutating PATH.
fn find_esptool_among(candidates: &[&str]) -> Result<String> {
    for candidate in candidates {
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
    read_mac_with(&find_esptool()?, port)
}

/// The testable body of `read_mac`: `bin` is the esptool entry point.
fn read_mac_with(bin: &str, port: &str) -> Result<ChipInfo> {
    let out = Command::new(bin)
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

/// The MAC address, tolerating both output generations.
///
/// esptool ≤4 prints `MAC: aa:bb:…`. esptool 5 prints the label padded to 20
/// columns (`f"{label + ':':<20}{mac}"`), and on some targets the only label is
/// `BASE MAC` — the C6/H2/H4 families print `BASE MAC`, `EUI64 MAC` and
/// `EXT_MAC` and no bare `MAC` line at all. A plain `MAC:` is preferred when
/// present; `BASE MAC:` is the fallback, since it is the same factory address.
fn parse_mac(output: &str) -> Option<String> {
    let labelled = |label: &str| -> Option<String> {
        output.lines().find_map(|l| {
            l.trim()
                .strip_prefix(label)
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
        })
    };
    labelled("MAC:").or_else(|| labelled("BASE MAC:"))
}

/// The chip description.
///
/// esptool ≤4 prints `Chip is ESP32-S3 (…)`; esptool 5 prints
/// `Chip type:          ESP32-S3 (…)`. Only matching the older spelling meant
/// the chip type silently vanished from the UI on any modern install.
fn parse_chip_type(output: &str) -> Option<String> {
    output.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("Chip type:")
            .or_else(|| l.strip_prefix("Chip is"))
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
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

    /// esptool 5 pads the label to 20 columns: `f"{label + ':':<20}{value}"`.
    const SAMPLE_V5: &str = "\
esptool v5.2.0
Connected to ESP32-S3 on /dev/ttyUSB0:
Chip type:          ESP32-S3 (QFN56) (revision v0.2)
Features:           Wi-Fi, BLE, Embedded PSRAM 8MB
Crystal frequency:  40MHz
MAC:                68:b6:b3:2d:f0:1c
Hard resetting via RTS pin...
";

    /// C6/H2/H4 report several MACs and no bare `MAC:` line.
    const SAMPLE_V5_BASE_MAC: &str = "\
esptool v5.2.0
Chip type:          ESP32-C6 (QFN40) (revision v0.1)
BASE MAC:           60:55:f9:f7:2c:a2
EUI64 MAC:          60:55:f9:ff:fe:f7:2c:a2
EXT_MAC:            ff:fe
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
    fn parses_esptool_v5_output() {
        // Regression guard: v5 renamed `Chip is` to `Chip type:`, so matching
        // only the old spelling silently dropped the chip type on every modern
        // install (v5.2.0 is what is on this machine).
        assert_eq!(parse_mac(SAMPLE_V5).as_deref(), Some("68:b6:b3:2d:f0:1c"));
        assert_eq!(
            parse_chip_type(SAMPLE_V5).as_deref(),
            Some("ESP32-S3 (QFN56) (revision v0.2)")
        );
    }

    #[test]
    fn falls_back_to_base_mac_when_there_is_no_bare_mac_line() {
        assert_eq!(
            parse_mac(SAMPLE_V5_BASE_MAC).as_deref(),
            Some("60:55:f9:f7:2c:a2")
        );
        assert_eq!(
            parse_chip_type(SAMPLE_V5_BASE_MAC).as_deref(),
            Some("ESP32-C6 (QFN40) (revision v0.1)")
        );
    }

    #[test]
    fn prefers_a_bare_mac_over_base_mac() {
        // Both present: the bare MAC is the one esptool considers primary.
        let both = "BASE MAC:  aa:aa:aa:aa:aa:aa\nMAC:  bb:bb:bb:bb:bb:bb\n";
        assert_eq!(parse_mac(both).as_deref(), Some("bb:bb:bb:bb:bb:bb"));
    }

    #[test]
    fn does_not_match_eui64_or_ext_mac_as_the_address() {
        // `EUI64 MAC:` and `EXT_MAC:` must not be mistaken for the factory MAC.
        let only_derived = "EUI64 MAC:  60:55:f9:ff:fe:f7:2c:a2\nEXT_MAC:  ff:fe\n";
        assert_eq!(parse_mac(only_derived), None);
    }

    #[test]
    fn no_mac_returns_none() {
        assert_eq!(parse_mac("Connecting...\nerror"), None);
    }

    #[test]
    fn empty_values_are_not_accepted() {
        // A label with nothing after it is not a MAC.
        assert_eq!(parse_mac("MAC:\n"), None);
        assert_eq!(parse_chip_type("Chip type:   \n"), None);
    }

    #[test]
    fn tolerates_crlf_line_endings() {
        let crlf = "Chip type:  ESP32\r\nMAC:  aa:bb:cc:dd:ee:ff\r\n";
        assert_eq!(parse_mac(crlf).as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(parse_chip_type(crlf).as_deref(), Some("ESP32"));
    }

    #[test]
    fn chip_type_missing_is_none_not_a_panic() {
        assert_eq!(parse_chip_type("MAC: aa:bb:cc:dd:ee:ff"), None);
    }

    // ---------- subprocess paths, via a fake esptool script ----------

    /// A fake esptool at an absolute path; `body` is its shell body.
    fn with_fake_esptool<T>(body: &str, f: impl FnOnce(&str) -> T) -> T {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-esptool");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        f(script.to_str().unwrap())
    }

    /// Exec-ing a just-written script can hit ETXTBSY when a parallel test
    /// forks while the write fd is momentarily open; retry until the fd is
    /// gone. Test-only — real esptool binaries are not being written to.
    fn retry_busy<T>(mut op: impl FnMut() -> crate::Result<T>) -> crate::Result<T> {
        for _ in 0..50 {
            match op() {
                Err(Error::Io(e)) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                r => return r,
            }
        }
        op()
    }

    #[test]
    fn find_esptool_among_picks_the_first_answering_candidate() {
        with_fake_esptool("exit 0", |script| {
            // The probe treats a failed spawn as "not this one", so an
            // ETXTBSY race reads as ToolMissing — retry like retry_busy does.
            let found = (0..50)
                .find_map(|_| {
                    find_esptool_among(&["/nonexistent/esptool", script])
                        .ok()
                        .or_else(|| {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                            None
                        })
                })
                .expect("fake esptool never became executable");
            assert_eq!(found, script);
        });
    }

    #[test]
    fn find_esptool_among_skips_a_candidate_whose_version_probe_fails() {
        with_fake_esptool("exit 1", |script| {
            assert!(find_esptool_among(&[script]).is_err());
        });
    }

    #[test]
    fn find_esptool_among_reports_tool_missing_with_the_install_hint() {
        let err = find_esptool_among(&["/nonexistent/esptool"]).unwrap_err();
        match err {
            Error::ToolMissing(msg) => assert!(msg.contains("pip install esptool"), "{msg}"),
            other => panic!("expected ToolMissing, got {other:?}"),
        }
    }

    #[test]
    fn read_mac_with_parses_a_successful_run() {
        let body = "cat <<'EOF'\nChip is ESP32-S3 (QFN56)\nMAC: 68:b6:b3:2d:f0:1c\nEOF";
        with_fake_esptool(body, |script| {
            let info = retry_busy(|| read_mac_with(script, "/dev/ttyACM0")).unwrap();
            assert_eq!(info.mac, "68:b6:b3:2d:f0:1c");
            assert_eq!(info.chip_type.as_deref(), Some("ESP32-S3 (QFN56)"));
            assert!(info.raw_output.contains("MAC: 68:b6:b3:2d:f0:1c"));
        });
    }

    #[test]
    fn read_mac_with_failing_exit_reports_tool_failed_with_stderr() {
        with_fake_esptool("echo 'serial port busy' >&2; exit 2", |script| {
            match retry_busy(|| read_mac_with(script, "/dev/ttyACM0")).unwrap_err() {
                Error::ToolFailed { status, stderr, .. } => {
                    assert_eq!(status, 2);
                    assert!(stderr.contains("serial port busy"), "{stderr}");
                }
                other => panic!("expected ToolFailed, got {other:?}"),
            }
        });
    }

    #[test]
    fn read_mac_with_no_mac_in_output_is_an_error_carrying_the_output() {
        with_fake_esptool("echo 'Connecting...'", |script| {
            let err = retry_busy(|| read_mac_with(script, "/dev/ttyACM0")).unwrap_err();
            assert!(err.to_string().contains("no MAC address"), "{err}");
            assert!(err.to_string().contains("Connecting..."), "{err}");
        });
    }
}
