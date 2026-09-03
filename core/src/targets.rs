//! The SoC target model — the ESP chip vocabulary.
//!
//! Ported from the `bancada-idf` sibling's `core/src/targets.rs`, pruned to its
//! **pure** subset: the closed set of ESP32-family chips and the folding of any
//! human spelling (`ESP32-S3`, `s3`, `esp32_s3`) onto the single canonical id
//! (`esp32s3`). The sibling's `idf.py`-driven `list_targets` is intentionally
//! left behind — it belongs with the ESP-IDF toolchain engine, not this
//! chip-facts table, which [`boardprofile`](crate::boardprofile) leans on.
//!
//! - [`Target::from_id`] is for *labelling* — arch, toolchain prefix, whether
//!   the chip has native USB — and returns `None` for an unknown id rather
//!   than guessing.
//! - [`normalize_id`] is deliberately table-free for the `esp32*` shapes, so an
//!   id from a newer IDF that this snapshot does not list still passes through
//!   unchanged rather than being rejected here.

use serde::{Deserialize, Serialize};

/// Instruction-set family. Decides which toolchain and which debug tooling
/// applies, and it is the only property that splits the target set roughly
/// in half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    Xtensa,
    RiscV,
}

impl Arch {
    pub fn label(self) -> &'static str {
        match self {
            Arch::Xtensa => "Xtensa",
            Arch::RiscV => "RISC-V",
        }
    }
}

/// One known SoC target.
///
/// `Serialize` only: every value of this type is a borrow of [`KNOWN_TARGETS`],
/// so there is nothing to deserialize *into* — a caller reading a target back
/// from JSON wants [`Target::from_id`] on the id, which re-validates it against
/// the table instead of trusting the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    /// The canonical id every tool accepts: `esp32s3`.
    pub id: &'static str,
    /// How Espressif writes it in prose: `ESP32-S3`.
    pub name: &'static str,
    pub arch: Arch,
    /// Binutils prefix for `addr2line`/`gdb`. Prefer `monitor_toolprefix` from
    /// `project_description.json` when a build directory exists — a given IDF
    /// version may ship a unified prefix instead of this per-chip one.
    pub toolchain_prefix: &'static str,
    /// Whether the SoC has a built-in USB-Serial/JTAG peripheral. When true a
    /// board can enumerate without a bridge chip, which is why its USB serial
    /// number is the chip MAC and why `before = usb_reset` applies.
    pub native_usb: bool,
}

/// The USB vendor id Espressif uses for native USB-Serial/JTAG and USB-OTG
/// devices. A port under this VID is a bare SoC, not a bridge.
pub const ESPRESSIF_USB_VID: u16 = 0x303a;

/// Snapshot of the targets ESP-IDF 5.x supports. See the module docs for why
/// this is a labelling table and not a gate.
pub const KNOWN_TARGETS: &[Target] = &[
    Target {
        id: "esp32",
        name: "ESP32",
        arch: Arch::Xtensa,
        toolchain_prefix: "xtensa-esp32-elf-",
        native_usb: false,
    },
    Target {
        id: "esp32s2",
        name: "ESP32-S2",
        arch: Arch::Xtensa,
        toolchain_prefix: "xtensa-esp32s2-elf-",
        native_usb: true,
    },
    Target {
        id: "esp32s3",
        name: "ESP32-S3",
        arch: Arch::Xtensa,
        toolchain_prefix: "xtensa-esp32s3-elf-",
        native_usb: true,
    },
    Target {
        id: "esp32c2",
        name: "ESP32-C2",
        arch: Arch::RiscV,
        toolchain_prefix: "riscv32-esp-elf-",
        native_usb: false,
    },
    Target {
        id: "esp32c3",
        name: "ESP32-C3",
        arch: Arch::RiscV,
        toolchain_prefix: "riscv32-esp-elf-",
        native_usb: true,
    },
    Target {
        id: "esp32c5",
        name: "ESP32-C5",
        arch: Arch::RiscV,
        toolchain_prefix: "riscv32-esp-elf-",
        native_usb: true,
    },
    Target {
        id: "esp32c6",
        name: "ESP32-C6",
        arch: Arch::RiscV,
        toolchain_prefix: "riscv32-esp-elf-",
        native_usb: true,
    },
    Target {
        id: "esp32c61",
        name: "ESP32-C61",
        arch: Arch::RiscV,
        toolchain_prefix: "riscv32-esp-elf-",
        native_usb: true,
    },
    Target {
        id: "esp32h2",
        name: "ESP32-H2",
        arch: Arch::RiscV,
        toolchain_prefix: "riscv32-esp-elf-",
        native_usb: true,
    },
    Target {
        id: "esp32p4",
        name: "ESP32-P4",
        arch: Arch::RiscV,
        toolchain_prefix: "riscv32-esp-elf-",
        native_usb: true,
    },
];

impl Target {
    /// Look up a canonical id. Returns `None` for anything not in the table —
    /// including a valid id from a newer IDF — so callers never invent an
    /// arch for a chip they do not know.
    pub fn from_id(id: &str) -> Option<&'static Target> {
        KNOWN_TARGETS.iter().find(|t| t.id == id)
    }

    /// Look up by any human spelling, via [`normalize_id`].
    pub fn lookup(input: &str) -> Option<&'static Target> {
        Target::from_id(&normalize_id(input))
    }
}

