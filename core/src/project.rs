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
/// Blink for a board whose onboard LED is an addressable WS2812. Not a
/// `SketchTemplate` of its own: the user picks "Blink", and which of the two
/// they get is a fact about their board, not a choice to put in front of them.
const TMPL_BLINK_RGB: &str = include_str!("templates/sketch/blink_rgb.ino.tmpl");
const TMPL_I2C_SCAN: &str = include_str!("templates/sketch/i2c_scan.ino.tmpl");
const TMPL_WIFI_SCAN: &str = include_str!("templates/sketch/wifi_scan.ino.tmpl");
const TMPL_BOARD_INFO: &str = include_str!("templates/sketch/board_info.ino.tmpl");
const TMPL_WAVEFORMS: &str = include_str!("templates/sketch/waveforms.ino.tmpl");
const TMPL_ANALOG_PLOT: &str = include_str!("templates/sketch/analog_plot.ino.tmpl");
const TMPL_SERIAL_ECHO: &str = include_str!("templates/sketch/serial_echo.ino.tmpl");

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
        id: "waveforms",
        label: "Waveforms",
        description: "Sine, triangle and sawtooth in plotter format — no wiring. The fastest way to see the Scope draw.",
        tmpl: TMPL_WAVEFORMS,
    },
    SketchTemplate {
        id: "analog-plot",
        label: "Analog plot",
        description: "Reads one ADC pin and plots it raw against a smoothed copy — a potentiometer is enough.",
        tmpl: TMPL_ANALOG_PLOT,
    },
    SketchTemplate {
        id: "serial-echo",
        label: "Serial echo",
        description: "Echoes what you type, numbered, with an idle heartbeat — proves the board listens, not just talks.",
        tmpl: TMPL_SERIAL_ECHO,
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

/// The chip an Arduino FQBN builds for.
///
/// This is the bridge between the two paradigms. [`crate::boardprofile`] is
/// keyed by **chip target** — `idf.py` has no board concept, so the table it
/// came from could only be — while an Arduino project is keyed by **FQBN**.
/// Both must reach the same board data, or the pin advice would differ
/// depending on which toolchain happens to build the project, which is worse
/// than having none.
///
/// It is a fold rather than a parse, because the esp32 core spells its board
/// segment two different ways: as the bare chip (`esp32:esp32:esp32s3`) and as
/// a board whose name merely *starts* with it
/// (`esp32:esp32:esp32doit-devkit-v1`). So the answer is the **longest** known
/// target id the folded segment begins with — `esp32s3` beats its own prefix
/// `esp32`, which is the difference between the right pin table and a
/// plausible wrong one.
///
/// `None` for an FQBN naming no ESP chip (`arduino:avr:uno`). That is the
/// honest answer, and callers must render it as "no profile for this board"
/// rather than as clean wiring.
pub fn target_for_fqbn(fqbn: &str) -> Option<&'static crate::targets::Target> {
    let board = fqbn.split(':').nth(2)?.trim();
    let folded = crate::targets::normalize_id(board);
    crate::targets::KNOWN_TARGETS
        .iter()
        .filter(|t| folded.starts_with(t.id))
        .max_by_key(|t| t.id.len())
}

/// The boards Bancada knows that are built around an FQBN's chip.
///
/// Empty for a chip with no modelled devkit and for a non-ESP board alike —
/// the caller cannot tell the two apart and does not need to, because the
/// rendering is the same: there is no board profile, so say so.
pub fn board_candidates_for_fqbn(fqbn: &str) -> Vec<&'static crate::boardprofile::Board> {
    match target_for_fqbn(fqbn) {
        Some(t) => crate::boardprofile::boards_for_target(t.id),
        None => Vec::new(),
    }
}

/// The file a project of this kind records its board in.
///
/// Two files because the two paradigms have nothing in common to write to.
/// An ESP-IDF project has no manifest, and the record must survive
/// `idf.py fullclean` (which regenerates `sdkconfig`, never
/// `sdkconfig.defaults`) and travel with the repo. An Arduino sketch has no
/// `sdkconfig` at all, but already carries `bancada.yaml`.
fn board_record_file(dir: &Path, kind: ProjectKind) -> PathBuf {
    match kind {
        ProjectKind::Idf => dir.join("sdkconfig.defaults"),
        // An `Unknown` directory is a folder someone opened, not a project.
        // It gets the manifest too, so that recording a board before the
        // sketch exists is not a special case.
        ProjectKind::Arduino | ProjectKind::Unknown => {
            dir.join(crate::ghlib::MANIFEST_NAME)
        }
    }
}

