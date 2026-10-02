//! Exercise the real upload path without flashing hardware.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use bancada_core::{cli::ArduinoCli, sketch::SketchProject};

fn fake_cli(dir: &std::path::Path) -> ArduinoCli {
    let bin = dir.join("arduino-cli");
    // Print each argument separately and fail the build: the repair must
    // precede compilation and remain saved even when compilation fails.
    std::fs::write(&bin, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 1\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    ArduinoCli::new(bin.to_string_lossy())
}

#[test]
fn upload_repairs_the_selected_profile_before_building_and_keeps_all_pins() {
    let dir = tempfile::tempdir().unwrap();
    let project = SketchProject::open(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("sketch.yaml"),
        r#"
default_profile: native
default_port: /dev/ttyACM0
profiles:
  native:
    fqbn: esp32:esp32:esp32s3:FlashSize=16M,CDCOnBoot=default,PSRAM=opi
    port: /dev/ttyACM0
    programmer: custom
    platforms:
      - platform: esp32:esp32 (3.3.0)
        platform_index_url: https://example.com/custom-index.json
        custom: keep
    libraries:
      - ArduinoJson (7.4.2)
      - dependency: Dep (1.0.0)
      - dir: ../local
  other:
    fqbn: esp32:esp32:esp32c6
"#,
    )
    .unwrap();
    let mut expected = project.load_yaml().unwrap();
    expected.profiles.get_mut("native").unwrap().fqbn =
        "esp32:esp32:esp32s3:FlashSize=16M,PSRAM=opi,CDCOnBoot=cdc".into();
    let mut lines = Vec::new();
    let result = fake_cli(dir.path())
        .upload(
            dir.path().to_str().unwrap(),
            Some("native"),
            Some("arduino:avr:uno"),
            "/dev/ttyACM0",
            |line| lines.push(line.line),
        )
        .unwrap();
    assert!(!result.success);
    assert_eq!(project.load_yaml().unwrap(), expected);
    assert!(lines[0].contains("enabled automatically"));
    assert!(lines.windows(2).any(|args| args == ["--profile", "native"]));
    assert!(!lines.iter().any(|line| line == "--fqbn"));

    // A repeat upload neither rewrites the profile nor announces a repair.
    let before = std::fs::read(project.dir.join("sketch.yaml")).unwrap();
    lines.clear();
    fake_cli(dir.path())
        .upload(
            dir.path().to_str().unwrap(),
            Some("native"),
            None,
            "/dev/ttyACM0",
            |line| lines.push(line.line),
        )
        .unwrap();
    assert_eq!(
        std::fs::read(project.dir.join("sketch.yaml")).unwrap(),
        before
    );
    assert!(!lines
        .iter()
        .any(|line| line.contains("enabled automatically")));
}

#[test]
fn ad_hoc_upload_passes_the_corrected_fqbn_to_compile() {
    let dir = tempfile::tempdir().unwrap();
    let mut lines = Vec::new();
    fake_cli(dir.path())
        .upload(
            dir.path().to_str().unwrap(),
            None,
            Some("esp32:esp32:esp32c6"),
            "/dev/ttyACM0",
            |line| lines.push(line.line),
        )
        .unwrap();
    assert!(lines
        .windows(2)
        .any(|args| args == ["--fqbn", "esp32:esp32:esp32c6:CDCOnBoot=cdc"]));
    assert!(!dir.path().join("sketch.yaml").exists());
}

#[test]
fn bridge_upload_does_not_rewrite_the_profile() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = "# preserve this comment\nprofiles:\n  s3:\n    fqbn: esp32:esp32:esp32s3\n";
    std::fs::write(dir.path().join("sketch.yaml"), yaml).unwrap();
    let mut lines = Vec::new();
    fake_cli(dir.path())
        .upload(
            dir.path().to_str().unwrap(),
            Some("s3"),
            None,
            "/dev/ttyUSB0",
            |line| lines.push(line.line),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sketch.yaml")).unwrap(),
        yaml
    );
    assert!(!lines
        .iter()
        .any(|line| line.contains("enabled automatically")));
}

#[test]
fn missing_profile_stops_before_upload_runs() {
    let dir = tempfile::tempdir().unwrap();
    let mut lines = Vec::new();
    let result = fake_cli(dir.path()).upload(
        dir.path().to_str().unwrap(),
        Some("missing"),
        Some("esp32:esp32:esp32s3"),
        "/dev/ttyACM0",
        |line| lines.push(line.line),
    );
    assert!(result.unwrap_err().to_string().contains("not found"));
    assert!(lines.is_empty());
}
