//! What the *core* says a pin name means.
//!
//! Sketches are written in the Arduino core's vocabulary — `LED_BUILTIN`,
//! `A0`, `SDA`, `SCL` — and a circuit manifest is written in Bancada's, which
//! knows only `GPIO<n>`, `3V3`, `5V` and `GND`. Bridging the two needs a fact
//! nobody in this repo holds: `A0` is GPIO36 on an `esp32` and GPIO1 on an
//! `esp32s3`.
//!
//! That fact is on disk, in the installed core's variant header, and this
//! module reads it rather than inventing a table. Reading it is also the only
//! way to be *correct* rather than merely broader, because of this shape in
//! `blink.ino.tmpl`:
//!
//! ```cpp
//! #ifndef LED_BUILTIN
//! #define LED_BUILTIN 2
//! #endif
//! ```
//!
//! On the ESP32-S3 the core's own header contains
//! `#define LED_BUILTIN LED_BUILTIN  // allow testing #ifdef LED_BUILTIN`
//! precisely so that guard fails — the real LED is an addressable RGB device
//! at `SOC_GPIO_PIN_COUNT + PIN_RGB_LED`, not a plain GPIO. A guess that only
//! read the sketch would confidently propose GPIO2 there, on the wrong pin, on
//! the board this feature primarily targets.
//!
//! Hence the three-way answer: a symbol is bound to a literal, defined but not
//! as a literal, or absent. The middle case is the one that matters, and it is
//! reported rather than resolved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What a core variant header says about one symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSymbol {
    /// Bound to an integer literal — usable as a GPIO.
    Defined(i64),
    /// Defined, but as an expression, an alias, or nothing at all. The symbol
    /// exists (so an `#ifndef` guard for it does **not** fire) but this module
    /// refuses to compute what it equals.
    DefinedNonLiteral,
    /// The header does not mention it.
    Absent,
}

impl CoreSymbol {
    /// Does the core define this name at all? This is what decides whether a
    /// sketch's `#ifndef`-guarded `#define` takes effect.
    pub fn is_defined(self) -> bool {
        !matches!(self, CoreSymbol::Absent)
    }
}

/// The pin vocabulary of one board's core variant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariantPins {
    /// The header these came from, quotable in a finding.
    pub source: String,
    symbols: BTreeMap<String, CoreSymbol>,
}

impl VariantPins {
    pub fn lookup(&self, name: &str) -> CoreSymbol {
        self.symbols
            .get(name)
            .copied()
            .unwrap_or(CoreSymbol::Absent)
    }

    /// True when no header was found. Every lookup is then `Absent`, which
    /// degrades the guess to honest "unresolved" findings rather than wrong
    /// pins — the correct failure when a core is not installed.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

/// Parse a `pins_arduino.h`.
///
/// Recognises `static const <type> NAME = <expr>;`, `const <type> NAME =
/// <expr>;` and `#define NAME <expr>`, recording a literal value when `<expr>`
/// is one and [`CoreSymbol::DefinedNonLiteral`] otherwise. A bare `#define
/// NAME` counts as defined too — that is exactly how a core advertises a
/// symbol to `#ifdef`.
pub fn parse_variant_header(source: &str, text: &str) -> VariantPins {
    let mut symbols = BTreeMap::new();
    let code = crate::cppscan::code_only(text);
    for raw in code.lines() {
        let line = raw.trim();
        if let Some(rest) = crate::cppscan::directive_tail(line, "define") {
            let mut parts = rest.split_whitespace();
            let Some(name) = parts.next() else { continue };
            if !crate::cppscan::is_ident(name) {
                continue;
            }
            let tail: Vec<&str> = parts.collect();
            let value = match tail.as_slice() {
                [one] => crate::cppscan::parse_int(one)
                    .map(CoreSymbol::Defined)
                    .unwrap_or(CoreSymbol::DefinedNonLiteral),
                _ => CoreSymbol::DefinedNonLiteral,
            };
            // A later #define does not overwrite an earlier literal: headers
            // commonly follow `static const uint8_t X = 8;` with
            // `#define X X`, and the literal is the useful half.
            let entry = symbols.entry(name.to_string()).or_insert(value);
            if matches!(entry, CoreSymbol::DefinedNonLiteral) {
                if let CoreSymbol::Defined(_) = value {
                    *entry = value;
                }
            }
            continue;
        }
        if let Some((name, value)) = parse_declaration(line) {
            let entry = symbols.entry(name).or_insert(value);
            if matches!(entry, CoreSymbol::DefinedNonLiteral) {
                if let CoreSymbol::Defined(_) = value {
                    *entry = value;
                }
            }
        }
    }
    VariantPins {
        source: source.to_string(),
        symbols,
    }
}

/// `static const uint8_t SDA = 8;` → `("SDA", Defined(8))`;
/// `static const uint8_t LED_BUILTIN = SOC_GPIO_PIN_COUNT + PIN_RGB_LED;`
/// → `("LED_BUILTIN", DefinedNonLiteral)`.
fn parse_declaration(line: &str) -> Option<(String, CoreSymbol)> {
    let body = line.strip_suffix(';')?;
    let mut words = body.split_whitespace().peekable();
    if words.peek() == Some(&"static") {
        words.next();
    }
    match words.next()? {
        "const" | "constexpr" => {}
        _ => return None,
    }
    let rest: Vec<&str> = words.collect();
    let eq = rest.iter().position(|w| *w == "=")?;
    if eq < 1 {
        return None;
    }
    let name = rest[eq - 1];
    if !crate::cppscan::is_ident(name) {
        return None;
    }
    if rest[..eq]
        .iter()
        .any(|w| w.contains('[') || w.contains('&'))
    {
        return None;
    }
    let value_words = &rest[eq + 1..];
    let value = match value_words {
        [one] => crate::cppscan::parse_int(one)
            .map(CoreSymbol::Defined)
            .unwrap_or(CoreSymbol::DefinedNonLiteral),
        [] => return None,
        _ => CoreSymbol::DefinedNonLiteral,
    };
    Some((name.to_string(), value))
}

/// The `build.variant` a `boards.txt` assigns to a board id.
pub fn variant_of(boards_txt: &str, board: &str) -> Option<String> {
    let key = format!("{board}.build.variant=");
    boards_txt
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&key))
        .map(|v| v.trim().to_string())
}

