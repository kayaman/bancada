use anyhow::{bail, Context};
use em_agent::{
    agent_args, build_settings_json, find_hook_disabling_settings, normalize_provider_event,
    user_message_json, AgentCfg, AgentProvider,
};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// One provider-backed assistant thread. Claude keeps its native streaming
/// process alive; Codex and Copilot run one headless command per turn and
/// resume it with the session ID observed in the previous JSONL stream.
pub struct AgentSession {
    provider: AgentProvider,
    project_dir: PathBuf,
    system_prompt: String,
    session_id: Arc<Mutex<Option<String>>>,
    event_tx: mpsc::UnboundedSender<String>,
    claude_child: Option<Child>,
    claude_stdin: Option<ChildStdin>,
    command_task: Option<JoinHandle<()>>,
    settings_path: Option<PathBuf>,
}

impl AgentSession {
    pub async fn spawn(
        provider: AgentProvider,
        project_dir: &Path,
        resume_session_id: Option<String>,
    ) -> anyhow::Result<(Self, mpsc::UnboundedReceiver<String>)> {
        let project_dir =
            std::fs::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
        let project_dir_str = project_dir.to_string_lossy();
        let system_prompt = design_assistant_prompt(&project_dir_str);
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let session_id = Arc::new(Mutex::new(resume_session_id.clone()));

        let mut session = Self {
            provider,
            project_dir,
            system_prompt,
            session_id,
            event_tx,
            claude_child: None,
            claude_stdin: None,
            command_task: None,
            settings_path: None,
        };

        if provider == AgentProvider::Claude {
            session.start_claude(resume_session_id).await?;
        }

        Ok((session, event_rx))
    }

    pub fn provider(&self) -> AgentProvider {
        self.provider
    }

    async fn start_claude(&mut self, resume_session_id: Option<String>) -> anyhow::Result<()> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(offending) = find_hook_disabling_settings(&self.project_dir, home.as_deref()) {
            bail!(
                "The assistant cannot start safely: {} sets \"disableAllHooks\", which would stop the file-confinement hook from running. Remove that setting and try again.",
                offending.display()
            );
        }

        let temp_dir = std::env::temp_dir();
        let temp_dir = std::fs::canonicalize(&temp_dir).unwrap_or(temp_dir);
        let exe = std::env::current_exe()
            .context("could not resolve this program's own executable path")?;
        let settings_json = build_settings_json(
            &exe,
            &self.project_dir.to_string_lossy(),
            &temp_dir.to_string_lossy(),
        );
        let settings_path = temp_dir.join(format!(
            "enclosure-maker-agent-settings-{}.json",
            random_token()
        ));
        em_agent::write_private_file(&settings_path, &settings_json)
            .context("could not write the agent's settings file")?;

        let cfg = AgentCfg {
            settings_path: settings_path.to_string_lossy().into_owned(),
            system_prompt_extra: self.system_prompt.clone(),
            resume_session_id,
        };
        let mut child = tokio::process::Command::new("claude")
            .args(agent_args(&cfg))
            .current_dir(&self.project_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                let _ = std::fs::remove_file(&settings_path);
                "could not spawn the `claude` CLI -- is it installed and on PATH?"
            })?;

        self.claude_stdin = Some(child.stdin.take().context("child had no stdin")?);
        let stdout = child.stdout.take().context("child had no stdout")?;
        relay_stdout(
            stdout,
            AgentProvider::Claude,
            self.event_tx.clone(),
            self.session_id.clone(),
            false,
        );
        relay_stderr(child.stderr.take(), AgentProvider::Claude);
        self.claude_child = Some(child);
        self.settings_path = Some(settings_path);
        Ok(())
    }

    pub async fn send_user_message(&mut self, text: &str) -> anyhow::Result<()> {
        if self.provider == AgentProvider::Claude {
            let mut line = user_message_json(text);
            line.push('\n');
            let stdin = self
                .claude_stdin
                .as_mut()
                .context("Claude session had no stdin")?;
            stdin
                .write_all(line.as_bytes())
                .await
                .context("could not write to the assistant's stdin")?;
            stdin
                .flush()
                .await
                .context("could not flush the assistant's stdin")?;
            return Ok(());
        }

        if let Some(previous) = self.command_task.take() {
            let _ = previous.await;
        }

        let resume_id = self.session_id.lock().unwrap().clone();
        let prompt = if resume_id.is_some() {
            text.to_string()
        } else {
            format!("{}\n\nUser request:\n{text}", self.system_prompt)
        };
        let mut command = tokio::process::Command::new(self.provider.executable());
        command
            .args(provider_args(self.provider, resume_id.as_deref()))
            .current_dir(&self.project_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().with_context(|| {
            format!(
                "could not spawn the `{}` CLI -- is it installed and on PATH?",
                self.provider.executable()
            )
        })?;
        let mut stdin = child.stdin.take().context("child had no stdin")?;
        stdin.write_all(prompt.as_bytes()).await?;
        stdin.shutdown().await?;

        let stdout = child.stdout.take().context("child had no stdout")?;
        let stderr = child.stderr.take();
        let provider = self.provider;
        let tx = self.event_tx.clone();
        let session_id = self.session_id.clone();
        self.command_task = Some(tokio::spawn(async move {
            let stderr_task = relay_stderr(stderr, provider);
            let saw_result = relay_stdout_lines(stdout, provider, &tx, &session_id).await;
            let status = child.wait().await;
            if let Some(task) = stderr_task {
                let _ = task.await;
            }
            if !saw_result {
                let message = match status {
                    Ok(status) if status.success() => None,
                    Ok(status) => Some(format!("{} exited with {status}", provider.executable())),
                    Err(error) => Some(format!(
                        "could not wait for {}: {error}",
                        provider.executable()
                    )),
                };
                let event = serde_json::json!({
                    "type": "result", "is_error": message.is_some(), "result": message
                });
                let _ = tx.send(event.to_string());
            }
        }));
        Ok(())
    }
}

