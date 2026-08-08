//! Creating a new sketch project.
//!
//! The work itself is done by `arduino-cli` (`sketch new`, `profile create`,
//! `profile lib add`) — see [`crate::cli`]. What lives here is the pure part:
//! validating a project name against Arduino's sketch-folder rules, and deriving
//! a sensible profile name from an FQBN.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// arduino-lint's limit on a sketch folder name.
const MAX_NAME_LEN: usize = 63;

const TMPL_BLINK: &str = include_str!("templates/sketch/blink.ino.tmpl");
const TMPL_I2C_SCAN: &str = include_str!("templates/sketch/i2c_scan.ino.tmpl");
const TMPL_WIFI_SCAN: &str = include_str!("templates/sketch/wifi_scan.ino.tmpl");
const TMPL_BOARD_INFO: &str = include_str!("templates/sketch/board_info.ino.tmpl");

/// A starter sketch a new project can begin life as.
#[derive(Clone, Copy, serde::Serialize)]
pub struct SketchTemplate {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    #[serde(skip)]
    tmpl: &'static str,
}

/// Every starter, Blink first — the order the UI presents them in.
pub const TEMPLATES: &[SketchTemplate] = &[
    SketchTemplate {
        id: "blink",
        label: "Blink",
        description: "The hardware hello-world: one upload proves the toolchain, the serial port and the board.",
        tmpl: TMPL_BLINK,
    },
    SketchTemplate {
        id: "i2c-scan",
        label: "I2C scanner",
        description: "Prints every I2C device that answers, rescanning periodically — proves module wiring.",
        tmpl: TMPL_I2C_SCAN,
    },
    SketchTemplate {
        id: "wifi-scan",
        label: "Wi-Fi scanner",
        description: "Lists nearby networks strongest-first with RSSI and channel — proves radio and antenna.",
        tmpl: TMPL_WIFI_SCAN,
    },
    SketchTemplate {
        id: "board-info",
        label: "Board info",
        description: "Chip model, MAC, flash and PSRAM sizes, last reset reason — esptool facts as a sketch.",
        tmpl: TMPL_BOARD_INFO,
    },
];

fn is_allowed(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// Validate a project name, returning it trimmed.
///
/// The name becomes both the folder and the main `.ino` basename, so unlike a
/// library name — where the display name and the folder are separate fields —
/// there is nothing to silently rewrite. Spaces are refused rather than
/// converted, because changing what the user typed changes the project's
/// identity.
pub fn validate_project_name(raw: &str) -> Result<String> {
    let name = raw.trim();

    if name.is_empty() {
        return Err(Error::Other("project name must not be empty".into()));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(Error::Other(
            "project name must not contain a path separator — choose the location separately"
                .into(),
        ));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(Error::Other(format!(
            "project name must be {MAX_NAME_LEN} characters or fewer (got {})",
            name.chars().count()
        )));
    }
    if name.starts_with('.') {
        return Err(Error::Other(
            "project name must not start with `.` — a dotted folder is hidden and arduino-cli skips it".into(),
        ));
    }
    if let Some(bad) = name.chars().find(|c| !is_allowed(*c)) {
        let hint = if bad == ' ' {
            " — use `_` or `-` instead of spaces"
        } else {
            ""
        };
        return Err(Error::Other(format!(
            "project name may only contain letters, digits, '_', '.' and '-' — found '{bad}'{hint}"
        )));
    }
    if !name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return Err(Error::Other(
            "project name must start with a letter or a digit".into(),
        ));
    }

    Ok(name.to_string())
}

/// A profile name derived from an FQBN: the board segment, with any board
/// options dropped.
///
/// `esp32:esp32:esp32s3:PSRAM=opi` yields `esp32s3`. Falls back to the whole
/// FQBN, sanitised, when it has an unexpected shape.
pub fn profile_name_for_fqbn(fqbn: &str) -> String {
    let sanitize = |s: &str| -> String {
        let out: String = s
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        out.trim_matches('_').to_string()
    };

    // vendor:arch:board[:options] — the board is the third segment.
    let board = fqbn.split(':').nth(2).unwrap_or("").trim();
    let candidate = sanitize(board);
    if candidate.is_empty() {
        let whole = sanitize(fqbn.trim());
        if whole.is_empty() {
            "default".to_string()
        } else {
            whole
        }
    } else {
        candidate
    }
}

/// Where a new project goes by default: `~/Projects` when the user has
/// one, otherwise the home directory itself.
///
/// `is_dir` follows symlinks, so a symlinked `~/Projects` counts; a plain
/// file that happens to be named `Projects` does not.
pub fn default_project_parent(home: &Path) -> PathBuf {
    let projects = home.join("Projects");
    if projects.is_dir() {
        projects
    } else {
        home.to_path_buf()
    }
}

/// Render the template with `id` for a project called `name`. `None` for an
/// id no template carries — the command layer turns that into a user error.
pub fn sketch_from_template(id: &str, name: &str) -> Option<String> {
    TEMPLATES
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.tmpl.replace("{name}", name))
}

