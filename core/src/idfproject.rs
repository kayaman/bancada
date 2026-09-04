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

/// Rewrite `project(<name>)` in a `CMakeLists.txt`'s text.
///
/// Surgical, like the `sdkconfig` marker: only the name inside the call
/// changes, so a rename produces a one-line diff and every comment the user
/// added survives.
fn rewrite_project_name(text: &str, new_name: &str) -> Option<String> {
    let start = text.find("project(")?;
    let open = start + "project(".len();
    let close = text[open..].find(')')? + open;
    Some(format!("{}{}{}", &text[..open], new_name, &text[close..]))
}

/// Rename an ESP-IDF project: its directory and its CMake name, together.
///
/// They are one identity, the same way an Arduino sketch's folder and its
/// main `.ino` basename are — the build names `<name>.elf` from `project()`,
/// so moving only the directory leaves a project whose output is named after
/// its old self.
///
/// **The rewrite happens before the move**, which is the same discipline
/// [`crate::project::rename_project`] follows for the opposite reason: there,
/// every in-directory edit precedes the one irreversible step. Here there are
/// only two steps, and putting the fallible one first means a failure leaves
/// the project exactly where it was.
///
/// The `main/<name>.c` source is deliberately **not** renamed. Unlike Arduino,
/// where `arduino-cli` only recognises `Foo/Foo.ino` as a sketch, ESP-IDF
/// names its sources in `main/CMakeLists.txt` and does not care what they are
/// called — renaming one would mean editing that file for no gain.
pub fn rename_idf_project(dir: &Path, new_name: &str) -> Result<crate::project::RenamedProject> {
    let name = validate_idf_name(new_name)?;
    let parent = dir
        .parent()
        .ok_or_else(|| Error::Other("the project has no parent directory".into()))?;
    let dest = parent.join(&name);
    if dest.symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "{} already exists — choose another name",
            dest.display()
        )));
    }

    let cmake = dir.join(CMAKELISTS);
    let text = std::fs::read_to_string(&cmake)?;
    let rewritten = rewrite_project_name(&text, &name).ok_or_else(|| {
        Error::Other(format!(
            "{} has no project(<name>) call — it is not an ESP-IDF project",
            cmake.display()
        ))
    })?;
    std::fs::write(&cmake, rewritten)?;

    if let Err(e) = std::fs::rename(dir, &dest) {
        // Put the name back rather than leaving a project that claims to be
        // something the directory is not.
        let _ = std::fs::write(&cmake, &text);
        return Err(e.into());
    }

    Ok(crate::project::RenamedProject {
        dir: dest,
        name,
        warnings: Vec::new(),
    })
}

/// Copy an ESP-IDF project to `dest_parent/new_name`.
///
/// `build/` and the generated `sdkconfig` are deliberately not copied: both
/// are CMake output naming the *source* project inside, so carrying them
/// across gives the duplicate stale artefacts under the wrong name.
/// `sdkconfig.defaults` **is** copied — it is hand-written, committed, and
/// carries the target and the board marker, which is exactly what should
/// survive. `.git` is skipped for the same reason the Arduino clone skips it:
/// a copy gets a fresh repository, never the original's history.
pub fn duplicate_idf_project(
    src_dir: &Path,
    dest_parent: &Path,
    new_name: &str,
) -> Result<crate::project::RenamedProject> {
    let name = validate_idf_name(new_name)?;
    let dest = dest_parent.join(&name);
    if dest.symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "{} already exists — choose another name or location",
            dest.display()
        )));
    }
    // Read the source's CMakeLists before copying, so a project that is not
    // one fails before anything is written.
    let text = std::fs::read_to_string(src_dir.join(CMAKELISTS))?;
    let rewritten = rewrite_project_name(&text, &name).ok_or_else(|| {
        Error::Other(format!(
            "{} has no project(<name>) call — it is not an ESP-IDF project",
            src_dir.join(CMAKELISTS).display()
        ))
    })?;

    std::fs::create_dir_all(dest_parent)?;
    if let Err(e) = copy_tree(src_dir, &dest) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    std::fs::write(dest.join(CMAKELISTS), rewritten)?;

    Ok(crate::project::RenamedProject {
        dir: dest,
        name,
        warnings: Vec::new(),
    })
}

