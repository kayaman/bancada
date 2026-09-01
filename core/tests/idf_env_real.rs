//! Live ESP-IDF environment resolution, against whatever is really installed.
//!
//! Opt-in with `BANCADA_IDF_LIVE=1`, in the same spirit as the agent and
//! compile suites: it needs a real ESP-IDF install, so it must not run in a
//! checkout that has none. Set `BANCADA_IDF_VERSION` to pick a specific
//! install when more than one is present.
//!
//! Run with:
//!   BANCADA_IDF_LIVE=1 cargo test -p bancada-core --test idf_env_real -- --nocapture

use std::path::{Path, PathBuf};

use bancada_core::idfenv;

fn registry_path() -> PathBuf {
    std::env::var_os("BANCADA_IDF_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").expect("HOME"))
                .join(".espressif/tools/eim_idf.json")
        })
}

fn enabled() -> bool {
    std::env::var("BANCADA_IDF_LIVE").as_deref() == Ok("1")
}

fn stamp(p: &Path) -> (std::time::SystemTime, u64) {
    let m = std::fs::metadata(p).expect("registry metadata");
    (m.modified().expect("mtime"), m.len())
}

#[test]
fn a_real_install_activates_and_yields_a_usable_toolchain() {
    if !enabled() {
        eprintln!("skipped: set BANCADA_IDF_LIVE=1 to run");
        return;
    }
    let reg_path = registry_path();
    let before = stamp(&reg_path);

    let json = std::fs::read_to_string(&reg_path).expect("read registry");
    let reg = idfenv::parse_registry(&json, &reg_path).expect("parse registry");
    let prefer = std::env::var("BANCADA_IDF_VERSION").ok();
    let install = idfenv::select_install(&reg, prefer.as_deref()).expect("select install");
    eprintln!("using ESP-IDF {} at {}", install.name, install.path.display());

    idfenv::validate_install(install).expect("install is complete");

    let inherited = std::env::var("PATH").unwrap_or_default();
    let env = idfenv::activate(install, &inherited).expect("activate");

    // The variable whose absence makes idf.py die inside Python with a
    // TypeError rather than a message.
    assert!(
        env.contains_key("ESP_IDF_VERSION"),
        "activation must publish ESP_IDF_VERSION"
    );
    assert_eq!(
        PathBuf::from(&env["IDF_PATH"]),
        install.path,
        "script and registry must agree"
    );

    // The inherited PATH has to survive, or the child loses /usr/bin.
    let path = &env["PATH"];
    for dir in inherited.split(':').filter(|d| !d.is_empty()).take(3) {
        assert!(path.contains(dir), "inherited PATH entry {dir} was lost");
    }

    // A cross-compiler must actually be reachable — this is what proves the
    // env map is sufficient, not merely well-formed.
    let found_gcc = path.split(':').filter(|d| !d.is_empty()).any(|d| {
        Path::new(d).join("riscv32-esp-elf-gcc").exists()
            || Path::new(d).join("xtensa-esp32s3-elf-gcc").exists()
    });
    assert!(found_gcc, "no ESP cross-compiler on the activated PATH");

    // The whole reason we use `-e` instead of sourcing: sourcing would run the
    // script's trailing `eim select` and rewrite the registry we just read.
    assert_eq!(
        stamp(&reg_path),
        before,
        "activation must not modify the registry"
    );
}

#[test]
fn a_bogus_registry_path_reports_that_esp_idf_is_not_installed() {
    // Runs unconditionally: it needs no install.
    let p = PathBuf::from("/nonexistent/eim_idf.json");
    let err = std::fs::read_to_string(&p).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
