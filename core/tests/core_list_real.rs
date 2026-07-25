//! Opt-in proof that the platform types still match what `arduino-cli` emits.
//!
//! The unit tests in `types.rs` parse *captured* JSON, which pins the shape we
//! designed against but cannot notice when a new arduino-cli release renames or
//! restructures a field. This test runs the real binary, so a contract drift
//! shows up here rather than as an opaque parse error in the Boards panel.
//!
//! ```text
//! cargo test -p bancada-core --test core_list_real -- --ignored --nocapture
//! ```
//!
//! Read-only by design: it never installs, upgrades or removes anything, so it
//! is safe to run against a real toolchain. The install path cannot be tested
//! this way — it would download hundreds of megabytes and mutate the machine.

use bancada_core::boards::{self, CoreStatus};
use bancada_core::cli::ArduinoCli;

#[test]
#[ignore = "needs arduino-cli"]
fn core_list_parses_and_derives_a_sane_view() {
    let cli = ArduinoCli::default();
    let platforms = cli.core_list().expect("core list --json");
    assert!(
        !platforms.is_empty(),
        "no platforms installed — install one, e.g. `arduino-cli core install arduino:avr`"
    );

    for p in &platforms {
        // Every installed platform must have a parseable id and a version.
        boards::parse_core_id(&p.id).unwrap_or_else(|e| panic!("bad id {:?}: {e}", p.id));
        assert!(
            !p.installed_version.is_empty(),
            "{} came from `core list` but reports no installed version",
            p.id
        );

        let v = boards::view(p);
        assert_ne!(
            v.status,
            CoreStatus::NotInstalled,
            "{} is installed, so its status must not be NotInstalled",
            p.id
        );
        assert!(!v.name.is_empty(), "{} produced an empty display name", p.id);

        // `releases` is the field most likely to be restructured upstream, and
        // the version picker is built entirely from its keys.
        assert!(
            !p.releases.is_empty(),
            "{} reported no releases — the `releases` map may have changed shape",
            p.id
        );
        assert!(
            v.versions.contains(&p.installed_version),
            "{}: installed {} is missing from the offered versions {:?}",
            p.id,
            p.installed_version,
            v.versions
        );

        // Newest-first ordering is what the picker relies on.
        if v.versions.len() > 1 {
            let newest = &v.versions[0];
            assert!(
                boards::sorted_versions(p).first() == Some(newest),
                "{}: version ordering is not stable",
                p.id
            );
        }
        println!(
            "{} {} ({:?}) — {} versions, {} boards",
            v.id,
            v.installed_version,
            v.status,
            v.versions.len(),
            v.boards.len()
        );
    }
}

#[test]
#[ignore = "needs arduino-cli"]
fn core_search_parses_uninstalled_platforms() {
    let cli = ArduinoCli::default();
    // "esp32" matches several platforms across indexes on any normal install.
    let platforms = cli.core_search("esp32").expect("core search --json");
    assert!(!platforms.is_empty(), "`core search esp32` returned nothing");

    // The point of this test: search reports platforms that are *not* installed,
    // which is signalled by an empty `installed_version` rather than a missing
    // key — so the view must classify at least one as NotInstalled without the
    // parse failing.
    let views: Vec<_> = platforms.iter().map(boards::view).collect();
    assert!(
        views.iter().any(|v| v.status == CoreStatus::NotInstalled),
        "expected at least one uninstalled match among {:?}",
        views.iter().map(|v| &v.id).collect::<Vec<_>>()
    );
    for v in &views {
        assert!(!v.id.is_empty());
        println!("{} ({:?}) — latest {}", v.id, v.status, v.latest_version);
    }
}
