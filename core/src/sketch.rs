//! Sketch project handling: file listing and `sketch.yaml` (build profiles).
//!
//! `sketch.yaml` is the single source of truth for reproducible builds — it
//! pins the core platform and libraries, and (crucially for Bancada) supports
//! **local/proprietary libraries by path** via `dir:` entries, which the
//! official IDE has no UI for.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

// ---------- sketch.yaml model ----------

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SketchYaml {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, Profile>,
    /// Top-level keys we don't model, e.g. `default_port`/`default_fqbn`
    /// written by `arduino-cli board attach`. Preserved verbatim through
    /// every load→save round trip instead of being silently dropped.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub fqbn: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<PlatformDep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<LibraryDep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// Profile-level keys we don't model, e.g. `programmer:`/`port_config:`.
    /// Preserved verbatim through every load→save round trip.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct PlatformDep {
    /// e.g. "esp32:esp32 (3.3.0)"
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_index_url: Option<String>,
    /// Unmodeled platform-entry keys. Preserved verbatim through every
    /// load→save round trip.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

/// A profile library dependency: a registry entry like
/// `"ArduinoJson (7.4.2)"`, a resolved dependency arduino-cli records as
/// `dependency: Name (v)`, or a local path entry `{ dir: ../libs/Foo }`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum LibraryDep {
    Registry(String),
    Dependency { dependency: String },
    Local { dir: String },
}

// ---------- project operations ----------

pub struct SketchProject {
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct SketchFile {
    /// Path relative to the sketch dir, e.g. "src/sensors/EnvSensor.cpp".
    pub rel_path: String,
    pub is_dir: bool,
}

/// How a local library path is written into `sketch.yaml`.
///
/// arduino-cli accepts either form. Relative keeps the project relocatable and
/// is right for a library inside the project tree; absolute is right for one
/// outside it, such as a library in the global sketchbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    Relative,
    Absolute,
}

