//! Referencing a library from a git repository by alias, pinned to a version.
//!
//! `sketch.yaml` can only name a registry library or a local directory, and a
//! `dir:` entry carries no version, so a pinned remote library is resolved here
//! and recorded in a sibling manifest (`bancada.yaml`) instead.
//!
//! Versions come from `git ls-remote --tags`, not the GitHub API: it needs no
//! token, has no rate limit, and works against any git host.
//!
//! Parsing, ranking and the manifest are pure and unit-tested; the two functions
//! that shell out to `git` are kept thin, the way [`crate::cli`] treats
//! `arduino-cli`.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Manifest filename, alongside `sketch.yaml` in the sketch directory.
pub const MANIFEST_NAME: &str = "bancada.yaml";
/// Directory holding materialised remote libraries, relative to the sketch.
pub const VENDOR_DIR: &str = ".bancada/libs";

/// A parsed `@owner/repo/path@ref` alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GhAlias {
    pub owner: String,
    pub repo: String,
    /// Path inside the repository; empty when the library is at the root.
    pub path: String,
    /// Library (and vendor directory) name.
    pub name: String,
    /// Explicit ref from the alias, when given.
    pub git_ref: Option<String>,
}

/// One tag from `git ls-remote --tags`, resolved to the commit it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTag {
    pub name: String,
    pub commit: String,
}

/// A pinned remote library recorded in `bancada.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub alias: String,
    /// The ref the user chose (usually a tag).
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// The commit that ref resolved to. A tag can move; this cannot.
    pub commit: String,
    /// Vendor path relative to the sketch directory.
    pub vendor: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default = "manifest_version")]
    pub version: u32,
    #[serde(default)]
    pub libraries: Vec<ManifestEntry>,
}

fn manifest_version() -> u32 {
    1
}

// ---------- alias parsing ----------

/// Parse `@owner/repo[/path…][@ref]`.
///
/// The ref is split off at the *last* `@` because a ref may itself contain `/`
/// (this repo tags libraries as `HomeNode/v1.1.0`).
pub fn parse_alias(raw: &str) -> Result<GhAlias> {
    let trimmed = raw.trim();
    let body = trimmed.strip_prefix('@').unwrap_or(trimmed);
    if body.is_empty() {
        return Err(bad_alias("alias is empty"));
    }

    let (locator, git_ref) = match body.rfind('@') {
        Some(i) => {
            let r = &body[i + 1..];
            if r.is_empty() {
                return Err(bad_alias("the ref after `@` is empty"));
            }
            (&body[..i], Some(r.to_string()))
        }
        None => (body, None),
    };

    let segments: Vec<&str> = locator.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Err(bad_alias(
            "expected at least an owner and a repository, as in @owner/repo/path",
        ));
    }
    if segments.iter().any(|s| *s == "." || *s == "..") {
        return Err(bad_alias("`.` and `..` are not allowed in a path"));
    }

    let owner = segments[0].to_string();
    let repo = segments[1].to_string();
    let path = segments[2..].join("/");
    let name = segments.last().map(|s| s.to_string()).unwrap_or_default();

    Ok(GhAlias {
        owner,
        repo,
        path,
        name,
        git_ref,
    })
}

fn bad_alias(why: &str) -> Error {
    Error::Other(format!(
        "{why} — expected @owner/repo/path/to/lib, optionally @ref"
    ))
}

impl GhAlias {
    /// Clone URL. HTTPS so existing git credential helpers apply.
    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}.git", self.owner, self.repo)
    }

    /// The alias without its ref, as it is stored in the manifest.
    pub fn canonical(&self) -> String {
        let mut s = format!("@{}/{}", self.owner, self.repo);
        if !self.path.is_empty() {
            s.push('/');
            s.push_str(&self.path);
        }
        s
    }

    /// Vendor path relative to the sketch directory.
    pub fn vendor_rel(&self) -> String {
        format!("{VENDOR_DIR}/{}", self.name)
    }
}

// ---------- ls-remote parsing and version ranking ----------

