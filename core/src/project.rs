//! Creating — and renaming — a sketch project.
//!
//! Creation itself is done by `arduino-cli` (`sketch new`, `profile create`,
//! `profile lib add`) — see [`crate::cli`]. What lives here is the pure part:
//! validating a project name against Arduino's sketch-folder rules, and deriving
//! a sensible profile name from an FQBN.
//!
//! Renaming is not pure and is not a directory rename either — the folder name
//! and the main `.ino` basename are one identity — so [`rename_project`] is a
//! filesystem operation of its own, built out of [`crate::clone`]'s parts.

use std::path::{Path, PathBuf};

use crate::{clone, library, Error, Result};

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

/// Registry libraries a board's profile cannot build without.
///
/// Profile builds are hermetic — they see only the libraries pinned in
/// sketch.yaml, never the globally installed ones — so a core that hard-requires
/// a companion library makes every fresh profile fail to compile until that
/// library is pinned. The UNO Q core is the known case: its bundled stub header
/// `#error`s out of `Arduino.h` itself unless Arduino_RouterBridge is present.
pub fn required_profile_libs(fqbn: &str) -> &'static [&'static str] {
    if fqbn.trim().starts_with("arduino:zephyr:") {
        &["Arduino_RouterBridge"]
    } else {
        &[]
    }
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

/// A finished rename.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RenamedProject {
    pub dir: PathBuf,
    pub name: String,
    /// Non-fatal notes, same contract as [`crate::clone::ClonedProject`]:
    /// sketch.yaml paths that may need attention, accumulated rather than
    /// aborting a rename that otherwise succeeded.
    pub warnings: Vec<String>,
}

