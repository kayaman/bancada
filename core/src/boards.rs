//! Derived views over `arduino-cli core list` / `core search` output.
//!
//! Pure: no subprocess, no network. Everything here is computed from a
//! [`Platform`] the CLI layer already fetched, so it is unit testable — which
//! matters because the interesting logic is version ordering, and arduino-cli's
//! version strings are not plain semver (`2.0.18-arduino.5`).

use serde::Serialize;

use crate::ghlib::version_key;
use crate::types::Platform;
use crate::{Error, Result};

/// A platform id split into its halves, e.g. `esp32:esp32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreId {
    pub packager: String,
    pub arch: String,
}

/// Validate a `packager:architecture` platform id.
///
/// Rejects an FQBN (`esp32:esp32:esp32s3`) explicitly: it is the single most
/// likely wrong thing to pass here, and three colon-separated parts would
/// otherwise silently become a packager and a malformed arch.
pub fn parse_core_id(raw: &str) -> Result<CoreId> {
    let trimmed = raw.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() == 3 {
        return Err(Error::Other(format!(
            "`{trimmed}` is a board FQBN, not a platform id — use its first two parts"
        )));
    }
    if parts.len() != 2 || parts.iter().any(|p| p.is_empty()) {
        return Err(Error::Other(format!(
            "`{trimmed}` is not a platform id (expected `packager:architecture`)"
        )));
    }
    Ok(CoreId {
        packager: parts[0].to_string(),
        arch: parts[1].to_string(),
    })
}

/// The `packager:architecture` platform id of a board FQBN
/// (`arduino:avr:uno` → `arduino:avr`). Board options after the third
/// segment are ignored; fewer than three segments is not an FQBN.
pub fn fqbn_platform_id(fqbn: &str) -> Result<String> {
    let trimmed = fqbn.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() < 3 || parts[..3].iter().any(|p| p.is_empty()) {
        return Err(Error::Other(format!(
            "`{trimmed}` is not a board FQBN (expected `packager:architecture:board`)"
        )));
    }
    Ok(format!("{}:{}", parts[0], parts[1]))
}

/// Where a platform stands relative to its newest published release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreStatus {
    NotInstalled,
    UpToDate,
    UpdateAvailable,
}

/// An empty `installed_version` is how `core search` marks a platform it knows
/// about but that is not installed — that, not a missing key, is the signal.
///
/// The comparison is numeric rather than a string inequality: arduino-cli can
/// report a `latest_version` that is not newer than what is installed (a
/// yanked release, or a locally installed build), and offering a pointless
/// "Update" is worse than showing nothing.
pub fn status(p: &Platform) -> CoreStatus {
    if p.installed_version.is_empty() {
        return CoreStatus::NotInstalled;
    }
    match (
        version_key(&p.installed_version),
        version_key(&p.latest_version),
    ) {
        (Some(installed), Some(latest)) if latest > installed => CoreStatus::UpdateAvailable,
        // Unparseable on either side: fall back to a plain difference rather
        // than claiming it is current.
        (None, _) | (_, None)
            if !p.latest_version.is_empty() && p.latest_version != p.installed_version =>
        {
            CoreStatus::UpdateAvailable
        }
        _ => CoreStatus::UpToDate,
    }
}

/// Installable versions, newest first.
///
/// `releases` is a JSON object keyed by version, so its keys *are* the version
/// list. Incompatible releases are dropped: arduino-cli has no toolchain for
/// them on this host and installing one fails, so offering it is a trap.
/// Versions that are not version-shaped sort last rather than being hidden.
pub fn sorted_versions(p: &Platform) -> Vec<String> {
    let mut versions: Vec<String> = p
        .releases
        .iter()
        .filter(|(_, rel)| rel.compatible)
        .map(|(v, _)| v.clone())
        .collect();
    versions.sort_by(|a, b| match (version_key(a), version_key(b)) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    });
    versions
}

/// Board names for one release, or an empty list when that version is unknown.
///
/// Names only — `core list`/`core search` do not carry FQBNs, so this cannot
/// feed a board picker; it is for showing what a platform contains.
pub fn board_names(p: &Platform, version: &str) -> Vec<String> {
    p.releases
        .get(version)
        .map(|rel| rel.boards.iter().map(|b| b.name.clone()).collect())
        .unwrap_or_default()
}

