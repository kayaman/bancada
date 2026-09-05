//! Identity keys for the hotplug watcher's port-set comparison.
//!
//! Pure: computed from [`serialport::SerialPortInfo`] values the polling
//! layer already fetched — no enumeration here, so it is unit testable
//! headlessly (same split as `boards.rs` over `Platform`).
//!
//! Why identity and not just the name: swapping the USB-C connector on an
//! ESP32-C6 DevKit replaces the native USB-Serial/JTAG device (303a:1001)
//! with the on-board CH343 bridge (1a86:55d3), and the newcomer reuses
//! `/dev/ttyACM0`. A name-only set is byte-identical across that swap, so
//! no `ports://changed` was emitted and the frontend kept showing the old
//! board on the new device — at any poll rate, by construction.

use serialport::{SerialPortInfo, SerialPortType};
use std::collections::{BTreeMap, BTreeSet};

use crate::types::{DetectedPort, Port};

/// Attached ports as arduino-cli would report them, minus the identification.
///
/// This is the port list when `arduino-cli` is not installed — an ESP-IDF-only
/// bench has no reason to have it, and without this the port picker stayed
/// empty and every rescan raised a "could not find arduino-cli" toast.
/// `matching_boards` is empty for every entry: identification is
/// arduino-cli's job and nothing here guesses at it. The `properties` keys and
/// value formats (`0x303A`) match arduino-cli's so [`crate::fleet`] can read a
/// fallback port the same way it reads a real one.
pub fn detected_from_serial(ports: &[SerialPortInfo]) -> Vec<DetectedPort> {
    let mut out: Vec<DetectedPort> = ports
        .iter()
        .map(|p| {
            let mut properties = BTreeMap::new();
            let mut hardware_id = String::new();
            let protocol_label = match &p.port_type {
                SerialPortType::UsbPort(u) => {
                    properties.insert("vid".to_string(), format!("0x{:04X}", u.vid));
                    properties.insert("pid".to_string(), format!("0x{:04X}", u.pid));
                    if let Some(s) = &u.serial_number {
                        properties.insert("serialNumber".to_string(), s.clone());
                        hardware_id = s.clone();
                    }
                    "Serial Port (USB)"
                }
                _ => "Serial Port",
            };
            DetectedPort {
                port: Port {
                    address: p.port_name.clone(),
                    label: p.port_name.clone(),
                    protocol: "serial".to_string(),
                    protocol_label: protocol_label.to_string(),
                    properties,
                    hardware_id,
                },
                matching_boards: Vec::new(),
            }
        })
        .collect();
    out.sort_by(|a, b| a.port.address.cmp(&b.port.address));
    out
}

/// A stable identity for one attached port: the device name plus, for USB
/// ports, `vid:pid:serial`. A bridge without a serial number degrades to
/// `vid:pid` — that still distinguishes the two sides of the C6 DevKit swap
/// (different silicon), though two identical serial-less bridges swapped
/// between the same names remain indistinguishable (documented limit).
pub fn port_key(p: &SerialPortInfo) -> String {
    match &p.port_type {
        SerialPortType::UsbPort(u) => match &u.serial_number {
            Some(s) => format!("{}|{:04x}:{:04x}:{}", p.port_name, u.vid, u.pid, s),
            None => format!("{}|{:04x}:{:04x}", p.port_name, u.vid, u.pid),
        },
        _ => p.port_name.clone(),
    }
}

