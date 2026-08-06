import { describe, expect, it } from "vitest";
import { AgentStore } from "../agentStore";
import type { AgentEvent } from "../types";
import { AGENT_STREAM_PONG_FIXTURE } from "./fixtures/agentStreamPong";
import { AGENT_VERIFY_ROUNDTRIP_FIXTURE } from "./fixtures/agentVerifyRoundtrip";
import { parseVerifyResult } from "../verifyResult";

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

describe("AgentStore: a tool call mid-turn is itself a text-bubble boundary", () => {
  // Regression coverage for the task-6 review finding: the CLI emits one
  // non-delta `assistant` line per model inference, and a tool-using turn
  // has (at least) two inferences — before the call and after the
  // tool_result — separated by a `result` in neither case (the turn is
  // still going). Without treating `tool_use` itself as a boundary, the
  // second inference's "authoritative" text silently overwrote the first
  // bubble in place instead of following it after the tool card.

  it("exact repro: pre-call and post-call text survive as two bubbles around the tool card", () => {
    const s = new AgentStore();
    s.push(assistantText("A: about to read the file"));
    s.push(toolUse("t1", "Read", { file_path: "/s/Blink.ino" }));
    s.push(toolResult("t1", "file contents", false));
    s.push(assistantText("B: here is what I found"));

    expect(s.snapshot().messages).toEqual([
      { kind: "assistant", text: "A: about to read the file" },
      {
        kind: "tool",
        id: "t1",
        name: "Read",
        input: { file_path: "/s/Blink.ino" },
        status: "ok",
        result: "file contents",
      },
      { kind: "assistant", text: "B: here is what I found" },
    ]);
  });

  it("deltas either side of a tool call, incl. text+tool_use on the same authoritative line: no loss, no dup", () => {
    const s = new AgentStore();
    // Pre-call: deltas build up, then the authoritative line for that same
    // inference carries BOTH the final text and the tool_use in one event
    // (a realistic shape: "I'll read the file: <tool_use>").
    s.push(streamDelta("A: about"));
    s.push(
      assistantTextAndToolUse("A: about to read the file", "t1", "Read", {
        file_path: "/s/Blink.ino",
      }),
    );
    s.push(toolResult("t1", "file contents", false));
    // Post-call: a fresh delta run, then its own authoritative line.
    s.push(streamDelta("B: here"));
    s.push(assistantText("B: here is what I found"));

    expect(s.snapshot().messages).toEqual([
      { kind: "assistant", text: "A: about to read the file" },
      {
        kind: "tool",
        id: "t1",
        name: "Read",
        input: { file_path: "/s/Blink.ino" },
        status: "ok",
        result: "file contents",
      },
      { kind: "assistant", text: "B: here is what I found" },
    ]);
  });

  it("two sequential tool calls in one turn each get their own bubble before and after", () => {
    const s = new AgentStore();
    s.push(assistantText("Step 1: read file A"));
    s.push(toolUse("t1", "Read", { file_path: "/a.ino" }));
    s.push(toolResult("t1", "contents a", false));
    s.push(assistantText("Step 2: read file B"));
    s.push(toolUse("t2", "Read", { file_path: "/b.ino" }));
    s.push(toolResult("t2", "contents b", false));
    s.push(assistantText("Done comparing both files"));

    expect(s.snapshot().messages).toEqual([
      { kind: "assistant", text: "Step 1: read file A" },
      {
        kind: "tool",
        id: "t1",
        name: "Read",
        input: { file_path: "/a.ino" },
        status: "ok",
        result: "contents a",
      },
      { kind: "assistant", text: "Step 2: read file B" },
      {
        kind: "tool",
        id: "t2",
        name: "Read",
        input: { file_path: "/b.ino" },
        status: "ok",
        result: "contents b",
      },
      { kind: "assistant", text: "Done comparing both files" },
    ]);
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

/** A single assistant line carrying both the inference's final text and its
 * tool_use — the realistic shape of "I'll read the file: <tool_use>". */
function assistantTextAndToolUse(
  text: string,
  id: string,
  name: string,
  input: unknown,
): AgentEvent {
  return {
    type: "assistant",
    message: {
      role: "assistant",
      content: [
        { type: "text", text },
        { type: "tool_use", id, name, input },
      ],
    },
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

// ---------- FE-C1: session identity ----------

describe("AgentStore: a stale close must not end a live newer session", () => {
  // The reachable sequence, all of it real app behaviour:
  //   1. session A is running
  //   2. "New session" fires agentStop() WITHOUT awaiting and clears the store
  //   3. the user sends again -> sendToAgent sees status "idle" -> session B
  //   4. session A's stdout EOF (its own thread, unsynchronised with
  //      agent_stop's return) finally delivers agent://closed{pid: A}
  // Step 4 used to flip B to "ended": panel says "Session ended", Send goes
  // disabled, while B's child is alive and still pushing events. The backend
  // already guarded its *kill* path with should_stop_agent; this is the same
  // guard for the store.
  it("ignores agent://closed for a session it is no longer showing", () => {
    const s = new AgentStore();
    s.sessionStarted(1001); // session A
    s.userSent("first");
    s.push({ type: "system", subtype: "init", session_id: "a" });
    expect(s.snapshot().status).toBe("running");

    s.clear(); // "New session"
    s.sessionStarted(2002); // session B
    s.userSent("second");
    s.push({ type: "system", subtype: "init", session_id: "b" });
    expect(s.snapshot().status).toBe("running");

    s.closed("the agent process ended", 1001); // session A's late EOF

    const snap = s.snapshot();
    expect(snap.status).toBe("running");
    expect(snap.closedReason).toBeUndefined();
  });

  it("still ends the session when the close is its own", () => {
    const s = new AgentStore();
    s.sessionStarted(2002);
    s.closed("the agent process ended", 2002);
    expect(s.snapshot().status).toBe("ended");
  });

  it("accepts a close with no pid, and any close before the pid is known", () => {
    // Backwards compatible in both directions: an unstamped event, and an
    // event arriving between `agent_start` being invoked and resolving.
    const noPid = new AgentStore();
    noPid.sessionStarted(7);
    noPid.closed("ended");
    expect(noPid.snapshot().status).toBe("ended");

    const noSession = new AgentStore();
    noSession.closed("ended", 42);
    expect(noSession.snapshot().status).toBe("ended");
  });

  it("drops verify events from a superseded session", () => {
    const s = new AgentStore();
    s.sessionStarted(2002);
    s.push({ type: "verify_started", pid: 1001 }); // session A's listener
    expect(s.snapshot().verifyRunning).toBe(false);
    s.push({ type: "verify_started", pid: 2002 });
    expect(s.snapshot().verifyRunning).toBe(true);
    s.push({ type: "verify_done", success: true, pid: 1001 });
    expect(s.snapshot().verifyRunning).toBe(true);
    s.push({ type: "verify_done", success: true, pid: 2002 });
    expect(s.snapshot().verifyRunning).toBe(false);
  });
});

describe("AgentStore: a superseded session's events die at clear(), not at the next start", () => {
  // Project switching makes this routine: teardown fires agentStop() without
  // awaiting and clear()s. Until the user sends in the new project, pidVal is
  // undefined — and "undefined means assume ours" would let the old child's
  // late EOF flip the fresh store to "ended" (Send disabled in a project
  // that never ran an agent). The cleared pid is remembered as superseded.
  it("ignores the cleared session's late close while no new session exists", () => {
    const s = new AgentStore();
    s.sessionStarted(41);
    s.userSent("hi");
    s.clear(); // project switched
    const v = s.version;

    s.closed("the agent process ended", 41); // old child's EOF

    const snap = s.snapshot();
    expect(snap.status).toBe("idle");
    expect(snap.closedReason).toBeUndefined();
    expect(s.version).toBe(v); // dropped events bump nothing
  });

  it("still ends the store for a genuinely unknown session's close", () => {
    // The documented contract survives: undefined-on-our-side means "assume
    // ours" for sessions this store never learned about.
    const s = new AgentStore();
    s.sessionStarted(41);
    s.clear();
    s.closed("ended", 99);
    expect(s.snapshot().status).toBe("ended");
  });

  it("remembers every superseded pid across multiple clears", () => {
    const s = new AgentStore();
    s.sessionStarted(41); // project A
    s.clear();
    s.sessionStarted(42); // project B
    s.clear();
    s.sessionStarted(43); // project C, live

    s.closed("eof", 41);
    s.closed("eof", 42);
    expect(s.snapshot().status).toBe("idle");

    s.closed("eof", 43); // the live session's own close still lands
    expect(s.snapshot().status).toBe("ended");
  });

  it("drops a superseded session's security_alarm and verify events", () => {
    const s = new AgentStore();
    s.sessionStarted(41);
    s.clear();
    const v = s.version;

    s.push({ type: "security_alarm", kind: "path_escape", detail: "x", pid: 41 });
    s.push({ type: "verify_started", pid: 41 });

    const snap = s.snapshot();
    expect(snap.alarm).toBeUndefined();
    expect(snap.status).toBe("idle");
    expect(snap.verifyRunning).toBe(false);
    expect(s.version).toBe(v);
  });
});

// ---------- FE-I1: MCP tool results arrive as content blocks ----------

describe("AgentStore: tool_result content shapes", () => {
  // bancada's own mcp::tool_result_json sends
  //   content: [{"type":"text","text":"success: true\nexit_code: 0\n\n..."}]
  // so JSON.stringify-ing the array (what this used to do) handed
  // parseVerifyResult a blob starting `[{"type":"text"...` — no `success:`
  // line — and every Verify card degraded to a bare "Verify finished".
  const toolUse: AgentEvent = {
    type: "assistant",
    message: {
      content: [
        { type: "tool_use", id: "t1", name: "mcp__bancada__verify", input: {} },
      ],
    },
  };

  it("unwraps a content-block array to its text", () => {
    const s = new AgentStore();
    s.push(toolUse);
    s.push({
      type: "user",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "t1",
            content: [
              { type: "text", text: "success: true\nexit_code: 0\n\nfine" },
            ],
          },
        ],
      },
    });
    const msg = s.snapshot().messages.find((m) => m.kind === "tool");
    expect(msg).toBeDefined();
    if (msg?.kind !== "tool") throw new Error("expected a tool card");
    expect(msg.result).toBe("success: true\nexit_code: 0\n\nfine");
    expect(msg.result?.startsWith("success:")).toBe(true);
  });

  it("joins multiple text blocks", () => {
    const s = new AgentStore();
    s.push(toolUse);
    s.push({
      type: "user",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "t1",
            content: [
              { type: "text", text: "first" },
              { type: "text", text: "second" },
            ],
          },
        ],
      },
    });
    const msg = s.snapshot().messages.find((m) => m.kind === "tool");
    if (msg?.kind !== "tool") throw new Error("expected a tool card");
    expect(msg.result).toBe("first\nsecond");
  });

  it("still passes a plain string through untouched", () => {
    const s = new AgentStore();
    s.push(toolUse);
    s.push({
      type: "user",
      message: {
        content: [
          { type: "tool_result", tool_use_id: "t1", content: "plain text" },
        ],
      },
    });
    const msg = s.snapshot().messages.find((m) => m.kind === "tool");
    if (msg?.kind !== "tool") throw new Error("expected a tool card");
    expect(msg.result).toBe("plain text");
  });

  it("falls back to JSON for a shape that is not all text blocks", () => {
    // A mixed array must not silently lose the non-text block.
    const s = new AgentStore();
    s.push(toolUse);
    s.push({
      type: "user",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "t1",
            content: [{ type: "text", text: "hi" }, { type: "image" }],
          },
        ],
      },
    });
    const msg = s.snapshot().messages.find((m) => m.kind === "tool");
    if (msg?.kind !== "tool") throw new Error("expected a tool card");
    expect(msg.result).toContain("image");
  });
});

