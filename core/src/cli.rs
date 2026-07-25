//! Thin, typed wrapper around the `arduino-cli` executable.
//!
//! Everything that returns data uses `--json`; long-running operations
//! (compile/upload) stream their human-readable output line by line through a
//! callback so the UI can show live progress.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

use crate::types::*;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct ArduinoCli {
    /// Path or name of the executable; default "arduino-cli" from PATH.
    pub bin: String,
}

impl Default for ArduinoCli {
    fn default() -> Self {
        Self {
            bin: "arduino-cli".to_string(),
        }
    }
}

impl ArduinoCli {
    pub fn new(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    // ---------- plumbing ----------

    fn base_command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn map_spawn_err(&self, e: std::io::Error) -> Error {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::ToolMissing(self.bin.clone())
        } else {
            Error::Io(e)
        }
    }

    /// Run to completion and parse stdout as JSON.
    fn run_json<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Result<T> {
        let mut full_args: Vec<&str> = args.to_vec();
        full_args.push("--json");
        let out = self
            .base_command(&full_args)
            .output()
            .map_err(|e| self.map_spawn_err(e))?;
        if !out.status.success() {
            return Err(Error::ToolFailed {
                tool: format!("{} {}", self.bin, args.join(" ")),
                status: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        serde_json::from_slice(&out.stdout).map_err(|source| Error::Json {
            what: format!("output of `{} {}`", self.bin, args.join(" ")),
            source,
        })
    }

    /// Run to completion and return stdout, for commands whose value is the
    /// side effect rather than parsed output (`sketch new`, `profile create`).
    /// `--json` is not passed: these print prose, and some reject the flag.
    fn run_ok(&self, args: &[&str]) -> Result<String> {
        let out = self
            .base_command(args)
            .output()
            .map_err(|e| self.map_spawn_err(e))?;
        if !out.status.success() {
            return Err(Error::ToolFailed {
                tool: format!("{} {}", self.bin, args.join(" ")),
                status: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run to completion, streaming stdout+stderr lines (interleaved) into
    /// `on_line`. Returns the exit status. Used for compile/upload.
    pub fn run_streaming(
        &self,
        args: &[&str],
        mut on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        let mut child = self
            .base_command(args)
            .spawn()
            .map_err(|e| self.map_spawn_err(e))?;

        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let (tx, rx) = mpsc::channel::<OutputLine>();
        let tx_err = tx.clone();

        let t_out = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                let _ = tx.send(OutputLine {
                    stream: OutputStream::Stdout,
                    line,
                });
            }
        });
        let t_err = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
                let _ = tx_err.send(OutputLine {
                    stream: OutputStream::Stderr,
                    line,
                });
            }
        });

        // Receive until both writer threads hang up.
        for line in rx {
            on_line(line);
        }
        let _ = t_out.join();
        let _ = t_err.join();

        let status = child.wait()?;
        Ok(RunResult {
            success: status.success(),
            exit_code: status.code().unwrap_or(-1),
        })
    }

    /// Spawn a long-lived subprocess (serial monitor) without waiting.
    /// stdin is piped so the caller can transmit to the board.
    pub fn spawn_raw(&self, args: &[&str]) -> Result<Child> {
        Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| self.map_spawn_err(e))
    }

    // ---------- queries ----------

    pub fn version(&self) -> Result<String> {
        let v: serde_json::Value = self.run_json(&["version"])?;
        Ok(v.get("VersionString")
            .or_else(|| v.get("version"))
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string())
    }

    pub fn board_list(&self) -> Result<Vec<DetectedPort>> {
        let r: BoardListResponse = self.run_json(&["board", "list"])?;
        Ok(r.detected_ports)
    }

    pub fn core_list(&self) -> Result<Vec<Platform>> {
        let r: CoreListResponse = self.run_json(&["core", "list"])?;
        Ok(r.platforms)
    }