fn design_assistant_prompt(project_dir: &str) -> String {
    const API_REFERENCE: &str = include_str!("../../../README.md");
    const PRINTING_GUIDANCE: &str = include_str!("../../../docs/printing-guidance.md");
    format!(
        "You are helping edit a parametric CAD project at {project_dir}, built on \
         enclosure-maker (a Rust CSG engine driven by .rhai scripts). Only edit .rhai files \
         unless the user clearly asks for something else. Never modify .git, .claude, \
         .codex, .github, or .enclosure-maker. Read, Grep, and Glob only see this project \
         directory; a path outside it is refused. `.enclosure-maker` inside the project is \
         chat and transform metadata, not the engine, and it does not contain `screw_boss` \
         or the other hardware functions. Their signatures and fixed dimensions are in the \
         scripting API reference below. The user has a live 3D preview open in their \
         browser that automatically re-renders whenever a .rhai file changes; do not explain \
         how to view changes because they appear automatically. Expose dimensions such as \
         boss heights, wall thicknesses, hole diameters, and clearances with param(...) so \
         the user can adjust them using numeric controls. Preserve named emit parts when \
         possible: manual part transforms are saved in .enclosure-maker/<script filename>.transforms.json \
         and applied after evaluation and during export. Read those offsets when positioning \
         parts, but never modify that metadata or duplicate its transforms in the script. \
         Keep the user posted while \
         you work: before each tool call, write one short sentence saying what you are about \
         to do and why, and after a result changes the plan, say what you found. Do not go \
         silent between steps.\n\n{PRINTING_GUIDANCE}\n\nFull scripting API reference:\n\n\
         {API_REFERENCE}"
    )
}

fn provider_args(provider: AgentProvider, resume_id: Option<&str>) -> Vec<String> {
    match provider {
        AgentProvider::Claude => Vec::new(),
        AgentProvider::Codex => {
            let mut args = vec!["exec".to_string()];
            if let Some(id) = resume_id {
                args.push("resume".to_string());
                args.push(id.to_string());
            }
            args.extend([
                "--json".to_string(),
                "-c".to_string(),
                "sandbox_mode=\"workspace-write\"".to_string(),
                "-c".to_string(),
                "approval_policy=\"never\"".to_string(),
                "--ignore-user-config".to_string(),
                "--skip-git-repo-check".to_string(),
                "-".to_string(),
            ]);
            args
        }
        AgentProvider::Copilot => {
            let mut args = vec![
                "--output-format=json".to_string(),
                "--available-tools=read,write".to_string(),
                "--allow-tool=read,write".to_string(),
                "--no-ask-user".to_string(),
                "--disable-builtin-mcps".to_string(),
                "--no-custom-instructions".to_string(),
                "--no-remote".to_string(),
                "--no-auto-update".to_string(),
            ];
            if let Some(id) = resume_id {
                args.push(format!("--resume={id}"));
            }
            args
        }
    }
}