/// Rename the sketch at `dir` to `new_name`, in place.
///
/// A project's folder name and its main `.ino` basename are one identity —
/// arduino-cli only sees `Foo/Foo.ino` as a sketch ([`crate::sketch::SketchProject::main_ino`]),
/// which is why the Explorer refuses to rename that file on its own
/// ([`crate::files::SketchFiles::rename_entry`]) and why this is a core
/// operation rather than a directory rename. It is
/// [`crate::clone::clone_project`] without the copy, and reuses its parts: the
/// same name policy, the same guarded retitle of the line-1 title comment, and
/// the same textual `sketch.yaml` rewrite.
///
/// **The order of operations is the design, not an accident.** A clone borrows
/// atomicity from a staging directory it can throw away; a rename cannot,
/// because the source *is* the target. So every in-directory edit happens
/// FIRST — the main `.ino` rename, its retitle, the `sketch.yaml` rewrite —
/// and the directory rename goes LAST. That way the single irreversible step
/// is the last one, and everything before it is rolled back if it fails. Do
/// not "simplify" this by moving the directory first: a failure after that
/// point would leave a moved folder whose main `.ino` still carries the old
/// name, which is not a sketch at all.
///
/// Every refusal below is checked before the first byte is written.
pub fn rename_project(dir: &Path, new_name: &str) -> Result<RenamedProject> {
    let name = validate_project_name(new_name)?;

    let old_name = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let main_ino = format!("{old_name}.ino");
    if old_name.is_empty() || !dir.is_dir() || !dir.join(&main_ino).is_file() {
        let expected = if old_name.is_empty() {
            "<name>.ino".to_string()
        } else {
            main_ino.clone()
        };
        return Err(Error::Other(format!(
            "{} is not a sketch folder — expected {}",
            dir.display(),
            dir.join(expected).display()
        )));
    }
    // is_file() above follows links, so a symlinked main ino would pass and
    // then be renamed under its OLD name — and the retitle would edit a file
    // that belongs to whichever project owns the real one.
    if dir
        .join(&main_ino)
        .symlink_metadata()?
        .file_type()
        .is_symlink()
    {
        return Err(Error::Other(format!(
            "the main sketch {} is a symlink — rename the project that owns the real file instead",
            dir.join(&main_ino).display()
        )));
    }
    if name == old_name {
        return Err(Error::Other(format!(
            "the project is already named `{name}`"
        )));
    }

    let src = dir.canonicalize()?;
    let src_name = src
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = src
        .parent()
        .ok_or_else(|| Error::Other(format!("{} has no parent directory", src.display())))?
        .to_path_buf();
    let dest = parent.join(&name);

    // symlink_metadata, not exists(): exists() reports false for a dangling
    // symlink while the rename would still land on it.
    if dest.symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "{} already exists — choose a different name",
            dest.display()
        )));
    }
    // Only after the exact-existence check: this branch assumes the match
    // differs from `name` in case alone. The match may be the project itself,
    // which needs its own wording — and its own refusal, because a one-step
    // case-only rename is not portable to a case-insensitive filesystem.
    if let Some(clash) = library::case_insensitive_clash(&parent, &name) {
        if clash == src_name {
            return Err(Error::Other(format!(
                "`{src_name}` and `{name}` differ only in case — rename it to a different name first, then to `{name}`"
            )));
        }
        return Err(Error::Other(format!(
            "a folder named `{clash}` already exists there and differs only in case"
        )));
    }
    // The main ino is renamed to `<name>.ino`; a same-named secondary sketch
    // file would be clobbered by that rename.
    if src.join(format!("{name}.ino")).symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "the project already contains {name}.ino — the renamed main sketch would overwrite it"
        )));
    }
    // A linked worktree's top-level `.git` is a FILE holding an absolute
    // `gitdir:` path, and the main repository holds the matching backlink in
    // `worktrees/<name>/gitdir`. A plain rename breaks both directions, and
    // neither is ours to rewrite — git has a command that does. (clone.rs
    // meets the same file from the other side, and never copies it.)
    if src
        .join(".git")
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_file())
    {
        return Err(Error::Other(format!(
            "{} is a git worktree checkout — its .git file and the main repository's backlink both record this path, so renaming the folder would break both; use `git worktree move` instead",
            src.display()
        )));
    }

    // ---- in-directory work first, the directory rename last ----

    let old_ino = src.join(&main_ino);
    let new_ino = src.join(format!("{name}.ino"));
    let yaml_path = src.join("sketch.yaml");
    let yaml_before = std::fs::read(&yaml_path).ok();

    // Undo the in-directory pass, so a failure leaves a project arduino-cli
    // still recognises. Best-effort throughout: the caller needs the original
    // error, not a rollback error on top of it.
    let rollback = || {
        if std::fs::rename(&new_ino, &old_ino).is_ok() {
            let _ = clone::retitle_main_ino(&old_ino, &name, &old_name);
        }
        if let Some(bytes) = &yaml_before {
            let _ = std::fs::write(&yaml_path, bytes);
        }
    };

    std::fs::rename(&old_ino, &new_ino)?;
    let mut warnings = Vec::new();
    if let Err(e) = clone::retitle_main_ino(&new_ino, &old_name, &name) {
        rollback();
        return Err(e);
    }
    // The yaml still lives in `src`; `src` → `dest` is the move it is being
    // rewritten for, and has not happened yet.
    if let Err(e) = clone::rewrite_sketch_yaml(&src, &src, &dest, &mut warnings) {
        rollback();
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&src, &dest) {
        rollback();
        return Err(Error::Other(format!(
            "could not rename {} to {}: {e}",
            src.display(),
            dest.display()
        )));
    }

    Ok(RenamedProject {
        dir: dest,
        name,
        warnings,
    })
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
    fn unoq_profiles_require_the_router_bridge_library() {
        // Any arduino:zephyr board (the UNO Q family), with or without options.
        assert_eq!(
            required_profile_libs("arduino:zephyr:unoq"),
            ["Arduino_RouterBridge"]
        );
        assert_eq!(
            required_profile_libs(" arduino:zephyr:unoq:opt=1 "),
            ["Arduino_RouterBridge"]
        );
    }

    #[test]
    fn other_boards_require_no_profile_libs() {
        for fqbn in ["arduino:avr:uno", "esp32:esp32:esp32s3", "", "zephyr"] {
            assert!(required_profile_libs(fqbn).is_empty(), "{fqbn}");
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
    fn i2c_scan_template_avoids_esp32_only_apis() {
        // must compile on cores whose Serial has no printf (AVR, the Uno Q's
        // BridgeMonitor) and whose Wire::begin takes no pin arguments
        // (everything that isn't ESP32) — found the hard way by retargeting
        // an i2c-scan project from esp32s3 to arduino:zephyr:unoq
        let s = sketch_from_template("i2c-scan", "ScanNode").unwrap();
        assert!(!s.contains("Serial.printf"), "printf is an ESP32-core extra");
        assert!(
            s.contains("#if defined(ARDUINO_ARCH_ESP32)"),
            "runtime pin override exists only on ESP32 cores and must be guarded"
        );
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

    // ----- rename_project -----

    fn read(p: PathBuf) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    /// A realistic project: titled main ino, a sketch.yaml, a nested file.
    fn sample_project(parent: &Path, name: &str) -> PathBuf {
        let dir = parent.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join(format!("{name}.ino")),
            format!("// {name} — demo\n\nvoid setup() {{}}\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("sketch.yaml"),
            "default_profile: p\nprofiles:\n  p:\n    fqbn: a:b:c\n    port: /dev/ttyACM0\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/util.h"), "#pragma once\n").unwrap();
        dir
    }

    #[test]
    fn rename_moves_the_directory_and_renames_the_main_ino() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        std::fs::write(dir.join("notes.md"), "keep me\n").unwrap();
        let before_yaml = read(dir.join("sketch.yaml"));

        let made = rename_project(&dir, "New").unwrap();

        let parent = tmp.path().canonicalize().unwrap();
        assert_eq!(made.dir, parent.join("New"));
        assert_eq!(made.name, "New");
        assert!(!dir.exists(), "the old directory must be gone");
        assert!(made.dir.join("New.ino").is_file());
        assert!(!made.dir.join("Old.ino").exists());
        // Line 1's title comment follows the name…
        assert_eq!(
            read(made.dir.join("New.ino")),
            "// New — demo\n\nvoid setup() {}\n"
        );
        // …and nothing else in the tree moved or changed.
        assert_eq!(read(made.dir.join("notes.md")), "keep me\n");
        assert_eq!(read(made.dir.join("src/util.h")), "#pragma once\n");
        assert_eq!(read(made.dir.join("sketch.yaml")), before_yaml);
        assert!(made.warnings.is_empty(), "{:?}", made.warnings);
    }

    #[test]
    fn rename_rebases_absolute_lib_paths_pointing_inside_the_project() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        std::fs::create_dir_all(dir.join("libs/EnvSensor")).unwrap();
        let abs_inside = dir.canonicalize().unwrap().join("libs/EnvSensor");
        std::fs::write(
            dir.join("sketch.yaml"),
            format!(
                "profiles:\n\
                 \x20 p:\n\
                 \x20   fqbn: a:b:c\n\
                 \x20   libraries:\n\
                 \x20     - PubSubClient (2.8.0)\n\
                 \x20     - dir: libs/EnvSensor\n\
                 \x20     - dir: {}\n",
                abs_inside.display()
            ),
        )
        .unwrap();

        let made = rename_project(&dir, "New").unwrap();
        let text = read(made.dir.join("sketch.yaml"));

        let new_abs = made.dir.join("libs/EnvSensor");
        assert!(
            text.contains(&format!("- dir: {}", new_abs.display())),
            "{text}"
        );
        assert!(!text.contains(&abs_inside.display().to_string()), "{text}");
        assert!(new_abs.is_dir(), "the rebased path must exist");
        // Registry and relative-inside deps are untouched.
        assert!(text.contains("- PubSubClient (2.8.0)\n"), "{text}");
        assert!(text.contains("- dir: libs/EnvSensor\n"), "{text}");
    }

    #[test]
    fn rename_leaves_absolute_lib_paths_outside_the_project_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        let outside = tmp.path().canonicalize().unwrap().join("shared/HomeNode");
        std::fs::create_dir_all(&outside).unwrap();
        let yaml = format!(
            "profiles:\n  p:\n    fqbn: a:b:c\n    libraries:\n      - dir: {}\n",
            outside.display()
        );
        std::fs::write(dir.join("sketch.yaml"), &yaml).unwrap();

        let made = rename_project(&dir, "New").unwrap();

        assert_eq!(read(made.dir.join("sketch.yaml")), yaml, "must stay verbatim");
        assert!(made.warnings.is_empty(), "{:?}", made.warnings);
    }

    #[test]
    fn rename_rejects_an_invalid_new_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        let err = rename_project(&dir, "My Project").unwrap_err().to_string();
        assert!(err.contains("instead of spaces"), "{err}");
        assert!(dir.join("Old.ino").is_file(), "the project must survive");
    }

    #[test]
    fn rename_refuses_a_missing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let err = rename_project(&tmp.path().join("nope"), "New")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a sketch folder"), "{err}");
    }

    #[test]
    fn rename_refuses_a_folder_without_a_main_ino() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Old");
        std::fs::create_dir_all(&dir).unwrap();
        let err = rename_project(&dir, "New").unwrap_err().to_string();
        assert!(err.contains("not a sketch folder"), "{err}");
        assert!(err.contains("Old.ino"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn rename_refuses_a_symlinked_main_ino() {
        // is_file() follows links, so a symlinked main ino would pass and then
        // be renamed under its OLD name, leaving the retitle editing a file
        // that is shared with another project.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Old");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(tmp.path().join("real.ino"), "// Old\n").unwrap();
        std::os::unix::fs::symlink("../real.ino", dir.join("Old.ino")).unwrap();

        let err = rename_project(&dir, "New").unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
        assert_eq!(read(tmp.path().join("real.ino")), "// Old\n");
    }

    #[test]
    fn rename_refuses_the_name_it_already_has() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        let err = rename_project(&dir, " Old ").unwrap_err().to_string();
        assert!(err.contains("already named"), "{err}");
    }

    #[test]
    fn rename_refuses_an_existing_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        std::fs::create_dir_all(tmp.path().join("New")).unwrap();
        let err = rename_project(&dir, "New").unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
        assert!(dir.join("Old.ino").is_file(), "the project must survive");
    }

    #[test]
    #[cfg(unix)]
    fn rename_refuses_a_dangling_symlink_at_the_destination() {
        // exists() reports false for a dangling symlink while the rename would
        // still land on it — symlink_metadata is what catches this.
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        std::os::unix::fs::symlink("nowhere", tmp.path().join("New")).unwrap();
        let err = rename_project(&dir, "New").unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
    }

    #[test]
    fn rename_refuses_a_case_only_clash_with_a_sibling() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        std::fs::create_dir_all(tmp.path().join("new")).unwrap();
        let err = rename_project(&dir, "New").unwrap_err().to_string();
        assert!(err.contains("differs only in case"), "{err}");
    }

    #[test]
    fn rename_refuses_a_case_only_change_of_the_project_itself() {
        // The clash is the source directory, so the generic "already exists"
        // wording would be nonsense — and a case-only rename is not safe to
        // do in one step on a case-insensitive filesystem.
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        let err = rename_project(&dir, "old").unwrap_err().to_string();
        assert!(err.contains("differ only in case"), "{err}");
        assert!(dir.join("Old.ino").is_file(), "the project must survive");
    }

    #[test]
    fn rename_refuses_when_a_secondary_ino_would_be_clobbered() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        std::fs::write(dir.join("New.ino"), "// secondary sketch file\n").unwrap();
        let err = rename_project(&dir, "New").unwrap_err().to_string();
        assert!(err.contains("New.ino"), "{err}");
        assert_eq!(read(dir.join("New.ino")), "// secondary sketch file\n");
    }

    #[test]
    fn rename_refuses_a_git_worktree_checkout() {
        // A linked worktree's top-level .git is a FILE holding an absolute
        // `gitdir:` path, with a backlink from the main repository. A plain
        // directory rename breaks both directions.
        let tmp = tempfile::tempdir().unwrap();
        let dir = sample_project(tmp.path(), "Old");
        let gitfile = "gitdir: /somewhere/repo/.git/worktrees/Old\n";
        std::fs::write(dir.join(".git"), gitfile).unwrap();

        let err = rename_project(&dir, "New").unwrap_err().to_string();
        assert!(err.contains("worktree"), "{err}");
        assert!(err.contains("git worktree move"), "{err}");
        assert_eq!(read(dir.join(".git")), gitfile, "the .git file is untouched");
        assert!(dir.join("Old.ino").is_file(), "the project must survive");
    }

    #[test]
    #[cfg(unix)]
    fn rename_rolls_back_the_ino_when_the_directory_rename_fails() {
        // The whole point of doing the in-directory work first: the one
        // irreversible step is last, so a failure there leaves a sketch
        // arduino-cli still recognises.
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("home");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = sample_project(&parent, "Old");
        std::fs::create_dir_all(dir.join("libs/X")).unwrap();
        let abs_inside = dir.canonicalize().unwrap().join("libs/X");
        let yaml = format!(
            "profiles:\n  p:\n    fqbn: a:b:c\n    libraries:\n      - dir: {}\n",
            abs_inside.display()
        );
        std::fs::write(dir.join("sketch.yaml"), &yaml).unwrap();
        let before_ino = read(dir.join("Old.ino"));

        // A read-only parent makes the final rename — and only it — fail.
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(parent.join(".probe"), "x").is_ok() {
            // Running as root ignores the mode bits; there is nothing to prove.
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let err = rename_project(&dir, "New").unwrap_err().to_string();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(err.contains("Old"), "{err}");
        assert!(!parent.join("New").exists(), "the project must not have moved");
        assert!(dir.join("Old.ino").is_file(), "the main ino must be back");
        assert!(!dir.join("New.ino").exists(), "the renamed ino must be gone");
        assert_eq!(read(dir.join("Old.ino")), before_ino, "title restored");
        assert_eq!(read(dir.join("sketch.yaml")), yaml, "sketch.yaml restored");
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