    /// The sketchbook root (`directories.user`).
    ///
    /// `config get` rather than `config dump`: dump omits every key left at its
    /// default, so on a stock install it does not report `directories` at all.
    /// With `--json` the value comes back as a quoted JSON string.
    pub fn sketchbook_dir(&self) -> Result<PathBuf> {
        let raw: String = self.run_json(&["config", "get", "directories.user"])?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(Error::Other(
                "arduino-cli reported an empty sketchbook path (directories.user)".into(),
            ));
        }
        Ok(PathBuf::from(trimmed))
    }

    /// Where globally installed (sketchbook) libraries live.
    pub fn sketchbook_libraries_dir(&self) -> Result<PathBuf> {
        Ok(self.sketchbook_dir()?.join("libraries"))
    }

    /// Every board of every installed platform, flattened for a picker.
    ///
    /// `board listall --json` repeats the whole platform (including all of its
    /// boards) inside each entry, so it is collapsed here rather than handed to
    /// the frontend as-is.
    pub fn board_listall(&self) -> Result<Vec<BoardOption>> {
        let r: BoardListAllResponse = self.run_json(&["board", "listall"])?;
        let mut out: Vec<BoardOption> = r
            .boards
            .into_iter()
            .filter(|b| !b.fqbn.is_empty() && b.platform.release.installed)
            .map(|b| BoardOption {
                fqbn: b.fqbn,
                name: b.name,
                platform_id: b.platform.metadata.id,
                platform_name: b.platform.release.name,
            })
            .collect();
        out.sort_by(|a, b| {
            a.platform_name
                .cmp(&b.platform_name)
                .then_with(|| a.name.cmp(&b.name))
        });
        out.dedup_by(|a, b| a.fqbn == b.fqbn);
        Ok(out)
    }

    // ---------- new projects ----------

    /// `arduino-cli sketch new <dir>` — creates the folder and its main `.ino`.
    ///
    /// `--overwrite` is deliberately never passed: an existing directory is
    /// refused by the caller instead.
    pub fn sketch_new(&self, dir: &Path) -> Result<()> {
        self.run_ok(&["sketch", "new", &dir.to_string_lossy()])?;
        Ok(())
    }

    /// `arduino-cli profile create` — writes `sketch.yaml`, resolving the
    /// platform version from what is installed rather than guessing it.
    pub fn profile_create(
        &self,
        sketch_dir: &Path,
        profile: &str,
        fqbn: &str,
        set_default: bool,
    ) -> Result<()> {
        let args = profile_create_args(sketch_dir, profile, fqbn, set_default);
        self.run_ok(&as_str_slice(&args))?;
        Ok(())
    }

    /// `arduino-cli profile lib add` — pins a registry library into a profile,
    /// resolving its dependencies.
    ///
    /// `spec` is `Name` or `Name@version`.
    pub fn profile_lib_add(&self, sketch_dir: &Path, profile: &str, spec: &str) -> Result<()> {
        let args = profile_lib_add_args(sketch_dir, profile, spec);
        self.run_ok(&as_str_slice(&args))?;
        Ok(())
    }

    // ---------- libraries ----------

    pub fn lib_search(&self, query: &str) -> Result<Vec<IndexedLibrary>> {
        let r: LibSearchResponse = self.run_json(&["lib", "search", query])?;
        Ok(r.libraries)
    }

    pub fn lib_list(&self) -> Result<Vec<InstalledLibrary>> {
        let r: LibListResponse = self.run_json(&["lib", "list"])?;
        Ok(r.installed_libraries.into_iter().map(|e| e.library).collect())
    }

    /// Install from the registry; `version: None` installs the latest.
    pub fn lib_install(&self, name: &str, version: Option<&str>) -> Result<()> {
        let spec = lib_spec(name, version);
        let _: serde_json::Value = self.run_json(&["lib", "install", &spec])?;
        Ok(())
    }

    pub fn lib_uninstall(&self, name: &str) -> Result<()> {
        let _: serde_json::Value = self.run_json(&["lib", "uninstall", name])?;
        Ok(())
    }

    pub fn lib_update_index(&self) -> Result<()> {
        let _: serde_json::Value = self.run_json(&["lib", "update-index"])?;
        Ok(())
    }

    // ---------- cores (platforms) ----------
    //
    // Installs, removals and index updates stream their output instead of
    // returning JSON: a platform is a multi-hundred-megabyte download, and
    // `run_json` would sit silent until it finished. `run_streaming` also does
    // not append `--json`, so what reaches the console is arduino-cli's own
    // human-readable progress.
    //
    // Third-party index URLs are deliberately not plumbed here. Bancada reads
    // whatever the user's arduino-cli config already knows and never edits
    // `board_manager.additional_urls` — same principle as refusing to flip
    // `library.enable_unsafe_install` for git installs.

    pub fn core_search(&self, query: &str) -> Result<Vec<Platform>> {
        let r: CoreListResponse = self.run_json(&["core", "search", query])?;
        Ok(r.platforms)
    }

    /// Install a platform; `version: None` installs the latest.
    pub fn core_install(
        &self,
        id: &str,
        version: Option<&str>,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        let args = core_install_args(id, version);
        self.run_streaming(&as_str_slice(&args), on_line)
    }

    pub fn core_uninstall(&self, id: &str, on_line: impl FnMut(OutputLine)) -> Result<RunResult> {
        let args = owned(&["core", "uninstall", id]);
        self.run_streaming(&as_str_slice(&args), on_line)
    }

    pub fn core_update_index(&self, on_line: impl FnMut(OutputLine)) -> Result<RunResult> {
        let args = owned(&["core", "update-index"]);
        self.run_streaming(&as_str_slice(&args), on_line)
    }

    // ---------- build & flash ----------

    /// Compile a sketch. Uses the sketch.yaml `profile` when given, otherwise
    /// falls back to an explicit `fqbn`. Extra local library paths can be
    /// passed for ad-hoc builds (mirrors `--library`).
    pub fn compile(
        &self,
        sketch_dir: &str,
        profile: Option<&str>,
        fqbn: Option<&str>,
        extra_lib_paths: &[String],
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        let args = compile_args(sketch_dir, profile, fqbn, extra_lib_paths);
        self.run_streaming(&as_str_slice(&args), on_line)
    }

    pub fn upload(
        &self,
        sketch_dir: &str,
        profile: Option<&str>,
        fqbn: Option<&str>,
        port: &str,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        let args = upload_args(sketch_dir, profile, fqbn, port);
        self.run_streaming(&as_str_slice(&args), on_line)
    }

    /// Spawn `arduino-cli monitor` as a long-lived child process.
    pub fn monitor(&self, port: &str, baudrate: u32) -> Result<Child> {
        let args = monitor_args(port, baudrate);
        self.spawn_raw(&as_str_slice(&args))
    }
}

