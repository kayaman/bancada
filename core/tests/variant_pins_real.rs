//! Opt-in proof that the real installed ESP32 core says what `variantpins`
//! assumes it says.
//!
//! The unit tests in `variantpins.rs` parse *captured* header text. This reads
//! the header actually installed on this machine, so a core release that
//! renames a symbol, moves a variant directory, or changes a pin number
//! surfaces here — rather than as a wiring proposal that quietly points at the
//! wrong GPIO.
//!
//! ```text
//! cargo test -p bancada-core --test variant_pins_real -- --ignored --nocapture
//! ```
//!
//! Read-only: it resolves `directories.data` and reads text files. Nothing is
//! written and no board is touched.
//!
//! Follows `docs/hardware-smoke-tests.md`: **fail loud on setup, skip quiet on
//! absence.** No ESP32 core installed is a skip; an installed core whose
//! header contradicts the design is the thing worth failing on.

use bancada_core::variantpins::{self, CoreSymbol};
use std::path::PathBuf;

fn data_dir() -> Option<PathBuf> {
    // `directories.data` defaults to ~/.arduino15 and is where arduino-cli
    // unpacks cores. Resolving it directly keeps this test independent of a
    // working arduino-cli invocation.
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".arduino15");
    dir.is_dir().then_some(dir)
}

#[test]
#[ignore = "needs an installed esp32 core"]
fn the_installed_s3_core_defines_led_builtin_without_a_literal() {
    let Some(data) = data_dir() else {
        eprintln!("skip: no ~/.arduino15");
        return;
    };
    let pins = variantpins::load(&data, "esp32:esp32:esp32s3");
    if pins.is_empty() {
        eprintln!("skip: no esp32 core installed");
        return;
    }
    eprintln!("read {}", pins.source);

    // The trap this whole feature is shaped around: the core defines
    // LED_BUILTIN itself (so blink's `#ifndef` guard does not fire), but as an
    // expression, so there is no literal to propose.
    let led = pins.lookup("LED_BUILTIN");
    assert!(
        led.is_defined(),
        "the S3 core must define LED_BUILTIN, else blink's #ifndef fires and \
         a proposal would claim GPIO2: got {led:?}"
    );
    assert_eq!(
        led,
        CoreSymbol::DefinedNonLiteral,
        "LED_BUILTIN resolved to a literal on the S3 — if the core changed to \
         a plain GPIO this is good news, but the guess's S3 handling and its \
         tests need revisiting"
    );

    // The aliases that must resolve, or I2C sensors infer no wires at all.
    for (symbol, expected) in [("SDA", 8), ("SCL", 9), ("A0", 1)] {
        assert_eq!(
            pins.lookup(symbol),
            CoreSymbol::Defined(expected),
            "{symbol} on esp32s3"
        );
    }
}

#[test]
#[ignore = "needs an installed esp32 core"]
fn the_classic_esp32_core_disagrees_with_the_s3_as_expected() {
    let Some(data) = data_dir() else {
        eprintln!("skip: no ~/.arduino15");
        return;
    };
    let pins = variantpins::load(&data, "esp32:esp32:esp32");
    if pins.is_empty() {
        eprintln!("skip: no esp32 core installed");
        return;
    }
    eprintln!("read {}", pins.source);

    // If these ever matched the S3's, a single hard-coded alias table would
    // have been fine and this module would be unnecessary. They do not.
    for (symbol, expected) in [("SDA", 21), ("SCL", 22), ("A0", 36)] {
        assert_eq!(
            pins.lookup(symbol),
            CoreSymbol::Defined(expected),
            "{symbol} on classic esp32"
        );
    }
}
