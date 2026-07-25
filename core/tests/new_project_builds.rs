//! Opt-in proof that a project Bancada creates actually builds.
//!
//! Drives the same three arduino-cli commands the `create_project` flow does,
//! then compiles the result *through its profile* — which is the real claim:
//! a new project must build with no further input.
//!
//! ```text
//! cargo test -p bancada-core --test new_project_builds -- --ignored --nocapture
//! ```
//!
//! `BANCADA_TEST_FQBN` overrides the board (default `arduino:avr:uno`, small and
//! fast). The platform must already be installed, since `profile create`
//! resolves its version from what is on disk.

use bancada_core::cli::ArduinoCli;
use bancada_core::project::{profile_name_for_fqbn, validate_project_name};

#[test]
#[ignore = "needs arduino-cli and an installed core"]
fn a_created_project_compiles_through_its_profile() {
    let fqbn = std::env::var("BANCADA_TEST_FQBN").unwrap_or_else(|_| "arduino:avr:uno".to_string());
    let cli = ArduinoCli::default();

    let tmp = tempfile::tempdir().unwrap();
    let name = validate_project_name("Demo_Project").unwrap();
    let profile = profile_name_for_fqbn(&fqbn);
    let dir = tmp.path().join(&name);

    cli.sketch_new(&dir).expect("sketch new");
    assert!(
        dir.join(format!("{name}.ino")).is_file(),
        "expected {name}.ino"
    );

    cli.profile_create(&dir, &profile, &fqbn, true)
        .expect("profile create");
    let yaml = std::fs::read_to_string(dir.join("sketch.yaml")).expect("sketch.yaml");
    assert!(yaml.contains(&fqbn), "{yaml}");
    assert!(
        yaml.contains(&format!("default_profile: {profile}")),
        "{yaml}"
    );
    // The platform version must be pinned, not left floating — that is the whole
    // point of driving `profile create` instead of writing the YAML by hand.
    assert!(
        yaml.contains('(') && yaml.contains(')'),
        "expected a pinned platform version in:\n{yaml}"
    );

    // A registry library, pinned, with its dependencies resolved by the engine.
    cli.profile_lib_add(&dir, &profile, "ArduinoJson@7.4.2")
        .expect("profile lib add");
    let yaml = std::fs::read_to_string(dir.join("sketch.yaml")).unwrap();
    assert!(yaml.contains("ArduinoJson (7.4.2)"), "{yaml}");

    // The real assertion: it builds through the profile, with no --fqbn.
    let mut lines = Vec::new();
    let result = cli
        .compile(&dir.to_string_lossy(), Some(&profile), None, &[], |l| {
            lines.push(l.line)
        })
        .expect("compile");
    assert!(
        result.success,
        "compiling the new project through profile `{profile}` failed:\n{}",
        lines.join("\n")
    );
}
