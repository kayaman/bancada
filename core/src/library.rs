//! Scaffolding for brand-new Arduino libraries (library format 1.5 rev 2.2).
//!
//! Pure: no subprocesses, no Tauri. The caller resolves the sketchbook's
//! `libraries/` directory and passes it in, which keeps every test here on a
//! `tempfile::tempdir()`.
//!
//! The generated skeleton is deliberately *complete* rather than minimal —
//! `library.properties` always carries every key the specification requires,
//! and the bundled example compiles against the stub class as-is, so a fresh
//! library can be verified immediately.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// The categories permitted by the library specification.
///
/// `Uncategorized` is deliberately absent: it is the value the tooling
/// *substitutes* when `category` is missing or unrecognised, not one a library
/// is allowed to declare. Writing it earns an arduino-lint LP038 finding.
pub const CATEGORIES: [&str; 9] = [
    "Display",
    "Communication",
    "Signal Input/Output",
    "Sensors",
    "Device Control",
    "Timing",
    "Data Storage",
    "Data Processing",
    "Other",
];

/// arduino-lint LP009: hard limit on the `name` field.
const MAX_NAME_LEN: usize = 63;
/// arduino-lint LP010: longer names are truncated in Library Manager listings.
const RECOMMENDED_NAME_LEN: usize = 16;

/// What the user typed. Blank optional fields are filled in by
/// [`create_library`]; no required `library.properties` key is ever omitted.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibrarySpec {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub maintainer: String,
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub paragraph: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub architectures: String,
}

/// The names derived from one user-supplied library name.
///
/// They differ because the three namespaces have different rules: the
/// `name` field allows spaces, a folder name does not, and a C++ identifier
/// allows neither spaces nor `.`/`-`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryIdent {
    /// `library.properties` `name=` — e.g. `My Sensor`
    pub display_name: String,
    /// Root folder and primary header basename — e.g. `My_Sensor`
    pub folder: String,
    /// C++ class name — e.g. `My_Sensor`
    pub class_name: String,
    /// Include guard — e.g. `MY_SENSOR_H`
    pub header_guard: String,
}

/// A library that was written to disk.
#[derive(Debug, Clone, Serialize)]
pub struct NewLibrary {
    pub dir: PathBuf,
    /// Paths relative to `dir`, in creation order.
    pub files: Vec<String>,
    /// Non-fatal specification advisories.
    pub warnings: Vec<String>,
}

const TMPL_PROPERTIES: &str = include_str!("templates/library/library.properties.tmpl");
const TMPL_HEADER: &str = include_str!("templates/library/header.h.tmpl");
const TMPL_SOURCE: &str = include_str!("templates/library/source.cpp.tmpl");
const TMPL_KEYWORDS: &str = include_str!("templates/library/keywords.txt.tmpl");
const TMPL_EXAMPLE: &str = include_str!("templates/library/example.ino.tmpl");

// ---------- validation ----------

fn is_allowed_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == ' ' || c == '_' || c == '.' || c == '-'
}

