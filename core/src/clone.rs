//! Cloning an existing sketch project under a new name.
//!
//! Pure filesystem work plus one best-effort `git init`. The copy is
//! assembled in a dot-prefixed staging directory *inside* the destination
//! parent — same filesystem, so the final move is a single atomic rename —
//! and both the fresh repository and the merged `.gitignore` land in staging
//! before that rename. The fresh repository has no commits and cannot gain
//! any until the rename publishes it, so the credential ignore rules exist
//! strictly before any possible commit even though the `.gitignore` is
//! written *after* the file copy (the copy would otherwise overwrite it
//! with the source's version).
//!
//! Known caveat: two concurrent clones with the same destination share one
//! staging path, and the second reclaims (deletes) the first's half-built
//! staging. The exact-destination check keeps the outcome safe — at most one
//! rename wins — but a concurrent half-copy can be discarded.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{ghlib, library, project, sketch, Error, Result};
use crate::git::merged_gitignore;

/// Directories excluded from a clone on top of the sketch walker's skip
/// list: `.bancada` is re-fetchable from `bancada.yaml`, and `.claude` is
/// per-checkout agent state.
const CLONE_EXTRA_SKIP_DIRS: &[&str] = &[".bancada", ".claude"];

/// A finished clone.
#[derive(Debug, Clone, Serialize)]
pub struct ClonedProject {
    pub dir: PathBuf,
    pub name: String,
    /// Non-fatal notes: skipped vendored libraries, shared symlinks, git
    /// trouble, and sketch.yaml paths that may need attention.
    pub warnings: Vec<String>,
}

/// Whether a *directory* named `name` is excluded from clones. Files with
/// these names are copied — only directories are build/VCS/tool state.
pub(crate) fn is_skipped_dir(name: &str) -> bool {
    sketch::SKIP_DIRS.contains(&name) || CLONE_EXTRA_SKIP_DIRS.contains(&name)
}

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

/// Clone the sketch at `src_dir` into `dest_parent/new_name`.
///
/// All-or-nothing: a failure part-way through leaves the destination absent
/// and the source untouched. Git being missing or broken is a warning, not
/// an error — the clone is still a valid sketch without a repository.
pub fn clone_project(
    src_dir: &Path,
    dest_parent: &Path,
    new_name: &str,
) -> Result<ClonedProject> {
    let name = project::validate_project_name(new_name)?;

    let src_name = src_dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let main_ino = format!("{src_name}.ino");
    if src_name.is_empty() || !src_dir.is_dir() || !src_dir.join(&main_ino).is_file() {
        let expected = if src_name.is_empty() {
            "<name>.ino".to_string()
        } else {
            main_ino.clone()
        };
        return Err(Error::Other(format!(
            "{} is not a sketch folder — expected {}",
            src_dir.display(),
            src_dir.join(expected).display()
        )));
    }
    // is_file() above follows links, so a symlinked main ino would pass and
    // then be recreated under its OLD name, breaking the retitle later.
    if src_dir
        .join(&main_ino)
        .symlink_metadata()?
        .file_type()
        .is_symlink()
    {
        return Err(Error::Other(format!(
            "the main sketch {} is a symlink — clone the project that owns the real file instead",
            src_dir.join(&main_ino).display()
        )));
    }

    std::fs::create_dir_all(dest_parent)?;
    let src = src_dir.canonicalize()?;
    let parent = dest_parent.canonicalize()?;
    let dest = parent.join(&name);

    if dest == src {
        return Err(Error::Other(
            "the clone would overwrite the source project — choose a different name or destination"
                .into(),
        ));
    }
    if parent.starts_with(&src) {
        return Err(Error::Other(format!(
            "{} is inside the source project — choose a destination outside {}",
            parent.display(),
            src.display()
        )));
    }
    // symlink_metadata, not exists(): exists() reports false for a dangling
    // symlink while the rename into place would still land on it.
    if dest.symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "{} already exists — choose a different name or location",
            dest.display()
        )));
    }
    // Only after the exact-existence check: this message assumes the match
    // differs from `name` in case alone.
    if let Some(clash) = library::case_insensitive_clash(&parent, &name) {
        return Err(Error::Other(format!(
            "a folder named `{clash}` already exists there and differs only in case"
        )));
    }
    // The clone renames the main ino to `<name>.ino`; a same-named secondary
    // sketch file in the source would be clobbered by that rename.
    if name != src_name && src.join(format!("{name}.ino")).symlink_metadata().is_ok() {
        return Err(Error::Other(format!(
            "the source project already contains {name}.ino — the renamed main sketch would overwrite it"
        )));
    }

    let staging = parent.join(format!(".bancada-clone-{name}"));
    if staging.symlink_metadata().is_ok() {
        // Leftover from a killed run. remove_dir_all fails on a regular file,
        // which is the honest outcome — we will not delete an unrelated file.
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir(&staging)?;
    let mut guard = Staging(Some(staging.clone()));
    let mut warnings = Vec::new();

    // Git first, files second — see the module doc for why the .gitignore
    // written after the copy still precedes any possible commit.
    match ghlib::git(&["-C", &staging.to_string_lossy(), "init"]) {
        Ok(_) => {}
        Err(Error::ToolMissing(_)) => {
            warnings.push("git not found — clone created without a repository".into());
        }
        Err(Error::ToolFailed { stderr, .. }) => {
            warnings.push(format!(
                "git init failed — clone created without a repository: {stderr}"
            ));
        }
        Err(e) => {
            warnings.push(format!(
                "git init failed — clone created without a repository: {e}"
            ));
        }
    }

    copy_dir(
        &src,
        &staging,
        &src,
        &main_ino,
        &format!("{name}.ino"),
        &mut warnings,
    )?;

    // Overwrites the .gitignore the copy brought over with the merged one.
    let existing = std::fs::read_to_string(src.join(".gitignore")).unwrap_or_default();
    let gi = staging.join(".gitignore");
    // fs::write follows symlinks: writing through a recreated .gitignore link
    // would append our entries into the link's target, outside the clone.
    // The clone gets a regular file instead.
    if gi
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        std::fs::remove_file(&gi)?;
    }
    std::fs::write(&gi, merged_gitignore(&existing))?;

    retitle_main_ino(&staging.join(format!("{name}.ino")), &src_name, &name)?;
    rewrite_sketch_yaml(&staging, &src, &dest, &mut warnings)?;

    // Small TOCTOU window: a directory created at `dest` after the check
    // above would be replaced by this rename if empty — single-user app,
    // accepted.
    std::fs::rename(&staging, &dest)?;
    guard.disarm();

    Ok(ClonedProject {
        dir: dest,
        name,
        warnings,
    })
}