/// Replace the main `.ino` that `arduino-cli sketch new` stubbed out with the
/// chosen starter. Callers guarantee `dir` was created by us moments ago, so
/// this never clobbers user content.
pub fn write_main_ino(dir: &Path, name: &str, template_id: &str) -> Result<()> {
    let sketch = sketch_from_template(template_id, name).ok_or_else(|| {
        Error::Other(format!(
            "unknown sketch template `{template_id}` — expected one of {}",
            TEMPLATES
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })?;
    std::fs::write(dir.join(format!("{name}.ino")), sketch)?;
    Ok(())
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_names() {
        for good in ["Blink", "home-node", "sensor_v2", "Demo.1", "esp32s3Test"] {
            assert_eq!(validate_project_name(good).unwrap(), good, "{good}");
        }
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(validate_project_name("  Blink \n").unwrap(), "Blink");
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_project_name("   ").is_err());
    }

    #[test]
    fn rejects_path_separators() {
        for bad in ["a/b", "a\\b", "../escape"] {
            let err = validate_project_name(bad).unwrap_err().to_string();
            assert!(err.contains("path separator"), "{bad}: {err}");
        }
    }

    #[test]
    fn rejects_spaces_with_a_useful_hint() {
        let err = validate_project_name("My Project").unwrap_err().to_string();
        assert!(err.contains("instead of spaces"), "{err}");
    }

    #[test]
    fn rejects_leading_dot() {
        let err = validate_project_name(".hidden").unwrap_err().to_string();
        assert!(err.contains("hidden"), "{err}");
    }

    #[test]
    fn rejects_leading_non_alphanumeric() {
        // `.` is caught earlier with a more specific message; `-` and `_` here.
        for bad in ["-lead", "_lead"] {
            let err = validate_project_name(bad).unwrap_err().to_string();
            assert!(
                err.contains("start with a letter or a digit"),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn rejects_over_63_chars() {
        let err = validate_project_name(&"A".repeat(64))
            .unwrap_err()
            .to_string();
        assert!(err.contains("63"), "{err}");
    }

    #[test]
    fn rejects_other_illegal_characters() {
        for bad in ["Caf\u{e9}", "a+b", "hi!", "q(1)"] {
            assert!(validate_project_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn derives_profile_name_from_the_board_segment() {
        assert_eq!(profile_name_for_fqbn("esp32:esp32:esp32s3"), "esp32s3");
        assert_eq!(profile_name_for_fqbn("arduino:avr:uno"), "uno");
    }

    #[test]
    fn drops_board_options() {
        assert_eq!(
            profile_name_for_fqbn("esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M"),
            "esp32s3"
        );
    }

    #[test]
    fn every_template_is_a_complete_named_program() {
        for t in TEMPLATES {
            let s = sketch_from_template(t.id, "TestNode").unwrap();
            assert!(s.starts_with("// TestNode — "), "{}: bad header", t.id);
            assert!(!s.contains("{name}"), "{}: unsubstituted name", t.id);
            assert!(s.contains("void setup()"), "{}: no setup", t.id);
            assert!(s.contains("void loop()"), "{}: no loop", t.id);
            assert!(s.contains("Serial.begin(115200)"), "{}: not serial-verbose", t.id);
        }
    }

    #[test]
    fn blink_template_guards_led_builtin() {
        // must compile on cores that don't define LED_BUILTIN
        let s = sketch_from_template("blink", "BlinkNode").unwrap();
        assert!(s.contains("#ifndef LED_BUILTIN"));
    }

    #[test]
    fn template_ids_are_unique_and_blink_leads() {
        assert_eq!(TEMPLATES[0].id, "blink");
        let mut ids: Vec<_> = TEMPLATES.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TEMPLATES.len(), "duplicate template id");
    }

    #[test]
    fn unknown_template_is_rejected_with_the_valid_ids() {
        assert!(sketch_from_template("nope", "X").is_none());
        let tmp = tempfile::tempdir().unwrap();
        let err = write_main_ino(tmp.path(), "X", "nope").unwrap_err().to_string();
        assert!(err.contains("unknown sketch template"), "{err}");
        assert!(err.contains("blink"), "should list valid ids: {err}");
    }

    #[test]
    fn default_parent_prefers_projects_dir() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("Projects")).unwrap();
        assert_eq!(
            default_project_parent(home.path()),
            home.path().join("Projects")
        );
    }

    #[test]
    fn default_parent_falls_back_to_home() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(default_project_parent(home.path()), home.path());
    }

    #[test]
    fn default_parent_ignores_a_projects_file() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("Projects"), "not a dir").unwrap();
        assert_eq!(default_project_parent(home.path()), home.path());
    }

    #[test]
    fn write_main_ino_lands_where_main_ino_looks() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Pisca");
        std::fs::create_dir(&dir).unwrap();
        write_main_ino(&dir, "Pisca", "blink").unwrap();

        let proj = crate::sketch::SketchProject::open(&dir).unwrap();
        let main = proj.main_ino().expect("main ino must be found");
        let text = std::fs::read_to_string(main).unwrap();
        assert!(text.contains("// Pisca — "));
    }

    #[test]
    fn sanitises_and_falls_back_on_odd_input() {
        // too few segments: fall back to the sanitised whole string
        assert_eq!(profile_name_for_fqbn("esp32:esp32"), "esp32_esp32");
        assert_eq!(profile_name_for_fqbn(""), "default");
        assert_eq!(profile_name_for_fqbn(":::"), "default");
        // a board name with punctuation stays usable as a YAML key
        assert_eq!(profile_name_for_fqbn("a:b:c.d"), "c_d");
    }
}