/// Fold a human spelling of a target into the id the toolchain accepts.
///
/// Handles the four shapes that actually turn up: the canonical id, the
/// hyphenated prose name Espressif prints (`ESP32-S3`), an underscored
/// variant (`esp32_s3`), and the bare suffix a person says out loud (`s3`,
/// `c6`). Anything else is lowercased and returned unchanged, so an
/// unrecognised-but-real target from a newer IDF still reaches `idf.py`.
pub fn normalize_id(input: &str) -> String {
    let t = input
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "");
    if t.is_empty() {
        return t;
    }
    if t.starts_with("esp32") || t.starts_with("esp8") {
        return t;
    }
    // A bare suffix: `s3`, `c6`, `c61`, `h2`, `p4`. Requires a letter followed
    // by digits so a stray word is not silently turned into a target.
    let mut chars = t.chars();
    let first = chars.next().unwrap();
    if matches!(first, 's' | 'c' | 'h' | 'p')
        && !chars.as_str().is_empty()
        && chars.all(|c| c.is_ascii_digit())
    {
        return format!("esp32{t}");
    }
    t
}

/// Parse the target list `idf.py --list-targets` prints.
///
/// The output is one id per line, but IDF also prints preview targets under a
/// `preview targets:` heading and may emit blank lines or a leading note. We
/// keep only lines that look like a bare target id, which drops the headings
/// without needing to know their exact wording — the headings contain a space
/// or a colon, real ids never do.
pub fn parse_list_targets(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            !l.contains(char::is_whitespace)
                && !l.contains(':')
                && l.starts_with("esp")
                && l.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map(str::to_string)
        .collect()
}

/// Map an esptool chip description (`"ESP32-S3 (QFN56) (revision v0.2)"`) to a
/// target id.
///
/// esptool prints the prose name plus package and revision detail, so we take
/// the leading token and normalise it. Returns `None` when the leading token
/// is not a chip we know, which is the honest answer for a chip newer than
/// this table.
pub fn target_from_chip_description(desc: &str) -> Option<&'static Target> {
    let head = desc.split_whitespace().next()?;
    Target::lookup(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_ids_pass_through() {
        assert_eq!(normalize_id("esp32s3"), "esp32s3");
        assert_eq!(normalize_id("esp32"), "esp32");
    }

    #[test]
    fn prose_names_normalize() {
        assert_eq!(normalize_id("ESP32-S3"), "esp32s3");
        assert_eq!(normalize_id("ESP32-C61"), "esp32c61");
        assert_eq!(normalize_id("esp32_s2"), "esp32s2");
        assert_eq!(normalize_id("  ESP32-P4  "), "esp32p4");
    }

    #[test]
    fn bare_suffixes_expand() {
        assert_eq!(normalize_id("s3"), "esp32s3");
        assert_eq!(normalize_id("c6"), "esp32c6");
        assert_eq!(normalize_id("c61"), "esp32c61");
        assert_eq!(normalize_id("h2"), "esp32h2");
        assert_eq!(normalize_id("P4"), "esp32p4");
    }

    #[test]
    fn a_word_is_not_a_suffix() {
        // `chip` starts with 'c' but is not `c<digits>`, so it must not
        // become `esp32chip`.
        assert_eq!(normalize_id("chip"), "chip");
        assert_eq!(normalize_id("s"), "s");
        assert_eq!(normalize_id("post"), "post");
    }

    #[test]
    fn unknown_but_plausible_target_survives_normalization() {
        // The point of the table-free path: a target from a newer IDF still
        // reaches idf.py, which is what will actually adjudicate it.
        assert_eq!(normalize_id("ESP32-C99"), "esp32c99");
        assert!(Target::lookup("esp32c99").is_none());
    }

    #[test]
    fn lookup_labels_a_known_target() {
        let t = Target::lookup("ESP32-S3").unwrap();
        assert_eq!(t.id, "esp32s3");
        assert_eq!(t.arch, Arch::Xtensa);
        assert!(t.native_usb);

        let c3 = Target::lookup("c3").unwrap();
        assert_eq!(c3.arch, Arch::RiscV);
        assert_eq!(c3.toolchain_prefix, "riscv32-esp-elf-");
    }

    #[test]
    fn classic_esp32_has_no_native_usb() {
        // The property that decides whether a board's USB serial number can be
        // trusted as its MAC — the original ESP32 always sits behind a bridge.
        assert!(!Target::from_id("esp32").unwrap().native_usb);
    }

    #[test]
    fn every_table_id_is_its_own_normal_form() {
        for t in KNOWN_TARGETS {
            assert_eq!(normalize_id(t.id), t.id, "{} is not canonical", t.id);
            assert_eq!(normalize_id(t.name), t.id, "{} does not fold", t.name);
        }
    }

    #[test]
    fn parses_list_targets_output() {
        let out = "\
esp32
esp32s2
esp32s3
esp32c3

preview targets:
esp32c61
";
        assert_eq!(
            parse_list_targets(out),
            vec!["esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c61"]
        );
    }

    #[test]
    fn list_targets_drops_prose_lines() {
        let out = "Supported targets are:\nesp32\nNote: run idf.py set-target\nesp32c6\n";
        assert_eq!(parse_list_targets(out), vec!["esp32", "esp32c6"]);
    }

    #[test]
    fn chip_description_maps_to_target() {
        let t = target_from_chip_description("ESP32-S3 (QFN56) (revision v0.2)").unwrap();
        assert_eq!(t.id, "esp32s3");
        let t = target_from_chip_description("ESP32-C6 (revision v0.0)").unwrap();
        assert_eq!(t.id, "esp32c6");
        assert!(target_from_chip_description("").is_none());
        assert!(target_from_chip_description("Unknown-Chip").is_none());
    }
}