fn relay_stdout(
    stdout: ChildStdout,
    provider: AgentProvider,
    tx: mpsc::UnboundedSender<String>,
    session_id: Arc<Mutex<Option<String>>>,
    finish_if_eof: bool,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let saw_result = relay_stdout_lines(stdout, provider, &tx, &session_id).await;
        if finish_if_eof && !saw_result {
            let _ = tx.send(
                serde_json::json!({"type":"result","is_error":true,"result":"assistant exited before completing the turn"})
                    .to_string(),
            );
        }
    })
}

async fn relay_stdout_lines(
    stdout: ChildStdout,
    provider: AgentProvider,
    tx: &mpsc::UnboundedSender<String>,
    session_id: &Arc<Mutex<Option<String>>>,
) -> bool {
    let mut lines = BufReader::new(stdout).lines();
    let mut saw_result = false;
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if line.trim().is_empty() => continue,
            Ok(Some(line)) => {
                let Ok(raw) = serde_json::from_str::<serde_json::Value>(&line) else {
                    eprintln!("assistant ({provider}): skipping a non-JSON stdout line");
                    continue;
                };
                for event in normalize_provider_event(provider, raw) {
                    if event.get("type").and_then(serde_json::Value::as_str) == Some("system") {
                        if let Some(id) =
                            event.get("session_id").and_then(serde_json::Value::as_str)
                        {
                            if !id.is_empty() {
                                *session_id.lock().unwrap() = Some(id.to_string());
                            }
                        }
                    }
                    saw_result |=
                        event.get("type").and_then(serde_json::Value::as_str) == Some("result");
                    // Echo to our own stdout, verbatim and already
                    // normalized: when launched by bancada
                    // (ENCLOSURE_MAKER_IMPORT), bancada pipes this process's
                    // stdout and mirrors these same lines into its own
                    // Enclosure tab (see bancada's `send_to_enclosure_maker`).
                    // Harmless when run standalone -- nothing reads this
                    // process's stdout in that case.
                    println!("{event}");
                    if tx.send(event.to_string()).is_err() {
                        return saw_result;
                    }
                }
            }
            Ok(None) => return saw_result,
            Err(error) => {
                eprintln!("assistant ({provider}): error reading stdout: {error}");
                return saw_result;
            }
        }
    }
}

fn relay_stderr(
    stderr: Option<tokio::process::ChildStderr>,
    provider: AgentProvider,
) -> Option<JoinHandle<()>> {
    stderr.map(|stderr| {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("assistant ({provider}, stderr): {line}");
            }
        })
    })
}

