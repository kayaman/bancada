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
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct PlatformDep {
    /// e.g. "esp32:esp32 (3.3.0)"
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_index_url: Option<String>,
}

/// A profile library dependency: either a registry entry like
/// `"ArduinoJson (7.4.2)"` or a local path entry `{ dir: ../libs/Foo }`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum LibraryDep {
    Registry(String),
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

    /// Add (or no-op if present) a local library `dir:` entry to a profile.
    /// The stored path is made relative to the sketch dir when possible, so
    /// the project stays relocatable.
    pub fn add_local_library(&self, profile_name: &str, lib_dir: &Path) -> Result<SketchYaml> {
        let mut y = self.load_yaml()?;
        let profile = y
            .profiles
            .get_mut(profile_name)
            .ok_or_else(|| Error::Other(format!("no profile named `{profile_name}`")))?;

        let stored = relativize(&self.dir, lib_dir);
        let dep = LibraryDep::Local { dir: stored };
        if !profile.libraries.contains(&dep) {
            profile.libraries.push(dep);
        }
        self.save_yaml(&y)?;
        Ok(y)
    }

    /// Add a registry library pin like "ArduinoJson (7.4.2)" to a profile.
    pub fn add_registry_library(
        &self,
        profile_name: &str,
        name: &str,
        version: &str,
    ) -> Result<SketchYaml> {
        let mut y = self.load_yaml()?;
        let profile = y
            .profiles
            .get_mut(profile_name)
            .ok_or_else(|| Error::Other(format!("no profile named `{profile_name}`")))?;
        let entry = format!("{name} ({version})");
        let dep = LibraryDep::Registry(entry);
        if !profile.libraries.contains(&dep) {
            profile.libraries.push(dep);
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
                LibraryDep::Registry(_) => None,
            })
            .collect())
    }
}

const SKIP_DIRS: &[&str] = &["build", ".git", ".pio", "node_modules", ".vscode"];

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

/// Best-effort relative path from `base` to `target` (walks up with `..`).
fn relativize(base: &Path, target: &Path) -> String {
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
    let mut parts: Vec<String> = std::iter::repeat("..".to_string())
        .take(base.len() - common)
        .collect();
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
}
