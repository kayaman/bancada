//! Opt-in proof that a bancada-written esp8266 profile actually builds.
//!
//! `arduino-cli compile -m <profile>` is hermetic: it resolves platforms from
//! Arduino's default index plus whatever `platform_index_url:` the profile
//! itself names. It ignores `board_manager.additional_urls` and will not fall
//! back on an installed core. `esp32` is in the default index and `esp8266` is
//! not, so two identically-written profiles behave differently — and every
//! esp8266 profile bancada wrote before `ensure_platform_index_urls` existed
//! died with "Platform not found: platform not installed".
//!
//! The unit tests cover the mapping with fixtures; only a real compile proves
//! the repair produces something arduino-cli accepts. That needs the CLI plus
//! an installed esp8266 core, so this is `#[ignore]`d.
//!
//! ```text
//! cargo test -p bancada-core --test profile_index_url_real -- --ignored --nocapture
//! ```

use bancada_core::cli::ArduinoCli;
use bancada_core::sketch::SketchProject;

const SKETCH: &str = "void setup() {}\nvoid loop() {}\n";

/// A profile pinning esp8266 with NO index url — exactly what bancada used to
/// write, and what `~/Projects/rain-sensor` was hit by on 2026-08-25.
const UNREPAIRED: &str = "\
default_profile: nodemcuv2
profiles:
  nodemcuv2:
    fqbn: esp8266:esp8266:nodemcuv2
    platforms:
      - platform: esp8266:esp8266 (3.1.2)
";

#[test]
#[ignore = "needs arduino-cli and an installed esp8266 core"]
fn the_index_url_repair_is_what_makes_an_esp8266_profile_build() {
    let cli = ArduinoCli::default();

    let installed = cli.core_list().unwrap_or_default();
    if !installed.iter().any(|p| p.id == "esp8266:esp8266") {
        eprintln!("skipping: esp8266:esp8266 is not installed");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("blink");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("blink.ino"), SKETCH).unwrap();
    std::fs::write(dir.join("sketch.yaml"), UNREPAIRED).unwrap();
    let dir_s = dir.to_string_lossy().to_string();

    // Before: the profile is unbuildable even though the core is installed.
    let before = cli
        .compile(&dir_s, Some("nodemcuv2"), None, &[], |_| {})
        .expect("compile should run, even if the build fails");
    assert!(
        !before.success,
        "a profile with no platform_index_url was expected to fail resolution, \
         but it built — has arduino-cli stopped being hermetic?"
    );

    // The repair, exactly as the Tauri layer performs it.
    let proj = SketchProject::open(&dir).unwrap();
    let indexes = cli.platform_indexes();
    let y = proj.ensure_platform_index_urls(&indexes).unwrap();
    let url = y.profiles["nodemcuv2"].platforms[0]
        .platform_index_url
        .as_deref();
    assert!(
        url.is_some_and(|u| u.contains("esp8266")),
        "expected an esp8266 index url to be filled in, got {url:?} \
         (is the esp8266 index in board_manager.additional_urls and downloaded?)"
    );

    // After: the same profile, same core, now builds.
    let after = cli
        .compile(&dir_s, Some("nodemcuv2"), None, &[], |_| {})
        .expect("compile should run");
    assert!(
        after.success,
        "the repaired profile still failed to build — the url was written but \
         arduino-cli did not accept it"
    );
}