/// Locate the variant header for `fqbn` under an arduino-cli data directory.
///
/// Layout: `<data>/packages/<vendor>/hardware/<arch>/<version>/`. When several
/// versions are installed the newest wins, ordered by
/// [`crate::ghlib::version_key`] so `3.10.0` beats `3.9.0` rather than losing
/// a string compare.
pub fn header_path(data_dir: &Path, fqbn: &str) -> Option<PathBuf> {
    let mut parts = fqbn.split(':');
    let vendor = parts.next()?;
    let arch = parts.next()?;
    let board = parts.next()?;
    let hardware = data_dir
        .join("packages")
        .join(vendor)
        .join("hardware")
        .join(arch);
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&hardware)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    versions.sort_by(|a, b| {
        let key = |p: &PathBuf| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let (ka, kb) = (key(a), key(b));
        match (
            crate::ghlib::version_key(&ka),
            crate::ghlib::version_key(&kb),
        ) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => ka.cmp(&kb),
        }
    });
    for root in versions {
        let boards_txt = std::fs::read_to_string(root.join("boards.txt")).unwrap_or_default();
        let Some(variant) = variant_of(&boards_txt, board) else {
            continue;
        };
        let candidate = root.join("variants").join(variant).join("pins_arduino.h");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Read the variant pin vocabulary for `fqbn`, or an empty one when the core
/// is not installed.
///
/// `data_dir` is arduino-cli's `directories.data`; the caller resolves it
/// (`config get`, the same idiom as
/// [`crate::cli::ArduinoCli::sketchbook_dir`]) so this stays testable against
/// a fixture tree.
pub fn load(data_dir: &Path, fqbn: &str) -> VariantPins {
    let Some(path) = header_path(data_dir, fqbn) else {
        return VariantPins::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return VariantPins::default();
    };
    parse_variant_header(&path.to_string_lossy(), &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The head of esp32 core 3.3.11's `variants/esp32s3/pins_arduino.h`,
    /// verbatim. Every line here is load-bearing for the S3 trap.
    const S3_HEADER: &str = r#"
#define USB_VID 0x303a
#define PIN_RGB_LED 48
// BUILTIN_LED can be used in new Arduino API digitalWrite() like in Blink.ino
static const uint8_t LED_BUILTIN = SOC_GPIO_PIN_COUNT + PIN_RGB_LED;
#define BUILTIN_LED LED_BUILTIN  // backward compatibility
#define LED_BUILTIN LED_BUILTIN  // allow testing #ifdef LED_BUILTIN
#define RGB_BUILTIN    LED_BUILTIN

static const uint8_t TX = 43;
static const uint8_t RX = 44;

static const uint8_t SDA = 8;
static const uint8_t SCL = 9;

static const uint8_t A0 = 1;
static const uint8_t A1 = 2;
"#;

    /// The same region of `variants/esp32/pins_arduino.h` — the point being
    /// that the answers differ per board.
    const CLASSIC_HEADER: &str = r#"
static const uint8_t SDA = 21;
static const uint8_t SCL = 22;
static const uint8_t A0 = 36;
"#;

    #[test]
    fn the_s3_defines_led_builtin_but_not_as_a_literal() {
        // The whole reason this module exists. `is_defined()` is true, so
        // blink's `#ifndef LED_BUILTIN` guard does not fire and its `2` is
        // dead text; but there is no literal to propose, so the guess must
        // say so instead of claiming a pin.
        let v = parse_variant_header("s3", S3_HEADER);
        assert_eq!(v.lookup("LED_BUILTIN"), CoreSymbol::DefinedNonLiteral);
        assert!(v.lookup("LED_BUILTIN").is_defined());
    }

    #[test]
    fn the_s3_gives_literal_i2c_and_analog_pins() {
        let v = parse_variant_header("s3", S3_HEADER);
        assert_eq!(v.lookup("SDA"), CoreSymbol::Defined(8));
        assert_eq!(v.lookup("SCL"), CoreSymbol::Defined(9));
        assert_eq!(v.lookup("A0"), CoreSymbol::Defined(1));
        assert_eq!(v.lookup("TX"), CoreSymbol::Defined(43));
    }

    #[test]
    fn the_classic_esp32_gives_different_values_for_the_same_names() {
        // Proof that a hard-coded alias table would have been wrong for one
        // board or the other.
        let v = parse_variant_header("classic", CLASSIC_HEADER);
        assert_eq!(v.lookup("SDA"), CoreSymbol::Defined(21));
        assert_eq!(v.lookup("SCL"), CoreSymbol::Defined(22));
        assert_eq!(v.lookup("A0"), CoreSymbol::Defined(36));
    }

    #[test]
    fn a_plain_define_with_a_literal_is_usable() {
        let v = parse_variant_header("s3", S3_HEADER);
        assert_eq!(v.lookup("PIN_RGB_LED"), CoreSymbol::Defined(48));
    }

    #[test]
    fn a_name_the_header_never_mentions_is_absent() {
        let v = parse_variant_header("s3", S3_HEADER);
        assert_eq!(v.lookup("DHTPIN"), CoreSymbol::Absent);
        assert!(!v.lookup("DHTPIN").is_defined());
    }

    #[test]
    fn a_literal_declaration_is_not_undone_by_a_later_self_alias() {
        // Headers routinely follow `static const uint8_t X = 8;` with
        // `#define X X`. Losing the 8 there would silently disable I2C
        // resolution on every ESP32 board.
        let v = parse_variant_header("x", "static const uint8_t SDA = 8;\n#define SDA SDA\n");
        assert_eq!(v.lookup("SDA"), CoreSymbol::Defined(8));
    }

    #[test]
    fn an_empty_variant_answers_absent_for_everything() {
        let v = VariantPins::default();
        assert!(v.is_empty());
        assert_eq!(v.lookup("SDA"), CoreSymbol::Absent);
    }

    #[test]
    fn variant_of_reads_the_boards_txt_key() {
        let boards = "\
esp32s3.name=ESP32S3 Dev Module
esp32s3.build.variant=esp32s3
esp32c6.build.variant=esp32c6
";
        assert_eq!(variant_of(boards, "esp32s3").as_deref(), Some("esp32s3"));
        assert_eq!(variant_of(boards, "esp32c6").as_deref(), Some("esp32c6"));
        assert_eq!(variant_of(boards, "nonesuch"), None);
    }

    #[test]
    fn load_picks_the_newest_installed_core_and_reads_its_variant() {
        let dir = tempfile::tempdir().unwrap();
        let write = |rel: &str, text: &str| {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        // 3.10.0 must beat 3.9.0 — a lexical sort would pick 3.9.0.
        for (version, sda) in [("3.9.0", "21"), ("3.10.0", "8")] {
            let base = format!("packages/esp32/hardware/esp32/{version}");
            write(
                &format!("{base}/boards.txt"),
                "esp32s3.build.variant=esp32s3\n",
            );
            write(
                &format!("{base}/variants/esp32s3/pins_arduino.h"),
                &format!("static const uint8_t SDA = {sda};\n"),
            );
        }
        let v = load(dir.path(), "esp32:esp32:esp32s3");
        assert_eq!(v.lookup("SDA"), CoreSymbol::Defined(8), "{}", v.source);
        assert!(v.source.contains("3.10.0"), "{}", v.source);
    }

    #[test]
    fn a_missing_core_degrades_to_empty_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let v = load(dir.path(), "esp32:esp32:esp32s3");
        assert!(v.is_empty());
        assert_eq!(v.lookup("SDA"), CoreSymbol::Absent);
    }
}
