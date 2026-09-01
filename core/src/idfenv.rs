//! Finding a usable ESP-IDF installation, and activating it headlessly.
//!
//! ## Why this module is not three lines
//!
//! Everything obvious about this fails, and each failure was verified rather
//! than assumed:
//!
//! - **The ambient `IDF_PATH` is deliberately ignored.** A GUI launched from a
//!   desktop file has no shell environment at all, and on a machine with more
//!   than one IDF tree the exported one is as likely to be a half-installed
//!   git clone as the working install. We resolve from the installer's own
//!   registry instead, so "which ESP-IDF" has one answer.
//! - **`idf_tools.py export` is not usable.** It reports `no installed
//!   versions` for toolchains that are present, complete, and at exactly the
//!   version the install's `tools.json` asks for. Reimplementing Espressif's
//!   resolution logic to work around that is a trap.
//! - **Hand-building the environment does not work either.** `idf.py` reads
//!   `ESP_IDF_VERSION` through the component manager
//!   (`idf_component_manager/idf_extensions.py`, `Version.coerce`), which
//!   raises `TypeError` — not a readable error — when it is unset.
//! - **Sourcing the activation script is actively harmful.** Its last
//!   statement is `eim select "<version>"`, so reading the environment that
//!   way *mutates the registry we are reading*. It would also mean
//!   interpolating a JSON-derived path into shell source, which is an
//!   injection surface.
//!
//! So we use the vendor script's own documented non-interactive mode:
//!
//! ```text
//! /bin/sh <activationScript> -e
//! ```
//!
//! which prints `KEY=VALUE` lines and exits before anything mutates. Two
//! traps in that output are handled by [`parse_activation_output`], and both
//! have tests: the printed `PATH` carries **no `$PATH` tail**, and
//! `SYSTEM_PATH` is a stale snapshot taken when the installer ran.
//!
//! ## Dependency note
//!
//! This is the crate's first **production** dependency on `/bin/sh` (it was
//! previously used only by `#[cfg(test)]` fixtures). It is spawned by absolute
//! path, never as `sh` resolved through `PATH`, so a hostile `PATH` cannot
//! substitute it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The installer's registry of ESP-IDF installations.
///
/// Unlike [`crate::sketch::SketchYaml`], this carries no `#[serde(flatten)]`
/// catch-all: we only ever *read* this file, so there is no round trip to
/// preserve unknown keys for. `installationConfig` (a large base64 blob) is
/// deliberately not modelled — nothing here needs it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EimRegistry {
    #[serde(default)]
    pub idf_installed: Vec<IdfInstall>,
    #[serde(default)]
    pub idf_selected_id: Option<String>,
}

/// One installed ESP-IDF, as the registry describes it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdfInstall {
    pub id: String,
    /// Human-readable version, e.g. `v6.0.1`.
    pub name: String,
    /// `IDF_PATH` — the IDF checkout itself.
    pub path: PathBuf,
    /// The interpreter of the venv this install was set up with.
    pub python: PathBuf,
    /// The shell script that publishes this install's environment.
    pub activation_script: PathBuf,
    #[serde(default)]
    pub idf_tools_path: PathBuf,
}

impl IdfInstall {
    /// The `idf.py` entry point for this install.
    pub fn idf_py(&self) -> PathBuf {
        self.path.join("tools").join("idf.py")
    }
}

