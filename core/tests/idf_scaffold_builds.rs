//! Opt-in proof that a project Bancada scaffolds actually builds.
//!
//! `idfproject.rs` writes an ESP-IDF tree by hand rather than calling
//! `idf.py create-project` — which is the right trade (creation must not need
//! an install) but moves the burden of correctness onto us. Its unit tests
//! prove the files are *written*; only this proves ESP-IDF agrees they are a
//! project. That is the same claim `new_project_builds.rs` makes for the
//! Arduino side, and the same reason it exists.
//!
//! Every starter is built, not just one: they differ in what they include
//! (NVS pulls in `nvs_flash`, Tasks pulls in FreeRTOS queues), so a template
//! that scaffolds cleanly and fails to link is exactly the bug this catches.
//!
//! ```text
//! BANCADA_IDF_LIVE=1 cargo test -p bancada-core --test idf_scaffold_builds -- --nocapture
//! ```

use std::path::PathBuf;

use bancada_core::{boardprofile, idf, idfenv, idfproject};

fn enabled() -> bool {
    std::env::var("BANCADA_IDF_LIVE").as_deref() == Ok("1")
}

/// The same two record files the app consults, with the same overrides.
fn registry_paths() -> idfenv::RegistryPaths {
    let home = std::env::var("HOME").expect("HOME");
    let mut paths = idfenv::RegistryPaths::under_home(std::path::Path::new(&home));
    if let Some(p) = std::env::var_os("BANCADA_IDF_REGISTRY") {
        paths.installer = PathBuf::from(p);
    }
    if let Some(p) = std::env::var_os("BANCADA_IDF_TOOLS_PATH") {
        paths.tools_dir = PathBuf::from(p);
    }
    paths
}

fn resolve() -> idf::IdfCli {
    let reg = idfenv::discover(&registry_paths()).expect("discover an install");
    let prefer = std::env::var("BANCADA_IDF_VERSION").ok();
    let install = idfenv::select_install(&reg, prefer.as_deref()).expect("select");
    let env =
        idfenv::activate(install, &std::env::var("PATH").unwrap_or_default()).expect("activate");
    let python = idfenv::venv_python(&env, &install.python);
    idf::IdfCli::new(python, install.path.clone(), env)
}

/// The chip every starter is built for. Small and fast; the templates are
/// target-independent by construction, which is what makes one enough.
const TARGET: &str = "esp32c3";

#[test]
fn every_starter_builds_as_scaffolded() {
    if !enabled() {
        eprintln!("skipped: set BANCADA_IDF_LIVE=1 to run");
        return;
    }
    let cli = resolve();
    let work = std::env::temp_dir().join(format!("bancada-scaffold-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();

    for t in idfproject::IdfTemplate::ALL {
        let name = format!("starter_{}", t.id());
        let s = idfproject::scaffold_idf_project(&work, &name, *t, Some(TARGET), None)
            .unwrap_or_else(|e| panic!("scaffolding {} failed: {e}", t.id()));
        let dir = std::path::Path::new(&s.dir);

        // No set-target call: the whole point of writing CONFIG_IDF_TARGET
        // into sdkconfig.defaults is that the first configure picks it up.
        // If that assumption is wrong, this build fails for the default
        // target instead — which is precisely what we want to find out.
        let mut log = Vec::new();
        let r = cli
            .build(dir, |l| log.push(l.line))
            .unwrap_or_else(|e| panic!("build of {} could not run: {e}", t.id()));
        assert!(
            r.success,
            "{} failed to build:\n{}",
            t.id(),
            idf::idf_failure_excerpt(&log).join("\n")
        );

        let bin = dir.join(format!("build/{name}.bin"));
        assert!(bin.exists(), "{}: no binary at {}", t.id(), bin.display());

        // The target actually configured, read back from the real sdkconfig
        // the build produced rather than from the defaults we wrote.
        let sdkconfig = std::fs::read_to_string(dir.join("sdkconfig")).expect("sdkconfig");
        assert_eq!(
            idf::parse_sdkconfig_target(&sdkconfig).as_deref(),
            Some(TARGET),
            "{}: sdkconfig.defaults did not carry the target through",
            t.id()
        );

        eprintln!(
            "{:>6}: {} bytes",
            t.id(),
            std::fs::metadata(&bin).unwrap().len()
        );
    }

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_board_chosen_at_creation_survives_into_the_built_project() {
    if !enabled() {
        eprintln!("skipped: set BANCADA_IDF_LIVE=1 to run");
        return;
    }
    let cli = resolve();
    let work = std::env::temp_dir().join(format!("bancada-scaffold-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();

    // A WS2812 board: the case where blink's *comment* matters as much as its
    // pin, because a plain toggle will not light it.
    let board = boardprofile::Board::from_id("esp32-c6-devkitc-1").expect("board");
    let s = idfproject::scaffold_idf_project(
        &work,
        "boarded",
        idfproject::IdfTemplate::Blink,
        Some("esp32c6"),
        Some(board),
    )
    .expect("scaffold");
    let dir = std::path::Path::new(&s.dir);

    let mut log = Vec::new();
    let r = cli.build(dir, |l| log.push(l.line)).expect("build ran");
    assert!(
        r.success,
        "boarded blink failed:\n{}",
        idf::idf_failure_excerpt(&log).join("\n")
    );

    // The marker survived a real configure — the reason it is a comment and
    // not a CONFIG_ key is that kconfgen would otherwise nag about it, so
    // this also proves it did not.
    assert_eq!(
        bancada_core::project::recorded_board(dir).map(|b| b.id),
        Some("esp32-c6-devkitc-1")
    );
    let noise: Vec<&String> = log
        .iter()
        .filter(|l| l.contains("unknown kconfig symbol") && l.contains("bancada"))
        .collect();
    assert!(noise.is_empty(), "kconfgen complained: {noise:?}");
}