/// Everything the UI needs about one platform, flattened.
///
/// Derived here rather than in the frontend so that version ordering — the one
/// genuinely tricky part — has a single tested implementation instead of a
/// duplicate in TypeScript.
#[derive(Debug, Clone, Serialize)]
pub struct CoreView {
    pub id: String,
    /// Human name of the displayed release, e.g. `Arduino AVR Boards`. Falls
    /// back to the id, which is all `core search` gives for some platforms.
    pub name: String,
    pub maintainer: String,
    pub website: String,
    /// Empty when not installed.
    pub installed_version: String,
    pub latest_version: String,
    pub status: CoreStatus,
    /// Installable versions, newest first.
    pub versions: Vec<String>,
    /// Board names of the displayed release.
    pub boards: Vec<String>,
}

/// The release a card describes: what is installed, or else the newest.
fn display_version(p: &Platform) -> String {
    if !p.installed_version.is_empty() {
        return p.installed_version.clone();
    }
    if !p.latest_version.is_empty() {
        return p.latest_version.clone();
    }
    sorted_versions(p).first().cloned().unwrap_or_default()
}

pub fn view(p: &Platform) -> CoreView {
    let shown = display_version(p);
    let name = p
        .releases
        .get(&shown)
        .map(|r| r.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| p.id.clone());
    CoreView {
        id: p.id.clone(),
        name,
        maintainer: p.maintainer.clone(),
        website: p.website.clone(),
        installed_version: p.installed_version.clone(),
        latest_version: p.latest_version.clone(),
        status: status(p),
        versions: sorted_versions(p),
        boards: board_names(p, &shown),
    }
}

/// The `platform:` value for a `sketch.yaml` profile, e.g.
/// `esp32:esp32 (3.3.11)` — the pinned form arduino-cli writes and reads.
pub fn platform_dep_entry(id: &str, version: &str) -> String {
    format!("{id} ({version})")
}