// ---------- A1/A2: the security alarm ----------

describe("AgentStore: security_alarm", () => {
  it("records the alarm and ends the session", () => {
    const s = new AgentStore();
    s.sessionStarted(5);
    s.push({ type: "verify_started", pid: 5 });
    s.push({
      type: "security_alarm",
      kind: "path_escape",
      detail: "The assistant tried to Write /etc/passwd",
      pid: 5,
    });
    const snap = s.snapshot();
    expect(snap.alarm).toEqual({
      kind: "path_escape",
      detail: "The assistant tried to Write /etc/passwd",
    });
    // The host has already killed the child, so no verify_done is coming.
    expect(snap.status).toBe("ended");
    expect(snap.verifyRunning).toBe(false);
  });

  it("ignores an alarm from a superseded session", () => {
    const s = new AgentStore();
    s.sessionStarted(2);
    s.push({ type: "security_alarm", kind: "path_escape", detail: "x", pid: 1 });
    expect(s.snapshot().alarm).toBeUndefined();
    expect(s.snapshot().status).not.toBe("ended");
  });

  it("describes an alarm whose detail is missing rather than rendering undefined", () => {
    const s = new AgentStore();
    s.push({ type: "security_alarm" } as AgentEvent);
    expect(s.snapshot().alarm?.kind).toBe("unknown");
    expect(typeof s.snapshot().alarm?.detail).toBe("string");
  });

  it("is cleared by clear()", () => {
    const s = new AgentStore();
    s.push({ type: "security_alarm", kind: "path_escape", detail: "x" });
    s.clear();
    expect(s.snapshot().alarm).toBeUndefined();
    expect(s.snapshot().pid).toBeUndefined();
  });
});