/// Whether the port set changed since the previous poll tick.
/// `prev == None` is the seed tick: never a change (boards present at
/// launch are not arrivals — the frontend does its own initial scan).
pub fn ports_changed(prev: Option<&BTreeSet<String>>, next: &BTreeSet<String>) -> bool {
    prev.is_some_and(|p| p != next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::UsbPortInfo;

    fn usb(name: &str, vid: u16, pid: u16, serial: Option<&str>) -> SerialPortInfo {
        SerialPortInfo {
            port_name: name.to_string(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid,
                pid,
                serial_number: serial.map(str::to_string),
                manufacturer: None,
                product: None,
            }),
        }
    }

    fn keyset(ports: &[SerialPortInfo]) -> BTreeSet<String> {
        ports.iter().map(port_key).collect()
    }

    #[test]
    fn same_name_different_device_is_a_change() {
        // The bench case: native USB-Serial/JTAG (303a:1001) swapped for the
        // CH343 bridge (1a86:55d3), both landing on /dev/ttyACM0.
        let before = keyset(&[usb(
            "/dev/ttyACM0",
            0x303a,
            0x1001,
            Some("20:6E:F1:08:9F:80"),
        )]);
        let after = keyset(&[usb("/dev/ttyACM0", 0x1a86, 0x55d3, Some("5B8E084513"))]);
        assert!(ports_changed(Some(&before), &after));
    }

    #[test]
    fn identical_ports_are_not_a_change() {
        let a = keyset(&[usb(
            "/dev/ttyACM0",
            0x303a,
            0x1001,
            Some("20:6E:F1:08:9F:80"),
        )]);
        let b = keyset(&[usb(
            "/dev/ttyACM0",
            0x303a,
            0x1001,
            Some("20:6E:F1:08:9F:80"),
        )]);
        assert!(!ports_changed(Some(&a), &b));
    }

    #[test]
    fn seed_tick_is_never_a_change() {
        let a = keyset(&[usb("/dev/ttyACM0", 0x303a, 0x1001, None)]);
        assert!(!ports_changed(None, &a));
    }

    #[test]
    fn two_boards_permuting_names_is_a_change() {
        // Set semantics alone would hide a pure name permutation; the key
        // binds name to identity, so a permutation differs.
        let before = keyset(&[
            usb("/dev/ttyACM0", 0x303a, 0x1001, Some("aa")),
            usb("/dev/ttyACM1", 0x1a86, 0x55d3, Some("bb")),
        ]);
        let after = keyset(&[
            usb("/dev/ttyACM0", 0x1a86, 0x55d3, Some("bb")),
            usb("/dev/ttyACM1", 0x303a, 0x1001, Some("aa")),
        ]);
        assert!(ports_changed(Some(&before), &after));
    }

    #[test]
    fn serialless_bridge_degrades_to_vid_pid() {
        // No serial number: the key still carries vid:pid, so swapping a
        // CH343 for a CP2102 on the same name is visible…
        let ch343 = keyset(&[usb("/dev/ttyACM0", 0x1a86, 0x55d3, None)]);
        let cp2102 = keyset(&[usb("/dev/ttyUSB0", 0x10c4, 0xea60, None)]);
        assert!(ports_changed(Some(&ch343), &cp2102));
        // …while two indistinguishable serial-less bridges are, by design,
        // not a change (nothing observable distinguishes them).
        let again = keyset(&[usb("/dev/ttyACM0", 0x1a86, 0x55d3, None)]);
        assert!(!ports_changed(Some(&ch343), &again));
    }

    #[test]
    fn fallback_ports_carry_arduino_cli_shaped_usb_properties() {
        let ports = [
            usb("/dev/ttyUSB0", 0x10c4, 0xea60, None),
            usb("/dev/ttyACM0", 0x303a, 0x1001, Some("44:1B:F6:CE:A3:B8")),
        ];
        let out = detected_from_serial(&ports);
        // Sorted by address, so the picker is stable across polls.
        assert_eq!(out[0].port.address, "/dev/ttyACM0");
        assert_eq!(out[0].port.protocol, "serial");
        assert_eq!(out[0].port.protocol_label, "Serial Port (USB)");
        assert_eq!(out[0].port.properties["vid"], "0x303A");
        assert_eq!(out[0].port.properties["pid"], "0x1001");
        assert_eq!(out[0].port.properties["serialNumber"], "44:1B:F6:CE:A3:B8");
        assert_eq!(out[0].port.hardware_id, "44:1B:F6:CE:A3:B8");
        assert!(out[0].matching_boards.is_empty(), "nothing here identifies");
        // A serial-less bridge has no serialNumber key at all, matching
        // arduino-cli, so fleet's "unidentified" path is taken.
        assert!(!out[1].port.properties.contains_key("serialNumber"));
        assert_eq!(out[1].port.hardware_id, "");
    }

    #[test]
    fn a_non_usb_fallback_port_has_no_usb_properties() {
        let p = SerialPortInfo {
            port_name: "/dev/ttyS0".to_string(),
            port_type: SerialPortType::Unknown,
        };
        let out = detected_from_serial(&[p]);
        assert_eq!(out[0].port.protocol_label, "Serial Port");
        assert!(out[0].port.properties.is_empty());
    }

    #[test]
    fn non_usb_port_keys_by_name_alone() {
        let p = SerialPortInfo {
            port_name: "/dev/ttyS0".to_string(),
            port_type: SerialPortType::Unknown,
        };
        assert_eq!(port_key(&p), "/dev/ttyS0");
    }
}
