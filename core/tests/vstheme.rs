//! VS Code theme reading: JSONC, `include` chains, and `.vsix` packages.
//!
//! An integration test rather than an inline `mod tests` because building the
//! `.vsix` fixtures needs `zip` as a dev-dependency, and because everything
//! under test here is deliberately public API — this is the boundary the
//! Tauri command calls across.

use std::io::Write;
use std::path::{Path, PathBuf};

use bancada_core::vstheme::{
    parse_jsonc, read_json_theme, read_theme_path, read_vsix, resolve_includes_fs, MAX_THEMES,
};
use serde_json::Value;

// ---------- JSONC ----------

#[test]
fn strips_line_and_block_comments() {
    let src = r##"{
        // a line comment
        "a": 1, /* inline */ "b": 2
        /* multi
           line */
    }"##;
    let v = parse_jsonc(src, "t").unwrap();
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], 2);
}

#[test]
fn does_not_treat_a_url_inside_a_string_as_a_comment() {
    // Why this stripper is string-aware where serialPrefs.ts's deliberately is
    // not: theme metadata carries URLs, and cutting at the `//` does not lose
    // one field, it makes the rest of the document unparseable.
    let src = r##"{"homepage": "https://example.com/x", "colors": {"a": "#fff"}}"##;
    let v = parse_jsonc(src, "t").unwrap();
    assert_eq!(v["homepage"], "https://example.com/x");
    assert_eq!(v["colors"]["a"], "#fff");
}

#[test]
fn keeps_escaped_quotes_inside_strings() {
    let src = r##"{"name": "say \"hi\" // not a comment"}"##;
    let v = parse_jsonc(src, "t").unwrap();
    assert_eq!(v["name"], r##"say "hi" // not a comment"##);
}

#[test]
fn strips_trailing_commas_in_objects_and_arrays() {
    let src = r##"{"a": [1, 2, 3,], "b": {"c": 1,},}"##;
    let v = parse_jsonc(src, "t").unwrap();
    assert_eq!(v["a"][2], 3);
    assert_eq!(v["b"]["c"], 1);
}

#[test]
fn keeps_a_comma_that_lives_inside_a_string() {
    let src = r##"{"a": "x,", "b": 1}"##;
    let v = parse_jsonc(src, "t").unwrap();
    assert_eq!(v["a"], "x,");
}

#[test]
fn reports_which_file_failed_to_parse() {
    let err = parse_jsonc("{ not json", "themes/dark.json").unwrap_err();
    assert!(err.to_string().contains("themes/dark.json"), "{err}");
}

// ---------- include ----------

#[test]
fn include_merges_colors_and_lets_the_includer_win() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join("base.json"),
        r##"{"type":"dark","colors":{"editor.background":"#000","editor.foreground":"#fff"}}"##,
    )
    .unwrap();
    std::fs::write(
        root.join("top.json"),
        r##"{"include":"./base.json","colors":{"editor.foreground":"#eee"}}"##,
    )
    .unwrap();

    let v = resolve_includes_fs(&root.join("top.json"), root).unwrap();
    assert_eq!(v["type"], "dark");
    assert_eq!(v["colors"]["editor.background"], "#000");
    assert_eq!(v["colors"]["editor.foreground"], "#eee");
    // The directive has been applied; leaving it would misdescribe the doc.
    assert!(v.get("include").is_none());
}

#[test]
fn include_concatenates_token_colors_base_first() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join("base.json"),
        r##"{"tokenColors":[{"scope":"comment"}]}"##,
    )
    .unwrap();
    std::fs::write(
        root.join("top.json"),
        r##"{"include":"./base.json","tokenColors":[{"scope":"keyword"}]}"##,
    )
    .unwrap();
    let v = resolve_includes_fs(&root.join("top.json"), root).unwrap();
    let arr = v["tokenColors"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "base first, then the includer's own rules");
    assert_eq!(arr[0]["scope"], "comment");
    assert_eq!(arr[1]["scope"], "keyword");
}