// ---------- FE-I1, against recorded traffic ----------

describe("AgentStore: real mcp__bancada__verify round trip", () => {
  // The end-to-end assertion the unit tests above approximate: replay the
  // recorded CLI stream and confirm the Verify cards come out readable by
  // parseVerifyResult. If `toResultText` regresses to JSON.stringify-ing
  // the content-block array, every expectation here fails at once.
  it("produces tool cards whose result text parseVerifyResult can read", () => {
    const s = new AgentStore();
    for (const line of AGENT_VERIFY_ROUNDTRIP_FIXTURE.split("\n")) {
      if (line.trim().length === 0) continue;
      s.push(JSON.parse(line) as AgentEvent);
    }

    const cards = s
      .snapshot()
      .messages.filter(
        (m): m is Extract<typeof m, { kind: "tool" }> => m.kind === "tool",
      );
    expect(cards).toHaveLength(2);
    for (const card of cards) {
      expect(card.name).toBe("mcp__bancada__verify");
      expect(card.result?.startsWith("success:")).toBe(true);
    }

    const failing = parseVerifyResult(cards[0].result ?? "");
    expect(failing.success).toBe(false);
    expect(failing.exitCode).toBe(1);
    expect(failing.summary).toContain("BANCADA_LIVE_SENTINEL_4711");

    const passing = parseVerifyResult(cards[1].result ?? "");
    expect(passing.success).toBe(true);
    expect(passing.exitCode).toBe(0);
    expect(passing.summary).toContain("Sketch uses");
  });

  it("confirms the recorded MCP result really is a content-block array", () => {
    // The premise of the bug — pinned so a future reader does not "simplify"
    // toResultText back on the assumption that content is always a string.
    const resultLine = AGENT_VERIFY_ROUNDTRIP_FIXTURE.split("\n").find(
      (l) => l.includes('"tool_result"') && l.includes("success:"),
    );
    expect(resultLine).toBeDefined();
    const ev = JSON.parse(resultLine as string);
    const block = ev.message.content[0];
    expect(Array.isArray(block.content)).toBe(true);
    expect(block.content[0].type).toBe("text");
  });
});