/// Entries never worth copying: CMake's output directory, the generated
/// `sdkconfig` and its backup, and git's own directory.
const NOT_COPIED: &[&str] = &["build", "sdkconfig", "sdkconfig.old", ".git", "managed_components"];

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if NOT_COPIED.contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
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

    /// A scaffolded project to operate on.
    fn made(parent: &std::path::Path, name: &str) -> std::path::PathBuf {
        let s = scaffold_idf_project(parent, name, IdfTemplate::Hello, Some("esp32c3"), None)
            .expect("scaffold");
        std::path::PathBuf::from(s.dir)
    }

    #[test]
    fn renaming_moves_the_directory_and_the_cmake_name_together() {
        // They are one identity: the build names <name>.elf from project(),
        // so moving only the directory leaves a project whose output is named
        // after its old self.
        let tmp = tempfile::tempdir().unwrap();
        let dir = made(tmp.path(), "before");

        let r = rename_idf_project(&dir, "after").expect("rename");

        assert_eq!(r.name, "after");
        assert!(!dir.exists(), "the old directory survived");
        assert!(r.dir.join("main/before.c").is_file(), "sources are not renamed");
        let cmake = std::fs::read_to_string(r.dir.join(CMAKELISTS)).unwrap();
        assert!(cmake.contains("project(after)"), "{cmake}");
        assert!(!cmake.contains("project(before)"), "{cmake}");
    }

    #[test]
    fn a_failed_rename_moves_nothing() {
        // The CMake rewrite happens first precisely so that a failure leaves
        // the project where it was rather than moved and mis-named.
        let tmp = tempfile::tempdir().unwrap();
        let dir = made(tmp.path(), "keep");
        std::fs::write(dir.join(CMAKELISTS), "# no project call here\n").unwrap();

        let err = rename_idf_project(&dir, "moved").unwrap_err();
        assert!(err.to_string().contains("project("), "{err}");
        assert!(dir.exists(), "the project moved despite failing");
        assert!(!tmp.path().join("moved").exists());
    }

    #[test]
    fn renaming_refuses_an_occupied_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = made(tmp.path(), "one");
        made(tmp.path(), "two");

        let err = rename_idf_project(&dir, "two").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        // And the source is untouched — including its CMake name.
        let cmake = std::fs::read_to_string(dir.join(CMAKELISTS)).unwrap();
        assert!(cmake.contains("project(one)"), "{cmake}");
    }

    #[test]
    fn renaming_validates_against_the_cmake_rule() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = made(tmp.path(), "fine");
        assert!(rename_idf_project(&dir, "2fast").is_err());
        assert!(dir.exists());
    }

    #[test]
    fn duplicating_copies_the_sources_but_not_the_build_output() {
        // build/ and the generated sdkconfig name the SOURCE project inside
        // them; carrying them across gives the copy stale artefacts under the
        // wrong name. sdkconfig.defaults is hand-written and must survive.
        let tmp = tempfile::tempdir().unwrap();
        let src = made(tmp.path(), "orig");
        std::fs::create_dir_all(src.join("build")).unwrap();
        std::fs::write(src.join("build/orig.elf"), "stale").unwrap();
        std::fs::write(src.join("sdkconfig"), "CONFIG_IDF_TARGET=\"esp32c3\"\n").unwrap();

        let d = duplicate_idf_project(&src, tmp.path(), "copy").expect("duplicate");

        assert!(d.dir.join("main/orig.c").is_file());
        assert!(d.dir.join(SDKCONFIG_DEFAULTS).is_file(), "defaults must survive");
        assert!(!d.dir.join("build").exists(), "build/ was copied");
        assert!(!d.dir.join("sdkconfig").exists(), "generated sdkconfig was copied");

        let cmake = std::fs::read_to_string(d.dir.join(CMAKELISTS)).unwrap();
        assert!(cmake.contains("project(copy)"), "{cmake}");
        // The source is untouched.
        let orig = std::fs::read_to_string(src.join(CMAKELISTS)).unwrap();
        assert!(orig.contains("project(orig)"), "{orig}");
    }

    #[test]
    fn duplicating_never_carries_the_sources_history() {
        // Same rule as the Arduino clone: a copy gets a fresh repository, not
        // the original's commits.
        let tmp = tempfile::tempdir().unwrap();
        let src = made(tmp.path(), "orig");
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        let d = duplicate_idf_project(&src, tmp.path(), "copy").expect("duplicate");
        assert!(!d.dir.join(".git").exists());
    }

    #[test]
    fn a_rewritten_project_call_changes_only_the_name() {
        // Surgical, like the sdkconfig marker: a rename should be a one-line
        // diff with every comment intact.
        let text = "# a comment\ncmake_minimum_required(VERSION 3.16)\nproject(old)\n# trailing\n";
        let out = rewrite_project_name(text, "new").unwrap();
        assert_eq!(
            out,
            "# a comment\ncmake_minimum_required(VERSION 3.16)\nproject(new)\n# trailing\n"
        );
        assert!(rewrite_project_name("no call at all\n", "new").is_none());
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
