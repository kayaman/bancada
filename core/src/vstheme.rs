//! Reading VS Code colour themes off disk: `.json` files and `.vsix` packages.
//!
//! This module is the only place in Bancada where a third-party file is
//! parsed, so the containment lives here rather than at the call site:
//!
//! * Nothing is ever extracted to disk. A `.vsix` is read entry-by-entry into
//!   memory, which makes zip-slip structurally impossible rather than merely
//!   guarded against — there is no path being written to.
//! * Every limit is explicit and small ([`MAX_ENTRY_BYTES`],
//!   [`MAX_INCLUDE_DEPTH`], [`MAX_THEMES`]). A colour theme is tens of
//!   kilobytes of JSON; anything claiming to be much more is not one.
//! * `include:` chains are resolved against the theme's own directory and
//!   refuse to leave it, so a theme cannot read `../../../.ssh/id_rsa` by
//!   asking politely.
//!
//! What this module deliberately does NOT do is decide what the colours mean.
//! It hands back merged JSON; the mapping onto Bancada's tokens, and the
//! contrast floors that mapping has to clear, live in `src/theme/vscode.ts`
//! next to the WCAG maths they need.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result};

/// Refuse any single file bigger than this. VS Code's own Dark+ is ~40 KB;
/// the largest themes on the marketplace are a few hundred. 8 MB is far past
/// generous and still bounds a decompression bomb to something survivable.
pub const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

/// `include` may chain, but not far. Real themes use one or two levels (a
/// "colour variant" including its base); this also bounds a cycle that the
/// visited-set below would otherwise have to catch alone.
pub const MAX_INCLUDE_DEPTH: usize = 8;

/// A `.vsix` may contribute several themes (light/dark variants). Cap it so a
/// hostile package cannot make the picker unusable.
pub const MAX_THEMES: usize = 24;

/// One theme found in a file or package, with its JSON already merged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeSource {
    /// Stable id: the package identity plus the theme's label.
    pub id: String,
    /// The label a human picked, from `contributes.themes[].label` or the
    /// theme's own `name`, falling back to the file stem.
    pub label: String,
    /// `dark` / `light` / `hc*` as declared, when it was declared.
    pub kind: Option<String>,
    /// The merged theme document, `include` chains already applied.
    pub json: String,
}

// ---------- JSONC ----------

/// Strips `//` and `/* */` comments and trailing commas from JSON-with-comments.
///
/// String-literal aware, which the equivalent in `serialPrefs.ts` explicitly
/// is not — and here it has to be. Theme files carry URLs in their metadata
/// (`"homepage": "https://..."`), and a naive stripper turns that into
/// `"homepage": "https:` and then fails to parse the rest of the document. The
/// cost of being wrong is not "one baud the user re-picks"; it is a theme that
/// mysteriously will not load.
///
/// Escapes are honoured so a `\"` inside a string does not end it.
pub fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' => match chars.peek() {
                Some('/') => {
                    for n in chars.by_ref() {
                        if n == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for n in chars.by_ref() {
                        if prev == '*' && n == '/' {
                            break;
                        }
                        // Keep newlines so error line numbers stay usable.
                        if n == '\n' {
                            out.push('\n');
                        }
                        prev = n;
                    }
                }
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }

    strip_trailing_commas(&out)
}

/// Removes `,` that is followed only by whitespace and a `}` or `]`.
fn strip_trailing_commas(src: &str) -> String {
    let bytes: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut in_string = false;
    let mut escaped = false;

    for (i, &c) in bytes.iter().enumerate() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }
        if c == ',' {
            let next = bytes[i + 1..]
                .iter()
                .find(|ch| !ch.is_whitespace())
                .copied();
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Parses JSON-with-comments.
pub fn parse_jsonc(src: &str, what: &str) -> Result<Value> {
    serde_json::from_str(&strip_jsonc(src)).map_err(|source| Error::Json {
        what: what.to_string(),
        source,
    })
}

// ---------- include resolution ----------

/// Merges `base` under `over`: `over` wins on scalars, objects merge
/// key-by-key, and `tokenColors` arrays concatenate base-first so an including
/// theme's rules are appended after (and therefore override) the ones it
/// inherits — which is the order VS Code applies them in.
fn merge(base: Value, over: Value) -> Value {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            let mut out: Map<String, Value> = b;
            for (k, v) in o {
                let merged = match out.remove(&k) {
                    Some(prev) if k == "tokenColors" => match (prev, v) {
                        (Value::Array(mut pa), Value::Array(va)) => {
                            pa.extend(va);
                            Value::Array(pa)
                        }
                        (_, v) => v,
                    },
                    Some(prev) => merge(prev, v),
                    None => v,
                };
                out.insert(k, merged);
            }
            Value::Object(out)
        }
        (_, over) => over,
    }
}

