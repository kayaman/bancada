//! File-write confinement for an embedded `claude` session, adapted from the
//! same design bancada uses for its Assistant panel (see that project's
//! `core/src/agent.rs`): a `PreToolUse` hook provides subtree containment (a
//! denylist cannot express "everything except this directory"), backed by
//! `permissions.deny` rules that hold even if the project's own
//! `.claude/settings.json` tries to disable hooks entirely (deny rules are
//! evaluated before hooks and are unaffected by that setting).
//!
//! This is a narrower adaptation than bancada's: no MCP tools, no ESP-IDF
//! docs server. The core confinement primitive -- "is this write path
//! inside the project root, outside any refused subtree" -- is the same
//! idea, reimplemented here for this project.

use serde_json::Value;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// Directory names that stay off-limits even though they're technically
/// inside the confined project root: the agent must not be able to rewrite
/// its own confinement config or git hooks, or its own persisted chat
/// history (the transcript is a record of what the agent did -- it should
/// not also be able to edit it).
pub const REFUSED_DIRS: &[&str] = &[".claude", ".git", ".enclosure-maker"];

/// Tool names the `PreToolUse` hook's matcher should fire on.
pub const GUARDED_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// The `matcher` regex for the `PreToolUse` hook entry.
pub fn guard_hook_matcher() -> String {
    GUARDED_TOOLS.join("|")
}

/// Is `candidate` a path the embedded agent is allowed to write?
///
/// Allowed only if it resolves to somewhere **inside `project_dir`** and no
/// path component below `project_dir` is one of [`REFUSED_DIRS`].
///
/// - Relative candidates resolve against `project_dir` (the agent's cwd).
/// - Symlinks are followed as far as the filesystem allows: the longest
///   existing prefix is canonicalized and the not-yet-existing tail is
///   folded lexically on top (a path being created, the common case for
///   `Write`, cannot be `canonicalize`d directly since it doesn't exist yet).
/// - `project_dir` itself is canonicalized the same way, so a project
///   reached through a symlinked parent still compares equal.
///
/// Fails closed: an empty candidate, or a `project_dir` that cannot resolve
/// to an absolute path, is not confined.
pub fn path_is_confined(project_dir: &Path, candidate: &str) -> bool {
    if candidate.is_empty() {
        return false;
    }
    let root = resolve_as_far_as_possible(project_dir);
    if !root.is_absolute() {
        return false;
    }

    let cand = Path::new(candidate);
    let joined = if cand.is_absolute() { cand.to_path_buf() } else { root.join(cand) };
    let resolved = resolve_as_far_as_possible(&joined);

    let Ok(below) = resolved.strip_prefix(&root) else {
        return false;
    };
    !below.components().any(|c| REFUSED_DIRS.iter().any(|d| c.as_os_str() == *d))
}

/// Canonicalize the longest prefix of `p` that actually exists, then apply
/// the remaining (non-existent) components lexically. `Path::canonicalize`
/// fails outright on a path whose leaf doesn't exist yet, which is the
/// normal case for a `Write` creating a new file.
fn resolve_as_far_as_possible(p: &Path) -> PathBuf {
    let mut comps: Vec<Component> = p.components().collect();
    let mut tail: Vec<OsString> = Vec::new();

    loop {
        let candidate: PathBuf = comps.iter().collect();
        match candidate.canonicalize() {
            Ok(resolved) => {
                let mut result = resolved;
                for part in tail.iter().rev() {
                    if part == ".." {
                        result.pop();
                    } else if part != "." {
                        result.push(part);
                    }
                }
                return result;
            }
            Err(_) => {
                let Some(last) = comps.pop() else {
                    // Nothing canonicalized at all (e.g. a bogus root) --
                    // fold everything lexically as a last resort.
                    let mut result = PathBuf::new();
                    for part in tail.iter().rev() {
                        if part == ".." {
                            result.pop();
                        } else if part != "." {
                            result.push(part);
                        }
                    }
                    return result;
                };
                tail.push(last.as_os_str().to_os_string());
            }
        }
    }
}

/// The `permissions.deny` rules that anchor the confinement even if the
/// project's own settings try to disable hooks. `project_dir` and
/// `temp_dir` must already be absolute and canonical.
///
/// Two syntax notes carried over from bancada's (probe-verified) findings
/// about the `claude` CLI: only `Edit(path)`/`Read(path)` patterns are
/// consulted for file permission checks (`Write(...)` is silently ignored,
/// but `Edit(...)` covers `Write`/`NotebookEdit`/`MultiEdit` too), and a
/// single leading `/` anchors to the directory *containing the settings
/// file* (a temp dir here) rather than the filesystem root -- `//` is the
/// filesystem-root anchor.
pub fn deny_rules(project_dir: &str, temp_dir: &str) -> Vec<String> {
    let project_dir = project_dir.trim_end_matches('/').trim_start_matches('/');
    let temp_dir = temp_dir.trim_end_matches('/').trim_start_matches('/');
    vec![
        format!("Edit(//{temp_dir}/enclosure-maker-agent-*)"),
        format!("Edit(//{project_dir}/.claude/**)"),
        format!("Edit(//{project_dir}/.mcp.json)"),
        format!("Edit(//{project_dir}/.claude.json)"),
        format!("Edit(//{project_dir}/.git/**)"),
        format!("Edit(//{project_dir}/.enclosure-maker/**)"),
        "Edit(~/.claude/**)".to_string(),
        "Edit(~/.claude.json)".to_string(),
        "Edit(~/.bashrc)".to_string(),
        "Edit(~/.zshrc)".to_string(),
        "Edit(~/.profile)".to_string(),
        "Edit(//etc/**)".to_string(),
    ]
}

