//! Host-side ESP-IDF resolution: where the installation is, and caching the
//! environment it publishes.
//!
//! The pure half lives in [`bancada_core::idfenv`]; what is here is the part
//! that must know about *this machine* (the installer registry and
//! `install.sh`'s record file under `$HOME`) and about *this process* (a cache
//! with a lifetime).
//!
//! ## Lock discipline
//!
//! [`IdfEnvCache`] is a **leaf lock**: it is taken alone and nothing else is
//! acquired while it is held. It *is* held across the one brief `/bin/sh`
//! activation spawn (roughly 30 ms), because the cache needs `&mut self` to
//! store the result — but never across a build or a flash, which are the
//! operations measured in minutes.
//!
//! The ordering rule that matters: resolve the backend **before** taking the
//! build gate. Activation must never happen while the gate is held, or a
//! first-ever IDF build would make a concurrent Verify wait on a shell spawn.
//!
//! ## Laziness
//!
//! Nothing here runs at startup. A user with no ESP-IDF must not pay a shell
//! spawn, and — just as important — must not be told at every launch that a
//! toolchain they do not use is missing. Resolution happens the first time an
//! ESP-IDF project is actually opened or built.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bancada_core::idf::IdfCli;
use bancada_core::idfenv::{self, IdfEnvError};

/// Identity of a file for cache-invalidation purposes.
///
/// `mtime` alone is not enough: an installer rewriting a file within the same
/// second is plausible, and `len` comes free from the same `stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    mtime: SystemTime,
    len: u64,
}

fn stamp(path: &Path) -> Option<FileStamp> {
    let m = std::fs::metadata(path).ok()?;
    Some(FileStamp {
        mtime: m.modified().ok()?,
        len: m.len(),
    })
}

/// What the frontend is told about ESP-IDF availability.
///
/// A struct rather than a thrown string because there are three genuinely
/// different answers — absent, present-but-broken, and usable — and the UI
/// says something different for each. Mirrors the shape of the agent probe.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IdfProbe {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idf_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Tools ESP-IDF pins that the activated `PATH` resolves elsewhere.
    /// Reported, never corrected — see [`bancada_core::idfenv::shadowed_tools`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadowed: Vec<String>,
}

impl IdfProbe {
    fn failed(e: &IdfEnvError) -> Self {
        Self {
            ok: false,
            version: None,
            idf_path: None,
            error: Some(e.to_string()),
            shadowed: Vec::new(),
        }
    }
}

/// Where ESP-IDF installations are recorded on this machine: the installer's
/// `eim_idf.json`, and `install.sh`'s `idf-env.json` under `IDF_TOOLS_PATH`.
///
/// `BANCADA_IDF_REGISTRY` overrides the installer file and
/// `BANCADA_IDF_TOOLS_PATH` the tools directory, which is what the live tests
/// use. The ambient `IDF_PATH` is deliberately **not** consulted: a windowed
/// app launched from a desktop file has no shell environment, and on a
/// machine with more than one IDF tree the exported one is as likely to be a
/// half-installed checkout as the working install.
pub fn registry_paths() -> idfenv::RegistryPaths {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut paths = idfenv::RegistryPaths::under_home(Path::new(&home));
    if let Some(p) = std::env::var_os("BANCADA_IDF_REGISTRY") {
        paths.installer = PathBuf::from(p);
    }
    if let Some(p) = std::env::var_os("BANCADA_IDF_TOOLS_PATH") {
        paths.tools_dir = PathBuf::from(p);
    }
    paths
}

/// Identity of both record files at once. `None` for one that is absent —
/// which is itself a state worth keying on, since creating the file is how
/// an install appears.
type RegistryStamp = (Option<FileStamp>, Option<FileStamp>);

fn registry_stamp(paths: &idfenv::RegistryPaths) -> RegistryStamp {
    (stamp(&paths.installer), stamp(&paths.idf_env()))
}

/// Resolved ESP-IDF, cached against the files it was derived from.
#[derive(Debug, Default)]
pub struct IdfEnvCache {
    /// Keyed on both record files.
    install: Option<(RegistryStamp, Result<idfenv::IdfInstall, String>)>,
    /// Keyed on the activation script (or `idf_tools.py`, for a manual install).
    activated: Option<(FileStamp, Result<IdfCli, String>)>,
    targets: Option<Vec<String>>,
}

