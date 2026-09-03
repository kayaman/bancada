//! Creating an ESP-IDF project.
//!
//! The counterpart to [`crate::project`]'s Arduino half, and deliberately not
//! merged into it: the two paradigms share a *name* and nothing else. An
//! Arduino project is a folder whose `.ino` basename must equal the folder's,
//! with a `sketch.yaml` profile pinning an FQBN. An ESP-IDF project is a
//! CMake project whose name lives inside `project(...)`, with a `main`
//! component beside it. Different files, different rules, different failure
//! modes — [`crate::project::classify`] is the one place that has to know both.
//!
//! ## Hermetic on purpose
//!
//! Nothing here runs `idf.py`. `idf.py create-project` exists and would work,
//! but shelling out to it would mean **you could not create an ESP-IDF project
//! without ESP-IDF installed** — and a first project is exactly when a user is
//! least likely to have a working install. The tree is four small files and a
//! directory; writing them is honest, instant, and testable without a
//! toolchain. Building still needs the real thing, and says so.
//!
//! This mirrors how the rest of `core` treats its engines: `cli.rs` and
//! `idf.rs` build argv, they do not reimplement compilers. Scaffolding is not
//! compiling.
//!
//! Vendored from the `bancada-idf` sibling, adapted to this crate's error type
//! and to [`crate::boardprofile`] — which is the same table the sibling's own
//! `boards.rs` became when it was absorbed.

use std::path::Path;

use crate::boardprofile::{Board, Led, LedKind};
use crate::{Error, Result};

pub const CMAKELISTS: &str = "CMakeLists.txt";
pub const MAIN_DIR: &str = "main";
pub const SDKCONFIG_DEFAULTS: &str = "sdkconfig.defaults";

const TMPL_CMAKELISTS: &str = include_str!("templates/idf/CMakeLists.txt.tmpl");
const TMPL_MAIN_CMAKELISTS: &str = include_str!("templates/idf/main-CMakeLists.txt.tmpl");
const TMPL_GITIGNORE: &str = include_str!("templates/idf/gitignore.tmpl");

/// A starter an ESP-IDF project can begin life as.
///
/// All four compile unchanged on every ESP-IDF target and need no wiring, so
/// "create a project and flash it" is a complete loop on any devkit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdfTemplate {
    /// Chip identity, features, flash size, and a repeating free-heap line.
    ///
    /// The default: it asks nothing of the hardware and tells you what you are
    /// holding, which is the most useful thing a first flash can do.
    #[default]
    Hello,
    /// Toggle a GPIO on a timer.
    Blink,
    /// Producer and consumer tasks over a FreeRTOS queue.
    Tasks,
    /// A boot counter in NVS that survives reset.
    Nvs,
}

/// One starter, as the UI lists it. The same shape as
/// [`crate::project::SketchTemplate`] so the two pickers render alike.
#[derive(Clone, Copy, serde::Serialize)]
pub struct IdfTemplateInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

impl IdfTemplate {
    pub const ALL: &'static [IdfTemplate] = &[
        IdfTemplate::Hello,
        IdfTemplate::Blink,
        IdfTemplate::Tasks,
        IdfTemplate::Nvs,
    ];

    pub fn id(self) -> &'static str {
        match self {
            IdfTemplate::Hello => "hello",
            IdfTemplate::Blink => "blink",
            IdfTemplate::Tasks => "tasks",
            IdfTemplate::Nvs => "nvs",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            IdfTemplate::Hello => "Hello",
            IdfTemplate::Blink => "Blink",
            IdfTemplate::Tasks => "Tasks",
            IdfTemplate::Nvs => "NVS",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            IdfTemplate::Hello => {
                "Chip model, features, flash size and a repeating free-heap line. Asks nothing of the hardware."
            }
            IdfTemplate::Blink => "Toggle a GPIO on a timer — takes the board's LED pin when one is known.",
            IdfTemplate::Tasks => "Producer and consumer FreeRTOS tasks over a queue.",
            IdfTemplate::Nvs => "A boot counter in NVS that survives reset — proves flash persistence.",
        }
    }

    pub fn from_id(id: &str) -> Option<IdfTemplate> {
        let id = id.trim().to_ascii_lowercase();
        IdfTemplate::ALL.iter().copied().find(|t| t.id() == id)
    }

    fn source(self) -> &'static str {
        match self {
            IdfTemplate::Hello => include_str!("templates/idf/app/hello.c.tmpl"),
            IdfTemplate::Blink => include_str!("templates/idf/app/blink.c.tmpl"),
            IdfTemplate::Tasks => include_str!("templates/idf/app/tasks.c.tmpl"),
            IdfTemplate::Nvs => include_str!("templates/idf/app/nvs.c.tmpl"),
        }
    }
}

