//! Opt-in live round trip against the real `claude` CLI, driven purely
//! through `bancada_core::agent` — no MCP listener, no src-tauri plumbing.
//! This is the "Pong" scenario from the agent-panel plan's Verification
//! section: prove `agent_args`/`user_message_json`/`parse_event` compose
//! into a working headless turn against the real binary, not just against
//! the recorded `agent_stream_pong.ndjson` fixture `core/src/agent.rs`'s
//! unit tests replay.
//!
//! Needs a `claude` CLI on PATH, a logged-in account, network, and it
//! spends real tokens — hence `#[ignore]` *and* an env gate, matching the
//! convention `src-tauri/src/lib.rs`'s
//! `live_claude_calls_the_verify_tool_end_to_end` already established:
//!
//! ```text
//! BANCADA_AGENT_LIVE=1 cargo test -p bancada-core --test agent_live -- --ignored --nocapture
//! ```

use bancada_core::agent::{self, AgentCfg, AgentEvent};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
#[ignore = "spawns the real claude CLI: needs login, network and tokens"]
fn live_claude_replies_pong_and_parses_as_system_init_and_result() {
    if std::env::var("BANCADA_AGENT_LIVE").is_err() {
        eprintln!("skipped: set BANCADA_AGENT_LIVE=1 to run the live pong round trip");
        return;
    }

    // No MCP server needed for this scenario, but `agent_args` always wires
    // one up (--mcp-config is not optional in the real argv) — an empty
    // server list is a valid, minimal MCP config file.
    let dir = tempfile::tempdir().unwrap();
    let mcp_config_path = dir.path().join("mcp.json");
    std::fs::write(&mcp_config_path, r#"{"mcpServers":{}}"#).unwrap();

    let sketch_dir = tempfile::tempdir().unwrap();

    let cfg = AgentCfg {
        mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
        system_prompt_extra: "You are being exercised by an automated test. \
            Reply with exactly the text: pong"
            .to_string(),
    };

    let mut child = Command::new("claude")
        .args(agent::agent_args(&cfg))
        .current_dir(sketch_dir.path())
        .env("MCP_TOOL_TIMEOUT", "600000")
        .env("MCP_TIMEOUT", "600000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn claude — is it on PATH and logged in?");

    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        agent::user_message_json("Reply with exactly: pong")
    )
    .unwrap();
    stdin.flush().unwrap();
    drop(stdin); // single turn: EOF lets the child finish

    let stdout = child.stdout.take().unwrap();
    let mut saw_system_init = false;
    let mut saw_successful_result = false;
    let mut result_text = String::new();
    let mut raw_lines: Vec<String> = Vec::new();

    for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
        raw_lines.push(line.clone());
        match agent::parse_event(&line) {
            Ok(AgentEvent::System(s)) if s.subtype == "init" => saw_system_init = true,
            Ok(AgentEvent::Result(r)) => {
                if !r.is_error {
                    saw_successful_result = true;
                }
                result_text = r.result.unwrap_or_default();
            }
            Ok(_) => {}
            Err(e) => {
                panic!("a line from the real claude CLI failed to parse as JSON: {e}\nline: {line}")
            }
        }
    }

    let status = child.wait().expect("wait on claude");

    eprintln!("--- transcript ({} lines) ---", raw_lines.len());
    for l in &raw_lines {
        eprintln!("{l}");
    }
    eprintln!("--- end transcript, exit status: {status:?} ---");

    assert!(
        saw_system_init,
        "no system/init line seen in the real transcript"
    );
    assert!(
        saw_successful_result,
        "no successful (is_error=false) result line seen in the real transcript"
    );
    assert!(
        result_text.to_lowercase().contains("pong"),
        "result text did not contain 'pong': {result_text:?}"
    );
}
