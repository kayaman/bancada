use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::str::FromStr;

/// A command-line coding agent supported by the embedded assistant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentProvider {
    #[default]
    Claude,
    Codex,
    Copilot,
}

impl AgentProvider {
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Copilot];

    pub fn executable(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
        }
    }
}

impl fmt::Display for AgentProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
        })
    }
}

impl FromStr for AgentProvider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "copilot" => Ok(Self::Copilot),
            _ => Err(format!("unknown assistant provider: {value}")),
        }
    }
}

/// Converts a provider's JSONL event into the small Claude-compatible event
/// vocabulary consumed by the preview UI. Unknown events are deliberately
/// ignored: both Codex and Copilot add event kinds over time.
pub fn normalize_provider_event(provider: AgentProvider, raw: Value) -> Vec<Value> {
    match provider {
        AgentProvider::Claude => normalize_claude(raw),
        AgentProvider::Codex => normalize_codex(raw),
        AgentProvider::Copilot => normalize_copilot(raw),
    }
}

fn normalize_claude(mut raw: Value) -> Vec<Value> {
    if raw.get("type").and_then(Value::as_str) == Some("system")
        && raw.get("subtype").and_then(Value::as_str) == Some("init")
    {
        raw["provider"] = json!(AgentProvider::Claude);
    }
    vec![raw]
}

fn system_init(provider: AgentProvider, session_id: &str, model: &str) -> Value {
    json!({
        "type": "system", "subtype": "init", "provider": provider,
        "session_id": session_id, "model": model,
    })
}

fn assistant_text(text: &str) -> Value {
    json!({
        "type": "assistant",
        "message": { "role": "assistant", "content": [{ "type": "text", "text": text }] }
    })
}

fn thinking_text(text: &str) -> Value {
    json!({
        "type": "assistant",
        "message": { "role": "assistant", "content": [{ "type": "thinking", "thinking": text }] }
    })
}

fn text_delta(text: &str) -> Value {
    json!({
        "type": "stream_event",
        "event": {
            "type": "content_block_delta",
            "delta": { "type": "text_delta", "text": text }
        }
    })
}

fn reasoning_text(item: &Value) -> Option<String> {
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    item.get("summary").and_then(summary_text)
}

