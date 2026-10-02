//! Integration test for the actual compiled `--agent-guard` entry point, not
//! just the pure `guard_decision` function it wraps: this is exactly the
//! invocation `claude`'s `PreToolUse` hook makes (a subprocess, JSON on
//! stdin, a decision on stdout, exit code 0 either way), so a bug in the
//! argv/stdin/exit-code plumbing around `guard_decision` wouldn't be caught
//! by unit-testing that function alone.

use std::io::Write;
use std::process::{Command, Stdio};

fn run_guard(project_dir: &str, stdin: &str) -> (String, i32) {
    let exe = env!("CARGO_BIN_EXE_enclosure-maker");
    let mut child = Command::new(exe)
        .args(["--agent-guard", project_dir])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn enclosure-maker --agent-guard");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn agent_guard_allows_an_in_project_write_and_exits_zero() {
    let dir = std::env::temp_dir().join(format!(
        "em-preview-guard-test-allow-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let stdin = serde_json::json!({
        "tool_name": "Write",
        "tool_input": { "file_path": dir.join("model.rhai").to_string_lossy() }
    })
    .to_string();
    let (stdout, code) = run_guard(&dir.to_string_lossy(), &stdin);
    assert_eq!(code, 0);
    assert_eq!(stdout, "", "an allowed edit must print nothing");
}

#[test]
fn agent_guard_denies_an_out_of_project_write_and_still_exits_zero() {
    let dir =
        std::env::temp_dir().join(format!("em-preview-guard-test-deny-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stdin = serde_json::json!({
        "tool_name": "Write",
        "tool_input": { "file_path": "/etc/passwd" }
    })
    .to_string();
    let (stdout, code) = run_guard(&dir.to_string_lossy(), &stdin);
    assert_eq!(code, 0, "a hook must exit 0 even when it denies -- non-zero means the hook itself failed, which fails open");
    assert!(stdout.contains("\"permissionDecision\":\"deny\""));
}
