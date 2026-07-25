//! Creating a new sketch project.
//!
//! The work itself is done by `arduino-cli` (`sketch new`, `profile create`,
//! `profile lib add`) — see [`crate::cli`]. What lives here is the pure part:
//! validating a project name against Arduino's sketch-folder rules, and deriving
//! a sensible profile name from an FQBN.

use crate::{Error, Result};

/// arduino-lint's limit on a sketch folder name.
const MAX_NAME_LEN: usize = 63;

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
    if !name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
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
            assert!(err.contains("start with a letter or a digit"), "{bad}: {err}");
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
    fn sanitises_and_falls_back_on_odd_input() {
        // too few segments: fall back to the sanitised whole string
        assert_eq!(profile_name_for_fqbn("esp32:esp32"), "esp32_esp32");
        assert_eq!(profile_name_for_fqbn(""), "default");
        assert_eq!(profile_name_for_fqbn(":::"), "default");
        // a board name with punctuation stays usable as a YAML key
        assert_eq!(profile_name_for_fqbn("a:b:c.d"), "c_d");
    }
}
