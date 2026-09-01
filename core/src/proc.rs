//! Subprocess plumbing shared by every external toolchain this crate drives.
//!
//! Extracted from [`crate::cli`] when ESP-IDF became a second build backend.
//! The two backends run different programs with different arguments, but the
//! *mechanics* — spawn, interleave stdout and stderr into one ordered stream,
//! join, translate a missing binary into [`Error::ToolMissing`] — are identical
//! and were worth having in exactly one place.
//!
//! Everything here takes an already-configured [`Command`]. Deciding the
//! program, the arguments and the environment belongs to the caller; this
//! module only runs it. That split is what lets `arduino-cli`'s and `idf.py`'s
//! argv builders both stay pure and separately unit-tested.

use std::io::{BufRead, BufReader};
use std::process::Command;
use std::sync::mpsc;

use crate::types::{OutputLine, OutputStream, RunResult};
use crate::{Error, Result};

/// Translate a spawn failure into a typed error.
///
/// `NotFound` is the one case worth distinguishing: it means the toolchain is
/// not installed, which is a different conversation from a toolchain that ran
/// and failed. `bin` names the executable, not the whole command line — the
/// message it produces is an instruction to install something.
pub(crate) fn map_spawn_err(e: std::io::Error, bin: &str) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        Error::ToolMissing(bin.to_string())
    } else {
        Error::Io(e)
    }
}

/// Run to completion, streaming stdout+stderr lines (interleaved) into
/// `on_line`. Returns the exit status.
///
/// A non-zero exit is **not** an `Err`: a failed compile is a normal outcome
/// that the caller reports through [`RunResult`], and only a failure to *run*
/// the tool at all is an error. Callers downstream (the build gate, the MCP
/// `verify` tool) depend on that distinction.
///
/// `cmd` must already have stdout and stderr piped.
pub(crate) fn stream(
    mut cmd: Command,
    bin: &str,
    mut on_line: impl FnMut(OutputLine),
) -> Result<RunResult> {
    let mut child = cmd.spawn().map_err(|e| map_spawn_err(e, bin))?;

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

/// Run to completion and return stdout, for commands whose value is the side
/// effect rather than parsed output.
///
/// `bin` names the executable (for a spawn failure); `display` names the whole
/// command line (for a non-zero exit), because the two errors are read by
/// different people for different reasons.
pub(crate) fn output(mut cmd: Command, bin: &str, display: &str) -> Result<String> {
    let out = cmd.output().map_err(|e| map_spawn_err(e, bin))?;
    if !out.status.success() {
        return Err(Error::ToolFailed {
            tool: display.to_string(),
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
