//! Route Arduino `Serial` to the native USB port used for an upload.

/// A corrected FQBN only when the known ESP32 default would leave this
/// native USB port silent. Keep all other options, including unknown ones.
pub fn enabled_fqbn(port: &str, fqbn: &str) -> Option<String> {
    let native = port
        .strip_prefix("/dev/ttyACM")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
        || port.starts_with("/dev/cu.usbmodem")
        || port.starts_with("/dev/tty.usbmodem");
    if !native {
        return None;
    }
    let mut parts = fqbn.split(':');
    if parts.next()? != "esp32" || parts.next()? != "esp32" {
        return None;
    }
    if !matches!(
        parts.next()?,
        "esp32s2" | "esp32s3" | "esp32c3" | "esp32c6" | "esp32h2"
    ) {
        return None;
    }
    let options = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let mut options: Vec<&str> = options.map(|s| s.split(',').collect()).unwrap_or_default();
    if options.iter().any(|s| s.split_once('=').is_none()) {
        return None;
    }
    let cdc: Vec<_> = options
        .iter()
        .filter_map(|s| s.strip_prefix("CDCOnBoot="))
        .collect();
    if cdc.iter().any(|v| *v != "default") {
        return None;
    }
    options.retain(|s| !s.starts_with("CDCOnBoot="));
    options.push("CDCOnBoot=cdc");
    Some(format!(
        "{}:{}",
        fqbn.split(':').take(3).collect::<Vec<_>>().join(":"),
        options.join(",")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_native_usb_and_preserves_other_options() {
        for board in ["esp32s2", "esp32s3", "esp32c3", "esp32c6", "esp32h2"] {
            for port in [
                "/dev/ttyACM0",
                "/dev/cu.usbmodem14201",
                "/dev/tty.usbmodem14201",
            ] {
                assert_eq!(
                    enabled_fqbn(port, &format!("esp32:esp32:{board}")),
                    Some(format!("esp32:esp32:{board}:CDCOnBoot=cdc"))
                );
            }
        }
        assert_eq!(
            enabled_fqbn(
                "/dev/ttyACM0",
                "esp32:esp32:esp32s3:FlashSize=16M,CDCOnBoot=default,PSRAM=opi"
            ),
            Some("esp32:esp32:esp32s3:FlashSize=16M,PSRAM=opi,CDCOnBoot=cdc".into())
        );
    }

    #[test]
    fn leaves_working_unknown_and_malformed_setups_alone() {
        for fqbn in [
            "esp32:esp32:esp32s3:CDCOnBoot=cdc",
            "esp32:esp32:esp32s3:CDCOnBoot=unknown",
            "esp32:esp32:esp32",
            "other:esp32:esp32s3",
            "arduino:avr:uno",
            "esp32s3",
            "esp32:esp32:esp32s3:garbage",
            "esp32:esp32:esp32s3:FlashSize=16M:PSRAM=opi",
        ] {
            assert_eq!(enabled_fqbn("/dev/ttyACM0", fqbn), None, "{fqbn}");
        }
        for port in [
            "/dev/ttyUSB0",
            "/dev/cu.usbserial-001",
            "COM3",
            "192.168.1.1",
            "/dev/ttyACM",
            "/dev/ttyACMgarbage",
        ] {
            assert_eq!(enabled_fqbn(port, "esp32:esp32:esp32s3"), None, "{port}");
        }
    }
}