/// The board a project records for itself, if it records one Bancada knows.
///
/// `None` covers three different situations on purpose — no record, an
/// unreadable file, and a recorded id with no data behind it. The rendering is
/// the same for all three (there is no board profile, say so), and the one
/// thing that must never happen is answering with a *different* board than the
/// project named. An id travels in the repo and may come from a newer Bancada
/// or be a typo; either way, inventing a match would give confidently wrong
/// pin advice.
pub fn recorded_board(dir: &Path) -> Option<&'static crate::boardprofile::Board> {
    let kind = detect_kind(dir);
    let path = board_record_file(dir, kind);
    let text = std::fs::read_to_string(&path).ok()?;

    let id = match kind {
        ProjectKind::Idf => crate::boardprofile::read_marker(&text)?,
        ProjectKind::Arduino | ProjectKind::Unknown => {
            serde_yaml::from_str::<crate::ghlib::Manifest>(&text)
                .ok()?
                .board?
        }
    };
    crate::boardprofile::Board::from_id(&id)
}

/// Record `board_id` as the project's board, leaving the rest of the file
/// alone.
///
/// Both writes are edits rather than rewrites: the marker is set in place in
/// whatever `sdkconfig.defaults` already says, and the manifest is loaded and
/// saved so library pins survive. The id is not validated here — callers pass
/// one from [`crate::boardprofile::KNOWN_BOARDS`], and a project that outlives
/// this build's table is exactly the case [`recorded_board`] already handles.
pub fn record_board(dir: &Path, board_id: &str) -> Result<()> {
    match detect_kind(dir) {
        ProjectKind::Idf => {
            let path = board_record_file(dir, ProjectKind::Idf);
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            std::fs::write(&path, crate::boardprofile::set_marker(&existing, board_id))?;
        }
        kind => {
            let mut manifest = crate::ghlib::Manifest::load(dir)?;
            manifest.board = Some(board_id.to_string());
            manifest.save(dir)?;
            let _ = kind;
        }
    }
    Ok(())
}

/// What Bancada can say about a project's board.
///
/// Four answers, because the UI says something different for each and must not
/// collapse them. The split that matters is [`Recorded`](BoardChoice::Recorded)
/// versus [`Inferred`](BoardChoice::Inferred): both carry a board and both
/// yield pin advice, but one is a fact the project states about itself and the
/// other is Bancada's guess from the chip. The board model's founding rule is
/// that a deterministic answer and a plausible one must not look alike, so they
/// are different variants rather than a board plus a boolean nobody renders.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum BoardChoice {
    /// The project names a board and this build has its data.
    ///
    /// A named field rather than a newtype: internally-tagged serde merges a
    /// newtype variant's fields in beside the tag, which would spread the
    /// board across the same object as `state` and make the IPC contract read
    /// as a bag of loose keys.
    Recorded {
        board: &'static crate::boardprofile::Board,
    },
    /// The chip has exactly one modelled devkit, so that is almost certainly
    /// what is on the bench — but nobody said so. Usable, and labelled.
    Inferred {
        board: &'static crate::boardprofile::Board,
    },
    /// Bancada knows the chip and has several candidate devkits, and guessing
    /// between them would be a coin toss. Never empty, never of length one.
    Unchosen {
        candidates: Vec<&'static crate::boardprofile::Board>,
    },
    /// No board data applies — a non-ESP board, or an ESP chip with no
    /// modelled devkit. The UI says so plainly rather than implying the
    /// wiring is clean.
    NoProfile,
}

impl BoardChoice {
    /// The board to give pin advice for, if there is one.
    ///
    /// Deliberately flattens `Recorded` and `Inferred`: every consumer that
    /// wants a pinout wants it either way, and only the *presentation* cares
    /// which. Consumers that must distinguish them match on the variant.
    pub fn board(&self) -> Option<&'static crate::boardprofile::Board> {
        match self {
            BoardChoice::Recorded { board } | BoardChoice::Inferred { board } => Some(board),
            BoardChoice::Unchosen { .. } | BoardChoice::NoProfile => None,
        }
    }
}

