use clap::{Parser, Subcommand};
use em_preview::{server, state::ChatState};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "enclosure-maker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the embedded chat assistant for a project, which drives
    /// FreeCAD directly through FreeCAD's own MCP server. FreeCAD itself
    /// (kept open, with its MCP server running) is the live model; this
    /// only serves the conversation.
    Chat {
        #[arg(long)]
        project_dir: PathBuf,
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
}

/// The `enclosure-maker --agent-guard <project_dir>` entry point: the
/// `PreToolUse` hook an embedded assistant session is started with. Reads
/// the CLI's hook JSON from stdin, adjudicates it with
/// `em_agent::guard_decision`, and prints a deny payload (or nothing) on
/// stdout. Always exits 0 -- a non-zero exit is how a hook reports its own
/// failure, which `claude` handles as "log and continue" (fail open, which
/// this must never do); the refusal is carried by the printed JSON instead.
///
/// Unchanged by the FreeCAD refactor: `Write`/`Edit`/`MultiEdit`/
/// `NotebookEdit` are no longer offered to the embedded session at all (see
/// `em_agent::TOOLS`), so this hook rarely fires now, but it stays as free
/// defense-in-depth against a future regression that re-adds one of them.
fn run_agent_guard(project_dir: &str) {
    let mut stdin_body = String::new();
    // A read error yields an empty body, which `guard_decision` denies --
    // failing closed, same as unparseable input.
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin_body);
    if let Some(deny) = em_agent::guard_decision(std::path::Path::new(project_dir), &stdin_body) {
        println!("{deny}");
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Checked before clap's normal parsing: this is a hidden entry point the
    // `claude` CLI itself invokes as a hook command, not a user-facing
    // subcommand.
    let raw_args: Vec<String> = std::env::args().collect();
    if let Some(idx) = raw_args.iter().position(|a| a == "--agent-guard") {
        if let Some(dir) = raw_args.get(idx + 1) {
            run_agent_guard(dir);
        }
        return Ok(());
    }

    let cli = Cli::parse();
    match cli.command {
        Command::Chat { project_dir, port } => run_chat(project_dir, port).await,
    }
}

async fn run_chat(project_dir: PathBuf, port: u16) -> anyhow::Result<()> {
    let chat = ChatState::new(project_dir);
    server::serve(port, chat).await
}