/// Copy `dir` into `dst` recursively, in sorted order for determinism.
///
/// Skips the directories [`is_skipped_dir`] names, renames the top-level main
/// `.ino`, recreates symlinks as symlinks (with a warning — the target is
/// shared with the source), and copies regular files with `fs::copy`, which
/// preserves unix permission bits.
fn copy_dir(
    dir: &Path,
    dst: &Path,
    root: &Path,
    main_ino: &str,
    new_ino: &str,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let at_root = dir == root;
    let mut entries: Vec<std::fs::DirEntry> =
        std::fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_s = name.to_string_lossy().into_owned();
        let ft = path.symlink_metadata()?.file_type();
        // A worktree checkout has a top-level .git FILE ("gitdir: …");
        // copying it would tie the clone to the source's repository — or
        // collide with the fresh .git directory git-init just made. Top
        // level only, any file type; everywhere else the dir-only skip
        // semantics below apply.
        if at_root && name_s == ".git" {
            continue;
        }
        if ft.is_symlink() {
            // Checked before is_dir(): a symlinked directory must be copied
            // as a link, not walked into.
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            #[cfg(unix)]
            {
                // read_link + symlink also round-trips a dangling link, which
                // fs::copy would refuse.
                let target = std::fs::read_link(&path)?;
                std::os::unix::fs::symlink(&target, dst.join(&name))?;
                warnings.push(format!("`{rel}` is a symlink shared with the source project"));
            }
            #[cfg(not(unix))]
            {
                warnings.push(format!("`{rel}` is a symlink and was not copied on this platform"));
            }
        } else if ft.is_dir() {
            if is_skipped_dir(&name_s) {
                if name_s == ".bancada" {
                    warnings.push(
                        "`.bancada/` (vendored libraries) was not copied — run gh restore in the clone to re-fetch them"
                            .into(),
                    );
                }
                continue;
            }
            let sub = dst.join(&name);
            std::fs::create_dir(&sub)?;
            copy_dir(&path, &sub, root, main_ino, new_ino, warnings)?;
        } else if ft.is_file() {
            let to = if at_root && name_s == main_ino {
                dst.join(new_ino)
            } else {
                dst.join(&name)
            };
            std::fs::copy(&path, to)?;
        } else {
            // FIFOs, sockets, devices: fs::copy would open them — and block
            // forever on a FIFO. A sketch has no use for special files.
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            warnings.push(format!("`{rel}` is not a regular file and was not copied"));
        }
    }
    Ok(())
}


/// Rewrite a main ino's title comment: when line 1 is exactly a
/// `// <src_name>` prefix followed by a non-alphanumeric character (or the
/// end of the line), that one occurrence becomes `new_name`. Everything else
/// stays byte-identical, and a non-UTF-8 file is left alone.
///
/// Shared with [`crate::project::rename_project`], which applies it in place.
pub(crate) fn retitle_main_ino(path: &Path, src_name: &str, new_name: &str) -> Result<()> {
    let Ok(text) = String::from_utf8(std::fs::read(path)?) else {
        return Ok(());
    };
    let line_end = text.find('\n').unwrap_or(text.len());
    let first = &text[..line_end];
    let prefix = format!("// {src_name}");
    if !first.starts_with(&prefix) {
        return Ok(());
    }
    let after = &first[prefix.len()..];
    if after.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        // `// Srcocity` is a longer word, not the project name.
        return Ok(());
    }
    let mut out = format!("// {new_name}{after}");
    out.push_str(&text[line_end..]);
    std::fs::write(path, out)?;
    Ok(())
}