impl SketchProject {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        if !dir.is_dir() {
            return Err(Error::Other(format!(
                "{} is not a directory",
                dir.display()
            )));
        }
        Ok(Self { dir })
    }

    /// Name of the main `.ino` (must match the folder name to be a valid sketch).
    pub fn main_ino(&self) -> Option<PathBuf> {
        let name = self.dir.file_name()?.to_str()?;
        let candidate = self.dir.join(format!("{name}.ino"));
        candidate.is_file().then_some(candidate)
    }

    /// Recursively list project files, skipping build output and VCS noise.
    pub fn list_files(&self) -> Result<Vec<SketchFile>> {
        let mut out = Vec::new();
        walk(&self.dir, &self.dir, &mut out)?;
        out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        Ok(out)
    }

    fn yaml_path(&self) -> PathBuf {
        self.dir.join("sketch.yaml")
    }

    pub fn load_yaml(&self) -> Result<SketchYaml> {
        let p = self.yaml_path();
        if !p.exists() {
            return Ok(SketchYaml::default());
        }
        let text = std::fs::read_to_string(&p)?;
        Ok(serde_yaml::from_str(&text)?)
    }

    pub fn save_yaml(&self, y: &SketchYaml) -> Result<()> {
        let text = serde_yaml::to_string(y)?;
        std::fs::write(self.yaml_path(), text)?;
        Ok(())
    }

    /// Create `profile_name` pointing at `fqbn`, creating sketch.yaml when
    /// absent. Never overwrites: a profile that already exists is an error.
    /// The sketch's first profile also becomes `default_profile`, so gaining
    /// a profile gives Verify/Flash a working default immediately.
    pub fn init_profile(&self, profile_name: &str, fqbn: &str) -> Result<SketchYaml> {
        self.add_profile(profile_name, fqbn, None, None)
    }

    /// Create `profile_name` for `fqbn`, optionally pinning a platform and
    /// copying another profile's `libraries:` list verbatim — registry pins,
    /// `dependency:` entries and `dir:` locals alike. Copying is what makes
    /// "flash the same project on a second board" build immediately: a
    /// profile build only sees its own pinned libraries.
    pub fn add_profile(
        &self,
        profile_name: &str,
        fqbn: &str,
        platform_entry: Option<&str>,
        copy_libs_from: Option<&str>,
    ) -> Result<SketchYaml> {
        let name = profile_name.trim();
        if name.is_empty() {
            return Err(Error::Other("a profile needs a name".into()));
        }
        let fqbn = fqbn.trim();
        if fqbn.is_empty() {
            return Err(Error::Other("a profile needs a board (FQBN)".into()));
        }
        let mut y = self.load_yaml()?;
        if y.profiles.contains_key(name) {
            return Err(Error::Other(format!("profile `{name}` already exists")));
        }
        let libraries = match copy_libs_from {
            Some(src) => y
                .profiles
                .get(src.trim())
                .ok_or_else(|| {
                    Error::Other(format!("profile `{}` not found to copy libraries from", src.trim()))
                })?
                .libraries
                .clone(),
            None => Vec::new(),
        };
        let platforms = platform_entry
            .map(|p| {
                vec![PlatformDep {
                    platform: p.to_string(),
                    platform_index_url: None,
                    extra: Default::default(),
                }]
            })
            .unwrap_or_default();
        y.profiles.insert(
            name.to_string(),
            Profile {
                fqbn: fqbn.to_string(),
                platforms,
                libraries,
                ..Default::default()
            },
        );
        if y.default_profile.is_none() {
            y.default_profile = Some(name.to_string());
        }
        self.save_yaml(&y)?;
        Ok(y)
    }

    /// Point an existing profile at a different board, replacing its platform
    /// pin and touching nothing else — name, libraries, port and notes stay.
    /// Replacing the pin is not optional: a leftover `arduino:avr` pin under
    /// an `arduino:zephyr` fqbn is exactly the broken state this prevents.
    pub fn retarget_profile(
        &self,
        profile_name: &str,
        fqbn: &str,
        platform_entry: &str,
    ) -> Result<SketchYaml> {
        let name = profile_name.trim();
        if name.is_empty() {
            return Err(Error::Other("a profile needs a name".into()));
        }
        let fqbn = fqbn.trim();
        if fqbn.is_empty() {
            return Err(Error::Other("a profile needs a board (FQBN)".into()));
        }
        let mut y = self.load_yaml()?;
        let p = y
            .profiles
            .get_mut(name)
            .ok_or_else(|| Error::Other(format!("profile `{name}` not found")))?;
        p.fqbn = fqbn.to_string();
        p.platforms = vec![PlatformDep {
            platform: platform_entry.to_string(),
            platform_index_url: None,
            extra: Default::default(),
        }];
        self.save_yaml(&y)?;
        Ok(y)
    }

    /// Add (or no-op if present) a local library `dir:` entry to a profile.
    /// The stored path is made relative to the sketch dir when possible, so
    /// the project stays relocatable.
    pub fn add_local_library(&self, profile_name: &str, lib_dir: &Path) -> Result<SketchYaml> {
        self.add_local_library_with(profile_name, lib_dir, PathStyle::Relative)
    }

    /// Add a local library `dir:` entry, choosing how the path is written.
    ///
    /// Use [`PathStyle::Absolute`] for libraries that live outside the project
    /// tree — a library in the global sketchbook would otherwise be stored as a
    /// brittle `../../Arduino/libraries/Foo`.
    pub fn add_local_library_with(
        &self,
        profile_name: &str,
        lib_dir: &Path,
        style: PathStyle,
    ) -> Result<SketchYaml> {
        if style == PathStyle::Absolute && !lib_dir.is_absolute() {
            return Err(Error::Other(format!(
                "an absolute dir: entry needs an absolute path, got {}",
                lib_dir.display()
            )));
        }
        let mut y = self.load_yaml()?;
        let profile = y
            .profiles
            .get_mut(profile_name)
            .ok_or_else(|| Error::Other(format!("no profile named `{profile_name}`")))?;

        // Compare resolved targets rather than the stored strings: the same
        // library may already be pinned in the other style, and two `dir:`
        // entries for one library would make arduino-cli see it twice.
        let target = normalize(&self.dir.join(lib_dir));
        let already = profile.libraries.iter().any(|l| match l {
            LibraryDep::Local { dir } => normalize(&self.dir.join(dir)) == target,
            LibraryDep::Registry(_) | LibraryDep::Dependency { .. } => false,
        });
        if !already {
            let stored = match style {
                PathStyle::Relative => relativize(&self.dir, lib_dir),
                PathStyle::Absolute => lib_dir.to_string_lossy().into_owned(),
            };
            profile.libraries.push(LibraryDep::Local { dir: stored });
        }
        self.save_yaml(&y)?;
        Ok(y)
    }

    // Registry pins are written by `arduino-cli profile lib add` rather than by
    // hand, so that dependencies are resolved too. Local `dir:` entries stay
    // here: `profile lib add` has no equivalent for them.

    /// Pin a platform version into a profile's `platforms:` list.
    ///
    /// Written by hand, unlike registry library pins, because arduino-cli offers
    /// nothing that edits an existing profile. `profile create` advertises
    /// "Create or update" but refuses on a second run — verified against 1.5.0:
    ///
    /// ```text
    /// $ arduino-cli profile create -m uno -b arduino:avr:uno ./demo
    /// Error initializing the project file: Profile 'uno' already exists
    /// ```
    ///
    /// and there is no `profile platform add`. So this is the only route, and it
    /// is not the same situation as the registry pins above.
    ///
    /// Replaces any existing pin of the *same platform* rather than skipping
    /// when absent: two entries for one platform at different versions are
    /// contradictory, not duplicates, and arduino-cli would resolve one of them
    /// arbitrarily. Any `platform_index_url` on the replaced entry is carried
    /// over — it describes where the platform comes from, not which version.
    pub fn add_platform(&self, profile_name: &str, id: &str, version: &str) -> Result<SketchYaml> {
        let mut y = self.load_yaml()?;
        let profile = y
            .profiles
            .get_mut(profile_name)
            .ok_or_else(|| Error::Other(format!("no profile named `{profile_name}`")))?;

        let entry = crate::boards::platform_dep_entry(id, version);
        let existing = profile
            .platforms
            .iter()
            .position(|p| crate::boards::platform_dep_id(&p.platform) == id);
        match existing {
            Some(i) => {
                profile.platforms[i].platform = entry;
            }
            None => profile.platforms.push(PlatformDep {
                platform: entry,
                platform_index_url: None,
                extra: Default::default(),
            }),
        }
        self.save_yaml(&y)?;
        Ok(y)
    }

    /// All local `dir:` library paths of a profile, resolved to absolute paths.
    pub fn local_library_paths(&self, profile_name: &str) -> Result<Vec<PathBuf>> {
        let y = self.load_yaml()?;
        let Some(profile) = y.profiles.get(profile_name) else {
            return Ok(vec![]);
        };
        Ok(profile
            .libraries
            .iter()
            .filter_map(|l| match l {
                LibraryDep::Local { dir } => {
                    let p = PathBuf::from(dir);
                    Some(if p.is_absolute() { p } else { self.dir.join(p) })
                }
                LibraryDep::Registry(_) | LibraryDep::Dependency { .. } => None,
            })
            .collect())
    }
}

