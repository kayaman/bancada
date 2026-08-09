//! JSON-RPC 2.0 / MCP (Model Context Protocol) dispatch for the embedded
//! agent's `verify` tool.
//!
//! This module is pure Rust with no knowledge of sockets, threads, or
//! Tauri — [`handle_request`] turns a request body (`&str`) into an
//! [`McpReply`] describing what to send back, and [`tool_result_json`] turns
//! a completed tool run into the JSON-RPC response body. That split keeps
//! every protocol decision (which method maps to which reply, how a
//! notification differs from a request, what an unknown tool/method error
//! looks like) unit-testable without standing up an HTTP listener. The
//! src-tauri layer (Task 5) owns the actual loopback `tiny_http` server: it
//! reads the request body, calls [`handle_request`], executes the tool for
//! `McpReply::CallTool`, and writes the HTTP response — including turning
//! `McpReply::NoContent` into a bare **HTTP 202 with an empty body**.
//!
//! ## Why `NoContent` / HTTP 202 is spec-required
//!
//! JSON-RPC 2.0 notifications (a request with no `id`, e.g. the MCP
//! handshake's `notifications/initialized`) must never receive a JSON-RPC
//! response body — there is no `id` to correlate a reply with. MCP clients
//! (including the `claude` CLI acting as an MCP client against this
//! server) treat a JSON body on a notification as a protocol violation and
//! will misbehave or drop the connection. [`handle_request`] therefore
//! special-cases "no `id` field at all" into its own [`McpReply::NoContent`]
//! variant distinct from [`McpReply::Json`], so the HTTP layer cannot
//! accidentally send a body for it.
//!
//! Malformed input (non-JSON bodies, structurally invalid JSON-RPC) is
//! handled the same way [`crate::agent::parse_event`] handles unknown
//! stream-json lines: it never panics, and degrades to a JSON-RPC error
//! response rather than propagating a Rust `Err`.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

// ---------- tool definitions ----------

/// One MCP tool's advertised shape, as returned by `tools/list` and matched
/// against by `tools/call`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// The `verify` tool definition: no arguments, compiles the current sketch
/// with the session's bound profile.
///
/// The sketch dir, profile, and fqbn are **not** parameters — they're bound
/// at MCP-listener spawn time from the session that started the agent (see
/// the design spec's Architecture section), so the schema is a bare empty
/// object with `additionalProperties: false` to reject any arguments the
/// agent might try to pass.
pub fn verify_tool_def() -> ToolDef {
    ToolDef {
        name: "verify".to_string(),
        description: "Compile the current sketch with the session's active \
            profile (the same build arduino-cli runs for the toolbar's \
            Verify button). Takes no arguments. Returns whether the build \
            succeeded, its exit code, and a capped summary of the build \
            output (all stderr lines plus the tail of stdout — errors are \
            never dropped, but very long output is truncated)."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    }
}

/// The `upload` tool definition: flash the current sketch to the UI-selected
/// board.
///
/// Same no-parameter principle as [`verify_tool_def`]: the sketch dir and
/// profile/fqbn are session-bound, and the port/baud mirror the UI selection
/// — the agent never chooses a flash target. The tool is additionally gated
/// by the panel's "Allow uploads" switch; unarmed calls return an error
/// instructing the agent to ask the user.
pub fn upload_tool_def() -> ToolDef {
    ToolDef {
        name: "upload".to_string(),
        description: "Flash (upload) the current sketch to the board selected \
            in the UI, compiling first — the same path as the toolbar's \
            Upload button. Takes no arguments; the target board, port and \
            profile are the ones selected in the UI. Requires the user's \
            'Allow uploads' switch to be on; if it is off this returns an \
            error and you must ask the user to enable it. Returns whether \
            the flash succeeded, its exit code, and a capped summary of the \
            output. The serial monitor is stopped for the flash and is NOT \
            restarted — call serial_read afterwards to restart it and watch \
            the boot output."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    }
}