/// Every starter, in the order the UI presents them.
pub fn idf_templates() -> Vec<IdfTemplateInfo> {
    IdfTemplate::ALL
        .iter()
        .map(|t| IdfTemplateInfo {
            id: t.id(),
            label: t.label(),
            description: t.description(),
        })
        .collect()
}

/// Maximum project-name length. CMake has no limit, but the name becomes the
/// ELF and BIN filename and an NVS namespace in one of the templates, and NVS
/// keys cap at 15 characters — so a long name is a footgun long before it is
/// a CMake problem.
pub const MAX_NAME_LEN: usize = 64;

/// Validate an ESP-IDF project name.
///
/// The name goes into `project(<name>)`, becomes `<name>.elf` / `<name>.bin`,
/// and is used as a C identifier prefix by the templates. So: ASCII letters,
/// digits, `_` and `-`, not starting with a digit or `-`.
///
/// **Stricter than [`crate::project::validate_project_name`]**, which allows a
/// leading digit and a `.` because Arduino's sketch rules do. A name legal
/// there can be illegal here, which is why the wizard validates against the
/// paradigm the user actually picked rather than against one shared rule.
///
/// Rejecting rather than sanitising is deliberate — silently turning
/// `my project` into `my_project` means the directory a user looks for is not
/// the one that was created.
pub fn validate_idf_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other("project name must not be empty".into()));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(Error::Other(format!(
            "project name must be {MAX_NAME_LEN} characters or fewer (got {})",
            name.chars().count()
        )));
    }
    let first = name.chars().next().expect("non-empty");
    if first.is_ascii_digit() || first == '-' {
        return Err(Error::Other(
            "project name must not start with a digit or `-` — it becomes a CMake target and a C identifier prefix".into(),
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-'))
    {
        let hint = if bad == ' ' {
            " — use `_` or `-` instead of spaces"
        } else {
            ""
        };
        return Err(Error::Other(format!(
            "project name must not contain `{bad}`{hint} (letters, digits, `_` and `-` only)"
        )));
    }
    Ok(name.to_string())
}

/// A finished scaffold.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IdfScaffold {
    pub dir: String,
    pub name: String,
    /// The chip written into `sdkconfig.defaults`, if one was chosen.
    pub target: Option<String>,
    /// The board recorded in `sdkconfig.defaults`, if one was chosen.
    pub board: Option<String>,
    /// Paths written, relative to `dir`, in creation order.
    pub files: Vec<String>,
}

/// Create a new ESP-IDF project under `parent`.
///
/// Writes into a **staging directory** and renames it into place at the end,
/// so a failure part-way through leaves nothing behind rather than a
/// half-project that [`crate::project::detect_kind`] would happily accept as
/// an ESP-IDF project. The staging directory is a dot-prefixed sibling, which
/// keeps the final rename on the same filesystem and therefore atomic.
///
/// `target` is written as `CONFIG_IDF_TARGET` in `sdkconfig.defaults` rather
/// than applied with `idf.py set-target`. Two reasons: set-target needs a
/// working ESP-IDF install, which creation deliberately does not; and the
/// defaults file is exactly the mechanism ESP-IDF provides for stating a
/// target before the first configure. The first build picks it up.
pub fn scaffold_idf_project(
    parent: &Path,
    name: &str,
    template: IdfTemplate,
    target: Option<&str>,
    board: Option<&Board>,
) -> Result<IdfScaffold> {
    let name = validate_idf_name(name)?;
    let dest = parent.join(&name);
    // symlink_metadata, not exists(): a broken symlink is still something we
    // must not write through. Same rule as `create_project`.
    if dest.symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "{} already exists — choose another name or location",
            dest.display()
        )));
    }
    std::fs::create_dir_all(parent)?;

    let staging = parent.join(format!(".{name}.staging"));
    if staging.symlink_metadata().is_ok() {
        std::fs::remove_dir_all(&staging)?;
    }
    match write_tree(&staging, &name, template, target, board) {
        Ok(files) => {
            std::fs::rename(&staging, &dest)?;
            Ok(IdfScaffold {
                dir: dest.to_string_lossy().into_owned(),
                name,
                target: target.map(str::to_string),
                board: board.map(|b| b.id.to_string()),
                files,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

fn write_tree(
    root: &Path,
    name: &str,
    template: IdfTemplate,
    target: Option<&str>,
    board: Option<&Board>,
) -> Result<Vec<String>> {
    std::fs::create_dir_all(root.join(MAIN_DIR))?;

    let mut files = vec![
        (CMAKELISTS.to_string(), render(TMPL_CMAKELISTS, name)),
        (
            format!("{MAIN_DIR}/{CMAKELISTS}"),
            render(TMPL_MAIN_CMAKELISTS, name),
        ),
        (
            format!("{MAIN_DIR}/{name}.c"),
            render_app(template.source(), name, board),
        ),
        (".gitignore".to_string(), TMPL_GITIGNORE.to_string()),
    ];

    // One file carries both facts, because ESP-IDF reads both from it. The
    // board goes in as a comment (see `boardprofile`'s marker docs — a real
    // CONFIG_ key would make kconfgen nag on every reconfigure); the target
    // is a genuine config key and goes in as one.
    let defaults = match (target, board) {
        (None, None) => None,
        (t, b) => {
            let mut text = String::new();
            if let Some(t) = t {
                text.push_str(&format!("CONFIG_IDF_TARGET=\"{t}\"\n"));
            }
            if let Some(b) = b {
                text = crate::boardprofile::set_marker(&text, b.id);
            }
            Some(text)
        }
    };
    if let Some(text) = defaults {
        files.push((SDKCONFIG_DEFAULTS.to_string(), text));
    }

    for (rel, body) in &files {
        std::fs::write(root.join(rel), body)?;
    }
    Ok(files.iter().map(|(rel, _)| rel.clone()).collect())
}

/// Substitute the one placeholder the structural templates use.
fn render(template: &str, name: &str) -> String {
    template.replace("{{PROJECT_NAME}}", name)
}

/// What the blink template should toggle, and the comment explaining why.
///
/// With no board there is nothing to know: GPIO2 (the classic devkit LED) and
/// the honest note. With a plain LED, its pin. With an addressable LED, still
/// its pin — so the toggle and the log line are real — plus a warning that a
/// plain toggle will not light it, and where to go for that.
///
/// Note the difference from the Arduino side, which swaps in a whole different
/// sketch for a WS2812 board. It can: the Arduino core ships `rgbLedWrite()`.
/// ESP-IDF's equivalent is the `led_strip` component, which is a *dependency*
/// the component manager must fetch — and a starter project that cannot build
/// until a registry download succeeds is a worse first experience than one
/// that builds instantly and explains itself.
pub fn blink_defaults(board: Option<&Board>) -> (u8, String) {
    match board.and_then(|b| b.led.map(|l| (b, l))) {
        None => (
            2,
            " * Set BLINK_GPIO to whichever pin your board's LED is on — it differs per\n \
             * devkit, and there is no portable way to know it. With no LED attached the\n \
             * log line still proves the loop is running."
                .to_string(),
        ),
        Some((
            b,
            Led {
                gpio,
                kind: LedKind::Plain,
            },
        )) => (
            gpio,
            format!(
                " * BLINK_GPIO is the onboard LED of the {} (GPIO{gpio}), from the board\n \
                 * profile chosen when this project was created.",
                b.name
            ),
        ),
        Some((
            b,
            Led {
                gpio,
                kind: LedKind::Ws2812,
            },
        )) => (
            gpio,
            format!(
                " * The {}'s onboard LED is an addressable WS2812 on GPIO{gpio}: a plain\n \
                 * toggle will not light it. BLINK_GPIO still points there so the toggle and\n \
                 * the log line are real; wire a plain LED to another pin, or pull in the\n \
                 * led_strip component to drive the RGB LED.",
                b.name
            ),
        ),
    }
}

/// Render an application template: the project name, plus the blink
/// placeholders (harmless on templates that do not carry them).
fn render_app(template: &str, name: &str, board: Option<&Board>) -> String {
    let (gpio, note) = blink_defaults(board);
    render(template, name)
        .replace("{{BLINK_GPIO}}", &gpio.to_string())
        .replace("{{BLINK_NOTE}}", &note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(id: &str) -> &'static Board {
        Board::from_id(id).expect(id)
    }

    #[test]
    fn accepts_reasonable_names() {
        for n in ["blink", "my_app", "sensor-node", "_internal", "app2"] {
            validate_idf_name(n).unwrap_or_else(|e| panic!("{n} rejected: {e}"));
        }
    }

    #[test]
    fn rejects_names_that_would_break_cmake_or_the_filesystem() {
        for n in ["", "my project", "2fast", "-leading", "a/b", "app!", "caf\u{e9}"] {
            assert!(validate_idf_name(n).is_err(), "{n:?} should be rejected");
        }
    }

    #[test]
    fn is_stricter_than_the_arduino_rule_where_the_paradigms_disagree() {
        // Both legal as Arduino sketch folders, neither legal as a CMake
        // project name. The wizard must validate against the chosen paradigm.
        for n in ["2fast", "my.app"] {
            assert!(crate::project::validate_project_name(n).is_ok(), "{n}");
            assert!(validate_idf_name(n).is_err(), "{n}");
        }
    }

    #[test]
    fn scaffolds_a_tree_idf_would_recognise() {
        let tmp = tempfile::tempdir().unwrap();
        let s = scaffold_idf_project(tmp.path(), "demo", IdfTemplate::Hello, None, None).unwrap();
        let dir = std::path::Path::new(&s.dir);

        let cmake = std::fs::read_to_string(dir.join(CMAKELISTS)).unwrap();
        assert!(crate::project::declares_cmake_project(&cmake), "{cmake}");
        assert!(cmake.contains("project(demo)"), "{cmake}");
        assert!(dir.join("main/CMakeLists.txt").is_file());
        assert!(dir.join("main/demo.c").is_file());
        assert!(dir.join(".gitignore").is_file());

        // And the classifier agrees, which is the claim that matters: the
        // toolbar, the Verify button and the backend all key off this.
        assert_eq!(
            crate::project::detect_kind(dir),
            crate::project::ProjectKind::Idf
        );
    }

    #[test]
    fn no_placeholder_survives_into_a_rendered_project() {
        let tmp = tempfile::tempdir().unwrap();
        for t in IdfTemplate::ALL {
            let name = format!("p_{}", t.id());
            let s = scaffold_idf_project(tmp.path(), &name, *t, None, None).unwrap();
            for rel in &s.files {
                let body = std::fs::read_to_string(std::path::Path::new(&s.dir).join(rel)).unwrap();
                assert!(!body.contains("{{"), "{rel} of {} kept a placeholder:\n{body}", t.id());
            }
        }
    }

    #[test]
    fn a_target_is_written_as_a_real_config_key() {
        let tmp = tempfile::tempdir().unwrap();
        let s =
            scaffold_idf_project(tmp.path(), "demo", IdfTemplate::Hello, Some("esp32c6"), None)
                .unwrap();
        let text =
            std::fs::read_to_string(std::path::Path::new(&s.dir).join(SDKCONFIG_DEFAULTS)).unwrap();
        assert!(text.contains(r#"CONFIG_IDF_TARGET="esp32c6""#), "{text}");
        // And it round-trips through the same parser the app reads it with.
        assert_eq!(
            crate::project::idf_target_of(std::path::Path::new(&s.dir)).as_deref(),
            Some("esp32c6")
        );
    }

    #[test]
    fn a_board_is_written_as_a_comment_marker_beside_the_target() {
        let tmp = tempfile::tempdir().unwrap();
        let s = scaffold_idf_project(
            tmp.path(),
            "demo",
            IdfTemplate::Blink,
            Some("esp32c6"),
            Some(board("esp32-c6-devkitc-1")),
        )
        .unwrap();
        let dir = std::path::Path::new(&s.dir);

        // Both facts in one file, and both readable by the app's own readers.
        assert_eq!(
            crate::project::recorded_board(dir).map(|b| b.id),
            Some("esp32-c6-devkitc-1")
        );
        assert_eq!(crate::project::idf_target_of(dir).as_deref(), Some("esp32c6"));
    }

    #[test]
    fn blink_takes_the_boards_led_pin() {
        let tmp = tempfile::tempdir().unwrap();
        let c6 = board("esp32-c6-devkitc-1");
        let s = scaffold_idf_project(tmp.path(), "demo", IdfTemplate::Blink, None, Some(c6))
            .unwrap();
        let src =
            std::fs::read_to_string(std::path::Path::new(&s.dir).join("main/demo.c")).unwrap();
        assert!(
            src.contains(&c6.led.unwrap().gpio.to_string()),
            "expected GPIO{} in:\n{src}",
            c6.led.unwrap().gpio
        );
        // A WS2812 board must say so rather than leaving a dark board unexplained.
        assert!(src.contains("WS2812"), "{src}");
    }

    #[test]
    fn a_failed_write_leaves_nothing_behind() {
        // The staging directory is the whole point: a half-written tree that
        // `detect_kind` accepts is worse than no tree at all.
        let tmp = tempfile::tempdir().unwrap();
        // A name that validates but whose destination is occupied by a file.
        std::fs::write(tmp.path().join("taken"), "not a directory").unwrap();
        let err = scaffold_idf_project(tmp.path(), "taken", IdfTemplate::Hello, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert!(!tmp.path().join(".taken.staging").exists());
    }

    #[test]
    fn every_template_id_round_trips() {
        for t in IdfTemplate::ALL {
            assert_eq!(IdfTemplate::from_id(t.id()), Some(*t));
        }
        assert_eq!(IdfTemplate::from_id("HELLO"), Some(IdfTemplate::Hello));
        assert_eq!(IdfTemplate::from_id("nope"), None);
        assert_eq!(idf_templates().len(), IdfTemplate::ALL.len());
    }
}