/// Repoint sketch.yaml `dir:` library deps so a project at `dest` resolves the
/// same libraries it did at `src`: absolute paths inside `src` are rebased onto
/// `dest`, relative paths that climb out of the sketch are re-aimed at their
/// original target from `dest`, and everything that already resolves
/// (inside-the-tree relatives, external absolutes) stays put.
///
/// `work_dir` is where the file to edit lives — the staging copy for a clone,
/// the project itself for [`crate::project::rename_project`], which runs this
/// before the directory moves. `src`/`dest` are the tree's old and new roots
/// either way.
///
/// The parse is read-only, purely to find the local deps; the rewrite itself
/// is textual because a serde round-trip is lossy — unknown arduino-cli keys,
/// comments and ordering would all be dropped or reshuffled.
pub(crate) fn rewrite_sketch_yaml(
    work_dir: &Path,
    src: &Path,
    dest: &Path,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let yaml_path = work_dir.join("sketch.yaml");
    if !yaml_path.is_file() {
        return Ok(());
    }
    // An absolute sketch.yaml symlink resolves outside the project, and the
    // rewrite below would edit the link's target. Leave links alone.
    if yaml_path.symlink_metadata()?.file_type().is_symlink() {
        warnings.push(
            "sketch.yaml is a symlink to a file outside the project — its library paths were not rewritten"
                .into(),
        );
        return Ok(());
    }
    let parsed = match sketch::SketchProject::open(work_dir)?.load_yaml() {
        Ok(y) => y,
        Err(e) => {
            warnings.push(format!(
                "sketch.yaml could not be parsed — its library paths were left unchanged: {e}"
            ));
            return Ok(());
        }
    };

    let mut rewrites: Vec<(String, String)> = Vec::new();
    // The same dir may be pinned by several profiles, but one line rewrite
    // (or one warning) covers every occurrence of the value.
    let mut seen = std::collections::BTreeSet::new();
    for profile in parsed.profiles.values() {
        for lib in &profile.libraries {
            let sketch::LibraryDep::Local { dir } = lib else {
                continue;
            };
            if !seen.insert(dir.clone()) {
                continue;
            }
            let p = Path::new(dir);
            if p.is_absolute() {
                if let Ok(suffix) = p.strip_prefix(src) {
                    let new = dest.join(suffix).to_string_lossy().into_owned();
                    rewrites.push((dir.clone(), new));
                }
                // Outside the source: valid from anywhere, leave it.
                continue;
            }
            // Lexical resolution (sketch::normalize), not canonicalize: the
            // target may not exist, and symlinks stay distinct on purpose.
            let resolved = sketch::normalize(&src.join(p));
            if resolved.starts_with(src) {
                // Copied along with the tree; still resolves from the clone.
                continue;
            }
            if resolved.symlink_metadata().is_ok() {
                // Escapes the sketch but exists: re-aim it so the clone
                // builds like the source. relativize prefers a clean `..`
                // spelling and falls back to the absolute target itself.
                rewrites.push((dir.clone(), sketch::relativize(dest, &resolved)));
            } else {
                warnings.push(format!(
                    "sketch.yaml library path `{dir}` points at nothing on disk ({}) — left unchanged",
                    resolved.display()
                ));
            }
        }
    }
    if rewrites.is_empty() {
        return Ok(());
    }

    let text = std::fs::read_to_string(&yaml_path)?;
    let mut applied = vec![false; rewrites.len()];
    let new_text: String = text
        .split_inclusive('\n')
        .map(|line| {
            if let Some(rest) = line.trim_start().strip_prefix("- dir:") {
                let value = rest.trim();
                if let Some(i) = rewrites.iter().position(|(old, _)| old == value) {
                    applied[i] = true;
                    let (old, new) = &rewrites[i];
                    return line.replacen(old.as_str(), new, 1);
                }
            }
            line.to_string()
        })
        .collect();
    // The textual pass only matches unquoted block-style `- dir:` lines; a
    // planned rewrite it missed (quoted, folded, flow style…) stays pointing
    // at the source and must be called out rather than left stale silently.
    for (i, (old, _)) in rewrites.iter().enumerate() {
        if !applied[i] {
            warnings.push(format!(
                "sketch.yaml library path `{old}` could not be rewritten — update it by hand"
            ));
        }
    }
    std::fs::write(&yaml_path, new_text)?;
    Ok(())
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic source sketch: titled main ino, a sketch.yaml with a port
    /// pin, and files three levels deep.
    fn sample_sketch(parent: &Path, name: &str) -> PathBuf {
        let dir = parent.join(name);
        std::fs::create_dir_all(dir.join("src/sensors/env")).unwrap();
        std::fs::write(
            dir.join(format!("{name}.ino")),
            format!("// {name} — demo\n\nvoid setup() {{}}\nvoid loop() {{}}\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("sketch.yaml"),
            "default_profile: esp32s3\nprofiles:\n  esp32s3:\n    fqbn: esp32:esp32:esp32s3\n    port: /dev/ttyACM0\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/util.h"), "#pragma once\n").unwrap();
        std::fs::write(
            dir.join("src/sensors/env/EnvSensor.cpp"),
            "// three levels down\n",
        )
        .unwrap();
        dir
    }

    /// A minimal sketch whose main ino carries exactly `ino`.
    fn sketch_with_ino(parent: &Path, name: &str, ino: &str) -> PathBuf {
        let dir = parent.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.ino")), ino).unwrap();
        dir
    }

    fn read(p: PathBuf) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    // ----- happy path -----

    #[test]
    fn happy_path_copies_the_tree_and_renames_the_main_ino() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        std::fs::write(src.join(".clang-format"), "BasedOnStyle: LLVM\n").unwrap();
        std::fs::write(src.join(".gitignore"), "custom.txt\n").unwrap();
        let before_ino = read(src.join("Src.ino"));
        let before_yaml = read(src.join("sketch.yaml"));
        let before_gitignore = read(src.join(".gitignore"));

        let parent = tmp.path().join("out");
        let made = clone_project(&src, &parent, "New").unwrap();

        let canon_parent = parent.canonicalize().unwrap();
        assert_eq!(made.dir, canon_parent.join("New"));
        assert_eq!(made.name, "New");
        assert!(made.dir.join("New.ino").is_file());
        assert!(!made.dir.join("Src.ino").exists());
        assert_eq!(
            read(made.dir.join("src/sensors/env/EnvSensor.cpp")),
            "// three levels down\n"
        );
        assert_eq!(read(made.dir.join("src/util.h")), "#pragma once\n");
        // Dotfiles are ordinary project files.
        assert_eq!(read(made.dir.join(".clang-format")), "BasedOnStyle: LLVM\n");
        // No staging residue next to the clone.
        for entry in std::fs::read_dir(&canon_parent).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().starts_with(".bancada-clone-"),
                "staging residue: {name:?}"
            );
        }
        // The source is byte-identical afterwards.
        assert_eq!(read(src.join("Src.ino")), before_ino);
        assert_eq!(read(src.join("sketch.yaml")), before_yaml);
        assert_eq!(read(src.join(".gitignore")), before_gitignore);
    }

    // ----- exclusions -----

    #[test]
    fn exclusions_skip_state_dirs_everywhere_but_not_files_named_build() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        for dir in ["build", ".git", ".bancada/libs", ".claude", "node_modules/pkg"] {
            std::fs::create_dir_all(src.join(dir)).unwrap();
        }
        std::fs::write(src.join("build/junk.o"), "o").unwrap();
        std::fs::write(src.join(".git/MARKER"), "from-source").unwrap();
        std::fs::write(src.join(".bancada/libs/x.h"), "x").unwrap();
        std::fs::write(src.join(".claude/settings.json"), "{}").unwrap();
        std::fs::write(src.join("node_modules/pkg/i.js"), "js").unwrap();
        // Nested skip dir with a sibling that must still arrive.
        std::fs::create_dir_all(src.join("src/deep/build")).unwrap();
        std::fs::write(src.join("src/deep/build/junk.o"), "o").unwrap();
        std::fs::write(src.join("src/deep/keep.cpp"), "keep\n").unwrap();
        // A regular FILE named `build` is project content, not build output.
        std::fs::create_dir_all(src.join("tools")).unwrap();
        std::fs::write(src.join("tools/build"), "#!/bin/sh\n").unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();

        assert!(!made.dir.join("build").exists());
        // git init creates a fresh .git; what must be absent is the source's.
        assert!(!made.dir.join(".git/MARKER").exists());
        assert!(!made.dir.join(".bancada").exists());
        assert!(!made.dir.join(".claude").exists());
        assert!(!made.dir.join("node_modules").exists());
        assert!(!made.dir.join("src/deep/build").exists());
        assert_eq!(read(made.dir.join("src/deep/keep.cpp")), "keep\n");
        assert_eq!(read(made.dir.join("tools/build")), "#!/bin/sh\n");
        assert!(
            made.warnings.iter().any(|w| w.contains("gh restore")),
            "{:?}",
            made.warnings
        );
    }

    #[test]
    fn a_top_level_git_file_from_a_worktree_is_not_copied() {
        // A git-worktree checkout has a top-level .git FILE ("gitdir: …").
        // Copying it would collide with the fresh .git directory (or, on a
        // git-less machine, tie the clone to the source's repository).
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let gitfile = "gitdir: /somewhere/repo/.git/worktrees/Src\n";
        std::fs::write(src.join(".git"), gitfile).unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();

        assert!(
            !made.dir.join(".git").is_file(),
            ".git in the clone must be absent or the fresh init directory"
        );
        assert_eq!(read(src.join(".git")), gitfile, "source .git file untouched");
    }

    #[test]
    fn exclusion_set_is_a_superset_of_the_sketch_walker_skip_list() {
        // Drift guard: a directory the file explorer hides must never be
        // copied into a clone either.
        for name in sketch::SKIP_DIRS {
            assert!(is_skipped_dir(name), "{name} must be excluded from clones");
        }
        for name in CLONE_EXTRA_SKIP_DIRS {
            assert!(is_skipped_dir(name), "{name} must be excluded from clones");
        }
    }

    // ----- retitle -----

    #[test]
    fn retitle_rewrites_a_titled_first_line_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(
            tmp.path(),
            "Src",
            "// Src — demo\nmqtt.publish(\"Src/state\");\n",
        );
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        assert_eq!(
            read(made.dir.join("New.ino")),
            "// New — demo\nmqtt.publish(\"Src/state\");\n"
        );
    }

    #[test]
    fn retitle_leaves_a_longer_word_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let ino = "// Srcocity notes\nint x;\n";
        let src = sketch_with_ino(tmp.path(), "Src", ino);
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        assert_eq!(read(made.dir.join("New.ino")), ino);
    }

    #[test]
    fn retitle_leaves_a_non_comment_first_line_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let ino = "int x;\n// Src\n";
        let src = sketch_with_ino(tmp.path(), "Src", ino);
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        assert_eq!(read(made.dir.join("New.ino")), ino);
    }

    // ----- .gitignore -----

    #[test]
    fn gitignore_is_created_with_the_required_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        assert_eq!(
            read(made.dir.join(".gitignore")),
            "build/\n.bancada/\n.env\nsecrets.h\narduino_secrets.h\n"
        );
    }

    #[test]
    fn gitignore_merge_fixes_a_missing_trailing_newline() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        std::fs::write(src.join(".gitignore"), "custom.txt").unwrap();
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        assert_eq!(
            read(made.dir.join(".gitignore")),
            "custom.txt\nbuild/\n.bancada/\n.env\nsecrets.h\narduino_secrets.h\n"
        );
        // The source's own .gitignore is untouched.
        assert_eq!(read(src.join(".gitignore")), "custom.txt");
    }

    #[test]
    #[cfg(unix)]
    fn gitignore_symlink_is_replaced_by_a_regular_merged_file() {
        // fs::write follows symlinks: writing the merged .gitignore through a
        // recreated link would append our entries into a file OUTSIDE the
        // clone — possibly inside the source project.
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        let shared = tmp.path().join("shared-gitignore");
        std::fs::write(&shared, "shared.txt\n").unwrap();
        std::os::unix::fs::symlink(&shared, src.join(".gitignore")).unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();

        // The link's target is byte-identical afterwards.
        assert_eq!(read(shared), "shared.txt\n");
        // The clone got a REGULAR .gitignore with the merged content.
        let meta = made.dir.join(".gitignore").symlink_metadata().unwrap();
        assert!(meta.file_type().is_file(), "must be a regular file");
        assert_eq!(
            read(made.dir.join(".gitignore")),
            "shared.txt\nbuild/\n.bancada/\n.env\nsecrets.h\narduino_secrets.h\n"
        );
    }

    #[test]
    fn gitignore_variant_spellings_are_not_duplicated() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        std::fs::write(src.join(".gitignore"), "build\n.env\n").unwrap();
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        let text = read(made.dir.join(".gitignore"));
        assert_eq!(text, "build\n.env\n.bancada/\nsecrets.h\narduino_secrets.h\n");
        let build_lines = text
            .lines()
            .filter(|l| matches!(l.trim(), "build" | "build/" | "/build" | "/build/"))
            .count();
        assert_eq!(build_lines, 1, "{text}");
    }

    // ----- sketch.yaml -----

    #[test]
    fn yaml_rewrites_inside_paths_and_preserves_everything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        std::fs::create_dir_all(src.join("libs/EnvSensor")).unwrap();
        std::fs::write(src.join("libs/EnvSensor/lib.h"), "x").unwrap();
        let abs_inside = src.canonicalize().unwrap().join("libs/EnvSensor");
        std::fs::write(
            src.join("sketch.yaml"),
            format!(
                "default_fqbn: esp32:esp32:esp32s3\n\
                 default_profile: esp32s3\n\
                 profiles:\n\
                 \x20 esp32s3:\n\
                 \x20   fqbn: esp32:esp32:esp32s3\n\
                 \x20   programmer: esptool\n\
                 \x20   port: /dev/ttyACM0\n\
                 \x20   libraries:\n\
                 \x20     - PubSubClient (2.8.0)\n\
                 \x20     - dir: libs/EnvSensor\n\
                 \x20     - dir: {}\n\
                 \x20     - dir: /somewhere/else/Lib\n",
                abs_inside.display()
            ),
        )
        .unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        let text = read(made.dir.join("sketch.yaml"));

        // Keys serde's model does not know survive byte-for-byte.
        assert!(text.contains("default_fqbn: esp32:esp32:esp32s3"), "{text}");
        assert!(text.contains("programmer: esptool"), "{text}");
        assert!(text.contains("port: /dev/ttyACM0"), "{text}");
        // The absolute inside-the-project dep now points into the clone…
        let new_abs = made.dir.join("libs/EnvSensor");
        assert!(
            text.contains(&format!("- dir: {}", new_abs.display())),
            "{text}"
        );
        assert!(!text.contains(&abs_inside.display().to_string()), "{text}");
        // …and that path exists in the clone.
        assert!(new_abs.join("lib.h").is_file());
        // Relative-inside and absolute-outside deps are untouched.
        assert!(text.contains("- dir: libs/EnvSensor\n"), "{text}");
        assert!(text.contains("- dir: /somewhere/else/Lib"), "{text}");
    }

    #[test]
    fn escaping_relative_lib_paths_are_rewritten_to_resolve() {
        let tmp = tempfile::tempdir().unwrap();
        // Sketch two levels down; the library outside it, reached with `..`.
        let src = sketch_with_ino(&tmp.path().join("p/a/b"), "Src", "// Src\n");
        std::fs::create_dir_all(tmp.path().join("p/a/libs/X")).unwrap();
        let yaml = "profiles:\n  p:\n    fqbn: a:b:c\n    libraries:\n      - dir: ../../libs/X\n";
        std::fs::write(src.join("sketch.yaml"), yaml).unwrap();

        // The clone lands two levels shallower, so `../../libs/X` dangles
        // unless it is repointed.
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        let text = read(made.dir.join("sketch.yaml"));

        let value = text
            .lines()
            .find_map(|l| l.trim_start().strip_prefix("- dir:"))
            .expect("dir entry must survive")
            .trim()
            .to_string();
        assert_ne!(value, "../../libs/X", "{text}");
        let target = tmp.path().canonicalize().unwrap().join("p/a/libs/X");
        let resolved = {
            let p = Path::new(&value);
            if p.is_absolute() {
                sketch::normalize(p)
            } else {
                sketch::normalize(&made.dir.join(p))
            }
        };
        assert_eq!(resolved, target, "{text}");
        // Only that one value changed; every other byte is intact.
        assert_eq!(text, yaml.replacen("../../libs/X", &value, 1));
        assert!(made.warnings.is_empty(), "{:?}", made.warnings);
    }

    #[test]
    fn a_dangling_relative_lib_path_is_left_with_a_warning() {
        // A target that does not exist was already broken in the source —
        // not ours to invent a destination for.
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(&tmp.path().join("a"), "Src", "// Src\n");
        let yaml = "profiles:\n  p:\n    fqbn: a:b:c\n    libraries:\n      - dir: ../nowhere/Y\n";
        std::fs::write(src.join("sketch.yaml"), yaml).unwrap();

        let made = clone_project(&src, &tmp.path().join("b"), "New").unwrap();

        assert_eq!(read(made.dir.join("sketch.yaml")), yaml, "must stay verbatim");
        assert!(
            made.warnings
                .iter()
                .any(|w| w.contains("../nowhere/Y") && w.contains("left unchanged")),
            "{:?}",
            made.warnings
        );
    }

    #[test]
    fn the_bench_shape_rewrites_every_escaping_relative_that_exists() {
        // The live repro: a sketch nested under Projects/embedded whose deps
        // climb out at three different depths, cloned somewhere shallower.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let src = sketch_with_ino(&root.join("Projects/embedded/soil"), "soil", "// soil\n");
        for lib in [
            "Projects/Arduino/libraries/HomeNode",
            "Projects/embedded/lib/StatusLed",
            "Projects/embedded/soil/shared/Util",
        ] {
            std::fs::create_dir_all(root.join(lib)).unwrap();
        }
        std::fs::create_dir_all(src.join("libs/Inner")).unwrap();
        std::fs::write(
            src.join("sketch.yaml"),
            "profiles:\n\
             \x20 p:\n\
             \x20   fqbn: a:b:c\n\
             \x20   libraries:\n\
             \x20     - HomeNode (1.0.0)\n\
             \x20     - dir: libs/Inner\n\
             \x20     - dir: ../../../Arduino/libraries/HomeNode\n\
             \x20     - dir: ../../lib/StatusLed\n\
             \x20     - dir: ../shared/Util\n",
        )
        .unwrap();

        let made = clone_project(&src, &root.join("out"), "New").unwrap();
        let text = read(made.dir.join("sketch.yaml"));

        // Registry and inside-the-tree deps are untouched.
        assert!(text.contains("- HomeNode (1.0.0)\n"), "{text}");
        assert!(text.contains("- dir: libs/Inner\n"), "{text}");
        // Every escaping relative still reaches its original target.
        let canon_root = root.canonicalize().unwrap();
        let values: Vec<String> = text
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("- dir:"))
            .map(|v| v.trim().to_string())
            .filter(|v| v != "libs/Inner")
            .collect();
        assert_eq!(values.len(), 3, "{text}");
        let resolved: Vec<PathBuf> = values
            .iter()
            .map(|v| {
                let p = Path::new(v);
                if p.is_absolute() {
                    sketch::normalize(p)
                } else {
                    sketch::normalize(&made.dir.join(p))
                }
            })
            .collect();
        for want in [
            canon_root.join("Projects/Arduino/libraries/HomeNode"),
            canon_root.join("Projects/embedded/lib/StatusLed"),
            canon_root.join("Projects/embedded/soil/shared/Util"),
        ] {
            assert!(resolved.contains(&want), "{want:?} not among {resolved:?}");
        }
        assert!(made.warnings.is_empty(), "{:?}", made.warnings);
    }

    #[test]
    #[cfg(unix)]
    fn yaml_symlink_skips_the_rewrite_with_a_warning() {
        // Rewriting through an absolute sketch.yaml symlink would edit the
        // link's target outside the clone; the rewrite must not happen.
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        std::fs::create_dir_all(src.join("libs/EnvSensor")).unwrap();
        let abs_inside = src.canonicalize().unwrap().join("libs/EnvSensor");
        let shared = tmp.path().join("shared-yaml");
        let yaml = format!(
            "profiles:\n  p:\n    fqbn: a:b:c\n    libraries:\n      - dir: {}\n",
            abs_inside.display()
        );
        std::fs::write(&shared, &yaml).unwrap();
        std::os::unix::fs::symlink(&shared, src.join("sketch.yaml")).unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();

        assert_eq!(read(shared), yaml, "link target must be untouched");
        assert!(
            made.dir
                .join("sketch.yaml")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            made.warnings.iter().any(|w| w.contains("not rewritten")),
            "{:?}",
            made.warnings
        );
    }

    #[test]
    fn yaml_quoted_dir_entries_warn_instead_of_going_stale() {
        // The textual pass only matches unquoted block-style `- dir:` lines;
        // a quoted entry stays pointing at the source, which deserves a
        // warning rather than silence.
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        std::fs::create_dir_all(src.join("libs/EnvSensor")).unwrap();
        let abs_inside = src.canonicalize().unwrap().join("libs/EnvSensor");
        let yaml = format!(
            "profiles:\n  p:\n    fqbn: a:b:c\n    libraries:\n      - dir: \"{}\"\n",
            abs_inside.display()
        );
        std::fs::write(src.join("sketch.yaml"), &yaml).unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();

        assert_eq!(read(made.dir.join("sketch.yaml")), yaml, "must stay verbatim");
        assert!(
            made.warnings
                .iter()
                .any(|w| w.contains("could not be rewritten") && w.contains("libs/EnvSensor")),
            "{:?}",
            made.warnings
        );
    }

    #[test]
    fn yaml_garbage_is_copied_verbatim_with_a_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sketch_with_ino(tmp.path(), "Src", "// Src\n");
        let garbage = "{ this is : not yaml\n";
        std::fs::write(src.join("sketch.yaml"), garbage).unwrap();
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        assert_eq!(read(made.dir.join("sketch.yaml")), garbage);
        assert!(
            made.warnings.iter().any(|w| w.contains("could not be parsed")),
            "{:?}",
            made.warnings
        );
    }

    // ----- errors -----

    #[test]
    fn rejects_an_invalid_project_name() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let err = clone_project(&src, &tmp.path().join("out"), "My Project")
            .unwrap_err()
            .to_string();
        assert!(err.contains("instead of spaces"), "{err}");
    }

    #[test]
    fn refuses_to_clone_onto_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let err = clone_project(&src, tmp.path(), "Src")
            .unwrap_err()
            .to_string();
        assert!(err.contains("source project"), "{err}");
        assert!(src.join("Src.ino").is_file(), "source must survive");
    }

    #[test]
    fn refuses_an_existing_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let parent = tmp.path().join("out");
        std::fs::create_dir_all(parent.join("New")).unwrap();
        let err = clone_project(&src, &parent, "New").unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
    }

    #[test]
    fn refuses_a_case_only_clash() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let parent = tmp.path().join("out");
        std::fs::create_dir_all(parent.join("new")).unwrap();
        let err = clone_project(&src, &parent, "New").unwrap_err().to_string();
        assert!(err.contains("differs only in case"), "{err}");
    }

    #[test]
    fn refuses_a_missing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let err = clone_project(&tmp.path().join("nope"), &tmp.path().join("out"), "New")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a sketch folder"), "{err}");
    }

    #[test]
    fn refuses_a_folder_without_a_main_ino() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Src");
        std::fs::create_dir_all(&src).unwrap();
        let err = clone_project(&src, &tmp.path().join("out"), "New")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a sketch folder"), "{err}");
        assert!(err.contains("Src.ino"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn refuses_a_symlinked_main_ino() {
        // is_file() follows links, so a symlinked main ino used to pass
        // validation and then break the retitle with a raw io error.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(tmp.path().join("real.ino"), "// Src\n").unwrap();
        std::os::unix::fs::symlink("../real.ino", src.join("Src.ino")).unwrap();

        let err = clone_project(&src, &tmp.path().join("out"), "New")
            .unwrap_err()
            .to_string();
        assert!(err.contains("symlink"), "{err}");
    }

    #[test]
    fn refuses_a_destination_inside_the_source() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let err = clone_project(&src, &src.join("nested"), "New")
            .unwrap_err()
            .to_string();
        assert!(err.contains("inside the source"), "{err}");
    }

    #[test]
    fn refuses_when_the_source_has_a_file_named_like_the_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        std::fs::write(src.join("New.ino"), "// secondary sketch file\n").unwrap();
        let err = clone_project(&src, &tmp.path().join("out"), "New")
            .unwrap_err()
            .to_string();
        assert!(err.contains("New.ino"), "{err}");
    }

    // ----- atomicity -----

    #[test]
    fn a_file_squatting_on_the_staging_path_fails_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let parent = tmp.path().join("out");
        std::fs::create_dir_all(&parent).unwrap();
        // remove_dir_all refuses a regular file, so creation fails
        // deterministically without chmod tricks.
        std::fs::write(parent.join(".bancada-clone-New"), "in the way").unwrap();

        assert!(clone_project(&src, &parent, "New").is_err());
        assert!(!parent.join("New").exists(), "destination must not appear");
        assert_eq!(
            read(parent.join(".bancada-clone-New")),
            "in the way",
            "an unrelated file must not be deleted"
        );
    }

    #[test]
    fn stale_staging_junk_is_reclaimed() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let parent = tmp.path().join("out");
        std::fs::create_dir_all(parent.join(".bancada-clone-New/junk")).unwrap();
        std::fs::write(parent.join(".bancada-clone-New/junk/x"), "old").unwrap();

        let made = clone_project(&src, &parent, "New").unwrap();
        assert!(made.dir.join("New.ino").is_file());
        assert!(!made.dir.join("junk").exists(), "stale content leaked in");
        assert!(!parent.join(".bancada-clone-New").exists());
    }

    // ----- symlinks -----

    #[test]
    #[cfg(unix)]
    fn relative_symlinks_are_recreated_with_a_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        std::os::unix::fs::symlink("src/util.h", src.join("alias.h")).unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        let link = made.dir.join("alias.h");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("src/util.h"));
        assert!(
            made.warnings
                .iter()
                .any(|w| w.contains("alias.h") && w.contains("symlink")),
            "{:?}",
            made.warnings
        );
    }

    #[test]
    #[cfg(unix)]
    fn dangling_symlinks_do_not_stop_the_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        std::os::unix::fs::symlink("nowhere", src.join("gone")).unwrap();

        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        assert_eq!(
            std::fs::read_link(made.dir.join("gone")).unwrap(),
            PathBuf::from("nowhere")
        );
        // The rest of the copy still completed.
        assert!(made.dir.join("src/util.h").is_file());
        assert!(
            made.warnings.iter().any(|w| w.contains("gone")),
            "{:?}",
            made.warnings
        );
    }

    #[test]
    fn clean_clone_has_no_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();
        // Hermetic on a git-less machine: a git-not-found warning is about
        // the environment, not this source tree.
        let non_git: Vec<_> = made
            .warnings
            .iter()
            .filter(|w| !w.contains("git"))
            .collect();
        assert!(non_git.is_empty(), "{:?}", made.warnings);
    }

    // ----- git -----

    #[test]
    #[ignore = "runs real git"]
    fn fresh_git_repo_has_an_unborn_head_and_the_source_gains_none() {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample_sketch(tmp.path(), "Src");
        let made = clone_project(&src, &tmp.path().join("out"), "New").unwrap();

        assert!(made.dir.join(".git").is_dir());
        // Unborn HEAD: the repository exists but holds no commits yet.
        let out = std::process::Command::new("git")
            .args(["-C", made.dir.to_str().unwrap(), "rev-parse", "--verify", "HEAD"])
            .output()
            .unwrap();
        assert!(!out.status.success(), "HEAD must be unborn (no commits)");
        assert!(!src.join(".git").exists(), "source must not gain a repo");
    }
}