// ---------- argument construction ----------
//
// Built as pure functions so the exact command line is unit-testable. These are
// not cosmetic: arduino-cli's flags are easy to get subtly wrong in ways that
// fail at runtime with a misleading message rather than at compile time. The
// `profile lib add` shape below is a real example — passing the sketch
// positionally makes arduino-cli look for `<cwd>.ino` and report a missing main
// file, which says nothing about the actual mistake.

fn as_str_slice(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn owned(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// A library spec for the registry: `Name` or `Name@version`.
fn lib_spec(name: &str, version: Option<&str>) -> String {
    match version {
        Some(v) if !v.trim().is_empty() => format!("{name}@{}", v.trim()),
        _ => name.to_string(),
    }
}

/// `compile [--profile P | --fqbn F] [--library P]… <sketch>`
///
/// A profile wins over an FQBN: a profile build is hermetic and already names
/// its board, so passing both would be contradictory.
fn compile_args(
    sketch_dir: &str,
    profile: Option<&str>,
    fqbn: Option<&str>,
    extra_lib_paths: &[String],
) -> Vec<String> {
    let mut args = owned(&["compile"]);
    if let Some(p) = profile {
        args.extend(owned(&["--profile", p]));
    } else if let Some(f) = fqbn {
        args.extend(owned(&["--fqbn", f]));
    }
    for p in extra_lib_paths {
        args.extend(owned(&["--library", p]));
    }
    args.push(sketch_dir.to_string());
    args
}

/// `upload -p <port> [--profile P | --fqbn F] <sketch>`
fn upload_args(
    sketch_dir: &str,
    profile: Option<&str>,
    fqbn: Option<&str>,
    port: &str,
) -> Vec<String> {
    let mut args = owned(&["upload", "-p", port]);
    if let Some(p) = profile {
        args.extend(owned(&["--profile", p]));
    } else if let Some(f) = fqbn {
        args.extend(owned(&["--fqbn", f]));
    }
    args.push(sketch_dir.to_string());
    args
}

/// `profile create -m <profile> -b <fqbn> [--set-default] <sketch>`
fn profile_create_args(
    sketch_dir: &Path,
    profile: &str,
    fqbn: &str,
    set_default: bool,
) -> Vec<String> {
    let mut args = owned(&["profile", "create", "-m", profile, "-b", fqbn]);
    if set_default {
        args.push("--set-default".to_string());
    }
    args.push(sketch_dir.to_string_lossy().into_owned());
    args
}

/// `profile lib add <spec> -m <profile> --sketch-path <sketch>`
///
/// The sketch **must** go through `--sketch-path`; see the note above.
fn profile_lib_add_args(sketch_dir: &Path, profile: &str, spec: &str) -> Vec<String> {
    let mut args = owned(&["profile", "lib", "add", spec, "-m", profile, "--sketch-path"]);
    args.push(sketch_dir.to_string_lossy().into_owned());
    args
}

/// `core install <id>[@version]`
///
/// A platform spec uses the same `name@version` grammar as a library, so
/// [`lib_spec`] builds it rather than a second near-identical helper.
fn core_install_args(id: &str, version: Option<&str>) -> Vec<String> {
    let mut args = owned(&["core", "install"]);
    args.push(lib_spec(id, version));
    args
}

/// `monitor -p <port> -c baudrate=<n>`
fn monitor_args(port: &str, baudrate: u32) -> Vec<String> {
    let mut args = owned(&["monitor", "-p", port, "-c"]);
    args.push(format!("baudrate={baudrate}"));
    args
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_prefers_the_profile_over_an_fqbn() {
        // Both supplied: the profile wins and --fqbn must not appear, or
        // arduino-cli would be told two contradictory things.
        let args = compile_args("/s", Some("esp32s3"), Some("arduino:avr:uno"), &[]);
        assert_eq!(args, ["compile", "--profile", "esp32s3", "/s"]);
        assert!(!args.iter().any(|a| a == "--fqbn"));
    }

    #[test]
    fn compile_falls_back_to_fqbn() {
        assert_eq!(
            compile_args("/s", None, Some("arduino:avr:uno"), &[]),
            ["compile", "--fqbn", "arduino:avr:uno", "/s"]
        );
    }

    #[test]
    fn compile_with_neither_target_still_names_the_sketch() {
        // arduino-cli then uses the sketch's default profile, so this is valid.
        assert_eq!(compile_args("/s", None, None, &[]), ["compile", "/s"]);
    }

    #[test]
    fn compile_repeats_library_for_each_path_and_keeps_sketch_last() {
        let libs = vec!["/a/Lib1".to_string(), "/b/Lib2".to_string()];
        let args = compile_args("/s", Some("p"), None, &libs);
        assert_eq!(
            args,
            [
                "compile", "--profile", "p", "--library", "/a/Lib1", "--library", "/b/Lib2", "/s",
            ]
        );
        // The sketch path is positional and must stay at the end.
        assert_eq!(args.last().unwrap(), "/s");
    }

    #[test]
    fn upload_always_passes_the_port_and_ends_with_the_sketch() {
        let args = upload_args("/s", Some("p"), None, "/dev/ttyUSB0");
        assert_eq!(
            args,
            ["upload", "-p", "/dev/ttyUSB0", "--profile", "p", "/s"]
        );
        assert_eq!(args.last().unwrap(), "/s");
    }

    #[test]
    fn upload_falls_back_to_fqbn_like_compile() {
        let args = upload_args("/s", None, Some("a:b:c"), "/dev/x");
        assert_eq!(args, ["upload", "-p", "/dev/x", "--fqbn", "a:b:c", "/s"]);
    }

    #[test]
    fn profile_create_appends_set_default_before_the_sketch() {
        let args = profile_create_args(Path::new("/s"), "esp32s3", "esp32:esp32:esp32s3", true);
        assert_eq!(
            args,
            [
                "profile",
                "create",
                "-m",
                "esp32s3",
                "-b",
                "esp32:esp32:esp32s3",
                "--set-default",
                "/s",
            ]
        );
    }

    #[test]
    fn profile_create_omits_set_default_when_not_wanted() {
        let args = profile_create_args(Path::new("/s"), "p", "a:b:c", false);
        assert!(!args.iter().any(|a| a == "--set-default"));
        assert_eq!(args.last().unwrap(), "/s");
    }

    #[test]
    fn profile_lib_add_passes_the_sketch_via_sketch_path_not_positionally() {
        // Regression guard for a real bug: passing the sketch positionally makes
        // arduino-cli look for `<cwd>.ino` and fail with "main file missing".
        let args = profile_lib_add_args(Path::new("/s"), "esp32s3", "ArduinoJson@7.4.2");
        assert_eq!(
            args,
            [
                "profile",
                "lib",
                "add",
                "ArduinoJson@7.4.2",
                "-m",
                "esp32s3",
                "--sketch-path",
                "/s",
            ]
        );
        // The library is the positional argument; the sketch never is.
        assert_eq!(args[3], "ArduinoJson@7.4.2");
        let sketch_at = args.iter().position(|a| a == "/s").unwrap();
        assert_eq!(args[sketch_at - 1], "--sketch-path");
    }

    #[test]
    fn core_install_pins_the_version_when_given() {
        assert_eq!(
            core_install_args("esp32:esp32", Some("3.3.11")),
            ["core", "install", "esp32:esp32@3.3.11"]
        );
    }

    #[test]
    fn core_install_without_a_version_takes_the_latest() {
        // No trailing `@`: arduino-cli reads that as an empty version constraint
        // and refuses the install.
        assert_eq!(
            core_install_args("esp32:esp32", None),
            ["core", "install", "esp32:esp32"]
        );
        assert_eq!(
            core_install_args("esp32:esp32", Some("")),
            ["core", "install", "esp32:esp32"]
        );
    }

    #[test]
    fn lib_spec_appends_a_version_only_when_present() {
        assert_eq!(lib_spec("ArduinoJson", Some("7.4.2")), "ArduinoJson@7.4.2");
        assert_eq!(lib_spec("ArduinoJson", None), "ArduinoJson");
        // A blank version must not produce a trailing `@`, which arduino-cli
        // reads as an empty version constraint.
        assert_eq!(lib_spec("ArduinoJson", Some("")), "ArduinoJson");
        assert_eq!(lib_spec("ArduinoJson", Some("  ")), "ArduinoJson");
    }

    #[test]
    fn lib_spec_keeps_names_with_spaces_intact() {
        // Registry names really do contain spaces, e.g. "Adafruit GFX Library";
        // they are one argv entry, not shell-quoted.
        assert_eq!(
            lib_spec("Adafruit GFX Library", Some("1.12.6")),
            "Adafruit GFX Library@1.12.6"
        );
    }

    #[test]
    fn monitor_encodes_the_baudrate_as_a_config_pair() {
        assert_eq!(
            monitor_args("/dev/ttyUSB0", 115200),
            ["monitor", "-p", "/dev/ttyUSB0", "-c", "baudrate=115200"]
        );
    }

    #[test]
    fn as_str_slice_preserves_order_and_content() {
        let owned_args = owned(&["a", "b b", "c"]);
        assert_eq!(as_str_slice(&owned_args), ["a", "b b", "c"]);
    }

    #[test]
    fn default_binary_is_arduino_cli_from_path() {
        assert_eq!(ArduinoCli::default().bin, "arduino-cli");
        assert_eq!(ArduinoCli::new("/opt/arduino-cli").bin, "/opt/arduino-cli");
    }
}
