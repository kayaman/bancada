//! First-run setup: what a new machine needs before Bancada can do its job,
//! and the pure half of finding out whether it has it.
//!
//! Bancada bundles no toolchain (see the README). On a fresh device that
//! shows up as a string of "could not find `arduino-cli` on PATH" toasts,
//! each true and none of them telling the user *what to do*. This module is
//! the answer: the catalogue of engines the app drives, what each one
//! unlocks, and the command that installs it — so the Setup panel can show
//! a checklist instead of the user reconstructing one from the docs.
//!
//! Everything here is pure. Spawning `--version` probes, reading `/etc/group`
//! and running installers is the Tauri layer's business (`src-tauri/src/setup.rs`);
//! this module decides *what* to probe and how to read the results, which is
//! the part worth unit-testing.

use std::path::{Path, PathBuf};

/// One engine Bancada drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    /// Stable id the frontend keys on.
    pub id: &'static str,
    /// The executable name as looked up on `PATH`.
    pub bin: &'static str,
    /// Display name.
    pub name: &'static str,
    /// What having it unlocks, in one line.
    pub purpose: &'static str,
    /// Whether Arduino work is impossible without it. Nothing is required
    /// for ESP-IDF-only benches, which is why `required` is advisory: the
    /// panel words it as "needed for Arduino projects", not as an error.
    pub required: bool,
    /// The command that installs it on a typical Linux desktop. Shown with a
    /// copy button; only arduino-cli's is also runnable from the panel.
    pub install: &'static str,
    /// Where to read more.
    pub docs: &'static str,
}

/// The official installer, pinned to the location the app can guarantee is
/// on its own `PATH` (see [`augmented_path`]).
pub const ARDUINO_CLI_INSTALL_URL: &str =
    "https://raw.githubusercontent.com/arduino/arduino-cli/master/install.sh";

/// The engines, in the order the panel lists them: the one Arduino work
/// depends on first, then the feature-unlockers.
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        id: "arduino-cli",
        bin: "arduino-cli",
        name: "arduino-cli",
        purpose: "Arduino boards, builds, uploads and libraries",
        required: true,
        install: "curl -fsSL https://raw.githubusercontent.com/arduino/arduino-cli/master/install.sh | BINDIR=~/.local/bin sh",
        docs: "https://arduino.github.io/arduino-cli/latest/installation/",
    },
    ToolSpec {
        id: "esptool",
        bin: "esptool",
        name: "esptool",
        purpose: "ESP utilities: read MAC address and chip info",
        required: false,
        install: "pip install --user esptool",
        docs: "https://docs.espressif.com/projects/esptool/",
    },
    ToolSpec {
        id: "git",
        bin: "git",
        name: "git",
        purpose: "Project version control, checkpoints and pinned libraries",
        required: false,
        install: "sudo dnf install git   # or: sudo apt install git / sudo zypper install git",
        docs: "https://git-scm.com/downloads",
    },
    ToolSpec {
        id: "gh",
        bin: "gh",
        name: "GitHub CLI",
        purpose: "One-button GitHub repository creation",
        required: false,
        install: "sudo dnf install gh   # or: sudo apt install gh — then: gh auth login",
        docs: "https://cli.github.com/",
    },
    ToolSpec {
        id: "claude",
        bin: "claude",
        name: "Claude Code",
        purpose: "The Assistant panel",
        required: false,
        install: "curl -fsSL https://claude.ai/install.sh | sh   # then run `claude` once to sign in",
        docs: "https://claude.com/product/claude-code",
    },
];

/// The engine with a given id.
pub fn tool(id: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.id == id)
}

/// Directories a per-user installer drops binaries into that a desktop-
/// launched app does not otherwise see.
///
/// A shell sources the profile that adds `~/.local/bin`; a `.desktop` launch
/// does not, so a tool the user just installed "is not on PATH" from the
/// app's point of view while their terminal finds it fine. Prepending these
/// once at startup closes that gap for every subprocess the app spawns, and
/// is what lets the panel's own arduino-cli install take effect without a
/// relaunch. Only directories that exist are added, and none is added twice.
pub fn augmented_path(path: &str, home: &Path, exists: impl Fn(&Path) -> bool) -> String {
    let candidates = [home.join(".local").join("bin"), home.join("bin")];
    let present: Vec<&str> = path.split(':').filter(|d| !d.is_empty()).collect();
    let mut out: Vec<String> = Vec::new();
    for dir in candidates {
        let s = dir.to_string_lossy().to_string();
        if exists(&dir) && !present.contains(&s.as_str()) && !out.contains(&s) {
            out.push(s);
        }
    }
    out.extend(present.iter().map(|d| d.to_string()));
    out.join(":")
}

