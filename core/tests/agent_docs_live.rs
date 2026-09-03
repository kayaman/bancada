//! Opt-in live check that the Espressif documentation server reaches an
//! embedded-shaped session, driven through `bancada_core::agent`'s real argv.
//!
//! This is the executable form of the probe that justified adding the server
//! at all. It asserts two different things depending on whether the user has
//! authenticated it, because both states are legitimate and each has a
//! property worth pinning:
//!
//! - **Before `claude mcp login espressif-docs`** the server reports
//!   `needs-auth` and contributes no tools. The property that matters is that
//!   this is *silent*: `unexpected_tools` must stay empty, or every ESP-IDF
//!   session would be stopped at init by the A2 backstop.
//! - **After** it, the tool must actually appear — and specifically it must
//!   appear even though Bancada passes `--strict-mcp-config` with its own
//!   generated config rather than reading the user's. That is the one thing
//!   about this integration that could not be settled by reading docs.
//!
//! ```text
//! BANCADA_AGENT_LIVE=1 cargo test -p bancada-core --test agent_docs_live -- --ignored --nocapture
//! ```

use bancada_core::agent::{self, AgentCfg, AgentEvent};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
#[ignore = "spawns the real claude CLI: needs login, network and tokens"]
fn live_the_docs_server_reaches_an_esp_idf_session() {
    if std::env::var("BANCADA_AGENT_LIVE").is_err() {
        eprintln!("skipped: set BANCADA_AGENT_LIVE=1 to run");
        return;
    }

    let dir = tempfile::tempdir().unwrap();

    // The config Bancada actually writes for an ESP-IDF session: its own
    // loopback server plus the documentation server as a bare URL. The
    // loopback entry points nowhere here — this scenario is about the tool
    // list at init, not about calling a tool.
    let mcp_config_path = dir.path().join("mcp.json");
    std::fs::write(
        &mcp_config_path,
        format!(
            r#"{{"mcpServers":{{"bancada":{{"type":"http","url":"http://127.0.0.1:1/mcp","headers":{{"Authorization":"Bearer x"}}}},"{}":{{"type":"http","url":"{}"}}}}}}"#,
            agent::ESPRESSIF_DOCS_SERVER,
            agent::ESPRESSIF_DOCS_URL
        ),
    )
    .unwrap();

    let settings_path = dir.path().join("settings.json");
    std::fs::write(&settings_path, "{}").unwrap();

    let cfg = AgentCfg {
        mcp_config_path: mcp_config_path.to_string_lossy().into_owned(),
        settings_path: settings_path.to_string_lossy().into_owned(),
        system_prompt_extra: "You are being exercised by an automated test. \
            Reply with exactly the text: ok"
            .to_string(),
        resume_session_id: None,
        // The whole point: this is the ESP-IDF shape.
        with_docs: true,
    };

    let mut child = Command::new("claude")
        .args(agent::agent_args(&cfg))
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn claude");

    let mut stdin = child.stdin.take().expect("stdin");
    std::thread::spawn(move || {
        let _ = stdin.write_all(agent::user_message_json("ok").as_bytes());
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    });

    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut init_tools: Option<Vec<String>> = None;
    let mut servers = String::new();

    for line in stdout.lines().map_while(Result::ok) {
        if let Ok(AgentEvent::System(system)) = agent::parse_event(&line) {
            if system.subtype == "init" {
                init_tools = Some(system.tools.clone());
                // `mcp_servers` is not modelled on AgentEvent, so read the
                // status straight out of the raw line.
                if let Some(at) = line.find("\"mcp_servers\"") {
                    servers = line[at..].chars().take(240).collect();
                }
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();

    let tools = init_tools.expect("no system/init line");
    eprintln!("servers: {servers}");
    eprintln!("tools ({}): {tools:?}", tools.len());

    // Whatever the auth state, the session must not be stopped by the A2
    // backstop. This is the assertion that has to hold in *both* states.
    let extra = agent::unexpected_tools(&tools, true);
    assert!(
        extra.is_empty(),
        "the A2 backstop would stop this session over: {extra:?}"
    );

    let authenticated = !servers.contains("needs-auth");
    if authenticated {
        assert!(
            tools.iter().any(|t| t == agent::ESPRESSIF_DOCS_TOOL),
            "the server is authenticated, so its tool must reach a \
             --strict-mcp-config session; got {tools:?}"
        );
        eprintln!("AUTHENTICATED: the docs tool reached the session");
    } else {
        assert!(
            !tools.iter().any(|t| t == agent::ESPRESSIF_DOCS_TOOL),
            "an unauthenticated server must contribute no tools"
        );
        eprintln!(
            "NOT AUTHENTICATED: inert and silent, as designed. \
             Run `claude mcp login {}` to exercise the other half.",
            agent::ESPRESSIF_DOCS_SERVER
        );
    }
}