/// Everything that can go wrong between "the user has ESP-IDF" and "we can run
/// `idf.py`".
///
/// This is a module-local enum rather than new [`crate::Error`] variants: that
/// enum is deliberately six-wide, and the Tauri layer's uniform
/// `.map_err(err_str)` depends on it staying that way.
#[derive(Debug, thiserror::Error)]
pub enum IdfEnvError {
    #[error("ESP-IDF is not installed — no installer registry at {path}")]
    NoRegistry { path: PathBuf },
    #[error("could not read the ESP-IDF registry at {path}: {source}")]
    RegistryUnreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the ESP-IDF registry at {path} is not valid JSON: {source}")]
    RegistryMalformed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("the ESP-IDF registry at {path} lists no installations")]
    NoInstalls { path: PathBuf },
    #[error("no ESP-IDF installation named `{wanted}` — available: {}", .available.join(", "))]
    UnknownInstall {
        wanted: String,
        available: Vec<String>,
    },
    #[error("several ESP-IDF installations and none is selected — available: {}", .available.join(", "))]
    NoSelection { available: Vec<String> },
    #[error("the ESP-IDF installation `{name}` is incomplete — missing: {}", .missing.join(", "))]
    IncompleteInstall { name: String, missing: Vec<String> },
    #[error("/bin/sh is required to activate ESP-IDF and is not available")]
    ShellMissing,
    #[error("activating ESP-IDF `{name}` failed with status {status}:\n{stderr}")]
    ActivationFailed {
        name: String,
        status: i32,
        stderr: String,
    },
    #[error("activating ESP-IDF `{name}` produced no {}", .missing.join(", "))]
    ActivationIncomplete { name: String, missing: Vec<String> },
    #[error("the ESP-IDF registry says {registry} but its activation script sets {script}")]
    RegistryScriptMismatch { registry: PathBuf, script: PathBuf },
}