/// Where `bin` resolves on `path`, if anywhere. The same walk `execvp` does,
/// minus the executable-bit check — a non-executable file of the right name
/// is a state worth *showing* rather than hiding behind "not found".
pub fn find_on_path(bin: &str, path: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    path.split(':')
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(bin))
        .find(|p| exists(p))
}

/// The first non-empty line of a `--version` output, trimmed. `git --version`
/// says `git version 2.51.0`; `gh --version` says two lines; `claude --version`
/// says `2.1.0 (Claude Code)`. One line is what the panel has room for.
pub fn version_line(stdout: &str) -> String {
    stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Groups that own serial devices, by distro family. `dialout` on Fedora,
/// openSUSE and Debian; `uucp` on Arch and derivatives.
pub const SERIAL_GROUPS: &[&str] = &["dialout", "uucp"];

/// Whether the user can open a serial port without root.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SerialAccess {
    /// A member of the group that owns the ports.
    Ok { group: String },
    /// Not a member; `group` is the one to join (from a device when one is
    /// attached, else the distro's conventional name).
    Missing { group: String },
}

/// Decide serial access from the user's groups and, when a device is
/// attached, the group that owns it.
///
/// The device's group is authoritative when known: on a distro this table
/// has never heard of, `/dev/ttyACM0` still says who owns it. Without a
/// device, the first of [`SERIAL_GROUPS`] the user belongs to counts, and
/// the first that *exists on the system* is what they are told to join.
pub fn serial_access(
    user_groups: &[String],
    device_group: Option<&str>,
    system_groups: &[String],
) -> SerialAccess {
    if let Some(g) = device_group {
        return if user_groups.iter().any(|u| u == g) {
            SerialAccess::Ok {
                group: g.to_string(),
            }
        } else {
            SerialAccess::Missing {
                group: g.to_string(),
            }
        };
    }
    if let Some(g) = SERIAL_GROUPS
        .iter()
        .find(|g| user_groups.iter().any(|u| u == *g))
    {
        return SerialAccess::Ok {
            group: g.to_string(),
        };
    }
    let needed = SERIAL_GROUPS
        .iter()
        .find(|g| system_groups.iter().any(|s| s == *g))
        .unwrap_or(&SERIAL_GROUPS[0]);
    SerialAccess::Missing {
        group: needed.to_string(),
    }
}

/// The command that grants serial access, for the panel to show.
pub fn serial_fix_command(group: &str) -> String {
    format!("sudo usermod -aG {group} $USER   # then log out and back in")
}

/// Parse `id -nG` output (`kayaman wheel dialout`) into group names.
pub fn parse_groups(id_output: &str) -> Vec<String> {
    id_output.split_whitespace().map(str::to_string).collect()
}

/// Group names defined in an `/etc/group` file.
pub fn group_names(etc_group: &str) -> Vec<String> {
    etc_group
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split(':').next())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect()
}

/// The name of the group with numeric id `gid` in an `/etc/group` file.
pub fn group_name_for_gid(etc_group: &str, gid: u32) -> Option<String> {
    etc_group
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            f.next()?;
            let id: u32 = f.next()?.parse().ok()?;
            (id == gid).then(|| name.to_string())
        })
        .next()
}

/// The two steps that install arduino-cli into `bindir`, as argv vectors.
///
/// Download, then run: the script is fetched to `script` first rather than
/// piped straight into `sh`, so a truncated download is a failed `curl`
/// rather than a half-executed installer. `BINDIR` is passed as an
/// environment variable, which is how the official script takes it.
/// One installer step: argv, plus the environment variables it needs.
pub type InstallStep = (Vec<String>, Vec<(String, String)>);