/// Validate a library name and derive every dependent identifier.
///
/// The returned warnings are advisory — they describe specification
/// recommendations the name misses, not reasons to refuse it.
pub fn validate_name(raw: &str) -> Result<(LibraryIdent, Vec<String>)> {
    // Collapse any whitespace run to a single space; this also trims.
    let display: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");

    if display.is_empty() {
        return Err(Error::Other("library name must not be empty".into()));
    }
    if display.chars().count() > MAX_NAME_LEN {
        return Err(Error::Other(format!(
            "library name must be {MAX_NAME_LEN} characters or fewer (got {})",
            display.chars().count()
        )));
    }
    if let Some(bad) = display.chars().find(|c| !is_allowed_name_char(*c)) {
        return Err(Error::Other(format!(
            "library name may only contain letters, digits, spaces, '_', '.' and '-' — found '{bad}'"
        )));
    }
    // The specification permits a leading digit, but the name becomes a C++
    // class name and `class 3Phase {` does not compile. Requiring a leading
    // letter also subsumes the "must contain a letter" rule.
    let first = display.chars().next().unwrap_or('\0');
    if !first.is_ascii_alphabetic() {
        return Err(Error::Other(format!(
            "library name must start with a letter (it becomes the C++ class name), found '{first}'"
        )));
    }

    let folder = display.replace(' ', "_").trim_end_matches('.').to_string();
    let class_name = to_identifier(&folder);
    let header_guard = format!("{}_H", class_name.to_ascii_uppercase());

    let mut warnings = Vec::new();
    if display.contains(' ') {
        warnings.push(format!(
            "name contains spaces — the folder will be `{folder}` and the header `{folder}.h` (arduino-lint LP015)"
        ));
    }
    if display.chars().count() > RECOMMENDED_NAME_LEN {
        warnings.push(format!(
            "names longer than {RECOMMENDED_NAME_LEN} characters are truncated in Library Manager listings (LP010)"
        ));
    }
    if display.to_ascii_lowercase().starts_with("arduino") {
        warnings.push(
            "names starting with \"Arduino\" are reserved for official libraries and are rejected by the Library Manager index (LP012)"
                .into(),
        );
    }

    Ok((
        LibraryIdent {
            display_name: display,
            folder,
            class_name,
            header_guard,
        },
        warnings,
    ))
}

/// Map a folder name to a legal C++ identifier: anything outside
/// `[A-Za-z0-9_]` becomes `_`, and runs of `_` collapse to one.
///
/// Collapsing matters: `__` anywhere and a leading `_` followed by an
/// uppercase letter are both reserved to the implementation in C++.
fn to_identifier(folder: &str) -> String {
    let mut out = String::with_capacity(folder.len());
    for c in folder.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

/// Validate a `category` literal. Blank is accepted and defaulted later.
pub fn validate_category(raw: &str) -> Result<()> {
    let c = raw.trim();
    if c.is_empty() || CATEGORIES.contains(&c) {
        return Ok(());
    }
    Err(Error::Other(format!(
        "category \"{c}\" is not one of the Arduino library specification categories: {}",
        CATEGORIES.join(", ")
    )))
}

/// Validate a `version`. Blank is accepted and defaulted later. Lenient about
/// the exact scheme, strict about characters that would break the file.
pub fn validate_version(raw: &str) -> Result<()> {
    let v = raw.trim();
    if v.is_empty() {
        return Ok(());
    }
    let shaped = v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '+' || c == '-');
    if shaped && v.chars().any(|c| c.is_ascii_digit()) {
        return Ok(());
    }
    Err(Error::Other(
        "version must be a semver-style release number like 0.1.0".into(),
    ))
}

// ---------- templating ----------

