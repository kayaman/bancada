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

    /// The configuration options a board exposes, so a profile can pin
    /// `esp32:esp32:esp32s3:CDCOnBoot=cdc` rather than a bare FQBN.
    ///
    /// The FQBN may itself carry options; arduino-cli then reports them as the
    /// `selected` values, which is how a saved profile's choices are read back.
    ///
    /// A blank FQBN is refused here rather than passed through: arduino-cli
    /// answers one with exit 1 and prints its complaint to *stdout*, so the
    /// resulting [`Error::ToolFailed`] would carry an empty stderr and explain
    /// nothing.
    pub fn board_details(&self, fqbn: &str) -> Result<BoardDetails> {
        if fqbn.trim().is_empty() {
            return Err(Error::Other("board details needs an FQBN".into()));
        }
        let args = board_details_args(fqbn);
        self.run_json(&as_str_slice(&args))
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
        Ok(r.installed_libraries
            .into_iter()
            .map(|e| e.library)
            .collect())
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

    /// Build the sketch and flash the result to `port`. The build is not
    /// optional: flashing without one sends whatever the build cache holds,
    /// which after a failed compile is stale or absent. See `upload_args`.
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

/// `compile -u -p <port> [--profile P | --fqbn F] <sketch>`
///
/// Deliberately `compile -u` and not `upload`: arduino-cli's `upload` flashes
/// the build cache without building it ("This does NOT compile the sketch prior
/// to upload"), so a sketch that failed to compile flashes the *previous*
/// binary — or, on a cache that never held one, makes esptool die on a missing
/// `<sketch>.ino.partitions.bin`. Folding the build in means a broken sketch
/// stops with its own compiler error and the board is left untouched.
///
/// Target selection follows `compile_args`: a profile wins over an FQBN.
fn upload_args(
    sketch_dir: &str,
    profile: Option<&str>,
    fqbn: Option<&str>,
    port: &str,
) -> Vec<String> {
    let mut args = owned(&["compile", "-u", "-p", port]);
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
    let mut args = owned(&[
        "profile",
        "lib",
        "add",
        spec,
        "-m",
        profile,
        "--sketch-path",
    ]);
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

/// `board details --fqbn <fqbn>`
///
/// The FQBN stays one argument even when it carries options
/// (`esp32:esp32:esp32s3:CDCOnBoot=cdc`): the `=` belongs to the FQBN grammar,
/// not to the command line.
fn board_details_args(fqbn: &str) -> Vec<String> {
    owned(&["board", "details", "--fqbn", fqbn.trim()])
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
                "compile",
                "--profile",
                "p",
                "--library",
                "/a/Lib1",
                "--library",
                "/b/Lib2",
                "/s",
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
            [
                "compile",
                "-u",
                "-p",
                "/dev/ttyUSB0",
                "--profile",
                "p",
                "/s"
            ]
        );
        assert_eq!(args.last().unwrap(), "/s");
    }

    #[test]
    fn upload_falls_back_to_fqbn_like_compile() {
        let args = upload_args("/s", None, Some("a:b:c"), "/dev/x");
        assert_eq!(
            args,
            ["compile", "-u", "-p", "/dev/x", "--fqbn", "a:b:c", "/s"]
        );
    }

    /// Regression: `arduino-cli upload` does not build. Flashing through a bare
    /// `upload` sends whatever the build cache happens to hold — and when the
    /// last compile failed it holds nothing, so esptool reports a missing
    /// `<sketch>.ino.partitions.bin` instead of the compile error that caused
    /// it. `compile -u` makes arduino-cli refuse to flash a build it did not
    /// just produce.
    #[test]
    fn upload_builds_first_so_a_failed_compile_never_reaches_the_board() {
        let args = upload_args("/s", Some("p"), None, "/dev/x");
        assert_eq!(args.first().unwrap(), "compile");
        assert!(args.iter().any(|a| a == "-u"));
        assert!(!args.iter().any(|a| a == "upload"));
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
    fn board_details_names_the_board_through_the_fqbn_flag() {
        assert_eq!(
            board_details_args("esp32:esp32:esp32s3"),
            ["board", "details", "--fqbn", "esp32:esp32:esp32s3"]
        );
    }

    #[test]
    fn board_details_keeps_an_option_bearing_fqbn_as_one_argument() {
        // `CDCOnBoot=cdc` is part of the FQBN, not a separate flag; splitting it
        // would make arduino-cli reject the argument.
        let args = board_details_args("  esp32:esp32:esp32s3:CDCOnBoot=cdc  ");
        assert_eq!(args.len(), 4, "{args:?}");
        assert_eq!(args[3], "esp32:esp32:esp32s3:CDCOnBoot=cdc", "and trimmed");
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

    // ---------- subprocess plumbing ----------
    //
    // `bin` is just a path, so pointing it at a stub script exercises the real
    // spawn/parse/error paths with no fixtures and no arduino-cli. That covers
    // the error taxonomy users actually see — "is it installed?", "the tool
    // failed", "the output made no sense" — which is otherwise only reachable by
    // breaking someone's machine.

    /// Serialises everything that writes-then-executes a script.
    ///
    /// Without it these tests fail intermittently with ETXTBSY ("Text file
    /// busy"): while one thread still has the stub open for writing, another
    /// thread's `fork` inherits that write descriptor, and the exec of the script
    /// is refused until the forked child execs and drops it. The files are
    /// distinct — the hazard is the *concurrency*, not a shared path — so the
    /// only reliable fix is to keep one write-then-exec in flight at a time.
    /// Measured before the guard: 7 failures in 12 parallel runs, 0 in 12 serial.
    #[cfg(unix)]
    static STUB_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(unix)]
    struct Stub {
        // Held for the stub's lifetime; released on drop.
        _guard: std::sync::MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
        cli: ArduinoCli,
        argv_log: std::path::PathBuf,
    }

    #[cfg(unix)]
    impl Stub {
        fn argv(&self) -> String {
            std::fs::read_to_string(&self.argv_log).unwrap_or_default()
        }
    }

    /// Run `f` against a fake arduino-cli whose body is `sh` source.
    ///
    /// Scoped rather than returned so the guard is always released — two live
    /// stubs in one test would deadlock on `STUB_GUARD`.
    #[cfg(unix)]
    fn with_stub<T>(body: &str, f: impl FnOnce(&Stub) -> T) -> T {
        use std::os::unix::fs::PermissionsExt;
        // Ignore poisoning: a panicking test must not cascade into the rest.
        let guard = STUB_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-arduino-cli");
        let argv_log = dir.path().join("argv");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nARGV_LOG='{}'\nprintf '%s\\n' \"$*\" >> \"$ARGV_LOG\"\n{body}\n",
                argv_log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let s = Stub {
            cli: ArduinoCli::new(script.to_string_lossy().into_owned()),
            argv_log,
            _dir: dir,
            _guard: guard,
        };
        f(&s)
    }

    #[test]
    fn a_missing_binary_reports_tool_missing_not_an_io_error() {
        // The UI turns this into "is arduino-cli installed?", so the variant
        // matters, not just the failure.
        let cli = ArduinoCli::new("bancada-definitely-no-such-binary");
        match cli.version() {
            Err(Error::ToolMissing(name)) => {
                assert!(name.contains("bancada-definitely-no-such-binary"), "{name}")
            }
            other => panic!("expected ToolMissing, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_non_zero_exit_reports_tool_failed_with_the_stderr() {
        with_stub("echo 'boom happened' >&2; exit 3", |s| {
            match s.cli.board_list() {
                Err(Error::ToolFailed { status, stderr, .. }) => {
                    assert_eq!(status, 3);
                    assert!(stderr.contains("boom happened"), "{stderr}");
                }
                other => panic!("expected ToolFailed, got {other:?}"),
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn unparseable_output_reports_a_json_error_naming_the_command() {
        with_stub("echo 'this is not json'", |s| match s.cli.board_list() {
            Err(Error::Json { what, .. }) => {
                assert!(what.contains("board list"), "{what}")
            }
            other => panic!("expected Json, got {other:?}"),
        });
    }

    #[cfg(unix)]
    #[test]
    fn run_json_appends_the_json_flag() {
        with_stub("echo '{}'", |s| {
            let _ = s.cli.board_list();
            assert!(s.argv().contains("board list --json"), "{}", s.argv());
        });
    }

    #[cfg(unix)]
    #[test]
    fn run_ok_does_not_append_the_json_flag() {
        // `sketch new` and `profile create` print prose; passing --json to them
        // is wrong, so the two runners must differ here.
        with_stub("exit 0", |s| {
            s.cli.sketch_new(Path::new("/tmp/Demo")).unwrap();
            let argv = s.argv();
            assert!(argv.contains("sketch new /tmp/Demo"), "{argv}");
            assert!(!argv.contains("--json"), "{argv}");
        });
    }

    #[cfg(unix)]
    #[test]
    fn version_prefers_versionstring_then_falls_back() {
        with_stub(r#"echo '{"VersionString":"1.5.0"}'"#, |s| {
            assert_eq!(s.cli.version().unwrap(), "1.5.0")
        });
        with_stub(r#"echo '{"version":"9.9.9"}'"#, |s| {
            assert_eq!(s.cli.version().unwrap(), "9.9.9")
        });
        with_stub("echo '{}'", |s| {
            assert_eq!(s.cli.version().unwrap(), "unknown")
        });
    }

    #[cfg(unix)]
    #[test]
    fn board_list_returns_the_detected_ports() {
        with_stub(
            r#"echo '{"detected_ports":[{"port":{"address":"/dev/ttyUSB0","protocol":"serial"}}]}'"#,
            |s| {
                let ports = s.cli.board_list().unwrap();
                assert_eq!(ports.len(), 1);
                assert_eq!(ports[0].port.address, "/dev/ttyUSB0");
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn lib_list_unwraps_each_entry_to_its_library() {
        with_stub(
            r#"echo '{"installed_libraries":[{"library":{"name":"PubSubClient","version":"2.8.0"}}]}'"#,
            |s| {
                let libs = s.cli.lib_list().unwrap();
                assert_eq!(libs.len(), 1);
                assert_eq!(libs[0].name, "PubSubClient");
                assert_eq!(libs[0].version, "2.8.0");
                assert!(s.argv().contains("lib list --json"), "{}", s.argv());
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn lib_search_passes_the_query_and_returns_the_libraries() {
        with_stub(
            r#"echo '{"libraries":[{"name":"ArduinoJson","latest":{"version":"7.4.3"}}]}'"#,
            |s| {
                let libs = s.cli.lib_search("json").unwrap();
                assert_eq!(libs.len(), 1);
                assert_eq!(libs[0].name, "ArduinoJson");
                assert!(s.argv().contains("lib search json --json"), "{}", s.argv());
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn board_listall_flattens_installed_boards_with_fqbns() {
        // One valid board, one without an fqbn, one on an uninstalled
        // platform — only the first survives.
        with_stub(
            r#"echo '{"boards":[
              {"name":"ESP32S3 Dev Module","fqbn":"esp32:esp32:esp32s3","platform":{"metadata":{"id":"esp32:esp32"},"release":{"name":"esp32","installed":true}}},
              {"name":"No FQBN","fqbn":"","platform":{"metadata":{"id":"x:y"},"release":{"name":"x","installed":true}}},
              {"name":"Not Installed","fqbn":"a:b:c","platform":{"metadata":{"id":"a:b"},"release":{"name":"a","installed":false}}}
            ]}'"#,
            |s| {
                let boards = s.cli.board_listall().unwrap();
                assert_eq!(boards.len(), 1);
                assert_eq!(boards[0].fqbn, "esp32:esp32:esp32s3");
                assert_eq!(boards[0].platform_id, "esp32:esp32");
                assert!(s.argv().contains("board listall --json"), "{}", s.argv());
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn board_listall_drops_uninstalled_and_fqbn_less_entries() {
        // The flattening is where a picker could silently offer a board that
        // `profile create` then refuses, so the filter is worth pinning.
        with_stub(
            r#"echo '{"boards":[
              {"name":"Installed","fqbn":"a:b:c","platform":{"metadata":{"id":"a:b"},"release":{"name":"Plat A","installed":true}}},
              {"name":"NotInstalled","fqbn":"x:y:z","platform":{"metadata":{"id":"x:y"},"release":{"name":"Plat X","installed":false}}},
              {"name":"NoFqbn","fqbn":"","platform":{"metadata":{"id":"a:b"},"release":{"name":"Plat A","installed":true}}}
            ]}'"#,
            |s| {
                let boards = s.cli.board_listall().unwrap();
                assert_eq!(boards.len(), 1, "{boards:?}");
                assert_eq!(boards[0].fqbn, "a:b:c");
                assert_eq!(boards[0].platform_name, "Plat A");
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn board_listall_sorts_by_platform_then_board_and_dedupes_fqbns() {
        with_stub(
            r#"echo '{"boards":[
              {"name":"Zeta","fqbn":"p:1:z","platform":{"metadata":{"id":"p"},"release":{"name":"Plat B","installed":true}}},
              {"name":"Alpha","fqbn":"p:1:a","platform":{"metadata":{"id":"p"},"release":{"name":"Plat B","installed":true}}},
              {"name":"Solo","fqbn":"q:1:s","platform":{"metadata":{"id":"q"},"release":{"name":"Plat A","installed":true}}},
              {"name":"Solo","fqbn":"q:1:s","platform":{"metadata":{"id":"q"},"release":{"name":"Plat A","installed":true}}}
            ]}'"#,
            |s| {
                let boards = s.cli.board_listall().unwrap();
                let names: Vec<&str> = boards.iter().map(|b| b.name.as_str()).collect();
                // Plat A first; Alpha before Zeta inside Plat B; the repeat gone.
                assert_eq!(names, ["Solo", "Alpha", "Zeta"]);
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn board_details_reads_the_menus_and_their_selected_values() {
        // The payload is a trimmed capture: one of the ESP32-S3's 17 options,
        // with `selected` present on the default value and absent on the other.
        with_stub(
            r#"echo '{"fqbn":"esp32:esp32:esp32s3","name":"ESP32S3 Dev Module","config_options":[
              {"option":"CDCOnBoot","option_label":"USB CDC On Boot","values":[
                {"value":"default","value_label":"Disabled","selected":true},
                {"value":"cdc","value_label":"Enabled"}]}
            ]}'"#,
            |s| {
                let d = s.cli.board_details("esp32:esp32:esp32s3").unwrap();
                assert_eq!(d.name, "ESP32S3 Dev Module");
                assert_eq!(d.config_options.len(), 1);
                let opt = &d.config_options[0];
                assert_eq!(opt.option, "CDCOnBoot");
                assert!(opt.values[0].selected);
                assert!(!opt.values[1].selected, "an absent key is not a choice");
                assert!(
                    s.argv()
                        .contains("board details --fqbn esp32:esp32:esp32s3 --json"),
                    "{}",
                    s.argv()
                );
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn board_details_accepts_a_board_with_no_menus() {
        // An UNO omits `config_options` entirely; erroring would make every
        // option-less board unopenable.
        with_stub(
            r#"echo '{"fqbn":"arduino:avr:uno","name":"Arduino UNO"}'"#,
            |s| {
                let d = s.cli.board_details("arduino:avr:uno").unwrap();
                assert_eq!(d.name, "Arduino UNO");
                assert!(d.config_options.is_empty());
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn board_details_refuses_an_empty_fqbn_without_running_the_tool() {
        // arduino-cli answers a blank --fqbn with exit 1 and its complaint on
        // *stdout*, so ToolFailed would surface an empty stderr and say
        // nothing. Refusing first keeps the message useful.
        with_stub("echo '{}'", |s| {
            for blank in ["", "   "] {
                match s.cli.board_details(blank) {
                    Err(Error::Other(msg)) => assert!(msg.contains("FQBN"), "{msg}"),
                    other => panic!("expected Other, got {other:?}"),
                }
            }
            assert_eq!(s.argv(), "", "arduino-cli must not have been spawned");
        });
    }

    #[cfg(unix)]
    #[test]
    fn sketchbook_dir_unquotes_the_json_string() {
        with_stub(r#"echo '"/home/me/Arduino"'"#, |s| {
            assert_eq!(
                s.cli.sketchbook_dir().unwrap(),
                PathBuf::from("/home/me/Arduino")
            );
            assert_eq!(
                s.cli.sketchbook_libraries_dir().unwrap(),
                PathBuf::from("/home/me/Arduino/libraries")
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_sketchbook_path_is_an_error_not_a_root_directory() {
        // Returning PathBuf::from("") would make every project land at "/".
        with_stub(r#"echo '"   "'"#, |s| {
            assert!(s.cli.sketchbook_dir().is_err())
        });
    }

    #[cfg(unix)]
    #[test]
    fn library_and_core_queries_unwrap_their_envelopes() {
        with_stub(
            r#"echo '{"libraries":[{"name":"ArduinoJson","latest":{"version":"7.4.3"}}]}'"#,
            |s| assert_eq!(s.cli.lib_search("json").unwrap()[0].name, "ArduinoJson"),
        );
        with_stub(
            r#"echo '{"installed_libraries":[{"library":{"name":"L","location":"user"}}]}'"#,
            |s| assert_eq!(s.cli.lib_list().unwrap()[0].location, "user"),
        );
        with_stub(r#"echo '{"platforms":[{"id":"esp32:esp32"}]}'"#, |s| {
            assert_eq!(s.cli.core_list().unwrap()[0].id, "esp32:esp32")
        });
    }

    #[cfg(unix)]
    #[test]
    fn lib_install_sends_name_at_version() {
        with_stub("echo '{}'", |s| {
            s.cli.lib_install("ArduinoJson", Some("7.4.2")).unwrap();
            assert!(
                s.argv().contains("lib install ArduinoJson@7.4.2"),
                "{}",
                s.argv()
            );
        });
        with_stub("echo '{}'", |s| {
            s.cli.lib_install("ArduinoJson", None).unwrap();
            let argv = s.argv();
            assert!(argv.contains("lib install ArduinoJson"), "{argv}");
            assert!(!argv.contains('@'), "{argv}");
        });
    }

    #[cfg(unix)]
    #[test]
    fn run_streaming_tags_stdout_and_stderr_separately() {
        with_stub("echo out-line; echo err-line >&2; exit 0", |s| {
            let mut seen = Vec::new();
            let result = s
                .cli
                .compile("/sketch", Some("p"), None, &[], |l| seen.push(l))
                .unwrap();
            assert!(result.success);
            assert_eq!(result.exit_code, 0);

            let of = |want: OutputStream| -> Vec<String> {
                seen.iter()
                    .filter(|l| l.stream == want)
                    .map(|l| l.line.clone())
                    .collect()
            };
            assert!(of(OutputStream::Stdout).contains(&"out-line".to_string()));
            assert!(of(OutputStream::Stderr).contains(&"err-line".to_string()));
        });
    }

    #[cfg(unix)]
    #[test]
    fn run_streaming_reports_a_failing_exit_code_without_erroring() {
        // A failed compile is a normal outcome the UI renders, not an Err.
        with_stub("echo 'error: expected ;' >&2; exit 1", |s| {
            let result = s
                .cli
                .compile("/sketch", None, Some("a:b:c"), &[], |_| {})
                .unwrap();
            assert!(!result.success);
            assert_eq!(result.exit_code, 1);
        });
    }
}