// ---------- turn_end / session usage / streaming (assistant UX spec) ----------

/** A realistic full result event, as the CLI emits it (usage included). */
function usageResult(over: Record<string, unknown> = {}): AgentEvent {
  return {
    type: "result",
    is_error: false,
    num_turns: 3,
    total_cost_usd: 0.02,
    duration_ms: 1000,
    result: "All done.",
    usage: { input_tokens: 100, output_tokens: 10, cache_read_input_tokens: 5 },
    ...over,
  } as AgentEvent;
}

describe("AgentStore: turn_end messages", () => {
  it("a result pushes a turn_end with usage, summary, and the turn's tools", () => {
    const s = new AgentStore();
    s.userSent("fix it");
    s.push(toolUse("t1", "Edit", { file_path: "/x/soil.ino" }));
    s.push(toolResult("t1", "ok", false));
    s.push(usageResult());
    const last = s.snapshot().messages.at(-1);
    expect(last).toEqual({
      kind: "turn_end",
      usage: {
        inputTokens: 100,
        outputTokens: 10,
        cacheReadTokens: 5,
        costUsd: 0.02,
        numTurns: 3,
        durationMs: 1000,
      },
      summary: "All done.",
      tools: [{ name: "Edit", status: "ok" }],
    });
  });

  it("a usage-less result still marks the turn boundary", () => {
    const s = new AgentStore();
    s.userSent("go");
    s.push(usageResult({ usage: undefined }));
    expect(s.snapshot().messages.at(-1)).toMatchObject({
      kind: "turn_end",
      usage: undefined,
    });
  });

  it("tools from an earlier turn are not re-attributed", () => {
    const s = new AgentStore();
    s.userSent("one");
    s.push(toolUse("t1", "Edit", {}));
    s.push(usageResult());
    s.userSent("two");
    s.push(toolUse("t2", "Grep", {}));
    s.push(usageResult());
    const ends = s.snapshot().messages.filter((m) => m.kind === "turn_end");
    expect(ends).toHaveLength(2);
    expect(ends[1]).toMatchObject({ tools: [{ name: "Grep", status: "running" }] });
  });
});

