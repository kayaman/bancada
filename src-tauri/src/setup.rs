//! Host-side setup: probing this machine for the engines Bancada drives, and
//! installing the one it cannot do Arduino work without.
//!
//! The catalogue and every parsing rule live in [`bancada_core::setup`]; this
//! module is the part that spawns `--version`, reads `/etc/group`, and runs
//! the arduino-cli installer.
//!
//! ## Why the app edits its own `PATH`
//!
//! [`ensure_user_bins_on_path`] runs once, before Tauri builds a window. A
//! `.desktop` launch inherits the session's environment, not a login shell's,
//! so `~/.local/bin` — where every per-user installer puts things, arduino-cli's
//! included — is routinely absent. Without this, "Install" in the Setup panel
//! would succeed and the very next probe would still say "not on PATH" until
//! the user logged out and in. `std::env::set_var` is process-wide and is
//! called here strictly before any thread exists, which is the one moment it
//! is sound to call.

use std::path::{Path, PathBuf};
use std::process::Command;

use bancada_core::cli::ArduinoCli;
use bancada_core::setup::{self, SerialAccess, ToolSpec};

/// Prepend the per-user binary directories to this process's `PATH`.
pub fn ensure_user_bins_on_path() {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return;
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let augmented = setup::augmented_path(&path, Path::new(&home), |p| p.is_dir());
    if augmented != path {
        std::env::set_var("PATH", augmented);
    }
}

/// The kernel drivers bound to the GPUs on this machine, from sysfs.
fn drm_drivers() -> Vec<String> {
    let Ok(cards) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut out: Vec<String> = cards
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            // `card0`, not `card0-HDMI-A-1` (a connector) or `renderD128`.
            n.starts_with("card") && !n.contains('-')
        })
        .filter_map(|e| std::fs::read_link(e.path().join("device").join("driver")).ok())
        .filter_map(|l| l.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Turn WebKitGTK's DMA-BUF renderer off on GPUs where it draws garbage.
///
/// Must run before the webview exists, like [`ensure_user_bins_on_path`].
/// An explicit setting in the environment — either value — is the user's
/// and is left alone. Returns the driver that triggered the workaround, so
/// `run()` can say so once on stderr.
pub fn ensure_webkit_renderer_works() -> Option<String> {
    if std::env::var_os(setup::WEBKIT_DMABUF_VAR).is_some() {
        return None;
    }
    let drivers = drm_drivers();
    let bad = setup::webkit_dmabuf_unsafe_driver(drivers.iter().map(String::as_str))?.to_string();
    std::env::set_var(setup::WEBKIT_DMABUF_VAR, "1");
    Some(bad)
}

/// One engine's state on this machine. Mirrors `ToolStatus` in `src/api.ts`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolStatus {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub required: bool,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Why it is not usable, when it is present but broken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub install: String,
    pub docs: String,
    /// Whether the panel can run the install itself (arduino-cli only).
    pub installable: bool,
}

/// Serial-port access for the current user. Mirrors `SerialStatus` in
/// `src/api.ts`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SerialStatus {
    pub access: SerialAccess,
    /// The device whose group was consulted, when one was attached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    pub fix: String,
}

/// Everything the Setup panel shows at once.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupReport {
    pub tools: Vec<ToolStatus>,
    pub serial: SerialStatus,
    /// The `PATH` the probes used — shown so "not on PATH" is checkable.
    pub path: String,
}

fn status_for(
    spec: &ToolSpec,
    ok: bool,
    version: Option<String>,
    detail: Option<String>,
) -> ToolStatus {
    let path = std::env::var("PATH").unwrap_or_default();
    ToolStatus {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        purpose: spec.purpose.to_string(),
        required: spec.required,
        ok,
        version,
        path: setup::find_on_path(spec.bin, &path, |p| p.is_file())
            .map(|p| p.display().to_string()),
        detail,
        install: spec.install.to_string(),
        docs: spec.docs.to_string(),
        installable: spec.id == "arduino-cli",
    }
}