/// The `serial_read` tool definition: read new serial-monitor output.
///
/// Reads everything this session has not yet seen from the monitor's ring
/// buffer (bounded; old lines fall off). Auto-starts the monitor on the
/// UI-selected port/baud when nothing owns the serial port. `wait_s` bounds
/// a short poll for fresh output — capped low so one call never monopolizes
/// the single-threaded MCP listener; the agent keeps waiting by calling
/// again.
pub fn serial_read_tool_def() -> ToolDef {
    ToolDef {
        name: "serial_read".to_string(),
        description: "Read serial monitor output that arrived since your \
            last serial_read, from the board selected in the UI. Starts the \
            serial monitor automatically (at the UI-selected port and baud) \
            if it is not running. Optional wait_s (0-10): wait up to that \
            many seconds for new output before returning; for longer waits \
            call again — output is buffered between calls, so nothing is \
            lost. Returns the new lines, with a note when old lines were \
            dropped from the buffer."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "wait_s": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10,
                    "description": "Seconds to wait for new output (default 0)."
                }
            },
            "additionalProperties": false
        }),
    }
}

/// The `serial_send` tool definition: transmit one line to the board.
///
/// Same path as the Monitor tab's send box (a newline is appended). Requires
/// the monitor to be running — `serial_read` starts it.
pub fn serial_send_tool_def() -> ToolDef {
    ToolDef {
        name: "serial_send".to_string(),
        description: "Send one line to the board over the running serial \
            monitor (a newline is appended), exactly like typing in the \
            Monitor tab. Errors if the monitor is not running — call \
            serial_read first to start it. To see the board's response, \
            call serial_read after sending."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "string",
                    "description": "The line to transmit (newline appended)."
                }
            },
            "required": ["data"],
            "additionalProperties": false
        }),
    }
}

// ---------- JSON-RPC wire types ----------

/// A JSON-RPC 2.0 request. `id` is kept as a raw [`Value`] because the spec
/// allows either a number or a string. `params` is tolerated as missing.
///
/// `id`'s `Option` here means "was the `id` key present at all", **not**
/// "is the id non-null" — those are different questions per JSON-RPC 2.0.
/// A key that is wholly absent is a notification (`None`). A key present
/// with an explicit JSON `null` (`"id": null`) is still a *request* that
/// must be answered, just one whose id happens to be `Value::Null`
/// (`Some(Value::Null)`) — plain `#[serde(default)]` on `Option<Value>`
/// can't distinguish these two cases on its own, because serde's `Option<T>`
/// deserializes a present-but-null value the same way as an absent one
/// (`None`). [`deserialize_present_id`] is the standard "double option"
/// workaround: it only runs when the key is present, so it can
/// unconditionally wrap the decoded value — even `Value::Null` itself — in
/// `Some`, leaving `#[serde(default)]` to supply `None` for the truly
/// separate "key absent" case.
#[derive(Debug, Clone, Deserialize)]
struct RpcRequest {
    #[serde(default)]
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present_id")]
    id: Option<Value>,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Value,
}

/// See the `id` field doc on [`RpcRequest`]: only invoked when the `id` key
/// is present in the JSON object, so it wraps whatever value was decoded —
/// including a literal JSON `null` — in `Some`, preserving the distinction
/// from a wholly absent key (which `#[serde(default)]` alone maps to
/// `None`).
fn deserialize_present_id<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

// ---------- dispatch ----------

/// What the host should do in response to a parsed JSON-RPC request.
#[derive(Debug, Clone, PartialEq)]
pub enum McpReply {
    /// An immediate JSON-RPC response body. HTTP 200, `application/json`.
    Json(String),
    /// A JSON-RPC notification (no `id`) — never gets a JSON body. HTTP 202,
    /// empty body. See the module doc for why this is spec-required.
    NoContent,
    /// A `tools/call` for a known tool: the host runs it, then formats the
    /// result with [`tool_result_json`].
    CallTool {
        id: Value,
        name: String,
        args: Value,
    },
}