/// Parse `git ls-remote --tags` output.
///
/// An annotated tag appears twice: once as the tag object and once as
/// `<name>^{}` for the commit it dereferences to. The commit is what a pin must
/// record, so `^{}` rows win.
pub fn parse_ls_remote(out: &str) -> Vec<RemoteTag> {
    let mut by_name: BTreeMap<String, String> = BTreeMap::new();
    for line in out.lines() {
        let mut parts = line.split_whitespace();
        let Some(sha) = parts.next() else { continue };
        let Some(reference) = parts.next() else {
            continue;
        };
        let Some(name) = reference.strip_prefix("refs/tags/") else {
            continue;
        };
        match name.strip_suffix("^{}") {
            // Dereferenced commit: always authoritative.
            Some(base) => {
                by_name.insert(base.to_string(), sha.to_string());
            }
            // Lightweight tag, or the tag object of an annotated one.
            None => {
                by_name.entry(name.to_string()).or_insert(sha.to_string());
            }
        }
    }
    by_name
        .into_iter()
        .map(|(name, commit)| RemoteTag { name, commit })
        .collect()
}

/// Numeric key for a tag, or `None` when it is not version-shaped.
///
/// Handles the `HomeNode/v1.1.0` convention by looking only at the segment after
/// the last `/`, and pads so `1.1` and `1.1.0` compare equal.
pub(crate) fn version_key(tag: &str) -> Option<Vec<u64>> {
    let last = tag.rsplit('/').next().unwrap_or(tag);
    let stripped = last.trim_start_matches(['v', 'V']);
    let core = stripped.split(['-', '+']).next().unwrap_or(stripped);
    if core.is_empty() {
        return None;
    }
    let mut parts: Vec<u64> = core
        .split('.')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() {
        return None;
    }
    parts.resize(4, 0);
    Some(parts)
}

/// Order tags for presentation: those namespaced under `lib_name` first (the
/// convention in a monorepo of libraries), then the rest; newest first within
/// each group, and tags that are not version-shaped last rather than hidden.
pub fn rank_versions(tags: &[RemoteTag], lib_name: &str) -> Vec<RemoteTag> {
    let mine = |t: &RemoteTag| t.name.split('/').next().is_some_and(|s| s == lib_name);

    let mut primary: Vec<RemoteTag> = tags.iter().filter(|t| mine(t)).cloned().collect();
    let mut rest: Vec<RemoteTag> = tags.iter().filter(|t| !mine(t)).cloned().collect();
    sort_newest_first(&mut primary);
    sort_newest_first(&mut rest);
    primary.extend(rest);
    primary
}

fn sort_newest_first(v: &mut [RemoteTag]) {
    v.sort_by(|a, b| match (version_key(&a.name), version_key(&b.name)) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });
}

// ---------- manifest ----------

impl Manifest {
    pub fn path_in(sketch_dir: &Path) -> PathBuf {
        sketch_dir.join(MANIFEST_NAME)
    }

    /// Load the manifest, treating a missing file as empty.
    pub fn load(sketch_dir: &Path) -> Result<Self> {
        let p = Self::path_in(sketch_dir);
        if !p.exists() {
            return Ok(Self {
                version: manifest_version(),
                libraries: Vec::new(),
            });
        }
        Ok(serde_yaml::from_str(&std::fs::read_to_string(&p)?)?)
    }

    pub fn save(&self, sketch_dir: &Path) -> Result<()> {
        std::fs::write(Self::path_in(sketch_dir), serde_yaml::to_string(self)?)?;
        Ok(())
    }

    /// Insert or replace the entry for an alias, keeping list order stable so a
    /// re-pin shows up as an edit rather than a move.
    pub fn upsert(&mut self, entry: ManifestEntry) {
        match self.libraries.iter_mut().find(|e| e.alias == entry.alias) {
            Some(existing) => *existing = entry,
            None => self.libraries.push(entry),
        }
    }
}

// ---------- git ----------

/// One git runner for the crate; see `crate::git::run`. Re-exported
/// `pub(crate)` (rather than a plain `use`) because `clone.rs` calls this as
/// `ghlib::git(..)`.
pub(crate) use crate::git::run as git;

/// Tags available in a remote, ranked for the given library name.
pub fn list_remote_tags(url: &str, lib_name: &str) -> Result<Vec<RemoteTag>> {
    let out = git(&["ls-remote", "--tags", url])?;
    Ok(rank_versions(&parse_ls_remote(&out), lib_name))
}