/// Decide which of the three answers applies.
///
/// `candidates` comes from [`board_candidates_for_fqbn`] for an Arduino
/// project or [`crate::boardprofile::boards_for_target`] for an ESP-IDF one —
/// the two paradigms differ in how they name a chip, and nowhere else. This
/// function is the *policy* and holds no paradigm knowledge at all.
pub fn resolve_board(
    dir: &Path,
    candidates: &[&'static crate::boardprofile::Board],
) -> BoardChoice {
    if let Some(board) = recorded_board(dir) {
        return BoardChoice::Recorded { board };
    }
    if candidates.is_empty() {
        return BoardChoice::NoProfile;
    }

    // One modelled devkit for the chip: adopt it, but as a guess. Today every
    // target in KNOWN_BOARDS has exactly one, so this is the branch New
    // Project actually takes — a wizard that demanded a choice with one option
    // would be asking a question whose answer is foregone. Nothing is written:
    // resolving a board must never mutate the project, so the guess lasts only
    // as long as the answer does, and recording it stays an explicit act.
    match candidates {
        [only] => BoardChoice::Inferred { board: only },
        many => BoardChoice::Unchosen {
            candidates: many.to_vec(),
        },
    }
}

/// The board a project is on, decided from the directory alone.
///
/// This is the one entry point the app uses. Everything paradigm-specific is
/// here and nowhere else: an Arduino sketch names its chip through its default
/// profile's FQBN, an ESP-IDF project through `CONFIG_IDF_TARGET`, and the two
/// meet at [`resolve_board`]. The same discipline [`detect_kind`] follows —
/// one answer, so the Board tab and the pin warnings cannot disagree.
pub fn project_board(dir: &Path) -> BoardChoice {
    let candidates = match detect_kind(dir) {
        ProjectKind::Arduino => fqbn_of_sketch(dir)
            .map(|f| board_candidates_for_fqbn(&f))
            .unwrap_or_default(),
        ProjectKind::Idf => idf_target_of(dir)
            .map(|t| crate::boardprofile::boards_for_target(&t))
            .unwrap_or_default(),
        // Nothing to build, so nothing to be on. A recorded board still wins
        // below — a folder can hold a marker before it holds a project.
        ProjectKind::Unknown => Vec::new(),
    };
    resolve_board(dir, &candidates)
}

/// The FQBN a sketch builds with: its default profile's, or its only
/// profile's when no default is named.
fn fqbn_of_sketch(dir: &Path) -> Option<String> {
    let yaml = crate::sketch::SketchProject::open(dir).ok()?.load_yaml().ok()?;
    let profile = match &yaml.default_profile {
        Some(name) => yaml.profiles.get(name)?,
        None if yaml.profiles.len() == 1 => yaml.profiles.values().next()?,
        // Several profiles and no default: which chip the project is "on" has
        // no answer, and picking one arbitrarily would be a guess wearing a
        // fact's clothes.
        None => return None,
    };
    Some(profile.fqbn.clone())
}

/// The chip an ESP-IDF project is configured for, from `sdkconfig` — or,
/// before a first build has resolved one, from `sdkconfig.defaults`.
pub fn idf_target_of(dir: &Path) -> Option<String> {
    ["sdkconfig", "sdkconfig.defaults"].iter().find_map(|name| {
        let text = std::fs::read_to_string(dir.join(name)).ok()?;
        crate::idf::parse_sdkconfig_target(&text)
    })
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
    sketch_from_template_for(id, name, None)
}

/// Render the template with `id`, using what the board model knows about
/// `board` where the starter has something to say about it.
///
/// Only Blink differs today, and it differs in two ways rather than one. The
/// pin is the easy half. The harder half is [`LedKind`]: an ESP32-S3-DevKitC-1
/// and a C6-DevKitC-1 carry an addressable WS2812, where `digitalWrite` does
/// precisely nothing — a board that flashes clean and then sits dark, which is
/// exactly the evening the board model exists to prevent. So a WS2812 board
/// gets a different sketch, not a different constant.
///
/// `None` for the board keeps the portable fallback, guard and all: with no
/// data, a `#ifndef LED_BUILTIN` the user can override is the honest shape.
///
/// [`LedKind`]: crate::boardprofile::LedKind
pub fn sketch_from_template_for(
    id: &str,
    name: &str,
    board: Option<&crate::boardprofile::Board>,
) -> Option<String> {
    use crate::boardprofile::LedKind;

    let t = TEMPLATES.iter().find(|t| t.id == id)?;

    // A WS2812 board replaces Blink outright; everything else renders its own
    // template and only fills in the LED block.
    if id == "blink" {
        if let Some(led) = board.and_then(|b| b.led) {
            let b = board.expect("led implies a board");
            if led.kind == LedKind::Ws2812 {
                return Some(
                    TMPL_BLINK_RGB
                        .replace("{name}", name)
                        .replace("{board}", b.name)
                        .replace("{led_pin}", &led.gpio.to_string()),
                );
            }
            return Some(t.tmpl.replace("{name}", name).replace(
                "{led_block}",
                &format!(
                    "#define LED_BUILTIN {}  // {}'s onboard LED",
                    led.gpio, b.name
                ),
            ));
        }
    }

    Some(t.tmpl.replace("{name}", name).replace(
        "{led_block}",
        "#ifndef LED_BUILTIN\n#define LED_BUILTIN 2  // most ESP32 dev boards; change if your LED is wired elsewhere\n#endif",
    ))
}

/// Replace the main `.ino` that `arduino-cli sketch new` stubbed out with the
/// chosen starter. Callers guarantee `dir` was created by us moments ago, so
/// this never clobbers user content.
pub fn write_main_ino(dir: &Path, name: &str, template_id: &str) -> Result<()> {
    write_main_ino_for(dir, name, template_id, None)
}

/// [`write_main_ino`], with the board whose facts the starter should use.
pub fn write_main_ino_for(
    dir: &Path,
    name: &str,
    template_id: &str,
    board: Option<&crate::boardprofile::Board>,
) -> Result<()> {
    let sketch = sketch_from_template_for(template_id, name, board).ok_or_else(|| {
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


// ---------- project kind ----------

/// Which toolchain a directory is built with.
///
/// Bancada drives two: `arduino-cli` for sketches and `idf.py` for ESP-IDF
/// projects. Which one applies is decided here, from the directory alone, and
/// **only here** — the frontend renders this answer rather than computing a
/// second copy of it that could drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Arduino,
    Idf,
    /// Neither — there is nothing here to build.
    Unknown,
}

/// Does this `CMakeLists.txt` declare a CMake *project*?
///
/// Merely existing is not enough: every ESP-IDF component directory has a
/// `CMakeLists.txt` too, and those call `idf_component_register` rather than
/// `project()`. This mirrors the check ESP-IDF's own tooling makes before
/// agreeing that a directory is a project at all.
///
/// CMake command names are case-insensitive, and a commented-out `project()`
/// obviously does not count.
pub fn declares_cmake_project(cmakelists: &str) -> bool {
    cmakelists.lines().any(|line| {
        let t = line.trim_start();
        if t.starts_with('#') {
            return false;
        }
        let Some(rest) = t.get(..7) else {
            return false;
        };
        if !rest.eq_ignore_ascii_case("project") {
            return false;
        }
        // `project (name)` is legal; `project_something(...)` is not a match.
        t[7..].trim_start().starts_with('(')
    })
}

/// Decide a project's kind from what the directory contains.
///
/// **ESP-IDF wins when both are present.** A folder can legitimately hold an
/// Arduino sketch beside a real IDF project — `arduino-esp32` ships as an IDF
/// component, so hybrids exist — and of the two, only `idf.py` can build such a
/// tree; `arduino-cli` cannot. Choosing IDF is therefore the reading that can
/// actually succeed. This is a deliberate decision rather than an accident of
/// ordering, which is why it has a test of its own.
pub fn classify(has_ino: bool, has_sketch_yaml: bool, has_idf_cmake: bool) -> ProjectKind {
    if has_idf_cmake {
        ProjectKind::Idf
    } else if has_ino || has_sketch_yaml {
        ProjectKind::Arduino
    } else {
        ProjectKind::Unknown
    }
}

/// Whether the directory holds a sketch's main file.
///
/// Deliberately looser than [`crate::sketch::SketchProject::main_ino`], which
/// insists on `<dirname>.ino`. For *detection* a mis-named `.ino` still means
/// "this is an Arduino sketch", and letting arduino-cli say "main file missing"
/// is a far better error than bancada saying "unknown project kind".
fn has_root_ino(dir: &Path) -> bool {
    if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
        if dir.join(format!("{name}.ino")).is_file() {
            return true;
        }
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path()
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("ino"))
    })
}

