import { describe, expect, it } from "vitest";
import { AgentStore } from "../agentStore";
import type { AgentEvent } from "../types";
import { AGENT_STREAM_PONG_FIXTURE } from "./fixtures/agentStreamPong";

// Fixture provenance: see fixtures/agentStreamPong.ts — copied verbatim from
// core/src/testdata/agent_stream_pong.ndjson (real `claude` 2.1.220 output
// recorded for core/src/agent.rs). Reused here so the store is exercised
// against the same real traffic the Rust parser is, not just hand-authored
// shapes.
function loadFixture(): AgentEvent[] {
  return AGENT_STREAM_PONG_FIXTURE.split("\n")
    .filter((line) => line.trim().length > 0)
    .map((line) => JSON.parse(line) as AgentEvent);
}

describe("AgentStore: pong fixture (full ingestion)", () => {
  it("collapses the recorded turn into one deduped assistant message plus a result", () => {
    const s = new AgentStore();
    for (const ev of loadFixture()) s.push(ev);
    const snap = s.snapshot();

    expect(snap.status).toBe("running"); // system/init sets it; result doesn't revert it
    expect(snap.sessionId).toBe("e6d519a3-6bf2-4e83-91de-eed164d81dce");

    const assistantMsgs = snap.messages.filter((m) => m.kind === "assistant");
    expect(assistantMsgs).toHaveLength(1);
    expect(assistantMsgs[0]).toMatchObject({ kind: "assistant", text: "pong" });

    expect(snap.lastResult).toEqual({
      isError: false,
      costUsd: 0.033232,
      numTurns: 1,
      text: "pong",
    });
  });

  it("every fixture line is a no-throw push, including the top-level rate_limit_event", () => {
    const s = new AgentStore();
    const v0 = s.version;
    expect(() => {
      for (const ev of loadFixture()) s.push(ev);
    }).not.toThrow();
    expect(s.version).toBeGreaterThan(v0);
  });
});

describe("AgentStore: assistant text dedupe", () => {
  it("deltas build up text; the authoritative assistant event replaces (not appends)", () => {
    const s = new AgentStore();
    s.push(streamDelta("Hel"));
    s.push(streamDelta("lo"));
    expect(s.snapshot().messages).toEqual([{ kind: "assistant", text: "Hello" }]);

    // The final non-delta assistant event carries the *authoritative* full
    // text — here deliberately different from the delta-built text, to
    // prove it replaces rather than concatenates.
    s.push(assistantText("Hello world"));
    const snap = s.snapshot();
    expect(snap.messages).toHaveLength(1);
    expect(snap.messages[0]).toEqual({ kind: "assistant", text: "Hello world" });
  });

  it("a result event ends the turn; the next delta starts a fresh assistant message", () => {
    const s = new AgentStore();
    s.push(streamDelta("first"));
    s.push({ type: "result", is_error: false, session_id: "s1" });
    s.push(streamDelta("second"));
    const msgs = s.snapshot().messages.filter((m) => m.kind === "assistant");
    expect(msgs).toEqual([
      { kind: "assistant", text: "first" },
      { kind: "assistant", text: "second" },
    ]);
  });

  it("a new user message ends the turn; the next delta starts a fresh assistant message", () => {
    const s = new AgentStore();
    s.push(streamDelta("first"));
    s.userSent("go on");
    s.push(streamDelta("second"));
    const kinds = s.snapshot().messages.map((m) => m.kind);
    expect(kinds).toEqual(["assistant", "user", "assistant"]);
  });
});

describe("AgentStore: tool_use / tool_result lifecycle", () => {
  it("a successful tool_result resolves the running tool card to ok", () => {
    const s = new AgentStore();
    s.push(toolUse("toolu_01", "Read", { file_path: "/s/Blink.ino" }));
    expect(s.snapshot().messages).toEqual([
      {
        kind: "tool",
        id: "toolu_01",
        name: "Read",
        input: { file_path: "/s/Blink.ino" },
        status: "running",
      },
    ]);

    s.push(toolResult("toolu_01", "file contents", false));
    expect(s.snapshot().messages).toEqual([
      {
        kind: "tool",
        id: "toolu_01",
        name: "Read",
        input: { file_path: "/s/Blink.ino" },
        status: "ok",
        result: "file contents",
      },
    ]);
  });

  it("is_error:true resolves the tool card to error", () => {
    const s = new AgentStore();
    s.push(toolUse("toolu_02", "Edit", { old_string: "a", new_string: "b" }));
    s.push(toolResult("toolu_02", "boom", true));
    const [msg] = s.snapshot().messages;
    expect(msg).toMatchObject({ kind: "tool", status: "error", result: "boom" });
  });

  it("a tool_use id seen twice is not duplicated into a second card", () => {
    const s = new AgentStore();
    s.push(toolUse("toolu_03", "Glob", { pattern: "*.ino" }));
    s.push(toolUse("toolu_03", "Glob", { pattern: "*.ino" }));
    expect(s.snapshot().messages).toHaveLength(1);
  });
});

