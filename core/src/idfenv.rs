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
//! ## Manual installs (`install.sh`)
//!
//! Not everyone uses the installer. The documented alternative — clone
//! `esp-idf`, run `install.sh`, source `export.sh` — never writes
//! `eim_idf.json`; on such a machine the installer registry is absent, or
//! present with an empty `idfInstalled` list (the `eim` binary was run once
//! and installed nothing). What `install.sh` *does* leave behind is
//! `$IDF_TOOLS_PATH/idf-env.json` (default `~/.espressif/idf-env.json`),
//! keyed by install with the checkout's version and path, and a venv under
//! `$IDF_TOOLS_PATH/python_env/idf<major.minor>_py<x.y>_env`.
//!
//! For those, [`discover`] falls back to `idf-env.json` and activation uses
//! the checkout's own `idf_tools.py export --format key-value`. The rustdoc
//! above says that command is unusable, and for *installer* layouts it is —
//! their tools live where `idf_tools.py` does not look. For a manual install
//! it is the exact code path `export.sh` runs, so it is the right one. Its
//! output differs from the installer script's in three ways, all handled and
//! tested: the `PATH` ends in a literal `$PATH` placeholder rather than a
//! complete list, it does not print `IDF_PATH` (we know it), and it prints an
//! `IDF_DEACTIVATE_FILE_PATH` naming a throwaway temp file.
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

/// How an installation was set up, which decides how it is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum InstallSource {
    /// Registered by the Espressif installer in `eim_idf.json`. Activated by
    /// running its vendor activation script with `-e`.
    #[default]
    Installer,
    /// A checkout set up by `install.sh`, recorded in `idf-env.json`.
    /// Activated by the checkout's own `idf_tools.py export`.
    Manual,
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
    /// What publishes this install's environment: the installer's shell
    /// script, or — for a manual install — the checkout's `idf_tools.py`.
    pub activation_script: PathBuf,
    #[serde(default)]
    pub idf_tools_path: PathBuf,
    /// Not in the file: every entry read from `eim_idf.json` is an
    /// [`InstallSource::Installer`], and manual entries are built in code.
    #[serde(skip)]
    pub source: InstallSource,
}

impl IdfInstall {
    /// The `idf.py` entry point for this install.
    pub fn idf_py(&self) -> PathBuf {
        self.path.join("tools").join("idf.py")
    }
}

/// The name of `install.sh`'s record file, relative to `IDF_TOOLS_PATH`.
pub const IDF_ENV_FILE: &str = "idf-env.json";

/// Where installations are recorded on one machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryPaths {
    /// The installer's `eim_idf.json`.
    pub installer: PathBuf,
    /// `IDF_TOOLS_PATH`: holds `idf-env.json` and `python_env/`.
    pub tools_dir: PathBuf,
}

impl RegistryPaths {
    /// The defaults under a home directory: `~/.espressif/tools/eim_idf.json`
    /// and `~/.espressif`.
    pub fn under_home(home: &Path) -> Self {
        let espressif = home.join(".espressif");
        Self {
            installer: espressif.join("tools").join("eim_idf.json"),
            tools_dir: espressif,
        }
    }

    /// The manual-install record file.
    pub fn idf_env(&self) -> PathBuf {
        self.tools_dir.join(IDF_ENV_FILE)
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
    #[error(
        "ESP-IDF is not installed — nothing is recorded in the installer registry at {installer} or by install.sh at {manual}"
    )]
    NotInstalled { installer: PathBuf, manual: PathBuf },
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
            E::NotInstalled { .. }
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

/// `idf-env.json`, as `install.sh` (`idf_tools.py`) writes it.
///
/// `idfInstalled` is a map keyed by install id. `idf_tools.py` itself pops a
/// stray `sha` key and drops any record it cannot read, so the values are
/// taken as loose JSON and filtered the same way rather than failing the whole
/// file on one odd entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdfEnvFile {
    #[serde(default)]
    idf_installed: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    idf_selected_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdfEnvRecord {
    version: String,
    path: PathBuf,
}

/// The venv directory `install.sh` creates for an IDF version, without the
/// Python version suffix: `idf6.1_py` matches `idf6.1_py3.13_env`.
fn venv_prefix(version: &str) -> String {
    format!("idf{}_py", version.trim_start_matches('v'))
}