#[test]
fn include_cycle_is_refused_rather_than_hung() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("a.json"), r##"{"include":"./b.json"}"##).unwrap();
    std::fs::write(root.join("b.json"), r##"{"include":"./a.json"}"##).unwrap();
    let err = resolve_includes_fs(&root.join("a.json"), root).unwrap_err();
    assert!(err.to_string().contains("cycle"), "{err}");
}

#[test]
fn include_cannot_escape_the_theme_directory() {
    // `include` is a path out of a file nobody here wrote. Unbounded, it
    // reaches anything the process can read.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("theme");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(tmp.path().join("secret.json"), r##"{"colors":{}}"##).unwrap();
    std::fs::write(
        root.join("evil.json"),
        r##"{"include":"../secret.json","colors":{}}"##,
    )
    .unwrap();
    let err = resolve_includes_fs(&root.join("evil.json"), &root).unwrap_err();
    assert!(err.to_string().contains("escapes root"), "{err}");
}

#[test]
fn absolute_include_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join("evil.json"),
        r##"{"include":"/etc/passwd","colors":{}}"##,
    )
    .unwrap();
    let err = resolve_includes_fs(&root.join("evil.json"), root).unwrap_err();
    assert!(err.to_string().contains("relative"), "{err}");
}

// ---------- loose .json ----------

#[test]
fn reads_a_plain_json_theme_with_comments_and_trailing_commas() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("solar.json");
    std::fs::write(
        &p,
        r##"{
            // real theme files look like this
            "name": "Solar",
            "type": "light",
            "colors": {"editor.background": "#fdf6e3",},
        }"##,
    )
    .unwrap();
    let src = read_json_theme(&p).unwrap();
    assert_eq!(src.label, "Solar");
    assert_eq!(src.kind.as_deref(), Some("light"));
    assert!(src.json.contains("fdf6e3"));
}

#[test]
fn a_theme_without_a_name_falls_back_to_its_filename() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("nameless.json");
    std::fs::write(&p, r##"{"colors":{}}"##).unwrap();
    assert_eq!(read_json_theme(&p).unwrap().label, "nameless");
}

#[test]
fn unknown_extensions_are_refused() {
    let err = read_theme_path(Path::new("/tmp/x.txt")).unwrap_err();
    assert!(err.to_string().contains(".vsix"), "{err}");
}

// ---------- .vsix ----------

fn make_vsix(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
    let path = dir.join("test.vsix");
    let f = std::fs::File::create(&path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in files {
        z.start_file(*name, opts).unwrap();
        z.write_all(body.as_bytes()).unwrap();
    }
    z.finish().unwrap();
    path
}

#[test]
fn reads_themes_a_vsix_contributes() {
    let tmp = tempfile::tempdir().unwrap();
    let vsix = make_vsix(
        tmp.path(),
        &[
            (
                "extension/package.json",
                r##"{"publisher":"acme","name":"neon","contributes":{"themes":[
                    {"label":"Neon Dark","uiTheme":"vs-dark","path":"./themes/dark.json"},
                    {"label":"Neon Light","uiTheme":"vs","path":"./themes/light.json"}
                ]}}"##,
            ),
            (
                "extension/themes/dark.json",
                r##"{"colors":{"editor.background":"#101014"}}"##,
            ),
            (
                "extension/themes/light.json",
                r##"{"colors":{"editor.background":"#ffffff"}}"##,
            ),
        ],
    );
    let out = read_vsix(&vsix).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].label, "Neon Dark");
    assert_eq!(out[0].kind.as_deref(), Some("dark"));
    assert_eq!(out[0].id, "vsix:acme.neon:Neon Dark");
    assert!(out[0].json.contains("101014"));
    // `uiTheme: "vs"` is the light spelling; the theme documents use "light".
    assert_eq!(out[1].kind.as_deref(), Some("light"));
}