describe("AgentStore: stderr collapsing", () => {
  it("collapses consecutive stderr lines into one message, appending lines", () => {
    const s = new AgentStore();
    s.push({ type: "stderr", line: "warning: unused variable" });
    s.push({ type: "stderr", line: "note: see also" });
    expect(s.snapshot().messages).toEqual([
      { kind: "stderr", line: "warning: unused variable\nnote: see also" },
    ]);
  });

  it("a non-stderr message in between starts a new stderr message", () => {
    const s = new AgentStore();
    s.push({ type: "stderr", line: "a" });
    s.userSent("hi");
    s.push({ type: "stderr", line: "b" });
    const stderrMsgs = s.snapshot().messages.filter((m) => m.kind === "stderr");
    expect(stderrMsgs).toEqual([
      { kind: "stderr", line: "a" },
      { kind: "stderr", line: "b" },
    ]);
  });
});

describe("AgentStore: verify_started / verify_done", () => {
  it("toggles verifyRunning", () => {
    const s = new AgentStore();
    expect(s.snapshot().verifyRunning).toBe(false);
    s.push({ type: "verify_started" });
    expect(s.snapshot().verifyRunning).toBe(true);
    s.push({ type: "verify_done", success: true });
    expect(s.snapshot().verifyRunning).toBe(false);
  });
});

describe("AgentStore: closed mid-turn", () => {
  it("marks the session ended without discarding the partial assistant text", () => {
    const s = new AgentStore();
    s.push(streamDelta("still typ"));
    s.closed("child process exited");
    const snap = s.snapshot();
    expect(snap.status).toBe("ended");
    expect(snap.closedReason).toBe("child process exited");
    expect(snap.messages).toEqual([{ kind: "assistant", text: "still typ" }]);
  });
});

describe("AgentStore: unknown event types", () => {
  it("is ignored — no throw, no version bump, no message", () => {
    const s = new AgentStore();
    s.push(streamDelta("keep"));
    const v = s.version;
    const msgs = s.snapshot().messages;
    expect(() =>
      s.push({ type: "rate_limit_event", rate_limit_info: {} } as unknown as AgentEvent),
    ).not.toThrow();
    expect(s.version).toBe(v);
    expect(s.snapshot().messages).toEqual(msgs);
  });

  it("a payload with no type field at all is also ignored", () => {
    const s = new AgentStore();
    const v = s.version;
    s.push({} as unknown as AgentEvent);
    expect(s.version).toBe(v);
  });
});

describe("AgentStore: clear()", () => {
  it("resets messages and every piece of session state", () => {
    const s = new AgentStore();
    s.push({ type: "system", subtype: "init", session_id: "s1" });
    s.userSent("hi");
    s.push(streamDelta("hey"));
    s.push({ type: "result", is_error: false, total_cost_usd: 0.01, session_id: "s1" });
    s.push({ type: "verify_started" });
    s.closed("bye");

    s.clear();
    const snap = s.snapshot();
    expect(snap.messages).toEqual([]);
    expect(snap.status).toBe("idle");
    expect(snap.lastResult).toBeUndefined();
    expect(snap.sessionId).toBeUndefined();
    expect(snap.verifyRunning).toBe(false);
    expect(snap.closedReason).toBeUndefined();

    // and the store is fully usable again afterwards
    s.userSent("fresh start");
    expect(s.snapshot().messages).toEqual([{ kind: "user", text: "fresh start" }]);
  });
});

describe("AgentStore: version discipline", () => {
  it("snapshot never bumps version", () => {
    const s = new AgentStore();
    s.push(streamDelta("x"));
    const v = s.version;
    s.snapshot();
    s.snapshot();
    expect(s.version).toBe(v);
  });
});

// ---------- helpers building raw claude-CLI stream-json shapes ----------

function streamDelta(text: string): AgentEvent {
  return {
    type: "stream_event",
    event: { type: "content_block_delta", delta: { type: "text_delta", text } },
  };
}

function assistantText(text: string): AgentEvent {
  return {
    type: "assistant",
    message: { role: "assistant", content: [{ type: "text", text }] },
  };
}

function toolUse(id: string, name: string, input: unknown): AgentEvent {
  return {
    type: "assistant",
    message: { role: "assistant", content: [{ type: "tool_use", id, name, input }] },
  };
}

function toolResult(toolUseId: string, content: unknown, isError: boolean): AgentEvent {
  return {
    type: "user",
    message: {
      role: "user",
      content: [{ type: "tool_result", tool_use_id: toolUseId, content, is_error: isError }],
    },
  };
}