/// Parse `install.sh`'s record file into the same shape the installer
/// registry gives, so selection and validation are shared.
///
/// `venvs` lists the entries of `<tools_dir>/python_env`, injected so this
/// stays pure. Each record's interpreter is the venv named for its IDF
/// version; with several (one per Python the user ran `install.sh` with) the
/// highest-sorting wins here and the export step's own
/// `IDF_PYTHON_ENV_PATH` corrects it — see [`venv_python`]. With none, the
/// interpreter is the path that *would* exist, so validation names it.
pub fn parse_idf_env(
    json: &str,
    path: &Path,
    tools_dir: &Path,
    venvs: &[PathBuf],
) -> Result<EimRegistry> {
    let file: IdfEnvFile =
        serde_json::from_str(json).map_err(|source| IdfEnvError::RegistryMalformed {
            path: path.to_path_buf(),
            source,
        })?;
    let mut installs = Vec::new();
    for (id, value) in file.idf_installed {
        let Ok(rec) = serde_json::from_value::<IdfEnvRecord>(value) else {
            continue;
        };
        let prefix = venv_prefix(&rec.version);
        let venv = venvs
            .iter()
            .filter(|v| {
                v.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix) && n.ends_with("_env"))
            })
            .max()
            .cloned()
            .unwrap_or_else(|| tools_dir.join("python_env").join(format!("{prefix}*_env")));
        installs.push(IdfInstall {
            id,
            name: format!("v{}", rec.version.trim_start_matches('v')),
            activation_script: rec.path.join("tools").join("idf_tools.py"),
            path: rec.path,
            python: venv.join("bin").join("python"),
            idf_tools_path: tools_dir.to_path_buf(),
            source: InstallSource::Manual,
        });
    }
    if installs.is_empty() {
        return Err(IdfEnvError::NoInstalls {
            path: path.to_path_buf(),
        });
    }
    Ok(EimRegistry {
        idf_installed: installs,
        idf_selected_id: file.idf_selected_id.filter(|s| !s.is_empty()),
    })
}

/// Find every recorded installation on this machine.
///
/// The installer registry wins when it lists anything. When it is absent or
/// empty — the `eim` binary was never run, or was run and installed nothing —
/// `idf-env.json` is consulted for a manual install. Only when neither
/// records an installation is ESP-IDF "not installed". A registry that exists
/// but cannot be read or parsed is reported as such, not skipped: silently
/// falling through to a different install would build against the wrong tree.
pub fn discover(paths: &RegistryPaths) -> Result<EimRegistry> {
    let read = |p: &Path| -> Result<Option<String>> {
        match std::fs::read_to_string(p) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(IdfEnvError::RegistryUnreadable {
                path: p.to_path_buf(),
                source,
            }),
        }
    };

    if let Some(json) = read(&paths.installer)? {
        match parse_registry(&json, &paths.installer) {
            Err(IdfEnvError::NoInstalls { .. }) => {}
            other => return other,
        }
    }

    let manual = paths.idf_env();
    if let Some(json) = read(&manual)? {
        let venvs: Vec<PathBuf> = std::fs::read_dir(paths.tools_dir.join("python_env"))
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        match parse_idf_env(&json, &manual, &paths.tools_dir, &venvs) {
            Err(IdfEnvError::NoInstalls { .. }) => {}
            other => return other,
        }
    }

    Err(IdfEnvError::NotInstalled {
        installer: paths.installer.clone(),
        manual,
    })
}

