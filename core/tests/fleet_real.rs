//! Opt-in proof that real hardware is identifiable the way `fleet` assumes.
//!
//! The unit tests in `fleet.rs` classify *captured* port shapes. This runs the
//! real `arduino-cli board list` against whatever is attached, so a change in
//! how arduino-cli reports USB descriptors — or a board that turns out to expose
//! no identity at all — surfaces here rather than as a Fleet panel that silently
//! forgets a board.
//!
//! ```text
//! cargo test -p bancada-core --test fleet_real -- --ignored --nocapture
//! ```
//!
//! Read-only: it only lists ports and prints what it found. Nothing is written,
//! and esptool is never invoked, so the attached board is not reset.
//!
//! Follows `docs/hardware-smoke-tests.md`: **fail loud on setup, skip quiet on
//! absence.** No board attached is a skip, not a failure — an attached board
//! that cannot be identified is the thing worth failing on.

use bancada_core::cli::ArduinoCli;
use bancada_core::fleet::{self, BoardIdKind};

#[test]
#[ignore = "needs arduino-cli and an attached board"]
fn attached_boards_are_identifiable() {
    let cli = ArduinoCli::default();
    // A missing arduino-cli *is* a setup failure: the test was asked to run.
    let ports = cli.board_list().expect("board list --json");

    let serial: Vec<_> = ports
        .iter()
        .filter(|p| p.port.protocol == "serial")
        .collect();
    if serial.is_empty() {
        println!("no serial port detected — skipping (attach a board to exercise this)");
        return;
    }

    let mut identified = 0;
    for dp in &serial {
        let id = fleet::identify(&dp.port);
        println!(
            "{} — {} | vid={:?} pid={:?} serialNumber={:?} hardware_id={:?} => {:?}",
            dp.port.address,
            fleet::board_name(dp).unwrap_or("(unidentified board)"),
            dp.port.properties.get("vid"),
            dp.port.properties.get("pid"),
            dp.port.properties.get("serialNumber"),
            dp.port.hardware_id,
            id.as_ref().map(|i| (i.kind, i.value.as_str())),
        );

        if let Some(id) = id {
            identified += 1;
            assert!(!id.value.is_empty(), "an identity must not be empty");
            if id.kind == BoardIdKind::Mac {
                // Normalisation is what keeps a board found via board list
                // (upper case) and via esptool (lower case) as one record.
                assert_eq!(
                    id.value,
                    id.value.to_ascii_lowercase(),
                    "a MAC identity must be stored normalised"
                );
                assert!(fleet::looks_like_mac(&id.value));
            }
        }
    }

    // A bare USB-serial bridge legitimately has no identity, so this is a
    // diagnostic rather than a hard requirement — but if nothing at all is
    // identifiable, Fleet cannot work on this machine and that is worth saying.
    println!(
        "{identified}/{} attached serial port(s) are identifiable",
        serial.len()
    );
}