#[test]
fn resolves_includes_between_entries_without_touching_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let vsix = make_vsix(
        tmp.path(),
        &[
            (
                "extension/package.json",
                r##"{"publisher":"a","name":"b","contributes":{"themes":[
                    {"label":"Variant","path":"./themes/variant.json"}]}}"##,
            ),
            (
                "extension/themes/base.json",
                r##"{"type":"dark","colors":{"editor.background":"#000000","editor.foreground":"#cccccc"}}"##,
            ),
            (
                "extension/themes/variant.json",
                r##"{"include":"./base.json","colors":{"editor.foreground":"#ffffff"}}"##,
            ),
        ],
    );
    let out = read_vsix(&vsix).unwrap();
    assert_eq!(out.len(), 1);
    let v: Value = serde_json::from_str(&out[0].json).unwrap();
    assert_eq!(v["colors"]["editor.background"], "#000000");
    assert_eq!(v["colors"]["editor.foreground"], "#ffffff");
}

#[test]
fn one_broken_variant_does_not_sink_the_package() {
    let tmp = tempfile::tempdir().unwrap();
    let vsix = make_vsix(
        tmp.path(),
        &[
            (
                "extension/package.json",
                r##"{"publisher":"a","name":"b","contributes":{"themes":[
                    {"label":"Broken","path":"./themes/missing.json"},
                    {"label":"Fine","path":"./themes/ok.json"}]}}"##,
            ),
            ("extension/themes/ok.json", r##"{"colors":{}}"##),
        ],
    );
    let out = read_vsix(&vsix).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].label, "Fine");
}

#[test]
fn a_vsix_with_no_themes_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let vsix = make_vsix(
        tmp.path(),
        &[("extension/package.json", r##"{"name":"not-a-theme"}"##)],
    );
    let err = read_vsix(&vsix).unwrap_err();
    assert!(err.to_string().contains("no colour themes"), "{err}");
}

#[test]
fn a_file_that_is_not_a_zip_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("fake.vsix");
    std::fs::write(&p, "this is not a zip at all").unwrap();
    let err = read_vsix(&p).unwrap_err();
    assert!(err.to_string().contains("readable .vsix"), "{err}");
}

#[test]
fn a_vsix_without_a_package_json_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let vsix = make_vsix(tmp.path(), &[("extension/themes/x.json", "{}")]);
    let err = read_vsix(&vsix).unwrap_err();
    assert!(err.to_string().contains("no package.json"), "{err}");
}

#[test]
fn a_theme_path_climbing_out_of_the_package_is_skipped() {
    // The zip-entry equivalent of the `include` escape: `contributes.themes[]`
    // is attacker-controlled too.
    let tmp = tempfile::tempdir().unwrap();
    let vsix = make_vsix(
        tmp.path(),
        &[
            (
                "extension/package.json",
                r##"{"publisher":"a","name":"b","contributes":{"themes":[
                    {"label":"Escape","path":"../../../../etc/passwd"},
                    {"label":"Fine","path":"./themes/ok.json"}]}}"##,
            ),
            ("extension/themes/ok.json", r##"{"colors":{}}"##),
        ],
    );
    let out = read_vsix(&vsix).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].label, "Fine");
}

#[test]
fn the_theme_count_is_capped() {
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<String> = (0..MAX_THEMES + 10)
        .map(|i| format!(r##"{{"label":"T{i}","path":"./t.json"}}"##))
        .collect();
    let pkg = format!(
        r##"{{"publisher":"a","name":"b","contributes":{{"themes":[{}]}}}}"##,
        entries.join(",")
    );
    let vsix = make_vsix(
        tmp.path(),
        &[
            ("extension/package.json", pkg.as_str()),
            ("extension/t.json", r##"{"colors":{}}"##),
        ],
    );
    assert_eq!(read_vsix(&vsix).unwrap().len(), MAX_THEMES);
}