fn summary_text(summary: &Value) -> Option<String> {
    let text = match summary {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str().map(str::to_string).or_else(|| {
                    item.get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({
        "type": "assistant",
        "message": { "role": "assistant", "content": [{
            "type": "tool_use", "id": id, "name": name, "input": input
        }] }
    })
}

fn tool_result(id: &str, content: Value, is_error: bool) -> Value {
    json!({
        "type": "user",
        "message": { "role": "user", "content": [{
            "type": "tool_result", "tool_use_id": id,
            "content": content, "is_error": is_error
        }] }
    })
}

fn result(is_error: bool, message: Option<&str>, usage: Option<&Value>) -> Value {
    let mut value = json!({ "type": "result", "is_error": is_error });
    if let Some(message) = message {
        value["result"] = json!(message);
    }
    if let Some(usage) = usage {
        value["usage"] = usage.clone();
    }
    value
}

fn normalize_codex(raw: Value) -> Vec<Value> {
    match raw.get("type").and_then(Value::as_str) {
        Some("thread.started") => vec![system_init(
            AgentProvider::Codex,
            raw.get("thread_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            raw.get("model").and_then(Value::as_str).unwrap_or("Codex"),
        )],
        Some("item.started") => {
            let item = raw.get("item").unwrap_or(&Value::Null);
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("codex-tool");
            match item.get("type").and_then(Value::as_str) {
                Some("command_execution") => vec![tool_use(
                    id,
                    "Shell",
                    json!({ "command": item.get("command").and_then(Value::as_str).unwrap_or_default() }),
                )],
                Some("mcp_tool_call") => vec![tool_use(
                    id,
                    item.get("tool").and_then(Value::as_str).unwrap_or("MCP"),
                    item.get("arguments").cloned().unwrap_or_else(|| json!({})),
                )],
                _ => Vec::new(),
            }
        }
        Some("item.completed") => {
            let item = raw.get("item").unwrap_or(&Value::Null);
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("codex-tool");
            match item.get("type").and_then(Value::as_str) {
                Some("agent_message") => item
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(assistant_text)
                    .into_iter()
                    .collect(),
                Some("reasoning") => reasoning_text(item)
                    .map(|text| thinking_text(&text))
                    .into_iter()
                    .collect(),
                Some("command_execution") => vec![tool_result(
                    id,
                    json!(item
                        .get("aggregated_output")
                        .and_then(Value::as_str)
                        .unwrap_or_default()),
                    item.get("status").and_then(Value::as_str) == Some("failed"),
                )],
                Some("mcp_tool_call") => vec![tool_result(
                    id,
                    item.get("result").cloned().unwrap_or_else(|| {
                        json!(item
                            .pointer("/error/message")
                            .and_then(Value::as_str)
                            .unwrap_or("done"))
                    }),
                    item.get("status").and_then(Value::as_str) == Some("failed"),
                )],
                Some("file_change") => {
                    let input = json!({ "changes": item.get("changes").cloned().unwrap_or_else(|| json!([])) });
                    vec![
                        tool_use(id, "Edit", input),
                        tool_result(
                            id,
                            json!(item
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or("completed")),
                            item.get("status").and_then(Value::as_str) == Some("failed"),
                        ),
                    ]
                }
                Some("error") => vec![result(
                    true,
                    item.get("message").and_then(Value::as_str),
                    None,
                )],
                _ => Vec::new(),
            }
        }
        Some("turn.completed") => vec![result(false, None, raw.get("usage"))],
        Some("turn.failed") => vec![result(
            true,
            raw.pointer("/error/message").and_then(Value::as_str),
            raw.get("usage"),
        )],
        Some("error") => vec![result(
            true,
            raw.get("message").and_then(Value::as_str),
            None,
        )],
        _ => Vec::new(),
    }
}

fn copilot_data(raw: &Value) -> &Value {
    raw.get("data").unwrap_or(raw)
}

fn copilot_tool_id(data: &Value) -> &str {
    data.get("toolCallId")
        .or_else(|| data.get("tool_call_id"))
        .or_else(|| data.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("copilot-tool")
}

fn normalize_copilot(raw: Value) -> Vec<Value> {
    let data = copilot_data(&raw);
    match raw.get("type").and_then(Value::as_str) {
        Some("session.start") => vec![system_init(
            AgentProvider::Copilot,
            data.get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            data.get("selectedModel")
                .or_else(|| data.get("model"))
                .and_then(Value::as_str)
                .unwrap_or("Copilot"),
        )],
        Some("assistant.message") => data
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(assistant_text)
            .into_iter()
            .collect(),
        Some("assistant.message_delta") | Some("assistant.delta") => data
            .get("delta")
            .or_else(|| data.get("content"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(text_delta)
            .into_iter()
            .collect(),
        Some("assistant.reasoning") | Some("reasoning") => data
            .get("content")
            .or_else(|| data.get("text"))
            .or_else(|| data.get("delta"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(thinking_text)
            .into_iter()
            .collect(),
        Some("tool.execution_start") => vec![tool_use(
            copilot_tool_id(data),
            data.get("toolName")
                .or_else(|| data.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("Tool"),
            data.get("arguments").cloned().unwrap_or_else(|| json!({})),
        )],
        Some("tool.execution_complete") => vec![tool_result(
            copilot_tool_id(data),
            data.pointer("/result/content")
                .or_else(|| data.get("result"))
                .cloned()
                .unwrap_or_else(|| json!("done")),
            data.get("success").and_then(Value::as_bool) == Some(false),
        )],
        Some("result") => vec![result(
            data.get("success").and_then(Value::as_bool) == Some(false)
                || data.get("is_error").and_then(Value::as_bool) == Some(true),
            data.get("error").and_then(Value::as_str),
            data.get("usage"),
        )],
        Some("session.error") => vec![result(
            true,
            data.get("message").and_then(Value::as_str),
            None,
        )],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_session_and_answer_become_ui_events() {
        let init = normalize_provider_event(
            AgentProvider::Codex,
            json!({"type":"thread.started","thread_id":"abc"}),
        );
        assert_eq!(init[0]["session_id"], "abc");
        assert_eq!(init[0]["provider"], "codex");

        let answer = normalize_provider_event(
            AgentProvider::Codex,
            json!({"type":"item.completed","item":{"id":"1","type":"agent_message","text":"done"}}),
        );
        assert_eq!(answer[0]["message"]["content"][0]["text"], "done");
    }

    #[test]
    fn copilot_tool_events_keep_their_correlation_id() {
        let start = normalize_provider_event(
            AgentProvider::Copilot,
            json!({"type":"tool.execution_start","data":{"toolCallId":"t1","toolName":"write","arguments":{"path":"a.rhai"}}}),
        );
        let done = normalize_provider_event(
            AgentProvider::Copilot,
            json!({"type":"tool.execution_complete","data":{"toolCallId":"t1","success":true,"result":{"content":"ok"}}}),
        );
        assert_eq!(start[0]["message"]["content"][0]["id"], "t1");
        assert_eq!(done[0]["message"]["content"][0]["tool_use_id"], "t1");
    }

    #[test]
    fn codex_reasoning_becomes_a_thinking_block() {
        let events = normalize_provider_event(
            AgentProvider::Codex,
            json!({"type":"item.completed","item":{"id":"r1","type":"reasoning","summary":["checking the script"]}}),
        );
        assert_eq!(
            events[0]["message"]["content"][0]["thinking"],
            "checking the script"
        );
    }

    #[test]
    fn copilot_deltas_stream_as_text() {
        let events = normalize_provider_event(
            AgentProvider::Copilot,
            json!({"type":"assistant.message_delta","data":{"delta":"hello"}}),
        );
        assert_eq!(events[0]["event"]["delta"]["text"], "hello");
    }
}