/// Choose which installation to use.
///
/// Order: an explicit preference (matched against `id` then `name`), then the
/// registry's own selection, then a sole entry. More than one with no way to
/// choose is an error rather than a guess — picking silently would mean
/// building against a different IDF than the user's terminal uses.
///
/// A name matches with or without its leading `v`: the installer says
/// `v6.0.1`, `idf-env.json` says `6.1`, and a user setting
/// `BANCADA_IDF_VERSION` should not have to know which wrote the record.
pub fn select_install<'a>(reg: &'a EimRegistry, prefer: Option<&str>) -> Result<&'a IdfInstall> {
    let names = || reg.idf_installed.iter().map(|i| i.name.clone()).collect();
    if let Some(want) = prefer {
        let bare = want.trim_start_matches('v');
        return reg
            .idf_installed
            .iter()
            .find(|i| i.id == want || i.name.trim_start_matches('v') == bare)
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
        _ => Err(IdfEnvError::NoSelection { available: names() }),
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

/// The `idf_tools.py` subcommand that prints a manual install's environment
/// as `KEY=VALUE` lines — the same call `export.sh` makes, minus the shell.
pub fn export_args() -> [&'static str; 3] {
    ["export", "--format", "key-value"]
}

/// Keys the activation output may carry that must never become environment
/// variables. `SYSTEM_PATH` is a snapshot of the user's `PATH` taken when the
/// installer ran; honouring it would resurrect a months-old `PATH`.
/// `IDF_DEACTIVATE_FILE_PATH` names a temp file `idf_tools.py export` writes
/// so a later `export --deactivate` can undo itself; nothing here deactivates.
const DROPPED_KEYS: &[&str] = &["SYSTEM_PATH", "IDF_DEACTIVATE_FILE_PATH"];

/// What `idf_tools.py export` prints in place of the caller's `PATH`, since
/// its `key-value` format is meant to be expanded by a shell.
const PATH_PLACEHOLDER: &str = "$PATH";

/// Variables without which `idf.py` cannot run. `ESP_IDF_VERSION` is the
/// load-bearing one: the component manager coerces it unconditionally and dies
/// with a `TypeError` if it is absent, so checking here converts an
/// uninterpretable Python traceback into a sentence.
const REQUIRED_KEYS: &[&str] = &["IDF_PATH", "ESP_IDF_VERSION"];

/// Turn the activation script's `KEY=VALUE` output into an environment map.
///
/// `inherited_path` is the `PATH` of the process that will spawn `idf.py`. The
/// installer script prints a *complete* IDF-first `PATH` with no `$PATH` tail,
/// so without appending this the child would lose `/bin`, `/usr/bin` and
/// everything else — this reproduces what the script's own
/// `export PATH="…:$PATH"` does when sourced. `idf_tools.py export` instead
/// ends its `PATH` with a literal `$PATH` entry; that placeholder is replaced
/// rather than appended to, so the child does not carry a directory literally
/// named `$PATH`.
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
        let merged = if path.split(':').any(|d| d == PATH_PLACEHOLDER) {
            path.split(':')
                .filter(|d| !d.is_empty())
                .flat_map(|d| {
                    if d == PATH_PLACEHOLDER {
                        inherited_path
                            .split(':')
                            .filter(|d| !d.is_empty())
                            .collect()
                    } else {
                        vec![d]
                    }
                })
                .collect::<Vec<&str>>()
                .join(":")
        } else if inherited_path.is_empty() {
            path
        } else {
            format!("{path}:{inherited_path}")
        };
        env.insert("PATH".to_string(), merged);
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

/// The lines a manual install's export does not print but `idf.py` needs.
///
/// `IDF_PATH` is the checkout we resolved the install from, and
/// `IDF_TOOLS_PATH` is where `idf-env.json` was found — the export was run
/// with both set, so these are the values it used. They go *before* the
/// script's output so that anything it does print wins, and
/// [`check_install_matches`] still catches a disagreement.
pub fn manual_env_preamble(install: &IdfInstall) -> String {
    format!(
        "IDF_PATH={}\nIDF_TOOLS_PATH={}\n",
        install.path.display(),
        install.idf_tools_path.display()
    )
}

/// The interpreter to run `idf.py` with, once the environment is known.
///
/// The activation output's `IDF_PYTHON_ENV_PATH` is authoritative: for a
/// manual install the registry-side guess may have picked among several
/// venvs, and for an installer one it simply agrees. `fallback` is used when
/// the output names no venv or names one that is not there.
pub fn venv_python(env: &BTreeMap<String, String>, fallback: &Path) -> PathBuf {
    env.get("IDF_PYTHON_ENV_PATH")
        .map(|p| Path::new(p).join("bin").join("python"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| fallback.to_path_buf())
}

/// Run the install's activation and return the environment it publishes.
///
/// An installer entry runs its vendor script under `/bin/sh -e`; a manual
/// entry runs the checkout's `idf_tools.py export` under the install's own
/// interpreter, with `IDF_PATH` and `IDF_TOOLS_PATH` pinned to what the
/// record says so it cannot wander to another checkout via the ambient
/// environment.
pub fn activate(install: &IdfInstall, inherited_path: &str) -> Result<BTreeMap<String, String>> {
    validate_install(install)?;

    let mut cmd = match install.source {
        InstallSource::Installer => {
            // Absolute path, never `sh` from PATH: this runs before we have
            // adjusted any environment, and the thing it produces is what we
            // then trust.
            let mut c = std::process::Command::new("/bin/sh");
            c.arg(&install.activation_script).args(activation_args());
            c
        }
        InstallSource::Manual => {
            let mut c = std::process::Command::new(&install.python);
            c.arg(&install.activation_script)
                .args(export_args())
                .env("IDF_PATH", &install.path)
                .env("IDF_TOOLS_PATH", &install.idf_tools_path)
                .env_remove("IDF_PYTHON_ENV_PATH");
            c
        }
    };
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            match install.source {
                InstallSource::Installer => IdfEnvError::ShellMissing,
                InstallSource::Manual => IdfEnvError::IncompleteInstall {
                    name: install.name.clone(),
                    missing: vec![format!("python ({})", install.python.display())],
                },
            }
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

    let mut stdout = String::new();
    if install.source == InstallSource::Manual {
        stdout.push_str(&manual_env_preamble(install));
    }
    stdout.push_str(&String::from_utf8_lossy(&out.stdout));
    let env = parse_activation_output(&stdout, inherited_path, &install.name)?;
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

    /// What `idf_tools.py export --format key-value` prints for a manual
    /// install, captured from a real `install.sh` checkout. Note what is
    /// *not* here: no `IDF_PATH`, no `IDF_TOOLS_PATH`, and a `PATH` that ends
    /// in a literal `$PATH`.
    const EXPORT_OUTPUT: &str = concat!(
        "OPENOCD_SCRIPTS=/home/u/.espressif/tools/openocd-esp32/v0.12.0/share/openocd/scripts\n",
        "ESP_ROM_ELF_DIR=/home/u/.espressif/tools/esp-rom-elfs/20241011/\n",
        "IDF_PYTHON_ENV_PATH=/home/u/.espressif/python_env/idf6.1_py3.13_env\n",
        "ESP_IDF_VERSION=6.1\n",
        "PATH=/home/u/.espressif/tools/xtensa-esp-elf/esp-15.2.0/xtensa-esp-elf/bin:",
        "/home/u/.espressif/python_env/idf6.1_py3.13_env/bin:/home/u/esp/esp-idf/tools:$PATH\n",
        "IDF_DEACTIVATE_FILE_PATH=/tmp/tmpoa3itu8eidf_504322\n",
    );

    fn manual_venvs() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/home/u/.espressif/python_env/idf5.4_py3.12_env"),
            PathBuf::from("/home/u/.espressif/python_env/idf6.1_py3.13_env"),
        ]
    }

    fn manual_install() -> IdfInstall {
        let reg = parse_idf_env(
            include_str!("testdata/idf-env.json"),
            Path::new("/home/u/.espressif/idf-env.json"),
            Path::new("/home/u/.espressif"),
            &manual_venvs(),
        )
        .unwrap();
        reg.idf_installed[0].clone()
    }

    /// The export output the way [`activate`] hands it to the parser for a
    /// manual install: preamble first, then what `idf_tools.py` printed.
    fn parse_manual_export(inherited_path: &str) -> Result<BTreeMap<String, String>> {
        let full = format!(
            "{}{}",
            manual_env_preamble(&manual_install()),
            EXPORT_OUTPUT
        );
        parse_activation_output(&full, inherited_path, "v6.1")
    }

    #[test]
    fn the_export_placeholder_is_replaced_by_the_inherited_path_not_appended_to() {
        let env = parse_manual_export("/usr/local/bin:/usr/bin").unwrap();
        let path = &env["PATH"];
        assert!(
            !path.contains("$PATH"),
            "the placeholder must not survive as a directory: {path}"
        );
        assert!(
            path.ends_with("/home/u/esp/esp-idf/tools:/usr/local/bin:/usr/bin"),
            "inherited PATH must take the placeholder's place, got {path}"
        );
    }

    #[test]
    fn the_export_placeholder_with_an_empty_inherited_path_vanishes() {
        let env = parse_manual_export("").unwrap();
        assert!(env["PATH"].ends_with("/home/u/esp/esp-idf/tools"));
    }

    #[test]
    fn the_deactivate_temp_file_is_not_exported() {
        let env = parse_manual_export("").unwrap();
        assert!(!env.contains_key("IDF_DEACTIVATE_FILE_PATH"));
    }

    #[test]
    fn a_manual_export_lacks_idf_path_until_the_preamble_supplies_it() {
        // Bare, the export is refused for the same reason a bare installer
        // script would be: idf.py needs IDF_PATH.
        let err = parse_activation_output(EXPORT_OUTPUT, "", "v6.1").unwrap_err();
        assert!(
            matches!(&err, IdfEnvError::ActivationIncomplete { missing, .. }
                         if missing == &["IDF_PATH".to_string()])
        );

        let install = manual_install();
        let full = format!("{}{}", manual_env_preamble(&install), EXPORT_OUTPUT);
        let env = parse_activation_output(&full, "", "v6.1").unwrap();
        assert_eq!(env["IDF_PATH"], "/home/u/esp/esp-idf");
        assert_eq!(env["IDF_TOOLS_PATH"], "/home/u/.espressif");
        assert!(check_install_matches(&env, &install).is_ok());
    }

    #[test]
    fn the_scripts_own_idf_path_wins_over_the_preamble() {
        // The preamble is a default, not an override: a script that does
        // print IDF_PATH is the authority, and a disagreement is then caught.
        let install = manual_install();
        let full = format!(
            "{}{}IDF_PATH=/elsewhere/esp-idf\n",
            manual_env_preamble(&install),
            EXPORT_OUTPUT
        );
        let env = parse_activation_output(&full, "", "v6.1").unwrap();
        assert_eq!(env["IDF_PATH"], "/elsewhere/esp-idf");
        assert!(matches!(
            check_install_matches(&env, &install),
            Err(IdfEnvError::RegistryScriptMismatch { .. })
        ));
    }

    #[test]
    fn the_exports_venv_is_preferred_over_the_registry_guess_when_present() {
        let tmp = tempfile::TempDir::new().unwrap();
        let venv = tmp.path().join("idf6.1_py3.13_env");
        std::fs::create_dir_all(venv.join("bin")).unwrap();
        std::fs::write(venv.join("bin").join("python"), "").unwrap();
        let mut env = BTreeMap::new();
        env.insert(
            "IDF_PYTHON_ENV_PATH".to_string(),
            venv.to_str().unwrap().to_string(),
        );
        let fallback = Path::new("/guess/bin/python");
        assert_eq!(venv_python(&env, fallback), venv.join("bin").join("python"));

        // Named but absent, or not named at all: the fallback stands.
        env.insert(
            "IDF_PYTHON_ENV_PATH".to_string(),
            "/nonexistent/venv".to_string(),
        );
        assert_eq!(venv_python(&env, fallback), fallback);
        assert_eq!(venv_python(&BTreeMap::new(), fallback), fallback);
    }

    // ---------- the manual-install record ----------

    #[test]
    fn a_manual_record_becomes_an_install_activated_by_idf_tools() {
        let i = manual_install();
        assert_eq!(i.source, InstallSource::Manual);
        assert_eq!(i.name, "v6.1");
        assert_eq!(i.id, "/home/u/esp/esp-idf-v6.1");
        assert_eq!(i.path, PathBuf::from("/home/u/esp/esp-idf"));
        assert_eq!(
            i.activation_script,
            PathBuf::from("/home/u/esp/esp-idf/tools/idf_tools.py")
        );
        assert_eq!(i.idf_tools_path, PathBuf::from("/home/u/.espressif"));
        // The venv for *this* IDF version, not the 5.4 one beside it.
        assert_eq!(
            i.python,
            PathBuf::from("/home/u/.espressif/python_env/idf6.1_py3.13_env/bin/python")
        );
    }

    #[test]
    fn a_manual_record_without_a_venv_names_the_missing_interpreter() {
        // install.sh not run (or run for another version): validation must
        // say which interpreter is missing rather than fail at spawn time.
        let reg = parse_idf_env(
            include_str!("testdata/idf-env.json"),
            Path::new("/r"),
            Path::new("/home/u/.espressif"),
            &[],
        )
        .unwrap();
        let err = validate_install_with(&reg.idf_installed[0], |p| {
            !p.to_string_lossy().contains("python_env")
        })
        .unwrap_err();
        match err {
            IdfEnvError::IncompleteInstall { missing, .. } => {
                assert_eq!(missing.len(), 1, "{missing:?}");
                assert!(
                    missing[0].contains("python_env/idf6.1_py*_env"),
                    "{missing:?}"
                );
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_manual_record_file_with_junk_entries_keeps_the_good_ones() {
        // idf_tools.py pops a `sha` key and drops unreadable records; so do we.
        let json = r#"{"idfInstalled":{
            "sha": "abc123",
            "broken": {"version": "6.0"},
            "/home/u/esp/esp-idf": {"version": "6.1", "path": "/home/u/esp/esp-idf"}
        }}"#;
        let reg = parse_idf_env(json, Path::new("/r"), Path::new("/t"), &[]).unwrap();
        assert_eq!(reg.idf_installed.len(), 1);
        assert_eq!(reg.idf_installed[0].name, "v6.1");
    }

    #[test]
    fn a_manual_record_file_with_nothing_usable_is_no_installs() {
        let err = parse_idf_env(
            r#"{"idfInstalled":{}}"#,
            Path::new("/r"),
            Path::new("/t"),
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, IdfEnvError::NoInstalls { .. }));
    }

    #[test]
    fn a_version_preference_matches_with_or_without_the_v() {
        let reg = parse_idf_env(
            include_str!("testdata/idf-env.json"),
            Path::new("/r"),
            Path::new("/t"),
            &[],
        )
        .unwrap();
        assert_eq!(select_install(&reg, Some("6.1")).unwrap().name, "v6.1");
        assert_eq!(select_install(&reg, Some("v6.1")).unwrap().name, "v6.1");
        assert!(select_install(&reg, Some("6.0")).is_err());
    }

    // ---------- discovery ----------

    fn paths_in(tmp: &tempfile::TempDir) -> RegistryPaths {
        RegistryPaths::under_home(tmp.path())
    }

    #[test]
    fn an_empty_installer_registry_falls_through_to_a_manual_install() {
        // The bench case: `eim` was run once and installed nothing, then
        // ESP-IDF was set up by hand. The empty registry must not be the
        // last word.
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = paths_in(&tmp);
        std::fs::create_dir_all(paths.installer.parent().unwrap()).unwrap();
        std::fs::write(
            &paths.installer,
            r#"{"idfInstalled":[],"idfSelectedId":"","version":"2.0"}"#,
        )
        .unwrap();
        std::fs::write(paths.idf_env(), include_str!("testdata/idf-env.json")).unwrap();

        let reg = discover(&paths).unwrap();
        assert_eq!(reg.idf_installed.len(), 1);
        assert_eq!(reg.idf_installed[0].source, InstallSource::Manual);
    }

    #[test]
    fn a_populated_installer_registry_wins_over_a_manual_record() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = paths_in(&tmp);
        std::fs::create_dir_all(paths.installer.parent().unwrap()).unwrap();
        std::fs::write(&paths.installer, include_str!("testdata/eim_idf.json")).unwrap();
        std::fs::write(paths.idf_env(), include_str!("testdata/idf-env.json")).unwrap();

        let reg = discover(&paths).unwrap();
        assert_eq!(reg.idf_installed[0].source, InstallSource::Installer);
        assert_eq!(reg.idf_installed[0].name, "v6.0.1");
    }

    #[test]
    fn a_malformed_installer_registry_is_reported_not_skipped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = paths_in(&tmp);
        std::fs::create_dir_all(paths.installer.parent().unwrap()).unwrap();
        std::fs::write(&paths.installer, "{not json").unwrap();
        std::fs::write(paths.idf_env(), include_str!("testdata/idf-env.json")).unwrap();
        assert!(matches!(
            discover(&paths),
            Err(IdfEnvError::RegistryMalformed { .. })
        ));
    }

    #[test]
    fn nothing_recorded_anywhere_names_both_places_looked() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = paths_in(&tmp);
        let err = discover(&paths).unwrap_err();
        match &err {
            IdfEnvError::NotInstalled { installer, manual } => {
                assert_eq!(installer, &paths.installer);
                assert_eq!(manual, &paths.idf_env());
            }
            other => panic!("got {other:?}"),
        }
        let msg = err.to_string();
        assert!(
            msg.contains("eim_idf.json") && msg.contains("idf-env.json"),
            "{msg}"
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
        let reg = parse_registry(include_str!("testdata/eim_idf.json"), Path::new("/r")).unwrap();
        assert_eq!(select_install(&reg, None).unwrap().name, "v6.0.1");
    }

    #[test]
    fn an_unknown_preference_names_what_is_available() {
        let reg = parse_registry(include_str!("testdata/eim_idf.json"), Path::new("/r")).unwrap();
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