pub(crate) const SKIP_DIRS: &[&str] = &["build", ".git", ".pio", "node_modules", ".vscode"];

fn walk(root: &Path, dir: &Path, out: &mut Vec<SketchFile>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            out.push(SketchFile {
                rel_path: rel(root, &path),
                is_dir: true,
            });
            walk(root, &path, out)?;
        } else {
            out.push(SketchFile {
                rel_path: rel(root, &path),
                is_dir: false,
            });
        }
    }
    Ok(())
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// Lexically collapse `.` and `..` so two spellings of one path compare equal.
///
/// Not `canonicalize`: that touches the filesystem, fails for a path that does
/// not exist yet, and resolves symlinks — and a sketchbook routinely contains
/// symlinked libraries we want to keep distinct from their targets.
pub(crate) fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // Only pop a real directory name; keep leading `..` as-is.
                if out
                    .components()
                    .next_back()
                    .is_some_and(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Best-effort relative path from `base` to `target` (walks up with `..`).
pub(crate) fn relativize(base: &Path, target: &Path) -> String {
    let base: Vec<_> = base.components().collect();
    let target_c: Vec<_> = target.components().collect();
    let common = base
        .iter()
        .zip(target_c.iter())
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        // Different roots (shouldn't happen on Linux) — keep absolute.
        return target.to_string_lossy().into_owned();
    }
    let mut parts: Vec<String> =
        std::iter::repeat_n("..".to_string(), base.len() - common).collect();
    parts.extend(
        target_c[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
default_profile: esp32s3
profiles:
  esp32s3:
    fqbn: esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PSRAM=opi
    platforms:
      - platform: esp32:esp32 (3.3.0)
        platform_index_url: https://espressif.github.io/arduino-esp32/package_esp32_index.json
    libraries:
      - PubSubClient (2.8.0)
      - dir: ../libs/EnvSensor
"#;

    #[test]
    fn parses_mixed_library_deps() {
        let y: SketchYaml = serde_yaml::from_str(SAMPLE).unwrap();
        assert_eq!(y.default_profile.as_deref(), Some("esp32s3"));
        let p = &y.profiles["esp32s3"];
        assert_eq!(p.libraries.len(), 2);
        assert_eq!(
            p.libraries[0],
            LibraryDep::Registry("PubSubClient (2.8.0)".into())
        );
        assert_eq!(
            p.libraries[1],
            LibraryDep::Local {
                dir: "../libs/EnvSensor".into()
            }
        );
    }

    #[test]
    fn yaml_roundtrip_preserves_structure() {
        let y: SketchYaml = serde_yaml::from_str(SAMPLE).unwrap();
        let text = serde_yaml::to_string(&y).unwrap();
        let y2: SketchYaml = serde_yaml::from_str(&text).unwrap();
        assert_eq!(y, y2);
    }

    /// arduino-cli (`board attach`) and hand-edits write keys `SketchYaml`,
    /// `Profile` and `PlatformDep` don't model: top-level `default_port`,
    /// profile-level `programmer:`/`port_config:`, and unknown platform keys.
    /// Every load→mutate→save round trip must keep them verbatim, or Bancada
    /// silently deletes user/arduino-cli state on the next save.
    const SAMPLE_WITH_EXTRAS: &str = r#"
default_profile: esp32s3
default_port: /dev/ttyACM0
profiles:
  esp32s3:
    fqbn: esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PSRAM=opi
    programmer: atmel_ice
    port_config:
      baudrate: 115200
    platforms:
      - platform: esp32:esp32 (3.3.0)
        platform_index_url: https://espressif.github.io/arduino-esp32/package_esp32_index.json
        platform_unknown_key: something
    libraries:
      - PubSubClient (2.8.0)
      - dir: ../libs/EnvSensor
"#;

    #[test]
    fn yaml_preserves_unmodeled_keys_across_a_mutating_op() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("sketch.yaml"), SAMPLE_WITH_EXTRAS).unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();

        // A realistic mutating op unrelated to any of the foreign keys.
        proj.add_profile("uno", "arduino:avr:uno", None, None)
            .unwrap();

        let reloaded = proj.load_yaml().unwrap();
        assert_eq!(
            reloaded.extra.get("default_port").and_then(|v| v.as_str()),
            Some("/dev/ttyACM0"),
            "top-level default_port must survive"
        );
        let esp32s3 = &reloaded.profiles["esp32s3"];
        assert_eq!(
            esp32s3.extra.get("programmer").and_then(|v| v.as_str()),
            Some("atmel_ice"),
            "profile-level programmer: must survive"
        );
        assert_eq!(
            esp32s3
                .extra
                .get("port_config")
                .and_then(|v| v.get("baudrate"))
                .and_then(|v| v.as_i64()),
            Some(115200),
            "profile-level port_config: must survive"
        );
        let platform = &esp32s3.platforms[0];
        assert_eq!(
            platform.platform_index_url.as_deref(),
            Some("https://espressif.github.io/arduino-esp32/package_esp32_index.json")
        );
        assert_eq!(
            platform
                .extra
                .get("platform_unknown_key")
                .and_then(|v| v.as_str()),
            Some("something"),
            "unknown platform key must survive"
        );
        // The mutation itself still landed.
        assert_eq!(reloaded.profiles["uno"].fqbn, "arduino:avr:uno");
    }

    #[test]
    fn add_local_library_creates_relative_dir_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let sketch_dir = tmp.path().join("monitor");
        let lib_dir = tmp.path().join("libs/EnvSensor");
        std::fs::create_dir_all(&sketch_dir).unwrap();
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::write(sketch_dir.join("sketch.yaml"), SAMPLE).unwrap();

        let proj = SketchProject::open(&sketch_dir).unwrap();
        let y = proj.add_local_library("esp32s3", &lib_dir).unwrap();
        let libs = &y.profiles["esp32s3"].libraries;
        assert!(libs.contains(&LibraryDep::Local {
            dir: "../libs/EnvSensor".into()
        }));
        // idempotent
        let y2 = proj.add_local_library("esp32s3", &lib_dir).unwrap();
        assert_eq!(y2.profiles["esp32s3"].libraries.len(), libs.len());

        // resolved paths are absolute
        let paths = proj.local_library_paths("esp32s3").unwrap();
        assert!(paths.iter().all(|p| p.is_absolute()));
    }

    /// A sketch with the SAMPLE yaml plus a library dir in an unrelated place,
    /// which is what a global-sketchbook library looks like from a project.
    fn sketch_and_outside_lib(tmp: &Path) -> (SketchProject, PathBuf) {
        let sketch_dir = tmp.join("projects/monitor");
        let lib_dir = tmp.join("sketchbook/libraries/EnvSensor");
        std::fs::create_dir_all(&sketch_dir).unwrap();
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::write(sketch_dir.join("sketch.yaml"), SAMPLE).unwrap();
        (SketchProject::open(&sketch_dir).unwrap(), lib_dir)
    }

    #[test]
    fn add_local_library_absolute_writes_absolute_dir_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, lib_dir) = sketch_and_outside_lib(tmp.path());

        let y = proj
            .add_local_library_with("esp32s3", &lib_dir, PathStyle::Absolute)
            .unwrap();
        // SAMPLE already carries a `dir: ../libs/EnvSensor` entry, so look for
        // the one we added rather than the first local entry.
        let want = lib_dir.to_string_lossy().into_owned();
        assert!(
            y.profiles["esp32s3"]
                .libraries
                .contains(&LibraryDep::Local { dir: want.clone() }),
            "absolute entry missing from {:?}",
            y.profiles["esp32s3"].libraries
        );
        assert!(Path::new(&want).is_absolute());

        // and it resolves back to the same place
        let paths = proj.local_library_paths("esp32s3").unwrap();
        assert!(paths.contains(&lib_dir));
    }

    #[test]
    fn relative_then_absolute_dedupes_to_one_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, lib_dir) = sketch_and_outside_lib(tmp.path());

        let y1 = proj.add_local_library("esp32s3", &lib_dir).unwrap();
        let before = y1.profiles["esp32s3"].libraries.len();
        // Same library, other style: must not add a second entry, or arduino-cli
        // would see the library twice.
        let y2 = proj
            .add_local_library_with("esp32s3", &lib_dir, PathStyle::Absolute)
            .unwrap();
        assert_eq!(y2.profiles["esp32s3"].libraries.len(), before);
    }

    #[test]
    fn absolute_then_relative_also_dedupes() {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, lib_dir) = sketch_and_outside_lib(tmp.path());

        let y1 = proj
            .add_local_library_with("esp32s3", &lib_dir, PathStyle::Absolute)
            .unwrap();
        let before = y1.profiles["esp32s3"].libraries.len();
        let y2 = proj.add_local_library("esp32s3", &lib_dir).unwrap();
        assert_eq!(y2.profiles["esp32s3"].libraries.len(), before);
    }

    #[test]
    fn absolute_style_rejects_a_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, _) = sketch_and_outside_lib(tmp.path());
        let err = proj
            .add_local_library_with("esp32s3", Path::new("libs/Foo"), PathStyle::Absolute)
            .unwrap_err()
            .to_string();
        assert!(err.contains("absolute"), "{err}");
    }

    #[test]
    fn add_local_library_absolute_errors_on_unknown_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, lib_dir) = sketch_and_outside_lib(tmp.path());
        let err = proj
            .add_local_library_with("nope", &lib_dir, PathStyle::Absolute)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no profile named"), "{err}");
    }

    #[test]
    fn normalize_collapses_dot_segments() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        // A leading `..` has nothing to pop and must survive.
        assert_eq!(normalize(Path::new("../x")), PathBuf::from("../x"));
    }

    #[test]
    fn missing_yaml_is_empty_project_file() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        let y = proj.load_yaml().unwrap();
        assert!(y.profiles.is_empty());
    }

    #[test]
    fn list_files_skips_build_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("build/esp32")).unwrap();
        std::fs::write(tmp.path().join("a.ino"), "").unwrap();
        std::fs::write(tmp.path().join("src/x.cpp"), "").unwrap();
        std::fs::write(tmp.path().join("build/esp32/junk.o"), "").unwrap();

        let proj = SketchProject::open(tmp.path()).unwrap();
        let files = proj.list_files().unwrap();
        let names: Vec<_> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(names.contains(&"a.ino"));
        assert!(names.contains(&"src/x.cpp"));
        assert!(!names.iter().any(|n| n.starts_with("build")));
    }

    // ---------- add_platform ----------

    fn sample_project(tmp: &tempfile::TempDir) -> SketchProject {
        std::fs::write(tmp.path().join("sketch.yaml"), SAMPLE).unwrap();
        SketchProject::open(tmp.path()).unwrap()
    }

    #[test]
    fn add_platform_replaces_the_same_platform_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = sample_project(&tmp);

        let y = proj
            .add_platform("esp32s3", "esp32:esp32", "3.3.11")
            .unwrap();
        let platforms = &y.profiles["esp32s3"].platforms;
        assert_eq!(
            platforms.len(),
            1,
            "one platform must not be pinned twice at two versions"
        );
        assert_eq!(platforms[0].platform, "esp32:esp32 (3.3.11)");
    }

    #[test]
    fn add_platform_keeps_the_index_url_of_the_entry_it_replaces() {
        // The index URL says where the platform comes from, not which version;
        // losing it would break resolution for a third-party core.
        let tmp = tempfile::tempdir().unwrap();
        let proj = sample_project(&tmp);

        let y = proj
            .add_platform("esp32s3", "esp32:esp32", "3.3.11")
            .unwrap();
        assert_eq!(
            y.profiles["esp32s3"].platforms[0]
                .platform_index_url
                .as_deref(),
            Some("https://espressif.github.io/arduino-esp32/package_esp32_index.json")
        );
    }

    #[test]
    fn add_platform_appends_a_different_platform() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = sample_project(&tmp);

        let y = proj
            .add_platform("esp32s3", "arduino:avr", "1.8.8")
            .unwrap();
        let entries: Vec<&str> = y.profiles["esp32s3"]
            .platforms
            .iter()
            .map(|p| p.platform.as_str())
            .collect();
        assert_eq!(entries, ["esp32:esp32 (3.3.0)", "arduino:avr (1.8.8)"]);
    }

    #[test]
    fn add_platform_is_idempotent_and_converges_on_the_last_version() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = sample_project(&tmp);

        proj.add_platform("esp32s3", "esp32:esp32", "3.2.0")
            .unwrap();
        proj.add_platform("esp32s3", "esp32:esp32", "3.3.11")
            .unwrap();
        let y = proj
            .add_platform("esp32s3", "esp32:esp32", "3.3.11")
            .unwrap();
        let platforms = &y.profiles["esp32s3"].platforms;
        assert_eq!(platforms.len(), 1);
        assert_eq!(platforms[0].platform, "esp32:esp32 (3.3.11)");
    }

    #[test]
    fn add_platform_survives_a_reload_from_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = sample_project(&tmp);

        proj.add_platform("esp32s3", "esp32:esp32", "3.3.11")
            .unwrap();
        let reloaded = proj.load_yaml().unwrap();
        assert_eq!(
            reloaded.profiles["esp32s3"].platforms[0].platform,
            "esp32:esp32 (3.3.11)"
        );
        // The rest of the profile is untouched.
        assert_eq!(reloaded.profiles["esp32s3"].libraries.len(), 2);
    }

    #[test]
    fn add_platform_errors_on_unknown_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = sample_project(&tmp);

        let err = proj
            .add_platform("nope", "esp32:esp32", "3.3.11")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no profile named `nope`"), "got: {err}");
    }

    // ---------- init_profile ----------

    #[test]
    fn init_profile_creates_yaml_and_sets_default() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();

        let y = proj.init_profile("weather", "esp32:esp32:esp32s3").unwrap();

        assert!(tmp.path().join("sketch.yaml").is_file());
        assert_eq!(y.profiles["weather"].fqbn, "esp32:esp32:esp32s3");
        assert_eq!(y.default_profile.as_deref(), Some("weather"));
        // And it round-trips from disk, not just in memory.
        assert_eq!(proj.load_yaml().unwrap(), y);
    }

    #[test]
    fn init_profile_rejects_a_duplicate_name() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.init_profile("weather", "esp32:esp32:esp32s3").unwrap();

        let err = proj.init_profile("weather", "esp32:esp32:esp32c6");
        assert!(err.is_err(), "a second `weather` must not overwrite the first");
        // The original is untouched.
        assert_eq!(
            proj.load_yaml().unwrap().profiles["weather"].fqbn,
            "esp32:esp32:esp32s3"
        );
    }

    #[test]
    fn init_profile_keeps_existing_default_and_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("sketch.yaml"),
            "default_profile: base\nprofiles:\n  base:\n    fqbn: esp32:esp32:esp32\n",
        )
        .unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();

        let y = proj.init_profile("c6", "esp32:esp32:esp32c6").unwrap();

        assert_eq!(y.default_profile.as_deref(), Some("base"), "default must not move");
        assert_eq!(y.profiles["base"].fqbn, "esp32:esp32:esp32");
        assert_eq!(y.profiles["c6"].fqbn, "esp32:esp32:esp32c6");
    }

    #[test]
    fn init_profile_rejects_blank_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        assert!(proj.init_profile("  ", "esp32:esp32:esp32").is_err());
        assert!(proj.init_profile("weather", "  ").is_err());
    }

    #[test]
    fn add_profile_copies_libraries_from_the_source_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.init_profile("uno", "arduino:avr:uno").unwrap();
        // One of each LibraryDep shape the copy must preserve.
        let mut y = proj.load_yaml().unwrap();
        y.profiles.get_mut("uno").unwrap().libraries = vec![
            LibraryDep::Registry("ArduinoJson (7.4.2)".into()),
            LibraryDep::Dependency { dependency: "MsgPack (0.4.2)".into() },
            LibraryDep::Local { dir: "../libs/Foo".into() },
        ];
        proj.save_yaml(&y).unwrap();

        let out = proj
            .add_profile("nano", "arduino:avr:nano", Some("arduino:avr (1.8.8)"), Some("uno"))
            .unwrap();
        let nano = &out.profiles["nano"];
        assert_eq!(nano.fqbn, "arduino:avr:nano");
        assert_eq!(nano.libraries, out.profiles["uno"].libraries);
        assert_eq!(nano.platforms.len(), 1);
        assert_eq!(nano.platforms[0].platform, "arduino:avr (1.8.8)");
        // The addition is persisted, not just returned.
        assert_eq!(proj.load_yaml().unwrap().profiles["nano"].fqbn, "arduino:avr:nano");
    }

    #[test]
    fn add_profile_does_not_touch_the_default_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.init_profile("uno", "arduino:avr:uno").unwrap();
        let out = proj
            .add_profile("nano", "arduino:avr:nano", None, Some("uno"))
            .unwrap();
        assert_eq!(out.default_profile.as_deref(), Some("uno"));
    }

    #[test]
    fn add_profile_rejects_an_unknown_copy_source() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.init_profile("uno", "arduino:avr:uno").unwrap();
        let err = proj
            .add_profile("nano", "arduino:avr:nano", None, Some("mega"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("mega"), "got: {err}");
    }

    #[test]
    fn add_profile_without_a_source_starts_with_no_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        let out = proj
            .add_profile("uno", "arduino:avr:uno", Some("arduino:avr (1.8.8)"), None)
            .unwrap();
        assert!(out.profiles["uno"].libraries.is_empty());
        // First profile still becomes the default (bootstrap behavior).
        assert_eq!(out.default_profile.as_deref(), Some("uno"));
    }

    #[test]
    fn parses_and_round_trips_dependency_library_entries() {
        // `profile lib add` writes resolved deps as `- dependency: Name (v)`.
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        std::fs::write(
            proj.dir.join("sketch.yaml"),
            r#"profiles:
  unoq:
    fqbn: arduino:zephyr:unoq
    libraries:
      - dependency: Arduino_RPClite (0.3.0)
      - Arduino_RouterBridge (0.4.3)
      - dir: ../libs/Foo
default_profile: unoq
"#,
        )
        .unwrap();
        let y = proj.load_yaml().expect("dependency: entries must parse");
        let libs = &y.profiles["unoq"].libraries;
        assert_eq!(libs.len(), 3);
        assert_eq!(
            libs[0],
            LibraryDep::Dependency { dependency: "Arduino_RPClite (0.3.0)".into() }
        );
        // Round-trip: saving must keep the mapping form arduino-cli understands.
        proj.save_yaml(&y).unwrap();
        let text = std::fs::read_to_string(proj.dir.join("sketch.yaml")).unwrap();
        assert!(text.contains("dependency: Arduino_RPClite (0.3.0)"), "{text}");
    }

    // ---------- retarget_profile ----------

    #[test]
    fn retarget_swaps_fqbn_and_platform_pin_keeping_everything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.add_profile("uno", "arduino:avr:uno", Some("arduino:avr (1.8.8)"), None)
            .unwrap();
        let mut y = proj.load_yaml().unwrap();
        {
            let p = y.profiles.get_mut("uno").unwrap();
            p.libraries = vec![LibraryDep::Registry("ArduinoJson (7.4.2)".into())];
            p.port = Some("/dev/ttyACM0".into());
            p.notes = Some("bench uno".into());
        }
        proj.save_yaml(&y).unwrap();

        let out = proj
            .retarget_profile("uno", "arduino:zephyr:unoq", "arduino:zephyr (0.90.0)")
            .unwrap();
        let p = &out.profiles["uno"];
        assert_eq!(p.fqbn, "arduino:zephyr:unoq");
        assert_eq!(p.platforms.len(), 1);
        assert_eq!(p.platforms[0].platform, "arduino:zephyr (0.90.0)");
        // The point of "in place": name, libraries, port and notes survive.
        assert_eq!(p.libraries, vec![LibraryDep::Registry("ArduinoJson (7.4.2)".into())]);
        assert_eq!(p.port.as_deref(), Some("/dev/ttyACM0"));
        assert_eq!(p.notes.as_deref(), Some("bench uno"));
        // Persisted, and default_profile untouched.
        assert_eq!(proj.load_yaml().unwrap().profiles["uno"].fqbn, "arduino:zephyr:unoq");
        assert_eq!(out.default_profile.as_deref(), Some("uno"));
    }

    #[test]
    fn retarget_to_the_same_board_is_a_no_op_success() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.add_profile("uno", "arduino:avr:uno", Some("arduino:avr (1.8.8)"), None)
            .unwrap();
        let out = proj
            .retarget_profile("uno", "arduino:avr:uno", "arduino:avr (1.8.8)")
            .unwrap();
        assert_eq!(out.profiles["uno"].fqbn, "arduino:avr:uno");
    }

    #[test]
    fn retarget_rejects_an_unknown_profile_and_blank_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.add_profile("uno", "arduino:avr:uno", None, None).unwrap();
        assert!(proj.retarget_profile("mega", "arduino:avr:nano", "arduino:avr (1.8.8)").is_err());
        assert!(proj.retarget_profile("uno", "  ", "arduino:avr (1.8.8)").is_err());
        // Blank profile name must also be rejected with the same error as add_profile uses
        let err = proj
            .retarget_profile("  ", "arduino:avr:nano", "arduino:avr (1.8.8)")
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs a name"), "got: {err}");
    }
}
