//! Which toolchain builds and flashes the open project.
//!
//! Bancada drives two: `arduino-cli` for sketches ([`crate::cli`]) and
//! `idf.py` for ESP-IDF projects ([`crate::idf`]). This module is the single
//! seam between them, and it is deliberately narrow — only *verify* and
//! *flash* are polymorphic, because only those two have a meaning in both
//! worlds.
//!
//! ## Why an enum and not a trait
//!
//! [`crate::cli::ArduinoCli::run_streaming`] takes `impl FnMut(OutputLine)`.
//! A `dyn Toolchain` is not object-safe with that signature, so a trait would
//! force `&mut dyn FnMut(OutputLine)` through every call site in the app for
//! no benefit. An enum also derives [`Clone`], which matters more than it
//! looks: the MCP tool context takes owned clones of everything at session
//! start precisely so a long build can never hold a lock the UI needs.
//!
//! The set is closed at two, and a third (PlatformIO) would be a
//! compiler-guided edit rather than a hunt for `impl` blocks.
//!
//! ## What is *not* here
//!
//! Board listing, core and library management stay on
//! [`crate::cli::ArduinoCli`]. They have no ESP-IDF analogue — "add a registry
//! library to a profile" is not a question `idf.py` can answer — and a trait
//! spanning them would be a wall of `unimplemented!()`. The serial monitor is
//! not here either, for the opposite reason: it belongs to *neither*
//! toolchain. The app opens the port itself (`serialport`), so an IDF-flashed
//! board and an Arduino one read identically.

use std::path::Path;

use crate::cli::ArduinoCli;
use crate::idf::IdfCli;
use crate::types::{OutputLine, RunResult};
use crate::Result;

/// Which toolchain, without carrying one.
///
/// Lives here rather than in [`crate::mcp`] so that module keeps depending on
/// nothing — it is a pure function of its inputs and must stay that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Arduino,
    Idf,
}

/// What to build, frozen for the duration of an operation.
///
/// The asymmetry is real rather than cosmetic. Arduino carries its board
/// identity on the command line, so `profile`/`fqbn` are *inputs*. ESP-IDF
/// keeps the target inside the project's `sdkconfig`, so `target` here is
/// **read, never sent** — it exists to label a flash tag, to show in the UI,
/// and to detect that the project changed underneath a live agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildSpec {
    Arduino {
        profile: Option<String>,
        fqbn: Option<String>,
    },
    Idf {
        target: String,
    },
}

impl BuildSpec {
    pub fn kind(&self) -> BackendKind {
        match self {
            BuildSpec::Arduino { .. } => BackendKind::Arduino,
            BuildSpec::Idf { .. } => BackendKind::Idf,
        }
    }

    /// A short human label for the flash tag and the status line.
    pub fn label(&self) -> String {
        match self {
            BuildSpec::Arduino { profile, fqbn } => profile
                .clone()
                .or_else(|| fqbn.clone())
                .unwrap_or_else(|| "arduino".to_string()),
            BuildSpec::Idf { target } => target.clone(),
        }
    }
}

/// A resolved toolchain.
#[derive(Debug, Clone)]
pub enum Backend {
    Arduino(ArduinoCli),
    Idf(IdfCli),
}

impl Backend {
    pub fn kind(&self) -> BackendKind {
        match self {
            Backend::Arduino(_) => BackendKind::Arduino,
            Backend::Idf(_) => BackendKind::Idf,
        }
    }

    /// Build without flashing.
    ///
    /// A failing build is `Ok(RunResult { success: false, .. })`, not an
    /// `Err`: only a failure to run the toolchain at all is an error. Every
    /// consumer downstream — the build gate, the MCP `verify` tool — depends
    /// on that distinction.
    pub fn verify(
        &self,
        dir: &Path,
        spec: &BuildSpec,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        match (self, spec) {
            (Backend::Arduino(cli), BuildSpec::Arduino { profile, fqbn }) => cli.compile(
                &dir.display().to_string(),
                profile.as_deref(),
                fqbn.as_deref(),
                &[],
                on_line,
            ),
            (Backend::Idf(cli), BuildSpec::Idf { .. }) => cli.build(dir, on_line),
            _ => Err(mismatch(self, spec)),
        }
    }

    /// Build and flash to `port`.
    ///
    /// Both backends build first — arduino-cli because we spell it
    /// `compile -u`, ESP-IDF because its `flash` action depends on the build —
    /// so neither can send a binary that did not just compile.
    pub fn flash(
        &self,
        dir: &Path,
        spec: &BuildSpec,
        port: &str,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        match (self, spec) {
            (Backend::Arduino(cli), BuildSpec::Arduino { profile, fqbn }) => cli.upload(
                &dir.display().to_string(),
                profile.as_deref(),
                fqbn.as_deref(),
                port,
                on_line,
            ),
            (Backend::Idf(cli), BuildSpec::Idf { .. }) => cli.flash(dir, port, on_line),
            _ => Err(mismatch(self, spec)),
        }
    }
}

/// A backend paired with a spec for the other kind. Not reachable through the
/// app — both are derived from one detection — but it is a programming error
/// worth naming rather than a silent wrong build.
fn mismatch(backend: &Backend, spec: &BuildSpec) -> crate::Error {
    crate::Error::Other(format!(
        "internal error: {:?} backend given a {:?} build spec",
        backend.kind(),
        spec.kind()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arduino() -> Backend {
        Backend::Arduino(ArduinoCli::new("arduino-cli"))
    }

    fn idf() -> Backend {
        Backend::Idf(IdfCli::new(
            "/p".into(),
            "/idf".into(),
            Default::default(),
        ))
    }

    #[test]
    fn a_spec_reports_the_backend_it_needs() {
        assert_eq!(
            BuildSpec::Idf {
                target: "esp32c6".into()
            }
            .kind(),
            BackendKind::Idf
        );
        assert_eq!(
            BuildSpec::Arduino {
                profile: None,
                fqbn: None
            }
            .kind(),
            BackendKind::Arduino
        );
    }

    #[test]
    fn the_label_prefers_the_profile_then_the_fqbn() {
        let p = BuildSpec::Arduino {
            profile: Some("esp32s3".into()),
            fqbn: Some("esp32:esp32:esp32s3".into()),
        };
        assert_eq!(p.label(), "esp32s3");
        let f = BuildSpec::Arduino {
            profile: None,
            fqbn: Some("arduino:avr:uno".into()),
        };
        assert_eq!(f.label(), "arduino:avr:uno");
        assert_eq!(
            BuildSpec::Idf {
                target: "esp32c3".into()
            }
            .label(),
            "esp32c3"
        );
    }

    #[test]
    fn a_mismatched_pair_is_refused_rather_than_silently_building_the_wrong_thing() {
        let spec = BuildSpec::Idf {
            target: "esp32c3".into(),
        };
        // Nothing is spawned: the pairing is checked before any command runs.
        let err = arduino()
            .verify(Path::new("/tmp/x"), &spec, |_| {})
            .unwrap_err();
        assert!(err.to_string().contains("internal error"), "{err}");

        let spec = BuildSpec::Arduino {
            profile: None,
            fqbn: None,
        };
        let err = idf().flash(Path::new("/tmp/x"), &spec, "/dev/x", |_| {}).unwrap_err();
        assert!(err.to_string().contains("internal error"), "{err}");
    }
}
