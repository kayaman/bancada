/// Configuration for one embedded `claude` session.
pub struct AgentCfg {
    /// Path to a JSON file holding the `--settings` payload (the
    /// `PreToolUse` hook plus `permissions.deny`). A *path*, not inline
    /// JSON: argv is visible to any local process via `/proc/<pid>/cmdline`
    /// on Linux, and the hook's command line has no business appearing
    /// there in full.
    pub settings_path: String,
    /// Extra context appended to the system prompt: project directory,
    /// pointers at README.md, and the instruction to keep edits to `.rhai`
    /// files.
    pub system_prompt_extra: String,
    /// `claude`'s own session id to resume via native `--resume`, when
    /// continuing a past chat whose transcript the CLI still has on disk.
    /// `None` starts a fresh session.
    pub resume_session_id: Option<String>,
}

/// The tools offered to the embedded session. No `Bash`/`Task` (no shelling
/// out, no subagents), no `WebFetch`/`WebSearch` (keeps the safety story
/// simple for v1 -- the whole scripting API is documented in the project's
/// own README.md, which `Read` already reaches), no `NotebookEdit` (not
/// meaningful for a `.rhai` project, and leaving it out of `--tools` avoids
/// giving `guarded_tool_path`'s `notebook_path` spelling any surface here).
pub const TOOLS: &str = "Read,Edit,Write,Glob,Grep";

/// Builds the `claude` argv (everything after the binary name) for an
/// embedded headless session.
///
/// `--tools` is the real capability boundary (it selects from the CLI's
/// built-in set; a tool left out is not offered to the model at all).
/// `--settings cfg.settings_path` carries the `PreToolUse` hook that refuses
/// out-of-project edits -- the boundary `--disallowedTools` alone is not
/// (probe-verified in bancada's design notes: with `--tools` absent,
/// `--disallowedTools` still left the full built-in set reachable). Kept
/// anyway as one more layer, same reasoning as the `permissions.deny` rules.
///
/// `--restricted --strict-mcp-config` isolate this headless child from the
/// invoking user's own Claude Code environment: without them, the embedded
/// session still inherits the user's installed plugins, skills, and every
/// MCP server connected in their *host* session (Slack/Gmail/Drive/etc
/// included, none of them in `TOOLS` above) -- probe-verified to balloon the
/// very first turn's prompt from ~1.7k to ~93k cache-creation tokens on a
/// machine with a typical plugin set, and to leak tool access the embedded
/// assistant was never meant to have. `--settings` and `--resume` keep
/// working with both flags set (also probe-verified); only the *ambient*
/// user/project/local settings, plugins, and foreign MCP servers are
/// dropped.
///
/// When [`AgentCfg::resume_session_id`] is `Some`, `--resume <id>` is
/// appended as the *final* pair, after `--append-system-prompt`'s value --
/// never spliced earlier, so the pinned prefix and every other flag's
/// relative position stays stable regardless of whether a session is
/// resuming or starting fresh.
pub fn agent_args(cfg: &AgentCfg) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--verbose".to_string(),
        "--include-partial-messages".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
        "--restricted".to_string(),
        "--strict-mcp-config".to_string(),
        "--tools".to_string(),
        TOOLS.to_string(),
        "--allowedTools".to_string(),
        TOOLS.to_string(),
        "--disallowedTools".to_string(),
        "Bash,Task,NotebookEdit,KillShell,BashOutput,WebFetch,WebSearch".to_string(),
        "--settings".to_string(),
        cfg.settings_path.clone(),
        "--append-system-prompt".to_string(),
        cfg.system_prompt_extra.clone(),
    ];
    if let Some(id) = &cfg.resume_session_id {
        args.push("--resume".to_string());
        args.push(id.clone());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(settings_path: &str, system_prompt_extra: &str) -> AgentCfg {
        AgentCfg {
            settings_path: settings_path.to_string(),
            system_prompt_extra: system_prompt_extra.to_string(),
            resume_session_id: None,
        }
    }

    #[test]
    fn argv_has_no_shell_or_bash_task_tools() {
        let args = agent_args(&cfg("/tmp/x.json", "ctx"));
        let joined = args.join(" ");
        assert!(joined.contains("--tools Read,Edit,Write,Glob,Grep"));
        assert!(!joined.contains("Bash,Task,NotebookEdit,KillShell,BashOutput,WebFetch,WebSearch,Read"));
        let disallowed_idx = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert!(args[disallowed_idx + 1].contains("Bash"));
        assert!(args[disallowed_idx + 1].contains("Task"));
    }

    #[test]
    fn argv_isolates_the_session_from_the_invoking_users_own_claude_environment() {
        // Without these, the embedded session inherits the *invoking* user's
        // own plugins, skills, and connected MCP servers (Slack/Gmail/Drive
        // included) -- probe-verified to balloon the first turn from ~1.7k
        // to ~93k cache-creation tokens and to leak tool access well beyond
        // `TOOLS`. `--settings` and `--resume` still work with both set.
        let args = agent_args(&cfg("/tmp/x.json", "ctx"));
        assert!(args.iter().any(|a| a == "--restricted"));
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn settings_path_is_passed_through_as_a_flag_value_not_inline() {
        let args = agent_args(&cfg("/tmp/secret-settings.json", ""));
        let idx = args.iter().position(|a| a == "--settings").unwrap();
        assert_eq!(args[idx + 1], "/tmp/secret-settings.json");
    }

    #[test]
    fn no_resume_id_means_no_resume_flag() {
        let args = agent_args(&cfg("/tmp/x.json", "ctx"));
        assert!(!args.iter().any(|a| a == "--resume"));
    }

    #[test]
    fn resume_id_appends_resume_as_the_final_pair() {
        let mut c = cfg("/tmp/x.json", "ctx");
        c.resume_session_id = Some("abc-123".to_string());
        let args = agent_args(&c);
        assert_eq!(&args[args.len() - 2], "--resume");
        assert_eq!(&args[args.len() - 1], "abc-123");
        // Everything before --resume is unchanged from the no-resume case.
        let without_resume = agent_args(&cfg("/tmp/x.json", "ctx"));
        assert_eq!(&args[..args.len() - 2], &without_resume[..]);
    }
}