impl Drop for AgentSession {
    fn drop(&mut self) {
        if let Some(child) = self.claude_child.as_mut() {
            let _ = child.start_kill();
        }
        if let Some(task) = self.command_task.take() {
            task.abort();
        }
        if let Some(path) = self.settings_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn random_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_answers_hardware_sizes_without_leaving_the_project() {
        let prompt = design_assistant_prompt("/tmp/proj");
        assert!(prompt.contains("Read, Grep, and Glob only see this project directory"));
        assert!(prompt.contains("not the engine"));
        assert!(prompt.contains("| M3 | 4.1 | 5.7 | 6.7 | 5.5 | 2.4 | 3.4 |"));
        assert!(prompt.contains("outer diameter is insert OD + 2 × `wall`"));
        assert!(prompt.contains("A successful preview is not an"));
        assert!(prompt.contains("do not add hole_comp again"));
    }

    #[test]
    fn codex_resume_uses_native_session_id_and_safe_mode() {
        let args = provider_args(AgentProvider::Codex, Some("thread-1"));
        assert_eq!(&args[..3], &["exec", "resume", "thread-1"]);
        assert!(args
            .iter()
            .any(|arg| arg == "sandbox_mode=\"workspace-write\""));
        assert!(args.iter().any(|arg| arg == "approval_policy=\"never\""));
        assert!(args.iter().any(|arg| arg == "--ignore-user-config"));
    }

    #[test]
    fn copilot_is_limited_to_read_and_write_tools() {
        let args = provider_args(AgentProvider::Copilot, Some("session-1"));
        assert!(args.iter().any(|arg| arg == "--available-tools=read,write"));
        assert!(args.iter().any(|arg| arg == "--resume=session-1"));
        assert!(!args.iter().any(|arg| arg.contains("allow-all")));
    }

    /// Diagnostic probe against the *real* `codex` CLI (requires it on PATH
    /// and authenticated) -- not run by default. `cargo test -- --ignored
    /// real_codex_second_turn_resumes_instead_of_restarting` to reproduce.
    /// Sends two turns on the same `AgentSession` and checks whether the
    /// second turn actually resumes (small prompt, same thread_id reused)
    /// instead of starting a brand-new session with the full system prompt.
    #[tokio::test]
    #[ignore]
    async fn real_codex_second_turn_resumes_instead_of_restarting() {
        let dir = std::env::temp_dir().join(format!("em-agent-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let (mut session, mut events) = AgentSession::spawn(AgentProvider::Codex, &dir, None)
            .await
            .expect("spawn");

        session
            .send_user_message("Remember the secret word: pineapple. Reply with just OK.")
            .await
            .unwrap();

        let mut first_thread_id = None;
        loop {
            let line = events.recv().await.expect("turn 1 produced no events");
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            eprintln!("turn1: {v}");
            if v["type"] == "system" {
                first_thread_id = v["session_id"].as_str().map(str::to_string);
            }
            if v["type"] == "result" {
                break;
            }
        }
        let first_thread_id = first_thread_id.expect("turn 1 never reported a session_id");
        let resume_id_before_turn2 = session.session_id.lock().unwrap().clone();
        eprintln!("AgentSession.session_id before turn 2: {resume_id_before_turn2:?}");
        assert_eq!(
            resume_id_before_turn2.as_deref(),
            Some(first_thread_id.as_str()),
            "session_id was not captured from turn 1 before turn 2 starts -- turn 2 will restart from scratch"
        );

        session
            .send_user_message("What was the secret word I just told you?")
            .await
            .unwrap();

        let mut second_thread_id = None;
        let mut answer = String::new();
        loop {
            let line = events.recv().await.expect("turn 2 produced no events");
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            eprintln!("turn2: {v}");
            if v["type"] == "system" {
                second_thread_id = v["session_id"].as_str().map(str::to_string);
            }
            if v["type"] == "assistant" {
                if let Some(text) = v["message"]["content"][0]["text"].as_str() {
                    answer.push_str(text);
                }
            }
            if v["type"] == "result" {
                break;
            }
        }

        assert!(
            second_thread_id.is_none() || second_thread_id.as_deref() == Some(first_thread_id.as_str()),
            "turn 2 reported a different thread_id ({second_thread_id:?}) than turn 1 ({first_thread_id}) -- it started a fresh session instead of resuming"
        );
        assert!(
            answer.to_lowercase().contains("pineapple"),
            "turn 2 did not recall context from turn 1 (answer: {answer:?}) -- resume did not actually carry history"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Same probe as above but for the default (Claude) provider, which is
    /// architecturally different: one persistent process fed multiple
    /// stream-json lines on the same stdin, rather than one process per
    /// turn. Checks that a second `send_user_message` on the same
    /// `AgentSession` reaches the *same* already-running process (no second
    /// `claude` child spawned) and that it recalls turn-1 context.
    #[tokio::test]
    #[ignore]
    async fn real_claude_second_message_reuses_the_live_process() {
        let dir = std::env::temp_dir().join(format!("em-agent-probe-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let (mut session, mut events) = AgentSession::spawn(AgentProvider::Claude, &dir, None)
            .await
            .expect("spawn");
        let pid_before = session.claude_child.as_ref().unwrap().id();

        session
            .send_user_message("Remember the secret word: pineapple. Reply with just OK.")
            .await
            .unwrap();
        loop {
            let line = events.recv().await.expect("turn 1 produced no events");
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            eprintln!("turn1: {v}");
            if v["type"] == "result" {
                break;
            }
        }

        session
            .send_user_message("What was the secret word I just told you?")
            .await
            .unwrap();
        let mut answer = String::new();
        loop {
            let line = events.recv().await.expect("turn 2 produced no events");
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            eprintln!("turn2: {v}");
            if v["type"] == "assistant" {
                if let Some(text) = v["message"]["content"][0]["text"].as_str() {
                    answer.push_str(text);
                }
            }
            if v["type"] == "result" {
                break;
            }
        }

        let pid_after = session.claude_child.as_ref().unwrap().id();
        assert_eq!(
            pid_before, pid_after,
            "a different/second `claude` process is backing turn 2 -- the process was respawned instead of reused"
        );
        assert!(
            answer.to_lowercase().contains("pineapple"),
            "turn 2 did not recall context from turn 1 (answer: {answer:?})"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
