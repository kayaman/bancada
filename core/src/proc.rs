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

use std::io::{BufRead, BufReader, Read};
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
    let t_out = spawn_line_reader(stdout, OutputStream::Stdout, tx.clone());
    let t_err = spawn_line_reader(stderr, OutputStream::Stderr, tx);

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

/// Forward one pipe to `tx` a line at a time until it closes.
///
/// Lines are decoded lossily rather than with `BufRead::lines`, which stops at
/// the first byte sequence that is not UTF-8. A reader that quits early stops
/// draining the pipe, and the child then blocks on a full pipe or dies of
/// SIGPIPE — a whole build lost to one stray byte in a compiler message.
fn spawn_line_reader(
    pipe: impl Read + Send + 'static,
    stream: OutputStream,
    tx: mpsc::Sender<OutputLine>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if buf.last() == Some(&b'\n') {
                buf.pop();
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
            }
            let line = String::from_utf8_lossy(&buf).into_owned();
            if tx.send(OutputLine { stream, line }).is_err() {
                break;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_output_does_not_stop_the_stream() {
        let mut lines = Vec::new();
        let result = stream(
            sh("printf 'before\\n\\377bad\\n'; seq 1 20000; echo done >&2"),
            "sh",
            |l| lines.push(l),
        )
        .unwrap();
        assert!(result.success, "child must not die of SIGPIPE");
        let stdout: Vec<&str> = lines
            .iter()
            .filter(|l| l.stream == OutputStream::Stdout)
            .map(|l| l.line.as_str())
            .collect();
        assert_eq!(stdout.len(), 20002);
        assert_eq!(stdout[0], "before");
        assert_eq!(stdout[1], "\u{FFFD}bad");
        assert_eq!(stdout.last(), Some(&"20000"));
        assert!(lines
            .iter()
            .any(|l| l.stream == OutputStream::Stderr && l.line == "done"));
    }

    #[cfg(unix)]
    #[test]
    fn crlf_and_unterminated_last_line_are_handled() {
        let mut lines = Vec::new();
        stream(sh("printf 'a\\r\\nb'"), "sh", |l| lines.push(l.line)).unwrap();
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn a_missing_binary_is_tool_missing() {
        let cmd = Command::new("bancada-definitely-not-a-binary");
        let err = stream(cmd, "bancada-definitely-not-a-binary", |_| {}).unwrap_err();
        assert!(matches!(err, Error::ToolMissing(_)), "{err}");
    }
}