describe("AgentStore: session usage totals", () => {
  it("accumulates across results", () => {
    const s = new AgentStore();
    s.push(usageResult());
    s.push(usageResult());
    expect(s.snapshot().sessionUsage).toEqual({
      costUsd: 0.04,
      inputTokens: 200,
      outputTokens: 20,
    });
  });

  it("is reset by clear()", () => {
    const s = new AgentStore();
    s.push(usageResult());
    s.clear();
    expect(s.snapshot().sessionUsage).toEqual({
      costUsd: 0,
      inputTokens: 0,
      outputTokens: 0,
    });
  });
});

describe("AgentStore: streaming flag", () => {
  it("flips on with a text delta and off with the result", () => {
    const s = new AgentStore();
    expect(s.snapshot().streaming).toBe(false);
    s.push(streamDelta("Hel"));
    expect(s.snapshot().streaming).toBe(true);
    s.push(usageResult());
    expect(s.snapshot().streaming).toBe(false);
  });

  it("clears when a tool call starts, on a new user message, and on close", () => {
    const s = new AgentStore();
    s.push(streamDelta("Let me look…"));
    s.push(toolUse("t1", "Read", {}));
    expect(s.snapshot().streaming).toBe(false);

    s.push(streamDelta("more"));
    s.userSent("stop that");
    expect(s.snapshot().streaming).toBe(false);

    s.push(streamDelta("again"));
    s.closed("bye");
    expect(s.snapshot().streaming).toBe(false);
  });
});
