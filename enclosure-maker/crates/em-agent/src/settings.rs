use crate::guard::{deny_rules, guard_hook_matcher};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// POSIX single-quoting: wrap in `'...'` and replace each embedded `'` with
/// `'\''`. The result is one shell word whatever the input contains.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The `--settings` JSON body: the `PreToolUse` hook (invoking `exe
/// --agent-guard <project_dir>`) plus the `permissions.deny` rules that
/// anchor the confinement even if hooks get disabled.
pub fn build_settings_json(exe: &Path, project_dir: &str, temp_dir: &str) -> String {
    let command = format!("{} --agent-guard {}", shell_quote(&exe.to_string_lossy()), shell_quote(project_dir));
    serde_json::json!({
        "permissions": { "deny": deny_rules(project_dir, temp_dir) },
        "hooks": {
            "PreToolUse": [{
                "matcher": guard_hook_matcher(),
                "hooks": [{ "type": "command", "command": command, "timeout": 15 }]
            }]
        }
    })
    .to_string()
}

/// Create `path` at 0600 and write `body`, never following or truncating an
/// existing file.
pub fn write_private_file(path: &Path, body: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
        if let Err(e) = file.write_all(body.as_bytes()) {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, body)
    }
}

/// Does a settings file body set `"disableAllHooks"` truthy? That would
/// neutralize the `PreToolUse` confinement hook entirely.
pub fn settings_disables_hooks(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    match value.get("disableAllHooks") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        Some(Value::Number(n)) => n.as_i64().is_some_and(|i| i != 0),
        _ => false,
    }
}

/// The settings files whose `disableAllHooks` would neutralize the
/// confinement hook: every `.claude/settings.json` and
/// `.claude/settings.local.json` from `project_dir` up to the filesystem
/// root, plus the user's own (`home`).
pub fn hook_disabling_settings_paths(project_dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let push_for = |dir: &Path, out: &mut Vec<PathBuf>| {
        for name in ["settings.json", "settings.local.json"] {
            let candidate = dir.join(".claude").join(name);
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    };

    let mut current = Some(project_dir);
    while let Some(dir) = current {
        push_for(dir, &mut paths);
        current = dir.parent();
    }
    if let Some(home) = home {
        push_for(home, &mut paths);
    }
    paths
}

/// Refuse to start when a settings file already switches hooks off. Checkable
/// before spawning, so a session whose boundary is known in advance not to
/// hold never starts. Returns the offending path, if any.
pub fn find_hook_disabling_settings(project_dir: &Path, home: Option<&Path>) -> Option<PathBuf> {
    for path in hook_disabling_settings_paths(project_dir, home) {
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        if settings_disables_hooks(&body) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_json_has_hook_and_deny_rules() {
        let json = build_settings_json(Path::new("/usr/bin/enclosure-maker"), "/home/me/proj", "/tmp");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "Write|Edit|MultiEdit|NotebookEdit");
        let command = value["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(command.contains("--agent-guard"));
        assert!(command.contains("/home/me/proj"));
        assert!(value["permissions"]["deny"].as_array().unwrap().len() > 5);
    }

    #[test]
    fn shell_quote_handles_embedded_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn detects_disable_all_hooks_true() {
        assert!(settings_disables_hooks(r#"{"disableAllHooks": true}"#));
        assert!(!settings_disables_hooks(r#"{"disableAllHooks": false}"#));
        assert!(!settings_disables_hooks(r#"{}"#));
        assert!(!settings_disables_hooks("not json"));
    }

    #[test]
    fn hook_disabling_paths_walk_up_to_root_and_include_home() {
        let paths = hook_disabling_settings_paths(Path::new("/a/b/c"), Some(Path::new("/home/me")));
        assert!(paths.contains(&PathBuf::from("/a/b/c/.claude/settings.json")));
        assert!(paths.contains(&PathBuf::from("/a/.claude/settings.json")));
        assert!(paths.contains(&PathBuf::from("/.claude/settings.json")));
        assert!(paths.contains(&PathBuf::from("/home/me/.claude/settings.json")));
    }
}
