//! The `--mcp-config` payload for an embedded `claude` session: exactly one
//! server, FreeCAD's own MCP server (`freecad-mcp`, run via `uvx`), so the
//! session can drive FreeCAD directly instead of editing a script file.
//!
//! Paired with `--strict-mcp-config` (set in [`crate::agent_args`]), this is
//! the same pattern bancada's own main agent uses for `espressif-docs` and
//! its own `mcp__bancada__*` server: the servers available to the session
//! are exactly the ones named in this generated file, never whatever the
//! invoking user happens to have registered in their own Claude Code
//! configuration.

/// The `--mcp-config` JSON body: one stdio server, `freecad`, launched as
/// `uvx freecad-mcp`.
pub fn build_mcp_config_json() -> String {
    serde_json::json!({
        "mcpServers": {
            "freecad": {
                "type": "stdio",
                "command": "uvx",
                "args": ["freecad-mcp"],
                "env": {}
            }
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn config_has_exactly_one_server_named_freecad() {
        let json = build_mcp_config_json();
        let value: Value = serde_json::from_str(&json).unwrap();
        let servers = value["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 1);
        assert!(servers.contains_key("freecad"));
    }

    #[test]
    fn freecad_server_runs_via_uvx() {
        let json = build_mcp_config_json();
        let value: Value = serde_json::from_str(&json).unwrap();
        let freecad = &value["mcpServers"]["freecad"];
        assert_eq!(freecad["type"], "stdio");
        assert_eq!(freecad["command"], "uvx");
        assert_eq!(freecad["args"][0], "freecad-mcp");
    }
}
