// Real `claude` 2.1.220 stream-json for a full `mcp__bancada__verify`
// round trip — two tool_use/tool_result pairs, recorded from the live gate
// (`BANCADA_AGENT_LIVE=1 cargo test -p bancada -- --ignored`) on 2026-08-01.
// The first verify is a FAILING build (stub compiler, stderr sentinel), the
// second a PASSING real `arduino-cli` compile of a Blink sketch.
//
// Why this fixture exists: nothing on the branch pinned the *shape* of an
// MCP tool result, and the shape turned out to be the thing that broke.
// `mcp::tool_result_json` sends a content-BLOCK ARRAY, and the CLI relays it
// as one — `content: [{"type":"text","text":"success: ..."}]` — whereas the
// CLI's own built-in file tools relay a plain string. `agentStore`'s
// `toResultText` used to `JSON.stringify` the array case, so
// `parseVerifyResult` saw `[{"type":"text"...` instead of a `success:` line
// and every Verify card in the panel degraded to a bare "Verify finished".
// Recorded traffic settles the question instead of guessing at it.

export const AGENT_VERIFY_ROUNDTRIP_FIXTURE = String.raw`
{"type":"assistant","message":{"model":"claude-fable-5","id":"msg_011CdcyZd789C29o6Nzx5Ju2","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_01HxTMD3Y9vdgfwSWstw2q4Y","name":"mcp__bancada__verify","input":{},"caller":{"type":"direct"}}],"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":5634,"cache_read_input_tokens":3564,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":5634},"output_tokens":4,"service_tier":"standard","inference_geo":"not_available"},"diagnostics":null,"context_management":null},"parent_tool_use_id":null,"session_id":"6135d1ae-b759-4079-8a97-6c3a677aea1b","uuid":"05eb153f-84e4-477a-87c4-9f0f484bf5a7","timestamp":"2026-08-02T00:29:05.223Z","request_id":"req_011CdcyZWLL28KGr16wXmPrc","tool_use_meta":[{"id":"toolu_01HxTMD3Y9vdgfwSWstw2q4Y","display_name":"Verify","server_display_name":"bancada"}]}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01HxTMD3Y9vdgfwSWstw2q4Y","type":"tool_result","content":[{"type":"text","text":"success: false\nexit_code: 1\n\n--- stderr ---\nBANCADA_LIVE_SENTINEL_4711\n"}]}]},"parent_tool_use_id":null,"session_id":"6135d1ae-b759-4079-8a97-6c3a677aea1b","uuid":"7407a6be-4d22-46d7-a962-bc44b57a29bb","timestamp":"2026-08-02T00:29:05.243Z","tool_use_result":[{"type":"text","text":"success: false\nexit_code: 1\n\n--- stderr ---\nBANCADA_LIVE_SENTINEL_4711\n"}]}
{"type":"assistant","message":{"model":"claude-fable-5","id":"msg_011CdcyZakjxRRcVkCiKGmgQ","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_01FZQgRJr8TQaogsC43Ktamg","name":"mcp__bancada__verify","input":{},"caller":{"type":"direct"}}],"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":9199,"cache_read_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":9199},"output_tokens":10,"service_tier":"standard","inference_geo":"not_available"},"diagnostics":null,"context_management":null},"parent_tool_use_id":null,"session_id":"5a6d66db-168b-485e-a960-f008388222a8","uuid":"792e104e-2bf5-4121-b5e1-bff440fab13a","timestamp":"2026-08-02T00:29:04.031Z","request_id":"req_011CdcyZZFCYdkDS5ZyzNuHp","tool_use_meta":[{"id":"toolu_01FZQgRJr8TQaogsC43Ktamg","display_name":"Verify","server_display_name":"bancada"}]}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01FZQgRJr8TQaogsC43Ktamg","type":"tool_result","content":[{"type":"text","text":"success: true\nexit_code: 0\n\n--- stdout ---\nSketch uses 1998 bytes (6%) of program storage space. Maximum is 32256 bytes.\nGlobal variables use 208 bytes (10%) of dynamic memory, leaving 1840 bytes for local variables. Maximum is 2048 bytes.\n"}]}]},"parent_tool_use_id":null,"session_id":"5a6d66db-168b-485e-a960-f008388222a8","uuid":"995af927-56b7-44b6-8216-dbcf020e5281","timestamp":"2026-08-02T00:29:04.498Z","tool_use_result":[{"type":"text","text":"success: true\nexit_code: 0\n\n--- stdout ---\nSketch uses 1998 bytes (6%) of program storage space. Maximum is 32256 bytes.\nGlobal variables use 208 bytes (10%) of dynamic memory, leaving 1840 bytes for local variables. Maximum is 2048 bytes.\n"}]}
`;
