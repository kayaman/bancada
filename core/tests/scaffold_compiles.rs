//! Opt-in proof that a freshly scaffolded library actually builds.
//!
//! The unit tests in `library.rs` assert the generated *text*; only a real
//! compile proves the stub class and its bundled example agree. That needs
//! `arduino-cli` plus an installed core, so the test is `#[ignore]`d.
//!
//! ```text
//! cargo test -p bancada-core --test scaffold_compiles -- --ignored --nocapture
//! ```
//!
//! Override the target with `BANCADA_TEST_FQBN` (default `arduino:avr:uno`,
//! chosen because it is small and fast; the stub touches only `millis()` and
//! `Serial`, so it is portable to any core).

use bancada_core::library::{create_library, LibrarySpec};

#[test]
#[ignore = "needs arduino-cli and an installed core"]
fn scaffolded_example_compiles() {
    let fqbn = std::env::var("BANCADA_TEST_FQBN").unwrap_or_else(|_| "arduino:avr:uno".to_string());

    let tmp = tempfile::tempdir().unwrap();
    let libs = tmp.path().join("libraries");
    let made = create_library(
        &libs,
        &LibrarySpec {
            name: "Blink Timer".into(),
            ..Default::default()
        },
    )
    .expect("scaffold");

    let example = made.dir.join("examples/Blink_Timer");
    assert!(example.join("Blink_Timer.ino").is_file());

    let out = std::process::Command::new("arduino-cli")
        .args(["compile", "--fqbn", &fqbn, "--library"])
        .arg(&made.dir)
        .arg(&example)
        .output()
        .expect("arduino-cli on PATH");

    assert!(
        out.status.success(),
        "compiling the scaffolded example for {fqbn} failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