/// Classify a directory on disk.
pub fn detect_kind(dir: &Path) -> ProjectKind {
    let cmake = dir.join("CMakeLists.txt");
    let has_idf_cmake = std::fs::read_to_string(&cmake)
        .map(|t| declares_cmake_project(&t))
        .unwrap_or(false);
    classify(
        has_root_ino(dir),
        dir.join("sketch.yaml").is_file(),
        has_idf_cmake,
    )
}

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

    fn board(id: &str) -> &'static crate::boardprofile::Board {
        crate::boardprofile::Board::from_id(id).expect(id)
    }

    #[test]
    fn blink_without_a_board_keeps_the_portable_fallback() {
        let s = sketch_from_template_for("blink", "Demo", None).unwrap();
        assert!(s.contains("#ifndef LED_BUILTIN"), "{s}");
        assert!(s.contains("digitalWrite"), "{s}");
    }

    #[test]
    fn blink_on_a_plain_led_board_names_the_pin_it_knows() {
        let doit = board("esp32-doit-devkit-v1");
        let s = sketch_from_template_for("blink", "Demo", Some(doit)).unwrap();
        // No guard: the pin is a fact about this board, not a guess to be
        // overridden, and the comment says which board said so.
        assert!(!s.contains("#ifndef LED_BUILTIN"), "{s}");
        assert!(s.contains(&format!("#define LED_BUILTIN {}", doit.led.unwrap().gpio)));
        assert!(s.contains(doit.name), "{s}");
    }

    #[test]
    fn blink_on_a_ws2812_board_drives_the_rgb_led_instead() {
        // The whole reason LedKind exists: digitalWrite does nothing to an
        // addressable LED, so this is different code, not a different pin.
        let s3 = board("esp32-s3-devkitc-1");
        let s = sketch_from_template_for("blink", "Demo", Some(s3)).unwrap();
        assert!(s.contains("rgbLedWrite"), "{s}");
        assert!(!s.contains("digitalWrite"), "{s}");
        assert!(s.contains(&s3.led.unwrap().gpio.to_string()), "{s}");
    }

    #[test]
    fn a_board_does_not_change_a_template_that_is_not_blink() {
        let s3 = board("esp32-s3-devkitc-1");
        assert_eq!(
            sketch_from_template_for("i2c-scan", "Demo", Some(s3)),
            sketch_from_template("i2c-scan", "Demo")
        );
    }

    #[test]
    fn the_name_is_still_substituted_on_every_board_path() {
        for b in [None, Some(board("esp32-doit-devkit-v1")), Some(board("esp32-s3-devkitc-1"))] {
            let s = sketch_from_template_for("blink", "MyNode", b).unwrap();
            assert!(s.contains("MyNode"), "{s}");
            assert!(!s.contains("{name}"), "{s}");
        }
    }

    #[test]
    fn board_choice_serialises_to_the_shape_the_frontend_declares() {
        // src/api.ts hand-writes this union. Nothing else forces the two to
        // agree, and a silent rename here would reach the UI as an undefined
        // field rather than an error, so the shape is asserted rather than
        // described. Keep in step with `BoardChoice` in api.ts.
        let b = board("esp32-s3-devkitc-1");

        let json = serde_json::to_value(BoardChoice::Recorded { board: b }).unwrap();
        assert_eq!(json["state"], "recorded");
        assert_eq!(json["board"]["id"], "esp32-s3-devkitc-1");

        let json = serde_json::to_value(BoardChoice::Inferred { board: b }).unwrap();
        assert_eq!(json["state"], "inferred");
        assert_eq!(json["board"]["id"], "esp32-s3-devkitc-1");

        let json = serde_json::to_value(BoardChoice::Unchosen {
            candidates: vec![b],
        })
        .unwrap();
        assert_eq!(json["state"], "unchosen");
        assert_eq!(json["candidates"][0]["id"], "esp32-s3-devkitc-1");

        let json = serde_json::to_value(BoardChoice::NoProfile).unwrap();
        assert_eq!(json["state"], "no-profile");
        // A unit variant must not acquire a payload key the union lacks.
        assert_eq!(json.as_object().unwrap().len(), 1);
    }

    #[test]
    fn every_caveat_id_the_frontend_lists_is_one_the_glossary_emits() {
        // The other half of the same contract: api.ts's `Caveat` union.
        let ids: Vec<String> = crate::boardprofile::caveat_glossary()
            .iter()
            .map(|c| serde_json::to_value(c.id).unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            ids,
            [
                "strapping",
                "input-only",
                "flash-or-psram",
                "usb-serial-jtag",
                "uart0-console",
                "jtag",
                "adc2-wifi-conflict",
                "onboard-led",
                "boot-button",
            ]
        );
    }

    #[test]
    fn an_arduino_project_finds_its_board_through_its_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();
        std::fs::write(
            dir.join("sketch.yaml"),
            "default_profile: esp32s3\nprofiles:\n  esp32s3:\n    fqbn: esp32:esp32:esp32s3\n",
        )
        .unwrap();

        assert_eq!(
            project_board(dir).board().map(|b| b.id),
            Some("esp32-s3-devkitc-1")
        );
    }

    #[test]
    fn an_idf_project_finds_its_board_through_its_target() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("CMakeLists.txt"), "project(demo)\n").unwrap();
        std::fs::write(dir.join("sdkconfig"), "CONFIG_IDF_TARGET=\"esp32c6\"\n").unwrap();

        assert_eq!(
            project_board(dir).board().map(|b| b.id),
            Some("esp32-c6-devkitc-1")
        );
    }

    #[test]
    fn a_folder_that_is_neither_paradigm_has_no_board() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(project_board(tmp.path()), BoardChoice::NoProfile);
    }

    #[test]
    fn an_arduino_project_on_a_non_esp_board_has_no_board_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();
        std::fs::write(
            dir.join("sketch.yaml"),
            "default_profile: uno\nprofiles:\n  uno:\n    fqbn: arduino:avr:uno\n",
        )
        .unwrap();

        assert_eq!(project_board(dir), BoardChoice::NoProfile);
    }

    #[test]
    fn a_recorded_board_wins_over_every_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();
        record_board(dir, "esp32-c6-devkitc-1").unwrap();

        // Candidates for a different chip entirely: what the project says
        // about itself is the authority, not what the FQBN suggests.
        let choice = resolve_board(dir, &[board("esp32-s3-devkitc-1")]);
        assert_eq!(choice, BoardChoice::Recorded { board: board("esp32-c6-devkitc-1") });
    }

    #[test]
    fn a_chip_with_no_modelled_devkit_has_no_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();

        assert_eq!(resolve_board(dir, &[]), BoardChoice::NoProfile);
    }

    #[test]
    fn a_lone_candidate_is_inferred_not_recorded() {
        // Adopted so the pin advice works with no ceremony, but flagged: the
        // user may own a different S3 devkit, and presenting a guess as a
        // recorded fact is the one outcome the board model must not produce.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();

        let choice = resolve_board(dir, &[board("esp32-s3-devkitc-1")]);
        assert_eq!(choice, BoardChoice::Inferred { board: board("esp32-s3-devkitc-1") });
    }

    #[test]
    fn inferring_a_board_does_not_write_it_to_the_project() {
        // The whole point of Inferred: nothing was decided, so nothing is
        // persisted. Opening a project must never mutate it.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();

        let _ = resolve_board(dir, &[board("esp32-s3-devkitc-1")]);
        assert!(recorded_board(dir).is_none());
        assert!(!dir.join("bancada.yaml").exists());
    }

    #[test]
    fn several_candidates_are_left_for_a_human_to_choose() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();

        let two = [board("esp32-s3-devkitc-1"), board("esp32-c6-devkitc-1")];
        assert_eq!(
            resolve_board(dir, &two),
            BoardChoice::Unchosen {
                candidates: two.to_vec()
            }
        );
    }

    #[test]
    fn an_idf_project_remembers_its_board_in_sdkconfig_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("CMakeLists.txt"), "project(demo)\n").unwrap();
        std::fs::write(
            dir.join("sdkconfig.defaults"),
            "CONFIG_ESPTOOLPY_FLASHSIZE_4MB=y\n# bancada.board = esp32-c6-devkitc-1\n",
        )
        .unwrap();

        assert_eq!(recorded_board(dir).unwrap().id, "esp32-c6-devkitc-1");
    }

    #[test]
    fn an_arduino_project_remembers_its_board_in_the_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();
        std::fs::write(
            dir.join("bancada.yaml"),
            "version: 1\nboard: esp32-s3-devkitc-1\nlibraries: []\n",
        )
        .unwrap();

        assert_eq!(recorded_board(dir).unwrap().id, "esp32-s3-devkitc-1");
    }

    #[test]
    fn a_project_that_names_no_board_records_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();
        assert!(recorded_board(dir).is_none());

        // A manifest that predates the field still loads, and still says none.
        std::fs::write(dir.join("bancada.yaml"), "version: 1\nlibraries: []\n").unwrap();
        assert!(recorded_board(dir).is_none());
    }

    #[test]
    fn a_recorded_board_bancada_does_not_know_is_not_invented() {
        // The id travels in the repo and may name a board a newer Bancada
        // added, or a typo. Either way there is no data behind it, and
        // answering with a *different* board would be the worst outcome.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();
        std::fs::write(
            dir.join("bancada.yaml"),
            "version: 1\nboard: some-board-from-2027\nlibraries: []\n",
        )
        .unwrap();

        assert!(recorded_board(dir).is_none());
    }

    #[test]
    fn recording_a_board_round_trips_through_both_paradigms() {
        for (marker_file, seed) in [
            ("sdkconfig.defaults", "CONFIG_X=y\n"),
            ("bancada.yaml", "version: 1\nlibraries: []\n"),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            if marker_file == "sdkconfig.defaults" {
                std::fs::write(dir.join("CMakeLists.txt"), "project(demo)\n").unwrap();
            } else {
                std::fs::write(dir.join("Demo.ino"), "void setup(){}\n").unwrap();
            }
            std::fs::write(dir.join(marker_file), seed).unwrap();

            record_board(dir, "esp32-s3-devkitc-1").unwrap();
            assert_eq!(
                recorded_board(dir).unwrap().id,
                "esp32-s3-devkitc-1",
                "{marker_file}"
            );
            // What was already in the file survives being written through.
            // Line by line, not as a block: the manifest is re-serialised, so
            // `board:` lands between the seeded keys rather than after them.
            let text = std::fs::read_to_string(dir.join(marker_file)).unwrap();
            for line in seed.lines().filter(|l| !l.trim().is_empty()) {
                assert!(text.contains(line), "{marker_file} lost {line:?}:\n{text}");
            }
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
    fn folds_an_fqbn_to_the_chip_it_builds_for() {
        // The esp32 core spells some board segments as the bare chip...
        assert_eq!(target_for_fqbn("esp32:esp32:esp32s3").unwrap().id, "esp32s3");
        assert_eq!(target_for_fqbn("esp32:esp32:esp32c6").unwrap().id, "esp32c6");
        // ...and others as a board name that merely starts with it. The chip
        // is the LONGEST known id the segment begins with, which is the whole
        // reason this is not a table lookup.
        assert_eq!(
            target_for_fqbn("esp32:esp32:esp32doit-devkit-v1").unwrap().id,
            "esp32"
        );
    }

    #[test]
    fn the_longest_target_wins_over_its_own_prefix() {
        // `esp32s3` begins with `esp32`; answering `esp32` here would hand the
        // user an S3 board's pins from the wrong chip's table.
        assert_eq!(target_for_fqbn("esp32:esp32:esp32s3").unwrap().id, "esp32s3");
        assert_eq!(
            target_for_fqbn("esp32:esp32:esp32c3-devkitm-1").unwrap().id,
            "esp32c3"
        );
    }

    #[test]
    fn board_options_do_not_change_the_chip() {
        assert_eq!(
            target_for_fqbn("esp32:esp32:esp32s3:PSRAM=opi,FlashMode=qio")
                .unwrap()
                .id,
            "esp32s3"
        );
    }

    #[test]
    fn a_non_esp_fqbn_has_no_chip_profile() {
        // The right answer is "no profile", not a guess. An AVR Uno has no
        // entry in the ESP target table and must not acquire one by prefix.
        for fqbn in ["arduino:avr:uno", "arduino:zephyr:unoq", "", "nonsense"] {
            assert!(target_for_fqbn(fqbn).is_none(), "{fqbn}");
        }
    }

    #[test]
    fn an_fqbn_reaches_the_boards_built_around_its_chip() {
        let boards = board_candidates_for_fqbn("esp32:esp32:esp32s3");
        assert!(
            boards.iter().any(|b| b.id == "esp32-s3-devkitc-1"),
            "{:?}",
            boards.iter().map(|b| b.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_board_bancada_has_no_profile_for_yields_no_candidates() {
        // Never block on missing board data, and never imply the wiring is
        // clean by returning something.
        assert!(board_candidates_for_fqbn("arduino:avr:uno").is_empty());
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
            assert!(
                s.contains("Serial.begin(115200)"),
                "{}: not serial-verbose",
                t.id
            );
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
        assert!(
            !s.contains("Serial.printf"),
            "printf is an ESP32-core extra"
        );
        assert!(
            s.contains("#if defined(ARDUINO_ARCH_ESP32)"),
            "runtime pin override exists only on ESP32 cores and must be guarded"
        );
    }

    #[test]
    fn plotter_templates_emit_labelled_numbers() {
        // The Scope's Plotter source parses `label:value` pairs out of the
        // serial stream. A starter that prints prose gives it nothing to
        // draw, which is indistinguishable from a broken scope.
        for id in ["waveforms", "analog-plot"] {
            let s = sketch_from_template(id, "PlotNode").unwrap();
            assert!(s.contains("Serial.print(\""), "{id}: prints no labels");
            assert!(s.contains(":\""), "{id}: no `label:` pair");
        }
    }

    #[test]
    fn universal_templates_avoid_core_specific_apis() {
        // These four are offered for *any* board, so they must build on cores
        // whose Serial has no printf (AVR, the Uno Q's BridgeMonitor) and
        // which define no ESP32 conveniences. Verified against arduino:avr,
        // esp32, esp8266 and arduino:zephyr — the same lesson `i2c-scan`
        // learned by being retargeted onto a Uno Q.
        for id in ["blink", "waveforms", "analog-plot", "serial-echo"] {
            let s = sketch_from_template(id, "AnyNode").unwrap();
            assert!(
                !s.contains("Serial.printf"),
                "{id}: printf is an ESP32-core extra"
            );
            assert!(!s.contains("ESP."), "{id}: ESP.* is ESP-only");
            assert!(!s.contains("<WiFi.h>"), "{id}: WiFi is not universal");
        }
    }

    #[test]
    fn the_analog_template_guards_its_pin() {
        // Same reasoning as blink's LED_BUILTIN guard: A0 is not universal,
        // and a starter must not fail to compile on a board that lacks it.
        let s = sketch_from_template("analog-plot", "AnalogNode").unwrap();
        assert!(s.contains("#ifndef ANALOG_PIN"));
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
        let err = write_main_ino(tmp.path(), "X", "nope")
            .unwrap_err()
            .to_string();
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

        assert_eq!(
            read(made.dir.join("sketch.yaml")),
            yaml,
            "must stay verbatim"
        );
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
        assert_eq!(
            read(dir.join(".git")),
            gitfile,
            "the .git file is untouched"
        );
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
        assert!(
            !parent.join("New").exists(),
            "the project must not have moved"
        );
        assert!(dir.join("Old.ino").is_file(), "the main ino must be back");
        assert!(
            !dir.join("New.ino").exists(),
            "the renamed ino must be gone"
        );
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

    // ---------- project kind ----------

    #[test]
    fn a_top_level_cmakelists_declares_a_project() {
        assert!(declares_cmake_project(
            "cmake_minimum_required(VERSION 3.16)\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\nproject(hello_world)\n"
        ));
    }

    #[test]
    fn cmake_command_names_are_case_insensitive() {
        assert!(declares_cmake_project("PROJECT (hello)\n"));
        assert!(declares_cmake_project("   Project(hello)\n"));
    }

    #[test]
    fn a_commented_out_project_line_does_not_count() {
        assert!(!declares_cmake_project("# project(hello)\n"));
    }

    #[test]
    fn a_component_cmakelists_is_not_a_project() {
        // Every ESP-IDF component has one of these; none of them is a project.
        assert!(!declares_cmake_project(
            "idf_component_register(SRCS \"a.c\"\n                       INCLUDE_DIRS \".\")\n"
        ));
    }

    #[test]
    fn a_word_merely_containing_project_is_not_a_match() {
        assert!(!declares_cmake_project("add_subdirectory(project)\n"));
        assert!(!declares_cmake_project("project_extras(foo)\n"));
    }

    #[test]
    fn esp_idf_wins_when_a_directory_looks_like_both() {
        // Deliberate: only idf.py can build such a tree, so it is the reading
        // that can succeed. Pinned so the choice stays explicit.
        assert_eq!(classify(true, true, true), ProjectKind::Idf);
        assert_eq!(classify(true, false, true), ProjectKind::Idf);
    }

    #[test]
    fn a_plain_sketch_is_arduino_and_an_empty_folder_is_unknown() {
        assert_eq!(classify(true, false, false), ProjectKind::Arduino);
        assert_eq!(classify(false, true, false), ProjectKind::Arduino);
        assert_eq!(classify(false, false, false), ProjectKind::Unknown);
    }
}