/// Replace `{{TOKEN}}` in a single left-to-right pass.
///
/// Single-pass matters: a chain of `str::replace` calls would re-scan text that
/// an earlier replacement inserted, so a user who types `{{CLASS}}` into the
/// summary field would see it expanded. Unknown tokens are left verbatim.
fn render(tmpl: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(tmpl.len() + 256);
    let mut rest = tmpl;
    while let Some(start) = rest.find("{{") {
        let (before, from_marker) = rest.split_at(start);
        out.push_str(before);
        let after_marker = &from_marker[2..];
        match after_marker.find("}}") {
            Some(end) => {
                let token = &after_marker[..end];
                match vars.iter().find(|(k, _)| *k == token) {
                    Some((_, value)) => out.push_str(value),
                    // Unknown token: emit it unchanged rather than swallowing it.
                    None => {
                        out.push_str("{{");
                        out.push_str(token);
                        out.push_str("}}");
                    }
                }
                rest = &after_marker[end + 2..];
            }
            // Unterminated `{{` — nothing left to substitute.
            None => {
                out.push_str("{{");
                rest = after_marker;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `library.properties` is line-oriented, so a newline inside a value would
/// silently corrupt the record. Flatten and trim.
fn sanitize_value(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

// ---------- scaffolding ----------

/// Removes a half-built staging directory unless explicitly disarmed, so every
/// `?` between creation and the final rename is covered.
struct Staging(Option<PathBuf>);

impl Staging {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// Scaffold a complete library under `libraries_dir`.
///
/// All-or-nothing: the tree is built in a dot-prefixed staging directory (which
/// arduino-cli skips while scanning) and moved into place with a single rename,
/// so a failure part-way through leaves nothing behind.
pub fn create_library(libraries_dir: &Path, spec: &LibrarySpec) -> Result<NewLibrary> {
    let (ident, warnings) = validate_name(&spec.name)?;
    validate_category(&spec.category)?;
    validate_version(&spec.version)?;

    let dest = libraries_dir.join(&ident.folder);
    // symlink_metadata, not exists(): a sketchbook commonly holds symlinked
    // libraries, and exists() reports false for a dangling one while
    // create_dir_all still fails on it.
    if dest.symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "{} already exists — choose a different library name or remove that folder first",
            dest.display()
        )));
    }
    if let Some(clash) = case_insensitive_clash(libraries_dir, &ident.folder) {
        return Err(Error::Other(format!(
            "a library folder named `{clash}` already exists and differs only in case"
        )));
    }

    // A fresh arduino-cli install has a sketchbook with no libraries/ yet.
    std::fs::create_dir_all(libraries_dir)?;

    let staging = libraries_dir.join(format!(".bancada-new-{}", ident.folder));
    if staging.symlink_metadata().is_ok() {
        // Leftover from a killed run. remove_dir_all fails on a regular file,
        // which is the honest outcome — we will not delete an unrelated file.
        std::fs::remove_dir_all(&staging)?;
    }

    let mut guard = Staging(Some(staging.clone()));

    let version = non_empty(&spec.version, "0.1.0");
    let author = non_empty(&spec.author, "Unknown");
    let maintainer = non_empty(&spec.maintainer, &author);
    let sentence = non_empty(
        &spec.sentence,
        &format!("{} library.", ident.display_name),
    );
    let paragraph = non_empty(&spec.paragraph, &sentence);
    let category = non_empty(&spec.category, "Other");
    let architectures = non_empty(&spec.architectures, "*");
    let url = sanitize_value(&spec.url);

    let vars: Vec<(&str, &str)> = vec![
        ("NAME", &ident.display_name),
        ("FOLDER", &ident.folder),
        ("CLASS", &ident.class_name),
        ("GUARD", &ident.header_guard),
        ("VERSION", &version),
        ("AUTHOR", &author),
        ("MAINTAINER", &maintainer),
        ("SENTENCE", &sentence),
        ("PARAGRAPH", &paragraph),
        ("CATEGORY", &category),
        ("URL", &url),
        ("ARCH", &architectures),
    ];

    let header_rel = format!("src/{}.h", ident.folder);
    let source_rel = format!("src/{}.cpp", ident.folder);
    let example_rel = format!("examples/{0}/{0}.ino", ident.folder);

    std::fs::create_dir_all(staging.join("src"))?;
    std::fs::create_dir_all(staging.join("examples").join(&ident.folder))?;

    let files = [
        ("library.properties".to_string(), TMPL_PROPERTIES),
        (header_rel, TMPL_HEADER),
        (source_rel, TMPL_SOURCE),
        ("keywords.txt".to_string(), TMPL_KEYWORDS),
        (example_rel, TMPL_EXAMPLE),
    ];

    let mut written = Vec::with_capacity(files.len());
    for (rel, tmpl) in &files {
        std::fs::write(staging.join(rel), render(tmpl, &vars))?;
        written.push(rel.clone());
    }

    std::fs::rename(&staging, &dest)?;
    guard.disarm();

    Ok(NewLibrary {
        dir: dest,
        files: written,
        warnings,
    })
}

fn non_empty(value: &str, fallback: &str) -> String {
    let v = sanitize_value(value);
    if v.is_empty() {
        sanitize_value(fallback)
    } else {
        v
    }
}

/// On a case-insensitive filesystem two names differing only in case are the
/// same directory, and arduino-cli would see a duplicate library either way.
fn case_insensitive_clash(libraries_dir: &Path, folder: &str) -> Option<String> {
    let entries = std::fs::read_dir(libraries_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.eq_ignore_ascii_case(folder) {
            return Some(name.into_owned());
        }
    }
    None
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> LibrarySpec {
        LibrarySpec {
            name: name.to_string(),
            ..Default::default()
        }
    }

    // ----- name validation -----

    #[test]
    fn derives_identifiers_from_a_spaced_name() {
        let (id, warns) = validate_name("My Sensor").unwrap();
        assert_eq!(id.display_name, "My Sensor");
        assert_eq!(id.folder, "My_Sensor");
        assert_eq!(id.class_name, "My_Sensor");
        assert_eq!(id.header_guard, "MY_SENSOR_H");
        assert!(warns.iter().any(|w| w.contains("spaces")));
    }

    #[test]
    fn collapses_repeated_spaces() {
        let (id, _) = validate_name("  My   Sensor  ").unwrap();
        assert_eq!(id.display_name, "My Sensor");
        assert_eq!(id.folder, "My_Sensor");
    }

    #[test]
    fn punctuation_becomes_underscores_in_the_class() {
        let (id, _) = validate_name("My-Lib.v2").unwrap();
        assert_eq!(id.folder, "My-Lib.v2");
        assert_eq!(id.class_name, "My_Lib_v2");
        assert_eq!(id.header_guard, "MY_LIB_V2_H");
    }

    #[test]
    fn strips_trailing_dots_from_the_folder() {
        let (id, _) = validate_name("Weird.").unwrap();
        assert_eq!(id.folder, "Weird");
        assert_eq!(id.class_name, "Weird");
    }

    #[test]
    fn rejects_empty_name() {
        assert!(validate_name("   ").is_err());
    }

    #[test]
    fn rejects_name_over_63_chars() {
        let long = "A".repeat(64);
        let err = validate_name(&long).unwrap_err().to_string();
        assert!(err.contains("63"), "{err}");
    }

    #[test]
    fn rejects_non_ascii_and_punctuation() {
        for bad in ["Café", "My/Lib", "a+b", "Lib(2)"] {
            let err = validate_name(bad).unwrap_err().to_string();
            assert!(err.contains("may only contain"), "{bad}: {err}");
        }
    }

    #[test]
    fn rejects_leading_digit() {
        let err = validate_name("3Phase").unwrap_err().to_string();
        assert!(err.contains("C++ class name"), "{err}");
    }

    #[test]
    fn rejects_name_without_letters() {
        for bad in ["123", "1.0", "_x"] {
            assert!(validate_name(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn warns_on_arduino_prefix_but_succeeds() {
        let (_, warns) = validate_name("ArduinoThing").unwrap();
        assert!(warns.iter().any(|w| w.contains("LP012")), "{warns:?}");
    }

    #[test]
    fn warns_over_16_chars() {
        let (_, warns) = validate_name("AbcdefghijklmnopqrsT").unwrap();
        assert!(warns.iter().any(|w| w.contains("LP010")), "{warns:?}");
    }

    #[test]
    fn short_simple_name_has_no_warnings() {
        let (_, warns) = validate_name("Blink").unwrap();
        assert!(warns.is_empty(), "{warns:?}");
    }

    // ----- category / version -----

    #[test]
    fn categories_are_the_nine_spec_values() {
        // Pins the enumeration: "Uncategorized" is the tooling's fallback for a
        // missing/invalid value and must never appear here.
        assert_eq!(
            CATEGORIES,
            [
                "Display",
                "Communication",
                "Signal Input/Output",
                "Sensors",
                "Device Control",
                "Timing",
                "Data Storage",
                "Data Processing",
                "Other",
            ]
        );
        assert!(!CATEGORIES.contains(&"Uncategorized"));
    }

    #[test]
    fn rejects_invalid_category() {
        assert!(validate_category("Sensors").is_ok());
        assert!(validate_category("").is_ok());
        assert!(validate_category("Uncategorized").is_err());
        assert!(validate_category("sensors").is_err()); // case-sensitive per spec
    }

    #[test]
    fn blank_category_defaults_to_other() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        create_library(&libs, &spec("Foo")).unwrap();
        let props = std::fs::read_to_string(libs.join("Foo/library.properties")).unwrap();
        assert!(props.contains("category=Other"), "{props}");
    }

    #[test]
    fn validates_version_shape() {
        assert!(validate_version("").is_ok());
        assert!(validate_version("1.0.0").is_ok());
        assert!(validate_version("0.1.0-rc1").is_ok());
        assert!(validate_version("v-alpha").is_err()); // no digit
        assert!(validate_version("1.0 beta").is_err()); // space
    }

    // ----- templating -----

    #[test]
    fn render_is_single_pass() {
        // A value that itself looks like a token must not be re-expanded.
        let out = render(
            "[{{SENTENCE}}][{{CLASS}}]",
            &[("SENTENCE", "{{CLASS}}"), ("CLASS", "Real")],
        );
        assert_eq!(out, "[{{CLASS}}][Real]");
    }

    #[test]
    fn render_leaves_unknown_tokens_alone() {
        assert_eq!(render("a{{NOPE}}b", &[("X", "y")]), "a{{NOPE}}b");
        assert_eq!(render("a{{unterminated", &[]), "a{{unterminated");
    }

    #[test]
    fn sanitizes_newlines_in_property_values() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        let s = LibrarySpec {
            name: "Foo".into(),
            sentence: "first\nsecond".into(),
            ..Default::default()
        };
        create_library(&libs, &s).unwrap();
        let props = std::fs::read_to_string(libs.join("Foo/library.properties")).unwrap();
        assert!(props.contains("sentence=first second"), "{props}");
        // 10 keys, one per line, plus the trailing newline.
        assert_eq!(props.lines().count(), 10, "{props}");
    }

    #[test]
    fn properties_contains_every_required_key() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        create_library(&libs, &spec("Foo")).unwrap();
        let props = std::fs::read_to_string(libs.join("Foo/library.properties")).unwrap();
        for key in [
            "name=",
            "version=",
            "author=",
            "maintainer=",
            "sentence=",
            "paragraph=",
            "category=",
            "url=",
            "architectures=",
        ] {
            assert!(props.contains(key), "missing {key} in:\n{props}");
        }
        assert!(props.contains("version=0.1.0"), "{props}");
        assert!(props.contains("architectures=*"), "{props}");
        assert!(props.contains("includes=Foo.h"), "{props}");
    }

    #[test]
    fn maintainer_defaults_to_author() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        let s = LibrarySpec {
            name: "Foo".into(),
            author: "Ada <ada@example.com>".into(),
            ..Default::default()
        };
        create_library(&libs, &s).unwrap();
        let props = std::fs::read_to_string(libs.join("Foo/library.properties")).unwrap();
        assert!(props.contains("maintainer=Ada <ada@example.com>"), "{props}");
    }

    #[test]
    fn keywords_txt_is_tab_separated() {
        // Canary: a formatter that turns these tabs into spaces silently breaks
        // syntax highlighting with no error anywhere else.
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        create_library(&libs, &spec("My Sensor")).unwrap();
        let kw = std::fs::read_to_string(libs.join("My_Sensor/keywords.txt")).unwrap();
        assert!(kw.contains("My_Sensor\tKEYWORD1"), "{kw}");
        assert!(kw.contains("begin\tKEYWORD2"), "{kw}");
        assert!(kw.contains("setInterval\tKEYWORD2"), "{kw}");
    }

    // ----- scaffolding -----

    #[test]
    fn creates_the_full_skeleton() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        let made = create_library(&libs, &spec("My Sensor")).unwrap();

        assert_eq!(made.dir, libs.join("My_Sensor"));
        assert_eq!(
            made.files,
            vec![
                "library.properties",
                "src/My_Sensor.h",
                "src/My_Sensor.cpp",
                "keywords.txt",
                "examples/My_Sensor/My_Sensor.ino",
            ]
        );
        for rel in &made.files {
            assert!(made.dir.join(rel).is_file(), "missing {rel}");
        }

        let header = std::fs::read_to_string(made.dir.join("src/My_Sensor.h")).unwrap();
        assert!(header.contains("#ifndef MY_SENSOR_H"), "{header}");
        assert!(header.contains("class My_Sensor {"), "{header}");
        // No unsubstituted tokens may survive anywhere.
        assert!(!header.contains("{{"), "{header}");

        let source = std::fs::read_to_string(made.dir.join("src/My_Sensor.cpp")).unwrap();
        assert!(source.contains("#include \"My_Sensor.h\""), "{source}");
        assert!(source.contains("My_Sensor::My_Sensor("), "{source}");

        let ino =
            std::fs::read_to_string(made.dir.join("examples/My_Sensor/My_Sensor.ino")).unwrap();
        assert!(ino.contains("#include <My_Sensor.h>"), "{ino}");
        assert!(ino.contains("My_Sensor lib(1000);"), "{ino}");
        assert!(!ino.contains("{{"), "{ino}");
    }

    #[test]
    fn example_only_calls_methods_the_header_declares() {
        // The example is the smoke test users compile first; if it references a
        // symbol the stub lacks, a brand-new library fails to build.
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        let made = create_library(&libs, &spec("Foo")).unwrap();
        let header = std::fs::read_to_string(made.dir.join("src/Foo.h")).unwrap();
        for symbol in ["begin(", "update(", "count(", "version("] {
            assert!(header.contains(symbol), "header lacks {symbol}:\n{header}");
        }
    }

    #[test]
    fn creates_the_libraries_dir_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("nested").join("libraries");
        assert!(!libs.exists());
        let made = create_library(&libs, &spec("Foo")).unwrap();
        assert!(made.dir.is_dir());
    }

    #[test]
    fn refuses_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        std::fs::create_dir_all(libs.join("Foo")).unwrap();
        std::fs::write(libs.join("Foo/keep.txt"), "sentinel").unwrap();

        let err = create_library(&libs, &spec("Foo")).unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
        // The pre-existing content must be untouched and no staging left over.
        assert_eq!(
            std::fs::read_to_string(libs.join("Foo/keep.txt")).unwrap(),
            "sentinel"
        );
        assert!(!libs.join(".bancada-new-Foo").exists());
    }

    #[test]
    #[cfg(unix)]
    fn refuses_a_dangling_symlink_destination() {
        // Regression guard for using exists() instead of symlink_metadata():
        // exists() follows the link and reports false, but create_dir_all still
        // fails. Sketchbooks really do contain symlinked libraries.
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        std::fs::create_dir_all(&libs).unwrap();
        std::os::unix::fs::symlink(dir.path().join("nonexistent"), libs.join("Foo")).unwrap();
        assert!(!libs.join("Foo").exists()); // dangling
        let err = create_library(&libs, &spec("Foo")).unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
    }

    #[test]
    fn refuses_a_case_only_difference() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        std::fs::create_dir_all(libs.join("foo")).unwrap();
        let err = create_library(&libs, &spec("Foo")).unwrap_err().to_string();
        assert!(err.contains("differs only in case"), "{err}");
    }

    #[test]
    fn leaves_nothing_behind_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        std::fs::create_dir_all(&libs).unwrap();
        // A regular file where the staging directory must go: remove_dir_all
        // refuses it, so creation fails deterministically without chmod tricks
        // (which no-op under root in CI).
        std::fs::write(libs.join(".bancada-new-Foo"), "in the way").unwrap();

        assert!(create_library(&libs, &spec("Foo")).is_err());
        assert!(!libs.join("Foo").exists(), "destination must not be created");
        assert_eq!(
            std::fs::read_to_string(libs.join(".bancada-new-Foo")).unwrap(),
            "in the way",
            "an unrelated file must not be deleted"
        );
    }

    #[test]
    fn stale_staging_dir_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries");
        std::fs::create_dir_all(libs.join(".bancada-new-Foo/junk")).unwrap();
        std::fs::write(libs.join(".bancada-new-Foo/junk/x"), "old").unwrap();

        let made = create_library(&libs, &spec("Foo")).unwrap();
        assert!(made.dir.join("library.properties").is_file());
        assert!(!libs.join(".bancada-new-Foo").exists());
        assert!(!made.dir.join("junk").exists(), "stale content leaked in");
    }
}