/// The platform id inside such an entry, so two pins of the same platform can
/// be recognised as the same platform at different versions.
///
/// Falls back to the whole trimmed string when there is no version suffix,
/// because a hand-written `sketch.yaml` may legitimately carry a bare id.
pub fn platform_dep_id(entry: &str) -> &str {
    match entry.split_once('(') {
        Some((id, _)) => id.trim(),
        None => entry.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PlatformBoard, PlatformRelease};
    use std::collections::BTreeMap;

    fn platform(installed: &str, latest: &str, releases: &[(&str, bool)]) -> Platform {
        let mut map = BTreeMap::new();
        for (v, compatible) in releases {
            map.insert(
                v.to_string(),
                PlatformRelease {
                    name: "Test Boards".into(),
                    version: v.to_string(),
                    boards: vec![PlatformBoard {
                        name: format!("Board for {v}"),
                    }],
                    compatible: *compatible,
                },
            );
        }
        Platform {
            id: "test:arch".into(),
            installed_version: installed.into(),
            latest_version: latest.into(),
            releases: map,
            ..Default::default()
        }
    }

    #[test]
    fn parses_a_platform_id() {
        let id = parse_core_id("esp32:esp32").unwrap();
        assert_eq!(id.packager, "esp32");
        assert_eq!(id.arch, "esp32");
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        assert_eq!(parse_core_id("  esp32:esp32 ").unwrap().arch, "esp32");
    }

    #[test]
    fn rejects_an_fqbn_with_a_message_that_says_what_to_do() {
        let err = parse_core_id("esp32:esp32:esp32s3")
            .unwrap_err()
            .to_string();
        assert!(err.contains("FQBN"), "got: {err}");
    }

    #[test]
    fn rejects_malformed_ids() {
        for bad in ["esp32", "", ":", "esp32:", ":esp32"] {
            assert!(parse_core_id(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn fqbn_platform_id_takes_the_first_two_segments() {
        assert_eq!(fqbn_platform_id("arduino:avr:uno").unwrap(), "arduino:avr");
        assert_eq!(
            fqbn_platform_id("esp32:esp32:esp32s3:CDCOnBoot=cdc").unwrap(),
            "esp32:esp32"
        );
    }

    #[test]
    fn fqbn_platform_id_rejects_malformed_fqbns() {
        for bad in ["arduino", "arduino:avr", "", ":a:b", "a::b"] {
            assert!(fqbn_platform_id(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn not_installed_when_the_installed_version_is_empty() {
        let p = platform("", "1.0.0", &[("1.0.0", true)]);
        assert_eq!(status(&p), CoreStatus::NotInstalled);
    }

    #[test]
    fn up_to_date_when_installed_matches_latest() {
        let p = platform("3.3.11", "3.3.11", &[("3.3.11", true)]);
        assert_eq!(status(&p), CoreStatus::UpToDate);
    }

    #[test]
    fn update_available_when_latest_is_newer() {
        let p = platform("3.2.0", "3.3.11", &[("3.2.0", true), ("3.3.11", true)]);
        assert_eq!(status(&p), CoreStatus::UpdateAvailable);
    }

    #[test]
    fn no_update_offered_when_latest_is_older_than_installed() {
        // A locally installed newer build, or a yanked release: a string
        // inequality would wrongly offer a downgrade as an "update".
        let p = platform("3.3.11", "3.2.0", &[("3.2.0", true)]);
        assert_eq!(status(&p), CoreStatus::UpToDate);
    }

    #[test]
    fn versions_are_newest_first() {
        let p = platform(
            "",
            "3.3.11",
            &[("3.2.0", true), ("3.3.11", true), ("2.0.17", true)],
        );
        assert_eq!(sorted_versions(&p), ["3.3.11", "3.2.0", "2.0.17"]);
    }

    #[test]
    fn versions_sort_numerically_not_lexically() {
        // "3.10.0" > "3.9.0" numerically but sorts the other way as a string.
        let p = platform("", "3.10.0", &[("3.9.0", true), ("3.10.0", true)]);
        assert_eq!(sorted_versions(&p), ["3.10.0", "3.9.0"]);
    }

    #[test]
    fn versions_handle_the_arduino_suffix_form() {
        let p = platform(
            "",
            "2.0.18-arduino.5",
            &[("2.0.18-arduino.5", true), ("2.0.17", true)],
        );
        assert_eq!(sorted_versions(&p), ["2.0.18-arduino.5", "2.0.17"]);
    }

    #[test]
    fn incompatible_releases_are_not_offered() {
        let p = platform("", "9.9.9", &[("1.0.0", true), ("9.9.9", false)]);
        assert_eq!(sorted_versions(&p), ["1.0.0"]);
    }

    #[test]
    fn board_names_come_from_the_named_release() {
        let p = platform("", "2.0.0", &[("1.0.0", true), ("2.0.0", true)]);
        assert_eq!(board_names(&p, "1.0.0"), ["Board for 1.0.0"]);
        assert!(board_names(&p, "nope").is_empty());
    }

    #[test]
    fn view_describes_the_installed_release_when_installed() {
        let p = platform("1.0.0", "2.0.0", &[("1.0.0", true), ("2.0.0", true)]);
        let v = view(&p);
        assert_eq!(v.status, CoreStatus::UpdateAvailable);
        assert_eq!(v.name, "Test Boards");
        // Boards shown are the installed release's, not the newest one's.
        assert_eq!(v.boards, ["Board for 1.0.0"]);
        assert_eq!(v.versions, ["2.0.0", "1.0.0"]);
    }

    #[test]
    fn view_describes_the_latest_release_when_not_installed() {
        let p = platform("", "2.0.0", &[("1.0.0", true), ("2.0.0", true)]);
        let v = view(&p);
        assert_eq!(v.status, CoreStatus::NotInstalled);
        assert_eq!(v.boards, ["Board for 2.0.0"]);
    }

    #[test]
    fn view_falls_back_to_the_id_when_a_release_has_no_name() {
        // `core search` returns some platforms with no release metadata at all.
        let p = Platform {
            id: "vendor:arch".into(),
            ..Default::default()
        };
        assert_eq!(view(&p).name, "vendor:arch");
        assert!(view(&p).versions.is_empty());
    }

    #[test]
    fn platform_dep_entry_round_trips_through_its_id() {
        let entry = platform_dep_entry("esp32:esp32", "3.3.11");
        assert_eq!(entry, "esp32:esp32 (3.3.11)");
        assert_eq!(platform_dep_id(&entry), "esp32:esp32");
    }

    #[test]
    fn platform_dep_id_accepts_a_bare_id() {
        // A hand-written sketch.yaml may pin without a version.
        assert_eq!(platform_dep_id("esp32:esp32"), "esp32:esp32");
        assert_eq!(platform_dep_id("  esp32:esp32  "), "esp32:esp32");
    }
}