/// `<bin> <arg>` → the first output line, or why it failed.
fn version_probe(bin: &str, arg: &str) -> Result<String, String> {
    match Command::new(bin).arg(arg).output() {
        Ok(out) if out.status.success() => {
            // Some tools print the version on stderr (older esptool did).
            let text = if out.stdout.is_empty() {
                &out.stderr
            } else {
                &out.stdout
            };
            Ok(setup::version_line(&String::from_utf8_lossy(text)))
        }
        Ok(out) => Err(format!(
            "`{bin} {arg}` exited with status {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(String::new()),
        Err(e) => Err(format!("could not start `{bin}`: {e}")),
    }
}

/// Probe every catalogued engine.
///
/// arduino-cli goes through the app's own wrapper so the version it reports is
/// the one builds will use; the rest are plain `--version` spawns. A tool that
/// is absent has an empty `detail` — "not found" is what the panel says for
/// that — while one that is present but fails to run carries the reason.
pub fn probe_tools(cli: &ArduinoCli) -> Vec<ToolStatus> {
    setup::TOOLS
        .iter()
        .map(|spec| {
            let probed = match spec.id {
                "arduino-cli" => cli.version().map_err(|e| match e {
                    bancada_core::Error::ToolMissing(_) => String::new(),
                    other => other.to_string(),
                }),
                // Modern installs ship `esptool`; older pip installs only
                // `esptool.py`. Same two-candidate rule as core::esptool.
                "esptool" => version_probe("esptool", "version")
                    .or_else(|e| version_probe("esptool.py", "version").or(Err(e))),
                _ => version_probe(spec.bin, "--version"),
            };
            match probed {
                Ok(v) => status_for(spec, true, Some(v), None),
                Err(detail) => {
                    status_for(spec, false, None, (!detail.is_empty()).then_some(detail))
                }
            }
        })
        .collect()
}

/// The group owning the first attached serial device, if any.
fn attached_device_group(etc_group: &str) -> Option<(String, String)> {
    use std::os::unix::fs::MetadataExt;
    let dev = std::fs::read_dir("/dev").ok()?;
    let mut names: Vec<String> = dev
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("ttyACM") || n.starts_with("ttyUSB"))
        .collect();
    names.sort();
    let name = names.into_iter().next()?;
    let meta = std::fs::metadata(format!("/dev/{name}")).ok()?;
    let group = setup::group_name_for_gid(etc_group, meta.gid())?;
    Some((format!("/dev/{name}"), group))
}

/// Can this user open a serial port?
pub fn probe_serial() -> SerialStatus {
    let etc_group = std::fs::read_to_string("/etc/group").unwrap_or_default();
    let user_groups = Command::new("id")
        .arg("-nG")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| setup::parse_groups(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    let device = attached_device_group(&etc_group);
    let access = setup::serial_access(
        &user_groups,
        device.as_ref().map(|(_, g)| g.as_str()),
        &setup::group_names(&etc_group),
    );
    let group = match &access {
        SerialAccess::Ok { group } | SerialAccess::Missing { group } => group.clone(),
    };
    SerialStatus {
        access,
        device: device.map(|(d, _)| d),
        fix: setup::serial_fix_command(&group),
    }
}

pub fn report(cli: &ArduinoCli) -> SetupReport {
    SetupReport {
        tools: probe_tools(cli),
        serial: probe_serial(),
        path: std::env::var("PATH").unwrap_or_default(),
    }
}

/// What the arduino-cli installer did. Mirrors `InstallOutcome` in `src/api.ts`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallOutcome {
    pub ok: bool,
    pub bindir: String,
    /// The installer's own output, both streams, for the panel to show.
    pub log: String,
}

/// Where the panel installs to: `~/.local/bin`, which
/// [`ensure_user_bins_on_path`] guarantees the app itself searches.
pub fn install_bindir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(Path::new(&home).join(".local").join("bin"))
}

/// Run the official arduino-cli installer into [`install_bindir`].
///
/// Two steps, download then run, so a truncated download fails as `curl`
/// rather than as a half-run script. The script's output is returned rather
/// than streamed: it is a few dozen lines and the panel shows it whole.
pub fn install_arduino_cli() -> Result<InstallOutcome, String> {
    let bindir = install_bindir()?;
    std::fs::create_dir_all(&bindir)
        .map_err(|e| format!("could not create {}: {e}", bindir.display()))?;
    let script = std::env::temp_dir().join(format!(
        "bancada-arduino-cli-install-{}.sh",
        std::process::id()
    ));
    let mut log = String::new();
    let mut ok = true;
    for (argv, env) in setup::arduino_cli_install_steps(&script, &bindir) {
        log.push_str(&format!("$ {}\n", argv.join(" ")));
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).envs(env);
        match cmd.output() {
            Ok(out) => {
                log.push_str(&String::from_utf8_lossy(&out.stdout));
                log.push_str(&String::from_utf8_lossy(&out.stderr));
                if !out.status.success() {
                    log.push_str(&format!(
                        "(exited with status {})\n",
                        out.status.code().unwrap_or(-1)
                    ));
                    ok = false;
                    break;
                }
            }
            Err(e) => {
                log.push_str(&format!("could not start `{}`: {e}\n", argv[0]));
                ok = false;
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&script);
    Ok(InstallOutcome {
        ok,
        bindir: bindir.display().to_string(),
        log,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smoke test against whatever this machine has: it asserts the shape
    /// of the report, never which tools are present, so it holds on a bare CI
    /// box and on a fully equipped bench alike.
    #[test]
    fn the_report_covers_every_catalogued_tool_and_names_a_serial_fix() {
        let r = report(&ArduinoCli::default());
        assert_eq!(r.tools.len(), setup::TOOLS.len());
        for (t, spec) in r.tools.iter().zip(setup::TOOLS) {
            assert_eq!(t.id, spec.id);
            assert_eq!(t.installable, spec.id == "arduino-cli");
            // A found tool has a version and no detail; a missing one has
            // neither; a broken one has a detail. Never a version *and* a detail.
            assert!(!(t.version.is_some() && t.detail.is_some()), "{t:?}");
            assert_eq!(t.ok, t.version.is_some(), "{t:?}");
        }
        assert!(r.serial.fix.starts_with("sudo usermod -aG "));
        assert!(!r.path.is_empty());
    }

    #[test]
    fn the_install_target_is_under_the_users_home() {
        let bindir = install_bindir().unwrap();
        assert!(bindir.ends_with(".local/bin"), "{}", bindir.display());
    }
}