pub fn arduino_cli_install_steps(script: &Path, bindir: &Path) -> [InstallStep; 2] {
    [
        (
            vec![
                "curl".to_string(),
                "-fsSL".to_string(),
                "-o".to_string(),
                script.display().to_string(),
                ARDUINO_CLI_INSTALL_URL.to_string(),
            ],
            vec![],
        ),
        (
            vec!["sh".to_string(), script.display().to_string()],
            vec![("BINDIR".to_string(), bindir.display().to_string())],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arduino_cli_is_the_only_required_tool_and_comes_first() {
        assert_eq!(TOOLS[0].id, "arduino-cli");
        let required: Vec<&str> = TOOLS.iter().filter(|t| t.required).map(|t| t.id).collect();
        assert_eq!(required, vec!["arduino-cli"]);
        assert!(tool("esptool").is_some());
        assert!(tool("nope").is_none());
    }

    #[test]
    fn user_bin_dirs_are_prepended_once_and_only_when_present() {
        let home = Path::new("/home/u");
        let exists = |p: &Path| p == Path::new("/home/u/.local/bin");
        // Missing from PATH and present on disk: prepended.
        assert_eq!(
            augmented_path("/usr/bin:/bin", home, exists),
            "/home/u/.local/bin:/usr/bin:/bin"
        );
        // Already there: not duplicated, order untouched.
        assert_eq!(
            augmented_path("/usr/bin:/home/u/.local/bin", home, exists),
            "/usr/bin:/home/u/.local/bin"
        );
        // ~/bin does not exist here, so it is never added.
        assert!(!augmented_path("/usr/bin", home, exists).contains("/home/u/bin"));
        // Empty PATH entries are dropped rather than becoming ".".
        assert_eq!(augmented_path("::/usr/bin:", home, |_| false), "/usr/bin");
    }

    #[test]
    fn find_on_path_walks_in_order() {
        let exists = |p: &Path| {
            p == Path::new("/home/u/.local/bin/arduino-cli")
                || p == Path::new("/usr/bin/arduino-cli")
        };
        assert_eq!(
            find_on_path(
                "arduino-cli",
                "/usr/local/bin:/home/u/.local/bin:/usr/bin",
                exists
            ),
            Some(PathBuf::from("/home/u/.local/bin/arduino-cli"))
        );
        assert_eq!(find_on_path("idf.py", "/usr/bin", exists), None);
    }

    #[test]
    fn version_line_takes_the_first_meaningful_line() {
        assert_eq!(version_line("git version 2.51.0\n"), "git version 2.51.0");
        assert_eq!(
            version_line("\ngh version 2.80.0 (2026-08-01)\nhttps://github.com/cli/cli/releases\n"),
            "gh version 2.80.0 (2026-08-01)"
        );
        assert_eq!(version_line(""), "");
    }

    #[test]
    fn the_attached_devices_group_is_authoritative() {
        let mine = parse_groups("u wheel uucp\n");
        // The device says dialout; membership in uucp does not help.
        assert_eq!(
            serial_access(&mine, Some("dialout"), &[]),
            SerialAccess::Missing {
                group: "dialout".into()
            }
        );
        assert_eq!(
            serial_access(&mine, Some("uucp"), &[]),
            SerialAccess::Ok {
                group: "uucp".into()
            }
        );
    }

    #[test]
    fn without_a_device_the_distros_group_is_named() {
        let none = parse_groups("u wheel");
        // Arch-style system: uucp exists, dialout does not.
        let arch = vec!["root".to_string(), "uucp".to_string()];
        assert_eq!(
            serial_access(&none, None, &arch),
            SerialAccess::Missing {
                group: "uucp".into()
            }
        );
        // Unknown system: the conventional default.
        assert_eq!(
            serial_access(&none, None, &[]),
            SerialAccess::Missing {
                group: "dialout".into()
            }
        );
        // Already a member of either: fine.
        let ok = parse_groups("u dialout");
        assert_eq!(
            serial_access(&ok, None, &arch),
            SerialAccess::Ok {
                group: "dialout".into()
            }
        );
        assert!(serial_fix_command("uucp").starts_with("sudo usermod -aG uucp"));
    }

    const ETC_GROUP: &str =
        "root:x:0:\n# a comment\ndialout:x:18:kayaman\nuucp:x:14:\n\nbroken line\n";

    #[test]
    fn etc_group_is_read_by_name_and_by_gid() {
        assert_eq!(
            group_names(ETC_GROUP),
            vec!["root", "dialout", "uucp", "broken line"]
        );
        assert_eq!(
            group_name_for_gid(ETC_GROUP, 18),
            Some("dialout".to_string())
        );
        assert_eq!(group_name_for_gid(ETC_GROUP, 999), None);
    }

    #[test]
    fn the_installer_is_downloaded_before_it_runs() {
        let [(dl, dl_env), (run, run_env)] =
            arduino_cli_install_steps(Path::new("/tmp/i.sh"), Path::new("/home/u/.local/bin"));
        assert_eq!(dl[0], "curl");
        assert!(dl.contains(&"-o".to_string()) && dl.contains(&"/tmp/i.sh".to_string()));
        assert_eq!(dl.last().unwrap(), ARDUINO_CLI_INSTALL_URL);
        assert!(dl_env.is_empty());
        assert_eq!(run, vec!["sh", "/tmp/i.sh"]);
        assert_eq!(
            run_env,
            vec![("BINDIR".to_string(), "/home/u/.local/bin".to_string())]
        );
    }
}