/// Removes a scratch clone unless disarmed.
struct Scratch(Option<PathBuf>);

impl Scratch {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// Fetch one subtree of a repository at `git_ref` into `dest`.
///
/// Returns the commit actually checked out. When `expect_commit` is given and
/// differs, the ref moved since it was recorded and the fetch is refused rather
/// than quietly building something else.
///
/// The subtree is *moved out of* the clone, so the result carries no `.git` —
/// otherwise it would be a nested repository inside the user's sketch repo.
pub fn fetch_subtree(
    alias: &GhAlias,
    git_ref: &str,
    dest: &Path,
    expect_commit: Option<&str>,
) -> Result<String> {
    let parent = dest
        .parent()
        .ok_or_else(|| Error::Other(format!("{} has no parent directory", dest.display())))?;
    std::fs::create_dir_all(parent)?;

    // Scratch clone next to the destination so the final move stays on one
    // filesystem and is therefore atomic.
    let scratch = parent.join(format!(".fetch-{}", alias.name));
    if scratch.exists() {
        std::fs::remove_dir_all(&scratch)?;
    }
    let mut guard = Scratch(Some(scratch.clone()));
    let scratch_s = scratch.to_string_lossy().into_owned();

    let url = alias.url();
    let mut args = vec![
        "clone",
        "--depth",
        "1",
        "--filter=blob:none",
        "--branch",
        git_ref,
    ];
    if !alias.path.is_empty() {
        args.push("--sparse");
    }
    args.extend_from_slice(&[url.as_str(), scratch_s.as_str()]);
    git(&args)?;

    if !alias.path.is_empty() {
        git(&["-C", &scratch_s, "sparse-checkout", "set", &alias.path])?;
    }

    let commit = git(&["-C", &scratch_s, "rev-parse", "HEAD"])?
        .trim()
        .to_string();
    if let Some(expected) = expect_commit {
        if expected != commit {
            return Err(Error::Other(format!(
                "`{git_ref}` now points at {commit} but the manifest pins {expected} — the tag moved; re-add the library to accept the new commit"
            )));
        }
    }

    let src = if alias.path.is_empty() {
        // Whole repo is the library: drop the git metadata before moving it.
        std::fs::remove_dir_all(scratch.join(".git"))?;
        scratch.clone()
    } else {
        scratch.join(&alias.path)
    };
    if !src.join("library.properties").is_file() {
        return Err(Error::Other(format!(
            "no library.properties in `{}` of {}/{} at {git_ref} — is the path right?",
            if alias.path.is_empty() {
                "<repo root>"
            } else {
                &alias.path
            },
            alias.owner,
            alias.repo
        )));
    }

    // Re-adding an alias is the update path, so replacing is correct here.
    if dest.symlink_metadata().is_ok() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::rename(&src, dest)?;

    guard.disarm();
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(commit)
}

/// Ensure `.bancada/` is ignored by the sketch's git repo, if it has one.
///
/// Returns true when the line was added. Vendored libraries are re-fetchable
/// from the manifest, so committing them would only bloat the repository.
pub fn ensure_gitignored(sketch_dir: &Path) -> Result<bool> {
    let p = sketch_dir.join(".gitignore");
    let current = std::fs::read_to_string(&p).unwrap_or_default();
    if current.lines().any(|l| {
        let t = l.trim();
        t == ".bancada" || t == ".bancada/" || t == "/.bancada" || t == "/.bancada/"
    }) {
        return Ok(false);
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(".bancada/\n");
    std::fs::write(&p, next)?;
    Ok(true)
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    // Real output, captured from `git ls-remote --tags` against the repo this
    // feature was designed for. Note the `^{}` pairs.
    const LS_REMOTE: &str = "\
edebdaeb6a64aa68013ed2de6b6d4be42179445f\trefs/tags/HomeNode/v1.1.0
ef11f73f90389bf15a1002ea8eb82a2662713257\trefs/tags/HomeNode/v1.1.0^{}
5e5719b0c9edc2933ab2eac307f6263912167dbf\trefs/tags/StatusLed/v1.0.0
ef11f73f90389bf15a1002ea8eb82a2662713257\trefs/tags/StatusLed/v1.0.0^{}
dcef8baaeecf81f20dc2c7ac2abe9ac60c7a7985\trefs/tags/StatusPanel/v1.0.0
9ca32e4aec2f8df3cba086dd475276cbe0f50219\trefs/tags/StatusPanel/v1.0.0^{}
";

    // ----- alias parsing -----

    #[test]
    fn parses_owner_repo_and_path() {
        let a = parse_alias("@kayaman/Arduino/libraries/HomeNode").unwrap();
        assert_eq!(a.owner, "kayaman");
        assert_eq!(a.repo, "Arduino");
        assert_eq!(a.path, "libraries/HomeNode");
        assert_eq!(a.name, "HomeNode");
        assert_eq!(a.git_ref, None);
        assert_eq!(a.url(), "https://github.com/kayaman/Arduino.git");
        assert_eq!(a.canonical(), "@kayaman/Arduino/libraries/HomeNode");
        assert_eq!(a.vendor_rel(), ".bancada/libs/HomeNode");
    }

    #[test]
    fn parses_library_at_repo_root() {
        let a = parse_alias("@kayaman/HomeNode").unwrap();
        assert_eq!(a.path, "");
        assert_eq!(a.name, "HomeNode");
        assert_eq!(a.canonical(), "@kayaman/HomeNode");
    }

    #[test]
    fn splits_ref_at_the_last_at_even_when_it_contains_slashes() {
        let a = parse_alias("@kayaman/Arduino/libraries/HomeNode@HomeNode/v1.1.0").unwrap();
        assert_eq!(a.path, "libraries/HomeNode");
        assert_eq!(a.git_ref.as_deref(), Some("HomeNode/v1.1.0"));
    }

    #[test]
    fn leading_at_is_optional() {
        assert_eq!(
            parse_alias("kayaman/Arduino").unwrap(),
            parse_alias("@kayaman/Arduino").unwrap()
        );
    }

    #[test]
    fn rejects_malformed_aliases() {
        for bad in ["", "@", "@onlyowner", "@owner/repo@", "@owner/../etc"] {
            assert!(parse_alias(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn alias_error_shows_the_expected_shape() {
        let err = parse_alias("@nope").unwrap_err().to_string();
        assert!(err.contains("@owner/repo/path/to/lib"), "{err}");
    }

    // ----- ls-remote -----

    #[test]
    fn dereferenced_commit_wins_over_the_tag_object() {
        let tags = parse_ls_remote(LS_REMOTE);
        let home = tags.iter().find(|t| t.name == "HomeNode/v1.1.0").unwrap();
        // ef11f73 is the ^{} commit; edebdae is the annotated tag object.
        assert_eq!(home.commit, "ef11f73f90389bf15a1002ea8eb82a2662713257");
        assert_eq!(tags.len(), 3, "one entry per tag, not two");
    }

    #[test]
    fn ignores_non_tag_refs_and_junk() {
        let out = "abc\trefs/heads/main\ngarbage\n\ndef\trefs/tags/v1.0.0\n";
        let tags = parse_ls_remote(out);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "v1.0.0");
    }

    #[test]
    fn lightweight_tags_keep_their_own_sha() {
        let tags = parse_ls_remote("aaa\trefs/tags/v2.0.0\n");
        assert_eq!(tags[0].commit, "aaa");
    }

    // ----- ranking -----

    #[test]
    fn ranks_the_librarys_own_namespace_first() {
        let ranked = rank_versions(&parse_ls_remote(LS_REMOTE), "HomeNode");
        assert_eq!(ranked[0].name, "HomeNode/v1.1.0");
        // the others follow, still present rather than filtered away
        assert_eq!(ranked.len(), 3);
    }

    #[test]
    fn sorts_newest_first_and_treats_1_1_as_1_1_0() {
        let tags = vec![
            RemoteTag {
                name: "Lib/v1.0.0".into(),
                commit: "a".into(),
            },
            RemoteTag {
                name: "Lib/v1.10.0".into(),
                commit: "b".into(),
            },
            RemoteTag {
                name: "Lib/v1.2.0".into(),
                commit: "c".into(),
            },
            RemoteTag {
                name: "Lib/v1.2".into(),
                commit: "d".into(),
            },
        ];
        let ranked = rank_versions(&tags, "Lib");
        let names: Vec<&str> = ranked.iter().map(|t| t.name.as_str()).collect();
        // 1.10.0 beats 1.2.0 (numeric, not lexicographic); 1.2 == 1.2.0
        assert_eq!(names[0], "Lib/v1.10.0");
        assert!(names[1].starts_with("Lib/v1.2"));
        assert!(names[2].starts_with("Lib/v1.2"));
        assert_eq!(names[3], "Lib/v1.0.0");
    }

    #[test]
    fn unversioned_tags_sort_last_but_are_kept() {
        let tags = vec![
            RemoteTag {
                name: "Lib/nightly".into(),
                commit: "a".into(),
            },
            RemoteTag {
                name: "Lib/v1.0.0".into(),
                commit: "b".into(),
            },
        ];
        let ranked = rank_versions(&tags, "Lib");
        assert_eq!(ranked[0].name, "Lib/v1.0.0");
        assert_eq!(ranked[1].name, "Lib/nightly");
    }

    #[test]
    fn prerelease_suffix_does_not_break_parsing() {
        let tags = vec![
            RemoteTag {
                name: "v1.0.0-rc1".into(),
                commit: "a".into(),
            },
            RemoteTag {
                name: "v0.9.0".into(),
                commit: "b".into(),
            },
        ];
        let ranked = rank_versions(&tags, "whatever");
        assert_eq!(ranked[0].name, "v1.0.0-rc1");
    }

    // ----- manifest -----

    fn entry(alias: &str, git_ref: &str, commit: &str) -> ManifestEntry {
        ManifestEntry {
            alias: alias.into(),
            git_ref: git_ref.into(),
            commit: commit.into(),
            vendor: ".bancada/libs/HomeNode".into(),
        }
    }

    #[test]
    fn missing_manifest_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let m = Manifest::load(tmp.path()).unwrap();
        assert_eq!(m.version, 1);
        assert!(m.libraries.is_empty());
    }

    #[test]
    fn manifest_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut m = Manifest::load(tmp.path()).unwrap();
        m.upsert(entry(
            "@kayaman/Arduino/libraries/HomeNode",
            "HomeNode/v1.1.0",
            "ef11f73",
        ));
        m.save(tmp.path()).unwrap();

        let back = Manifest::load(tmp.path()).unwrap();
        assert_eq!(back.libraries, m.libraries);
        // `ref` is a Rust keyword; make sure it is not serialised as `git_ref`.
        let text = std::fs::read_to_string(Manifest::path_in(tmp.path())).unwrap();
        assert!(text.contains("ref: HomeNode/v1.1.0"), "{text}");
        assert!(!text.contains("git_ref"), "{text}");
    }

    #[test]
    fn upsert_replaces_in_place() {
        let mut m = Manifest::default();
        m.upsert(entry("@a/b/c", "v1", "aaa"));
        m.upsert(entry("@x/y/z", "v1", "bbb"));
        m.upsert(entry("@a/b/c", "v2", "ccc"));
        assert_eq!(m.libraries.len(), 2);
        assert_eq!(m.libraries[0].git_ref, "v2");
        assert_eq!(m.libraries[0].commit, "ccc");
        assert_eq!(m.libraries[1].alias, "@x/y/z", "order preserved");
    }

    // ----- gitignore -----

    #[test]
    fn adds_gitignore_entry_once() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(ensure_gitignored(tmp.path()).unwrap());
        assert!(!ensure_gitignored(tmp.path()).unwrap(), "idempotent");
        let text = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(text.matches(".bancada/").count(), 1, "{text}");
    }

    #[test]
    fn gitignore_append_keeps_existing_content_and_newline() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "build/").unwrap();
        assert!(ensure_gitignored(tmp.path()).unwrap());
        let text = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(text, "build/\n.bancada/\n");
    }

    #[test]
    fn recognises_existing_variants() {
        for existing in [".bancada", "/.bancada/", ".bancada/"] {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::write(tmp.path().join(".gitignore"), format!("{existing}\n")).unwrap();
            assert!(!ensure_gitignored(tmp.path()).unwrap(), "{existing}");
        }
    }
}