impl From<IdfEnvError> for crate::Error {
    fn from(e: IdfEnvError) -> Self {
        use IdfEnvError as E;
        match e {
            // "not installed" and "installed but unusable" are both instructions
            // to go install something, which is what ToolMissing renders.
            E::NoRegistry { .. }
            | E::NoInstalls { .. }
            | E::IncompleteInstall { .. }
            | E::ShellMissing => crate::Error::ToolMissing(e.to_string()),
            E::RegistryUnreadable { ref source, .. } => {
                crate::Error::Io(std::io::Error::new(source.kind(), e.to_string()))
            }
            E::RegistryMalformed { .. } => crate::Error::Other(e.to_string()),
            E::ActivationFailed { status, .. } => crate::Error::ToolFailed {
                tool: "esp-idf activation".to_string(),
                status,
                stderr: e.to_string(),
            },
            _ => crate::Error::Other(e.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, IdfEnvError>;

// ---------- discovery (pure) ----------

/// Parse the installer registry.
pub fn parse_registry(json: &str, path: &Path) -> Result<EimRegistry> {
    let reg: EimRegistry =
        serde_json::from_str(json).map_err(|source| IdfEnvError::RegistryMalformed {
            path: path.to_path_buf(),
            source,
        })?;
    if reg.idf_installed.is_empty() {
        return Err(IdfEnvError::NoInstalls {
            path: path.to_path_buf(),
        });
    }
    Ok(reg)
}

/// Choose which installation to use.
///
/// Order: an explicit preference (matched against `id` then `name`), then the
/// registry's own selection, then a sole entry. More than one with no way to
/// choose is an error rather than a guess — picking silently would mean
/// building against a different IDF than the user's terminal uses.
pub fn select_install<'a>(reg: &'a EimRegistry, prefer: Option<&str>) -> Result<&'a IdfInstall> {
    let names = || reg.idf_installed.iter().map(|i| i.name.clone()).collect();
    if let Some(want) = prefer {
        return reg
            .idf_installed
            .iter()
            .find(|i| i.id == want || i.name == want)
            .ok_or_else(|| IdfEnvError::UnknownInstall {
                wanted: want.to_string(),
                available: names(),
            });
    }
    if let Some(sel) = reg.idf_selected_id.as_deref() {
        if let Some(hit) = reg.idf_installed.iter().find(|i| i.id == sel) {
            return Ok(hit);
        }
    }
    match reg.idf_installed.len() {
        1 => Ok(&reg.idf_installed[0]),
        _ => Err(IdfEnvError::NoSelection {
            available: names(),
        }),
    }
}

/// Refuse an installation whose pieces are not on disk — **before** anything is
/// spawned.
///
/// This is what structurally excludes a half-installed IDF tree: it is checked
/// by `stat` alone, so a broken entry costs no subprocess and produces a
/// message naming what is missing rather than a toolchain error much later.
/// `exists` is injected so the whole rule is unit-testable, the same split
/// [`crate::esptool`] uses for probing.
pub fn validate_install_with(install: &IdfInstall, exists: impl Fn(&Path) -> bool) -> Result<()> {
    let mut missing = Vec::new();
    if !exists(&install.python) {
        missing.push(format!("python ({})", install.python.display()));
    }
    if !exists(&install.idf_py()) {
        missing.push(format!("idf.py ({})", install.idf_py().display()));
    }
    if !exists(&install.activation_script) {
        missing.push(format!(
            "activation script ({})",
            install.activation_script.display()
        ));
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(IdfEnvError::IncompleteInstall {
            name: install.name.clone(),
            missing,
        })
    }
}

pub fn validate_install(install: &IdfInstall) -> Result<()> {
    validate_install_with(install, |p| p.exists())
}

// ---------- activation ----------

/// The one argument that makes the vendor activation script print its
/// environment and exit, instead of activating a shell.
pub fn activation_args() -> [&'static str; 1] {
    ["-e"]
}

/// Keys the activation script prints that must never become environment
/// variables. `SYSTEM_PATH` is a snapshot of the user's `PATH` taken when the
/// installer ran; honouring it would resurrect a months-old `PATH`.
const DROPPED_KEYS: &[&str] = &["SYSTEM_PATH"];

/// Variables without which `idf.py` cannot run. `ESP_IDF_VERSION` is the
/// load-bearing one: the component manager coerces it unconditionally and dies
/// with a `TypeError` if it is absent, so checking here converts an
/// uninterpretable Python traceback into a sentence.
const REQUIRED_KEYS: &[&str] = &["IDF_PATH", "ESP_IDF_VERSION"];

/// Turn the activation script's `KEY=VALUE` output into an environment map.
///
/// `inherited_path` is the `PATH` of the process that will spawn `idf.py`. The
/// script prints a *complete* IDF-first `PATH` with no `$PATH` tail, so
/// without appending this the child would lose `/bin`, `/usr/bin` and
/// everything else — this reproduces what the script's own
/// `export PATH="…:$PATH"` does when sourced.
pub fn parse_activation_output(
    stdout: &str,
    inherited_path: &str,
    name: &str,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for line in stdout.lines() {
        // Splitting on the *first* `=` keeps values that contain one, such as
        // IDF_COMPONENT_LOCAL_STORAGE_URL=file:///…, intact. A line with no
        // `=` at all is a diagnostic, not a variable, and is dropped rather
        // than turned into a bogus key.
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || DROPPED_KEYS.contains(&key) {
            continue;
        }
        env.insert(key.to_string(), value.to_string());
    }

    if let Some(path) = env.get("PATH").cloned() {
        if !inherited_path.is_empty() {
            env.insert("PATH".to_string(), format!("{path}:{inherited_path}"));
        }
    }

    let missing: Vec<String> = REQUIRED_KEYS
        .iter()
        .filter(|k| !env.contains_key(**k))
        .map(|k| k.to_string())
        .collect();
    if !missing.is_empty() {
        return Err(IdfEnvError::ActivationIncomplete {
            name: name.to_string(),
            missing,
        });
    }
    Ok(env)
}

/// Cross-check that the script and the registry describe the same install.
/// A mismatch means the registry is stale, and building against whichever one
/// happened to win would be silently wrong.
pub fn check_install_matches(env: &BTreeMap<String, String>, install: &IdfInstall) -> Result<()> {
    let script = PathBuf::from(env.get("IDF_PATH").map(String::as_str).unwrap_or(""));
    if script != install.path {
        return Err(IdfEnvError::RegistryScriptMismatch {
            registry: install.path.clone(),
            script,
        });
    }
    Ok(())
}

/// Run the vendor activation script and return the environment it publishes.
pub fn activate(install: &IdfInstall, inherited_path: &str) -> Result<BTreeMap<String, String>> {
    validate_install(install)?;

    // Absolute path, never `sh` from PATH: this runs before we have adjusted
    // any environment, and the thing it produces is what we then trust.
    let out = std::process::Command::new("/bin/sh")
        .arg(&install.activation_script)
        .args(activation_args())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                IdfEnvError::ShellMissing
            } else {
                IdfEnvError::RegistryUnreadable {
                    path: install.activation_script.clone(),
                    source: e,
                }
            }
        })?;

    if !out.status.success() {
        return Err(IdfEnvError::ActivationFailed {
            name: install.name.clone(),
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }

    let env = parse_activation_output(
        &String::from_utf8_lossy(&out.stdout),
        inherited_path,
        &install.name,
    )?;
    check_install_matches(&env, install)?;
    Ok(env)
}

// ---------- toolchain shadowing ----------

/// A tool the activated `PATH` resolves outside ESP-IDF even though ESP-IDF
/// ships a pinned copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadowed {
    pub tool: String,
    /// What `PATH` actually finds first.
    pub found: PathBuf,
    /// The copy ESP-IDF pinned and expected to be used.
    pub pinned: PathBuf,
}

