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
use std::collections::BTreeSet;

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
    fn non_usb_port_keys_by_name_alone() {
        let p = SerialPortInfo {
            port_name: "/dev/ttyS0".to_string(),
            port_type: SerialPortType::Unknown,
        };
        assert_eq!(port_key(&p), "/dev/ttyS0");
    }
}
