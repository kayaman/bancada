//! Protocol types for driving the `claude` CLI as a supervised child
//! process, via its `--input-format stream-json --output-format
//! stream-json` wire protocol.
//!
//! ## Why the parser is tolerant
//!
//! This protocol is undocumented (the Claude Agent SDK is the documented
//! surface; the CLI shells the same protocol internally), so the exact set
//! of `type` values and their fields isn't a contract this can rely on
//! staying fixed across CLI releases. [`parse_event`] therefore never fails
//! on a `type` it doesn't recognize -- any such line becomes
//! [`AgentEvent::Unknown`] carrying the raw [`serde_json::Value`]. The typed
//! variants all use `#[serde(default)]` on every field so a *known* type
//! with an unexpected or missing field degrades gracefully instead of
//! failing the whole line. The only `Err` case is a line that isn't valid
//! JSON at all -- a future CLI release adding fields or event types must
//! never crash or wedge a session.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One parsed line of `claude --output-format stream-json` output.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    System(SystemEvent),
    Assistant(AssistantEvent),
    User(UserEvent),
    StreamEvent(StreamEvent),
    Result(ResultEvent),
    /// Any line whose `type` is missing, unrecognized, or structurally
    /// failed to decode as its known variant.
    Unknown(Value),
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct SystemEvent {
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct AssistantEvent {
    #[serde(default)]
    pub message: AssistantMessage,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

/// A content block inside an assistant message. `thinking` blocks (and
/// anything else future CLI versions add) fall into `Other` rather than
/// failing the surrounding message.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolUse {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        input: Value,
    },
    #[serde(other)]
    Other,
}

/// `{"type":"user","message":{...}}` -- carries tool results back to the
/// model; the mirror image of what [`user_message_json`] builds.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct UserEvent {
    #[serde(default)]
    pub message: UserMessage,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct UserMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Vec<UserContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserContentBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolResult {
        #[serde(default)]
        tool_use_id: String,
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

/// `{"type":"stream_event","event":{...}}` -- a partial-message delta
/// wrapper around the Anthropic Messages streaming format. `event` is kept
/// as a raw [`Value`] rather than modeled in full;
/// [`StreamEvent::text_delta`] pulls out the one piece worth rendering
/// incrementally.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StreamEvent {
    #[serde(default)]
    pub event: Value,
}

impl StreamEvent {
    pub fn text_delta(&self) -> Option<&str> {
        if self.event.get("type").and_then(Value::as_str) != Some("content_block_delta") {
            return None;
        }
        let delta = self.event.get("delta")?;
        if delta.get("type").and_then(Value::as_str) != Some("text_delta") {
            return None;
        }
        delta.get("text").and_then(Value::as_str)
    }
}

/// `{"type":"result",...}` -- the final line of a turn.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ResultEvent {
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub session_id: String,
}

#[derive(Deserialize)]
struct StreamEventEnvelope {
    #[serde(default)]
    event: Value,
}

/// Parse one line of `claude --output-format stream-json` output. Never
/// fails on valid JSON -- see the module doc. The only `Err` is a line that
/// doesn't parse as JSON at all.
pub fn parse_event(line: &str) -> Result<AgentEvent, serde_json::Error> {
    let value: Value = serde_json::from_str(line)?;
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

    let event = match event_type {
        "system" => serde_json::from_value(value.clone()).ok().map(AgentEvent::System),
        "assistant" => serde_json::from_value(value.clone()).ok().map(AgentEvent::Assistant),
        "user" => serde_json::from_value(value.clone()).ok().map(AgentEvent::User),
        "stream_event" => serde_json::from_value::<StreamEventEnvelope>(value.clone())
            .ok()
            .map(|raw| AgentEvent::StreamEvent(StreamEvent { event: raw.event })),
        "result" => serde_json::from_value(value.clone()).ok().map(AgentEvent::Result),
        _ => None,
    };

    Ok(event.unwrap_or(AgentEvent::Unknown(value)))
}

/// Builds one NDJSON line (no trailing newline) sending a user text message
/// on the agent's stdin.
pub fn user_message_json(text: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": text}]
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc-123","model":"claude-sonnet-5","tools":["Read","Edit"]}"#;
        match parse_event(line).unwrap() {
            AgentEvent::System(s) => {
                assert_eq!(s.subtype, "init");
                assert_eq!(s.session_id, "abc-123");
                assert_eq!(s.model, "claude-sonnet-5");
                assert_eq!(s.tools, vec!["Read", "Edit"]);
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    #[test]
    fn parses_assistant_text_and_tool_use() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"tool_use","id":"1","name":"Edit","input":{"file_path":"a.rhai"}}]}}"#;
        match parse_event(line).unwrap() {
            AgentEvent::Assistant(a) => {
                assert_eq!(a.message.content.len(), 2);
                assert_eq!(a.message.content[0], ContentBlock::Text { text: "hi".to_string() });
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn unknown_type_becomes_unknown_not_error() {
        let line = r#"{"type":"rate_limit_event","foo":"bar"}"#;
        match parse_event(line).unwrap() {
            AgentEvent::Unknown(v) => assert_eq!(v["type"], "rate_limit_event"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn thinking_block_falls_into_other_not_error() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"}]}}"#;
        match parse_event(line).unwrap() {
            AgentEvent::Assistant(a) => assert_eq!(a.message.content[0], ContentBlock::Other),
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_event("not json at all").is_err());
    }

    #[test]
    fn stream_event_text_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hel"}}}"#;
        match parse_event(line).unwrap() {
            AgentEvent::StreamEvent(s) => assert_eq!(s.text_delta(), Some("hel")),
            other => panic!("expected StreamEvent, got {other:?}"),
        }
    }

    #[test]
    fn stream_event_non_text_delta_returns_none() {
        let line = r#"{"type":"stream_event","event":{"type":"message_stop"}}"#;
        match parse_event(line).unwrap() {
            AgentEvent::StreamEvent(s) => assert_eq!(s.text_delta(), None),
            other => panic!("expected StreamEvent, got {other:?}"),
        }
    }

    #[test]
    fn user_message_json_shape() {
        let json = user_message_json("hello");
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["content"][0]["text"], "hello");
    }
}