/// The path a guarded tool is about to write, whichever key it lives under
/// (`Write`/`Edit`/`MultiEdit` carry `file_path`; `NotebookEdit` carries
/// `notebook_path`).
pub fn guarded_tool_path(tool_input: &Value) -> Option<&str> {
    tool_input.get("file_path").or_else(|| tool_input.get("notebook_path")).and_then(Value::as_str)
}

/// Decide one `PreToolUse` hook invocation. `hook_stdin` is the JSON object
/// the CLI writes to the hook's stdin. Returns `Some(json)` -- the deny
/// payload to print on stdout -- when the edit must be refused, `None` when
/// the hook should stay silent and let normal permission flow proceed.
///
/// Fails closed: unparseable stdin, a guarded tool with no path, or a
/// non-string path all deny.
pub fn guard_decision(project_dir: &Path, hook_stdin: &str) -> Option<String> {
    let deny = |reason: String| Some(deny_json(&reason));

    let Ok(input) = serde_json::from_str::<Value>(hook_stdin) else {
        return deny("enclosure-maker could not read the tool input and refuses the edit.".to_string());
    };

    let tool = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
    if !tool.is_empty() && !GUARDED_TOOLS.contains(&tool) {
        return None;
    }

    let file_path = input.get("tool_input").and_then(guarded_tool_path);
    let Some(file_path) = file_path else {
        return deny(format!("enclosure-maker refuses a {tool} with no path it can check against the project directory."));
    };

    if path_is_confined(project_dir, file_path) {
        return None;
    }
    deny(format!(
        "enclosure-maker refuses this edit: {file_path} is outside the project directory {} (or is part of its {} configuration). Only files inside the project may be edited.",
        project_dir.display(),
        REFUSED_DIRS.join("/")
    ))
}

fn deny_json(reason: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("em-agent-guard-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn confines_a_plain_in_project_write() {
        let root = tempdir();
        assert!(path_is_confined(&root, "model.rhai"));
        assert!(path_is_confined(&root, &root.join("examples/foo.rhai").to_string_lossy()));
    }

    #[test]
    fn rejects_traversal_out_of_the_project() {
        let root = tempdir();
        assert!(!path_is_confined(&root, "../../etc/passwd"));
        assert!(!path_is_confined(&root, "/etc/passwd"));
    }

    #[test]
    fn rejects_refused_dirs_inside_the_project() {
        let root = tempdir();
        assert!(!path_is_confined(&root, ".claude/settings.json"));
        assert!(!path_is_confined(&root, ".git/hooks/pre-commit"));
        assert!(!path_is_confined(&root, "sub/.claude/x"));
    }

    #[test]
    fn empty_candidate_is_not_confined() {
        let root = tempdir();
        assert!(!path_is_confined(&root, ""));
    }

    #[test]
    fn symlink_escaping_the_project_is_rejected() {
        #[cfg(unix)]
        {
            let root = tempdir();
            let outside = tempdir();
            let link = root.join("escape");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(!path_is_confined(&root, "escape/whatever.rhai"));
        }
    }

    #[test]
    fn guard_decision_allows_in_project_edit() {
        let root = tempdir();
        let stdin = serde_json::json!({
            "tool_name": "Write",
            "tool_input": { "file_path": root.join("model.rhai").to_string_lossy() }
        })
        .to_string();
        assert_eq!(guard_decision(&root, &stdin), None);
    }

    #[test]
    fn guard_decision_denies_out_of_project_edit() {
        let root = tempdir();
        let stdin = serde_json::json!({
            "tool_name": "Write",
            "tool_input": { "file_path": "/etc/passwd" }
        })
        .to_string();
        let decision = guard_decision(&root, &stdin);
        assert!(decision.is_some());
        assert!(decision.unwrap().contains("\"permissionDecision\":\"deny\""));
    }

    #[test]
    fn guard_decision_passes_through_unguarded_tools() {
        let root = tempdir();
        let stdin = serde_json::json!({ "tool_name": "Read", "tool_input": { "file_path": "/etc/passwd" } }).to_string();
        assert_eq!(guard_decision(&root, &stdin), None);
    }

    #[test]
    fn guard_decision_fails_closed_on_garbage_input() {
        let root = tempdir();
        assert!(guard_decision(&root, "not json").is_some());
        let missing_path = serde_json::json!({ "tool_name": "Write", "tool_input": {} }).to_string();
        assert!(guard_decision(&root, &missing_path).is_some());
    }

    #[test]
    fn deny_rules_strip_slashes_and_cover_expected_paths() {
        let rules = deny_rules("/home/me/proj", "/tmp");
        assert!(rules.iter().any(|r| r == "Edit(//home/me/proj/.claude/**)"));
        assert!(rules.iter().any(|r| r == "Edit(//home/me/proj/.enclosure-maker/**)"));
        assert!(rules.iter().any(|r| r == "Edit(//tmp/enclosure-maker-agent-*)"));
        assert!(rules.iter().any(|r| r == "Edit(~/.claude/**)"));
    }

    #[test]
    fn refuses_writes_into_the_chat_history_directory() {
        let root = tempdir();
        assert!(!path_is_confined(&root, ".enclosure-maker/chats/foo.ndjson"));
    }
}
