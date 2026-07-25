//! Opt-in proof that a pinned alias really resolves, fetches and builds.
//!
//! Needs network and `git`, so it is `#[ignore]`d:
//!
//! ```text
//! cargo test -p bancada-core --test gh_fetch -- --ignored --nocapture
//! ```

use bancada_core::ghlib::{fetch_subtree, list_remote_tags, parse_alias, Manifest, ManifestEntry};

const ALIAS: &str = "@kayaman/Arduino/libraries/HomeNode";
const REF: &str = "HomeNode/v1.1.0";

#[test]
#[ignore = "needs network and git"]
fn lists_versions_for_a_real_repo() {
    let a = parse_alias(ALIAS).unwrap();
    let tags = list_remote_tags(&a.url(), &a.name).expect("ls-remote");

    assert!(!tags.is_empty(), "expected tags");
    // The library's own namespace must rank first.
    assert!(
        tags[0].name.starts_with("HomeNode/"),
        "expected a HomeNode/* tag first, got {:?}",
        tags.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    // Commits, not annotated tag objects: every SHA must be a real commit, so
    // the same tag listed twice by ls-remote collapses to one entry.
    assert_eq!(
        tags.iter().filter(|t| t.name == REF).count(),
        1,
        "the ^{{}} row should have collapsed into one entry"
    );
}

#[test]
#[ignore = "needs network and git"]
fn fetches_vendors_and_pins_a_library() {
    let a = parse_alias(ALIAS).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let sketch = tmp.path();
    let dest = sketch.join(a.vendor_rel());

    let commit = fetch_subtree(&a, REF, &dest, None).expect("fetch");
    assert_eq!(commit.len(), 40, "expected a full SHA, got {commit:?}");

    // It is a library...
    assert!(dest.join("library.properties").is_file());
    assert!(dest.join("src").is_dir());
    // ...and not a nested git repo inside the user's sketch.
    assert!(!dest.join(".git").exists(), ".git must not be vendored");
    // The scratch clone is gone.
    assert!(
        !sketch.join(".bancada/libs/.fetch-HomeNode").exists(),
        "scratch clone left behind"
    );

    // Re-fetching at the same ref is the update path and must succeed, not
    // refuse the way create_library does.
    let again = fetch_subtree(&a, REF, &dest, Some(&commit)).expect("re-fetch");
    assert_eq!(again, commit);

    // A wrong expected commit means the tag moved: refuse, naming both.
    let bogus = "0".repeat(40);
    let err = fetch_subtree(&a, REF, &dest, Some(&bogus))
        .unwrap_err()
        .to_string();
    assert!(err.contains("moved"), "{err}");

    // Manifest round-trips through a real sketch dir.
    let mut m = Manifest::load(sketch).unwrap();
    m.upsert(ManifestEntry {
        alias: a.canonical(),
        git_ref: REF.into(),
        commit: commit.clone(),
        vendor: a.vendor_rel(),
    });
    m.save(sketch).unwrap();
    assert_eq!(Manifest::load(sketch).unwrap().libraries[0].commit, commit);
}

#[test]
#[ignore = "needs network, git, and an installed core"]
fn a_fetched_librarys_example_compiles() {
    let fqbn =
        std::env::var("BANCADA_TEST_FQBN").unwrap_or_else(|_| "esp32:esp32:esp32".to_string());

    let a = parse_alias(ALIAS).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join(a.vendor_rel());
    fetch_subtree(&a, REF, &dest, None).expect("fetch");

    // Compile the library's own first example, which is the closest thing to a
    // real consumer of it.
    let examples = dest.join("examples");
    let Some(example) = std::fs::read_dir(&examples)
        .ok()
        .and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.is_dir()))
    else {
        eprintln!("no examples/ in the fetched library — skipping the compile check");
        return;
    };

    // A library may ship a credentials header as `secrets.h.example` and
    // gitignore the real one, as this one does. Seeding it is what a user does
    // before their first build, so the test does it too rather than asserting a
    // failure it caused itself.
    for header in ["secrets.h", "arduino_secrets.h"] {
        let example_file = dest.join(format!("{header}.example"));
        if example_file.is_file() && !example.join(header).exists() {
            std::fs::copy(&example_file, example.join(header)).expect("seed credentials header");
        }
    }

    let out = std::process::Command::new("arduino-cli")
        .args(["compile", "--fqbn", &fqbn, "--library"])
        .arg(&dest)
        .arg(&example)
        .output()
        .expect("arduino-cli on PATH");

    assert!(
        out.status.success(),
        "compiling {} for {fqbn} failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        example.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