/// Dispatches one JSON-RPC request body against the given tool set.
///
/// Never panics: a non-JSON body or a structurally invalid JSON-RPC message
/// becomes a JSON-RPC error reply (parse error `-32700` / invalid request
/// `-32600`, both with a `null` id — the spec's convention when the id
/// itself couldn't be determined), never a Rust panic or `Err`.
pub fn handle_request(body: &str, tools: &[ToolDef]) -> McpReply {
    let value: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return McpReply::Json(error_response(&Value::Null, -32700, "parse error")),
    };

    // A request must be a JSON object to have a `method` at all; anything
    // else (array, scalar, ...) is structurally invalid JSON-RPC.
    if !value.is_object() {
        return McpReply::Json(error_response(&Value::Null, -32600, "invalid request"));
    }

    let request: RpcRequest = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(_) => return McpReply::Json(error_response(&Value::Null, -32600, "invalid request")),
    };

    // Only a wholly *absent* `id` key is a JSON-RPC notification and must
    // never get a JSON body (see module doc). An explicit `"id": null` is
    // still a request — `deserialize_present_id` already turned that case
    // into `Some(Value::Null)`, so it falls through to a real reply below
    // with `id` echoed back as `null`, same as any other method.
    let Some(id) = request.id else {
        return McpReply::NoContent;
    };

    match request.method.as_str() {
        "initialize" => McpReply::Json(initialize_response(&id, &request.params)),
        "ping" => McpReply::Json(success_response(&id, serde_json::json!({}))),
        "tools/list" => McpReply::Json(tools_list_response(&id, tools)),
        "tools/call" => handle_tools_call(&id, &request.params, tools),
        _ => McpReply::Json(error_response(&id, -32601, "method not found")),
    }
}

fn initialize_response(id: &Value, params: &Value) -> String {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");

    success_response(
        id,
        serde_json::json!({
            "protocolVersion": protocol_version,
            "capabilities": {"tools": {}},
            "serverInfo": {
                "name": "bancada",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn tools_list_response(id: &Value, tools: &[ToolDef]) -> String {
    let wire_tools: Vec<Value> = tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema
            })
        })
        .collect();
    success_response(id, serde_json::json!({ "tools": wire_tools }))
}

fn handle_tools_call(id: &Value, params: &Value, tools: &[ToolDef]) -> McpReply {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if !tools.iter().any(|t| t.name == name) {
        return McpReply::Json(error_response(id, -32602, "unknown tool"));
    }

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    McpReply::CallTool {
        id: id.clone(),
        name,
        args,
    }
}

// ---------- result wrapper ----------

/// Formats a completed tool run as a JSON-RPC response whose result is an
/// MCP `CallToolResult`: a single text content block plus `isError`.
pub fn tool_result_json(id: &Value, text: &str, is_error: bool) -> String {
    success_response(
        id,
        serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "isError": is_error
        }),
    )
}

// ---------- response helpers ----------

fn success_response(id: &Value, result: Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
    .to_string()
}

fn error_response(id: &Value, code: i64, message: &str) -> String {
    let error = RpcError {
        code,
        message: message.to_string(),
    };
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error
    })
    .to_string()
}

// ---------- auth ----------