/// Joins `rel` onto `dir` and refuses the result if it escapes `root`.
///
/// `include` is a path from a file we did not write. Resolving it naively
/// lets a theme reach anywhere the process can read; this keeps it inside the
/// directory the theme was loaded from. Rejects absolute paths outright.
fn safe_join(root: &Path, dir: &Path, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(Error::Other(format!(
            "theme include must be a relative path: {rel}"
        )));
    }
    let mut out = dir.to_path_buf();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(Error::Other(format!("theme include escapes root: {rel}")));
                }
            }
            _ => return Err(Error::Other(format!("unsupported include path: {rel}"))),
        }
    }
    if !out.starts_with(root) {
        return Err(Error::Other(format!("theme include escapes root: {rel}")));
    }
    Ok(out)
}

/// Resolves a theme document's `include` chain from the filesystem.
///
/// `root` bounds where includes may reach — the theme's own directory for a
/// loose `.json`, the extension's directory for one unpacked from a `.vsix`.
pub fn resolve_includes_fs(path: &Path, root: &Path) -> Result<Value> {
    let mut visited = BTreeSet::new();
    resolve_from(path, root, &mut visited, 0, &|p| {
        std::fs::read_to_string(p).map_err(Error::Io)
    })
}

/// The same resolution against an arbitrary reader, so a `.vsix`'s in-memory
/// entries can be chained without ever touching disk.
pub fn resolve_includes_with(
    path: &Path,
    root: &Path,
    read: &dyn Fn(&Path) -> Result<String>,
) -> Result<Value> {
    let mut visited = BTreeSet::new();
    resolve_from(path, root, &mut visited, 0, read)
}

fn resolve_from(
    path: &Path,
    root: &Path,
    visited: &mut BTreeSet<PathBuf>,
    depth: usize,
    read: &dyn Fn(&Path) -> Result<String>,
) -> Result<Value> {
    if depth > MAX_INCLUDE_DEPTH {
        return Err(Error::Other(format!(
            "theme include chain deeper than {MAX_INCLUDE_DEPTH}"
        )));
    }
    if !visited.insert(path.to_path_buf()) {
        return Err(Error::Other(format!(
            "theme include cycle at {}",
            path.display()
        )));
    }

    let text = read(path)?;
    let doc = parse_jsonc(&text, &path.display().to_string())?;

    let include = doc
        .get("include")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(rel) = include else { return Ok(doc) };

    let dir = path.parent().unwrap_or(root);
    let base_path = safe_join(root, dir, &rel)?;
    let base = resolve_from(&base_path, root, visited, depth + 1, read)?;

    let mut merged = merge(base, doc);
    // `include` has been applied; leaving it in would be a lie about the
    // document that survives into anything that re-reads it.
    if let Some(obj) = merged.as_object_mut() {
        obj.remove("include");
    }
    Ok(merged)
}

// ---------- .vsix ----------

