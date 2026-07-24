//! Thin, typed wrapper around the `arduino-cli` executable.
//!
//! Everything that returns data uses `--json`; long-running operations
//! (compile/upload) stream their human-readable output line by line through a
//! callback so the UI can show live progress.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
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
        let spec = match version {
            Some(v) => format!("{name}@{v}"),
            None => name.to_string(),
        };
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
        let mut args: Vec<&str> = vec!["compile"];
        if let Some(p) = profile {
            args.extend_from_slice(&["--profile", p]);
        } else if let Some(f) = fqbn {
            args.extend_from_slice(&["--fqbn", f]);
        }
        for p in extra_lib_paths {
            args.extend_from_slice(&["--library", p]);
        }
        args.push(sketch_dir);
        self.run_streaming(&args, on_line)
    }

    pub fn upload(
        &self,
        sketch_dir: &str,
        profile: Option<&str>,
        fqbn: Option<&str>,
        port: &str,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        let mut args: Vec<&str> = vec!["upload", "-p", port];
        if let Some(p) = profile {
            args.extend_from_slice(&["--profile", p]);
        } else if let Some(f) = fqbn {
            args.extend_from_slice(&["--fqbn", f]);
        }
        args.push(sketch_dir);
        self.run_streaming(&args, on_line)
    }

    /// Spawn `arduino-cli monitor` as a long-lived child process.
    pub fn monitor(&self, port: &str, baudrate: u32) -> Result<Child> {
        let cfg = format!("baudrate={baudrate}");
        self.spawn_raw(&["monitor", "-p", port, "-c", &cfg])
    }
}