/// Checks an `Authorization` header value against an expected bearer token.
///
/// Exact match of `Bearer <token>` only; `None` or any other scheme/format
/// returns `false`. Not constant-time — the MCP listener is bound to
/// `127.0.0.1` only, so timing side-channels aren't a threat model concern
/// here (per the design spec's safety model). The HTTP 401 emission itself
/// is the host's job (Task 5); this only makes the decision testable.
pub fn check_bearer(header_value: Option<&str>, token: &str) -> bool {
    match header_value {
        Some(h) => h == format!("Bearer {token}"),
        None => false,
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<ToolDef> {
        vec![verify_tool_def()]
    }

    // ---------- initialize ----------

    #[test]
    fn initialize_echoes_the_requested_protocol_version() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["result"]["protocolVersion"], "2024-11-05");
                assert_eq!(
                    value["result"]["capabilities"]["tools"],
                    serde_json::json!({})
                );
                assert_eq!(value["result"]["serverInfo"]["name"], "bancada");
                assert_eq!(
                    value["result"]["serverInfo"]["version"],
                    env!("CARGO_PKG_VERSION")
                );
                assert_eq!(value["id"], 1);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn initialize_defaults_protocol_version_when_absent() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["result"]["protocolVersion"], "2025-06-18");
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn initialize_tolerates_missing_params() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["result"]["protocolVersion"], "2025-06-18");
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    // ---------- notifications ----------

    #[test]
    fn notification_with_no_id_becomes_no_content() {
        let body = r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#;
        assert_eq!(handle_request(body, &tools()), McpReply::NoContent);
    }

    #[test]
    fn notification_without_params_becomes_no_content() {
        let body = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert_eq!(handle_request(body, &tools()), McpReply::NoContent);
    }

    #[test]
    fn explicit_null_id_is_a_real_request_not_a_notification() {
        // Per JSON-RPC 2.0, a request with an explicit `"id": null` is a
        // request, not a notification — a client sending one is waiting
        // for a reply and would hang forever against NoContent.
        let body = r#"{"jsonrpc":"2.0","id":null,"method":"initialize","params":{}}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["id"], Value::Null);
                assert_eq!(value["result"]["protocolVersion"], "2025-06-18");
            }
            other => panic!(
                "expected a real Json reply (explicit null id is still a request), got {other:?}"
            ),
        }
    }

    // ---------- tools/list ----------

    #[test]
    fn tools_list_wire_shape_uses_camel_case_input_schema() {
        let body = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                let tool = &value["result"]["tools"][0];
                assert_eq!(tool["name"], "verify");
                assert!(tool["description"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("compile"));
                // camelCase on the wire, not snake_case.
                assert!(tool.get("inputSchema").is_some());
                assert!(tool.get("input_schema").is_none());
                assert_eq!(tool["inputSchema"]["type"], "object");
                assert_eq!(tool["inputSchema"]["properties"], serde_json::json!({}));
                assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    // ---------- hardware tool definitions ----------

    #[test]
    fn upload_tool_takes_no_arguments_like_verify() {
        let def = upload_tool_def();
        assert_eq!(def.name, "upload");
        // Same no-parameter principle as verify: the target is bound to the
        // session + UI selection, never chosen by the agent.
        assert_eq!(
            def.input_schema,
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        );
        let desc = def.description.to_lowercase();
        assert!(desc.contains("flash"));
        assert!(desc.contains("allow uploads"));
    }

    #[test]
    fn serial_read_tool_accepts_only_a_bounded_wait() {
        let def = serial_read_tool_def();
        assert_eq!(def.name, "serial_read");
        assert_eq!(def.input_schema["type"], "object");
        assert_eq!(def.input_schema["additionalProperties"], false);
        let wait = &def.input_schema["properties"]["wait_s"];
        assert_eq!(wait["type"], "integer");
        assert_eq!(wait["minimum"], 0);
        assert_eq!(wait["maximum"], 10);
        // wait_s is optional: nothing is required.
        assert!(def.input_schema.get("required").is_none());
        let desc = def.description.to_lowercase();
        assert!(desc.contains("call again"));
    }

    #[test]
    fn serial_send_tool_requires_the_data_line() {
        let def = serial_send_tool_def();
        assert_eq!(def.name, "serial_send");
        assert_eq!(
            def.input_schema["properties"]["data"]["type"],
            "string"
        );
        assert_eq!(def.input_schema["required"], serde_json::json!(["data"]));
        assert_eq!(def.input_schema["additionalProperties"], false);
    }

    #[test]
    fn tools_list_advertises_all_four_tools() {
        let all = vec![
            verify_tool_def(),
            upload_tool_def(),
            serial_read_tool_def(),
            serial_send_tool_def(),
        ];
        let body = r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#;
        match handle_request(body, &all) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                let names: Vec<&str> = value["result"]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["name"].as_str().unwrap())
                    .collect();
                assert_eq!(names, ["verify", "upload", "serial_read", "serial_send"]);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn tools_call_serial_send_extracts_its_data_argument() {
        let all = vec![verify_tool_def(), serial_send_tool_def()];
        let body = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"serial_send","arguments":{"data":"AT+RST"}}}"#;
        match handle_request(body, &all) {
            McpReply::CallTool { name, args, .. } => {
                assert_eq!(name, "serial_send");
                assert_eq!(args["data"], "AT+RST");
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    // ---------- tools/call ----------

    #[test]
    fn tools_call_known_tool_extracts_arguments() {
        let body = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"verify","arguments":{"foo":"bar"}}}"#;
        match handle_request(body, &tools()) {
            McpReply::CallTool { id, name, args } => {
                assert_eq!(id, serde_json::json!(3));
                assert_eq!(name, "verify");
                assert_eq!(args, serde_json::json!({"foo": "bar"}));
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    #[test]
    fn tools_call_missing_arguments_key_defaults_to_empty_object() {
        let body = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"verify"}}"#;
        match handle_request(body, &tools()) {
            McpReply::CallTool { args, .. } => {
                assert_eq!(args, serde_json::json!({}));
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    #[test]
    fn tools_call_unknown_tool_is_invalid_params_error() {
        let body = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"reboot"}}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["error"]["code"], -32602);
                assert!(value["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("unknown tool"));
                assert_eq!(value["id"], 5);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    // ---------- unknown method ----------

    #[test]
    fn unknown_method_is_method_not_found_error() {
        let body = r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["error"]["code"], -32601);
                assert_eq!(value["id"], 6);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    // ---------- ping ----------

    #[test]
    fn ping_returns_empty_result() {
        let body = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["result"], serde_json::json!({}));
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    // ---------- malformed input ----------

    #[test]
    fn non_json_body_is_parse_error_with_null_id() {
        let reply = handle_request("not json at all", &tools());
        match reply {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["error"]["code"], -32700);
                assert_eq!(value["id"], Value::Null);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn json_array_body_is_invalid_request() {
        let reply = handle_request("[1,2,3]", &tools());
        match reply {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["error"]["code"], -32600);
                assert_eq!(value["id"], Value::Null);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn empty_body_never_panics() {
        // Just asserting this returns *something* rather than panicking.
        let _ = handle_request("", &tools());
    }

    // ---------- id round-trip ----------

    #[test]
    fn string_id_is_echoed_as_a_string() {
        let body = r#"{"jsonrpc":"2.0","id":"req-abc","method":"ping"}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["id"], "req-abc");
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn numeric_id_is_echoed_as_a_number() {
        let body = r#"{"jsonrpc":"2.0","id":42,"method":"ping"}"#;
        match handle_request(body, &tools()) {
            McpReply::Json(json) => {
                let value: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["id"], 42);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    // ---------- tool_result_json ----------

    #[test]
    fn tool_result_json_success_shape() {
        let json = tool_result_json(&serde_json::json!(1), "build succeeded", false);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["result"]["content"][0]["type"], "text");
        assert_eq!(value["result"]["content"][0]["text"], "build succeeded");
        assert_eq!(value["result"]["isError"], false);
        assert_eq!(value["id"], 1);
    }

    #[test]
    fn tool_result_json_error_shape() {
        let json = tool_result_json(&serde_json::json!("req-1"), "build failed: ...", true);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["result"]["content"][0]["text"], "build failed: ...");
        assert_eq!(value["result"]["isError"], true);
        assert_eq!(value["id"], "req-1");
    }

    // ---------- check_bearer ----------

    #[test]
    fn check_bearer_correct_token_matches() {
        assert!(check_bearer(Some("Bearer sekrit"), "sekrit"));
    }

    #[test]
    fn check_bearer_wrong_token_fails() {
        assert!(!check_bearer(Some("Bearer wrong"), "sekrit"));
    }

    #[test]
    fn check_bearer_missing_header_fails() {
        assert!(!check_bearer(None, "sekrit"));
    }

    #[test]
    fn check_bearer_non_bearer_scheme_fails() {
        assert!(!check_bearer(Some("Basic sekrit"), "sekrit"));
        assert!(!check_bearer(Some("sekrit"), "sekrit"));
    }
}