/// Reads every theme a `.vsix` contributes.
///
/// A `.vsix` is a zip whose `extension/package.json` lists its themes under
/// `contributes.themes[]`, each with a `path` and usually a `label`. Entries
/// are read into memory only; nothing is written anywhere.
pub fn read_vsix(path: &Path) -> Result<Vec<ThemeSource>> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| Error::Other(format!("not a readable .vsix: {e}")))?;

    // Read the whole archive's text entries up front. A theme package is
    // small, and holding them lets `include` chains resolve between entries
    // without a second pass over the zip.
    let mut entries: Vec<(String, String)> = Vec::new();
    for i in 0..zip.len() {
        let mut e = match zip.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !e.is_file() {
            continue;
        }
        // `name()` is the raw archive path. It is never used to open anything
        // — only compared and reported — so a `..` inside it is inert.
        let name = e.name().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        if e.size() > MAX_ENTRY_BYTES {
            continue;
        }
        let mut buf = String::new();
        // Take one byte past the cap so a lying header cannot outrun it.
        if e.by_ref()
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_string(&mut buf)
            .is_err()
        {
            continue; // not UTF-8 text; not a theme
        }
        if buf.len() as u64 > MAX_ENTRY_BYTES {
            continue;
        }
        entries.push((name, buf));
    }

    let pkg_name = entries
        .iter()
        .map(|(n, _)| n.as_str())
        .find(|n| *n == "extension/package.json" || *n == "package.json")
        .ok_or_else(|| Error::Other("no package.json in .vsix".into()))?
        .to_string();
    let pkg_dir = Path::new(&pkg_name)
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
    let pkg_text = entries
        .iter()
        .find(|(n, _)| *n == pkg_name)
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    let pkg = parse_jsonc(&pkg_text, "package.json")?;

    let ident = {
        let publisher = pkg.get("publisher").and_then(Value::as_str).unwrap_or("");
        let name = pkg.get("name").and_then(Value::as_str).unwrap_or("theme");
        if publisher.is_empty() {
            name.to_string()
        } else {
            format!("{publisher}.{name}")
        }
    };

    let contributed = pkg
        .get("contributes")
        .and_then(|c| c.get("themes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let read = |p: &Path| -> Result<String> {
        let key = normalise(p);
        entries
            .iter()
            .find(|(n, _)| normalise(Path::new(n)) == key)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| Error::Other(format!("theme file not in package: {}", p.display())))
    };

    let mut out = Vec::new();
    for entry in contributed.into_iter().take(MAX_THEMES) {
        let Some(rel) = entry.get("path").and_then(Value::as_str) else {
            continue;
        };
        let theme_path = match safe_join(Path::new(""), &pkg_dir, rel.trim_start_matches("./")) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let doc = match resolve_includes_with(&theme_path, Path::new(""), &read) {
            Ok(d) => d,
            Err(_) => continue, // one broken variant must not sink the package
        };
        let label = entry
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| doc.get("name").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| {
                theme_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Theme".into())
            });
        let kind = entry
            .get("uiTheme")
            .and_then(Value::as_str)
            .map(ui_theme_to_kind)
            .or_else(|| doc.get("type").and_then(Value::as_str).map(str::to_owned));

        out.push(ThemeSource {
            id: format!("vsix:{ident}:{label}"),
            label,
            kind,
            json: doc.to_string(),
        });
    }

    if out.is_empty() {
        return Err(Error::Other(
            "this .vsix contributes no colour themes".into(),
        ));
    }
    Ok(out)
}

/// `vs-dark` / `vs` / `hc-black` are the `uiTheme` spellings; the theme
/// documents themselves use `dark` / `light` / `hc`.
fn ui_theme_to_kind(ui: &str) -> String {
    match ui {
        "vs" => "light",
        "hc-light" => "hcLight",
        "hc-black" => "hc",
        _ => "dark",
    }
    .to_string()
}

/// Archive paths are `/`-separated regardless of host; compare them that way.
fn normalise(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Reads a loose `.json` theme, resolving `include` against its own directory.
pub fn read_json_theme(path: &Path) -> Result<ThemeSource> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_ENTRY_BYTES {
        return Err(Error::Other(format!(
            "theme file is larger than {MAX_ENTRY_BYTES} bytes"
        )));
    }
    let root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let doc = resolve_includes_fs(path, &root)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Theme".into());
    let label = doc
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| stem.clone());
    let kind = doc.get("type").and_then(Value::as_str).map(str::to_owned);
    Ok(ThemeSource {
        id: format!("file:{stem}:{label}"),
        label,
        kind,
        json: doc.to_string(),
    })
}

/// Reads whichever of the two a path is, by extension.
pub fn read_theme_path(path: &Path) -> Result<Vec<ThemeSource>> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "vsix" => read_vsix(path),
        "json" | "jsonc" => Ok(vec![read_json_theme(path)?]),
        _ => Err(Error::Other(
            "expected a .json colour theme or a .vsix package".into(),
        )),
    }
}