/// Tools ESP-IDF pins that a system copy commonly shadows.
const SHADOWABLE: &[&str] = &["cmake", "ninja"];

/// Report tools whose first hit on `path` is outside `idf_tools_path` while a
/// copy *inside* it exists further along.
///
/// This is a real condition rather than a hypothetical: the vendor activation
/// script places `/usr/bin` ahead of its own pinned tool directories, so a
/// system CMake wins. We report it and leave `PATH` alone — silently
/// reordering would make builds here differ from builds in the user's own
/// terminal, which is the worst kind of bug report to receive.
pub fn shadowed_tools(
    path: &str,
    idf_tools_path: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Vec<Shadowed> {
    let dirs: Vec<&str> = path.split(':').filter(|d| !d.is_empty()).collect();
    let mut out = Vec::new();
    for tool in SHADOWABLE {
        let mut first: Option<PathBuf> = None;
        let mut pinned: Option<PathBuf> = None;
        for dir in &dirs {
            let candidate = Path::new(dir).join(tool);
            if !exists(&candidate) {
                continue;
            }
            let inside = Path::new(dir).starts_with(idf_tools_path);
            if first.is_none() {
                first = Some(candidate.clone());
            }
            if inside && pinned.is_none() {
                pinned = Some(candidate);
            }
        }
        if let (Some(found), Some(pinned)) = (first, pinned) {
            if found != pinned {
                out.push(Shadowed {
                    tool: tool.to_string(),
                    found,
                    pinned,
                });
            }
        }
    }
    out
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape `/bin/sh <script> -e` prints, captured from a real
    /// install. Abridged in the middle of PATH only.
    const REAL_OUTPUT: &str = concat!(
        "PATH=/usr/bin:/home/u/.espressif/tools/cmake/4.0.3/bin:",
        "/home/u/.espressif/tools/xtensa-esp-elf/esp-15.2.0_20251204/xtensa-esp-elf/bin:",
        "/home/u/.espressif/tools/python/v6.0.1/venv/bin\n",
        "SYSTEM_PATH=/home/u/stale/from/install/time:/usr/bin\n",
        "ESP_IDF_VERSION=6.0.1\n",
        "IDF_TOOLS_PATH=/home/u/.espressif/tools\n",
        "IDF_COMPONENT_LOCAL_STORAGE_URL=file:///home/u/.espressif/tools\n",
        "IDF_PATH=/home/u/.espressif/v6.0.1/esp-idf\n",
        "ESP_ROM_ELF_DIR=/home/u/.espressif/tools/esp-rom-elfs/20241011/\n",
        "IDF_PYTHON_ENV_PATH=/home/u/.espressif/tools/python/v6.0.1/venv\n",
    );

    fn install() -> IdfInstall {
        let reg = parse_registry(
            include_str!("testdata/eim_idf.json"),
            Path::new("/reg.json"),
        )
        .unwrap();
        reg.idf_installed[0].clone()
    }

    // ---------- activation output ----------

    #[test]
    fn the_inherited_path_is_appended_to_the_scripts_path() {
        // The script prints a complete IDF-first PATH with no `$PATH` tail, so
        // without this the child loses every system directory it needs.
        let env =
            parse_activation_output(REAL_OUTPUT, "/usr/lib64/ccache:/usr/local/bin", "v6.0.1")
                .unwrap();
        let path = &env["PATH"];
        assert!(path.starts_with("/usr/bin:/home/u/.espressif/tools/cmake"));
        assert!(
            path.ends_with(":/usr/lib64/ccache:/usr/local/bin"),
            "inherited PATH must survive, got {path}"
        );
    }

    #[test]
    fn system_path_is_dropped_because_it_is_an_install_time_snapshot() {
        let env = parse_activation_output(REAL_OUTPUT, "/usr/bin", "v6.0.1").unwrap();
        assert!(!env.contains_key("SYSTEM_PATH"));
    }

    #[test]
    fn a_value_containing_an_equals_sign_survives_intact() {
        // Splitting on the last `=` would truncate this URL.
        let env = parse_activation_output(REAL_OUTPUT, "", "v6.0.1").unwrap();
        assert_eq!(
            env["IDF_COMPONENT_LOCAL_STORAGE_URL"],
            "file:///home/u/.espressif/tools"
        );
    }

    #[test]
    fn a_line_without_an_equals_sign_does_not_become_a_variable() {
        let noisy = format!("WARNING: something happened\n{REAL_OUTPUT}");
        let env = parse_activation_output(&noisy, "", "v6.0.1").unwrap();
        assert!(!env.contains_key("WARNING: something happened"));
        assert_eq!(env["ESP_IDF_VERSION"], "6.0.1");
    }

    #[test]
    fn a_missing_esp_idf_version_is_refused_here_rather_than_by_python() {
        // Without it, idf.py dies inside the component manager with a
        // TypeError from Version.coerce(None), which tells the user nothing.
        let without = REAL_OUTPUT.replace("ESP_IDF_VERSION=6.0.1\n", "");
        let err = parse_activation_output(&without, "", "v6.0.1").unwrap_err();
        assert!(
            matches!(&err, IdfEnvError::ActivationIncomplete { missing, .. }
                     if missing.iter().any(|m| m == "ESP_IDF_VERSION")),
            "got {err:?}"
        );
    }

    #[test]
    fn a_registry_that_disagrees_with_its_script_is_refused() {
        let env = parse_activation_output(REAL_OUTPUT, "", "v6.0.1").unwrap();
        let mut other = install();
        other.path = PathBuf::from("/somewhere/else");
        assert!(matches!(
            check_install_matches(&env, &other),
            Err(IdfEnvError::RegistryScriptMismatch { .. })
        ));
        assert!(check_install_matches(&env, &install()).is_ok());
    }

    // ---------- registry ----------

    #[test]
    fn the_registry_selection_wins_over_ordering() {
        let reg =
            parse_registry(include_str!("testdata/eim_idf.json"), Path::new("/r")).unwrap();
        assert_eq!(select_install(&reg, None).unwrap().name, "v6.0.1");
    }

    #[test]
    fn an_unknown_preference_names_what_is_available() {
        let reg =
            parse_registry(include_str!("testdata/eim_idf.json"), Path::new("/r")).unwrap();
        let err = select_install(&reg, Some("v5.0")).unwrap_err();
        match err {
            IdfEnvError::UnknownInstall { wanted, available } => {
                assert_eq!(wanted, "v5.0");
                assert_eq!(available, vec!["v6.0.1".to_string()]);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_registry_with_no_installations_is_an_error_not_an_empty_list() {
        let err = parse_registry(r#"{"idfInstalled":[]}"#, Path::new("/r")).unwrap_err();
        assert!(matches!(err, IdfEnvError::NoInstalls { .. }));
    }

    #[test]
    fn several_installs_and_no_selection_refuses_rather_than_guessing() {
        let json = r#"{"idfInstalled":[
            {"id":"a","name":"v5.4","path":"/a","python":"/a/p","activationScript":"/a/s"},
            {"id":"b","name":"v6.0.1","path":"/b","python":"/b/p","activationScript":"/b/s"}
        ]}"#;
        let reg = parse_registry(json, Path::new("/r")).unwrap();
        match select_install(&reg, None).unwrap_err() {
            IdfEnvError::NoSelection { available } => assert_eq!(available, vec!["v5.4", "v6.0.1"]),
            other => panic!("got {other:?}"),
        }
        // ...but an explicit preference still resolves.
        assert_eq!(select_install(&reg, Some("v6.0.1")).unwrap().id, "b");
    }

    // ---------- the decoy guard ----------

    #[test]
    fn an_incomplete_install_is_refused_without_running_anything() {
        // This is the rule that excludes a half-installed IDF tree. It is
        // reached by `stat` alone: `validate_install_with` takes no command
        // runner at all, so there is nothing it *could* spawn.
        let probed = std::cell::RefCell::new(Vec::new());
        let err = validate_install_with(&install(), |p| {
            probed.borrow_mut().push(p.to_path_buf());
            false
        })
        .unwrap_err();
        match err {
            IdfEnvError::IncompleteInstall { name, missing } => {
                assert_eq!(name, "v6.0.1");
                assert_eq!(missing.len(), 3, "python, idf.py and the script");
            }
            other => panic!("got {other:?}"),
        }
        assert_eq!(probed.borrow().len(), 3);
    }

    #[test]
    fn a_complete_install_passes_validation() {
        assert!(validate_install_with(&install(), |_| true).is_ok());
    }

    // ---------- shadowing ----------

    #[test]
    fn a_system_cmake_ahead_of_the_pinned_one_is_reported() {
        let tools = Path::new("/idf/tools");
        let path = "/usr/bin:/idf/tools/cmake/4.0.3/bin:/idf/tools/ninja/1.12.1";
        let found = shadowed_tools(path, tools, |p| {
            matches!(
                p.to_str().unwrap(),
                "/usr/bin/cmake"
                    | "/usr/bin/ninja"
                    | "/idf/tools/cmake/4.0.3/bin/cmake"
                    | "/idf/tools/ninja/1.12.1/ninja"
            )
        });
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].tool, "cmake");
        assert_eq!(found[0].found, PathBuf::from("/usr/bin/cmake"));
        assert_eq!(
            found[0].pinned,
            PathBuf::from("/idf/tools/cmake/4.0.3/bin/cmake")
        );
    }

    #[test]
    fn the_pinned_tools_first_reports_nothing() {
        let tools = Path::new("/idf/tools");
        let path = "/idf/tools/cmake/4.0.3/bin:/usr/bin";
        let found = shadowed_tools(path, tools, |p| {
            matches!(
                p.to_str().unwrap(),
                "/usr/bin/cmake" | "/idf/tools/cmake/4.0.3/bin/cmake"
            )
        });
        assert!(found.is_empty(), "got {found:?}");
    }

    #[test]
    fn a_tool_esp_idf_does_not_pin_is_not_reported() {
        // Only a tool with an IDF copy *available* can be shadowed; a system
        // cmake with no pinned counterpart is simply the cmake we use.
        let found = shadowed_tools("/usr/bin", Path::new("/idf/tools"), |p| {
            p.to_str().unwrap() == "/usr/bin/cmake"
        });
        assert!(found.is_empty(), "got {found:?}");
    }
}