impl IdfEnvCache {
    /// The chosen installation, re-reading the records only when one changed.
    ///
    /// Negative results are cached under the same key, so a machine with no
    /// ESP-IDF does not respawn anything on every poll — but they expire the
    /// moment a file does change, so installing ESP-IDF takes effect without
    /// restarting Bancada.
    fn install(&mut self) -> Result<idfenv::IdfInstall, String> {
        let paths = registry_paths();
        let now = registry_stamp(&paths);
        if now == (None, None) {
            self.install = None;
            return Err(IdfEnvError::NotInstalled {
                manual: paths.idf_env(),
                installer: paths.installer,
            }
            .to_string());
        }
        if let Some((seen, cached)) = &self.install {
            if *seen == now {
                return cached.clone();
            }
        }
        let resolved = (|| {
            let reg = idfenv::discover(&paths).map_err(|e| e.to_string())?;
            let prefer = std::env::var("BANCADA_IDF_VERSION").ok();
            let install =
                idfenv::select_install(&reg, prefer.as_deref()).map_err(|e| e.to_string())?;
            idfenv::validate_install(install).map_err(|e| e.to_string())?;
            Ok(install.clone())
        })();
        self.install = Some((now, resolved.clone()));
        // A different install invalidates whatever we activated for the old one.
        self.activated = None;
        self.targets = None;
        resolved
    }

    /// A runnable `idf.py`, activating only when the script changed.
    pub fn cli(&mut self) -> Result<IdfCli, String> {
        let install = self.install()?;
        let script = install.activation_script.clone();
        let Some(now) = stamp(&script) else {
            return Err(IdfEnvError::IncompleteInstall {
                name: install.name.clone(),
                missing: vec![format!("activation script ({})", script.display())],
            }
            .to_string());
        };
        if let Some((seen, cached)) = &self.activated {
            if *seen == now {
                return cached.clone();
            }
        }
        let inherited = std::env::var("PATH").unwrap_or_default();
        let built = idfenv::activate(&install, &inherited)
            .map_err(|e| e.to_string())
            .map(|env| {
                let python = idfenv::venv_python(&env, &install.python);
                IdfCli::new(python, install.path.clone(), env)
            });
        self.activated = Some((now, built.clone()));
        self.targets = None;
        built
    }

    /// Chip targets this ESP-IDF supports, asked once per activation.
    ///
    /// Costs a Python start-up, so it is cached; it cannot change without the
    /// installation changing, which already invalidates this.
    pub fn targets(&mut self) -> Result<Vec<String>, String> {
        if let Some(t) = &self.targets {
            return Ok(t.clone());
        }
        let cli = self.cli()?;
        let list = cli.list_targets().map_err(|e| e.to_string())?;
        self.targets = Some(list.clone());
        Ok(list)
    }

    /// Is ESP-IDF usable, and if not, why?
    pub fn probe(&mut self) -> IdfProbe {
        let install = match self.install() {
            Ok(i) => i,
            Err(e) => {
                return IdfProbe {
                    ok: false,
                    version: None,
                    idf_path: None,
                    error: Some(e),
                    shadowed: Vec::new(),
                }
            }
        };
        match self.cli() {
            Ok(cli) => {
                let shadowed = cli
                    .env
                    .get("PATH")
                    .map(|p| {
                        idfenv::shadowed_tools(p, &install.idf_tools_path, |c| c.exists())
                            .into_iter()
                            .map(|s| {
                                format!(
                                    "{} resolves to {}, not ESP-IDF's {}",
                                    s.tool,
                                    s.found.display(),
                                    s.pinned.display()
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                IdfProbe {
                    ok: true,
                    version: Some(install.name.clone()),
                    idf_path: Some(install.path.display().to_string()),
                    error: None,
                    shadowed,
                }
            }
            Err(e) => IdfProbe {
                ok: false,
                version: Some(install.name.clone()),
                idf_path: Some(install.path.display().to_string()),
                error: Some(e),
                shadowed: Vec::new(),
            },
        }
    }
}

#[allow(dead_code)]
fn _assert_probe_shape(e: &IdfEnvError) -> IdfProbe {
    IdfProbe::failed(e)
}
