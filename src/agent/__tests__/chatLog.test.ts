import { describe, expect, it, vi } from "vitest";
import { AgentStore } from "../agentStore";
import type { AgentEvent } from "../types";
import { ChatRecorder, chatFileName, replayChat, type ChatOp } from "../chatLog";

// The recorder's whole contract is "what exact NDJSON lines reach the
// injected send" — so every test drives it against a spy that captures
// (file, line) pairs, and the replay tests assert the strongest property
// the design names: replaying a recorded chat is snapshot-deep-equal to
// having driven a fresh AgentStore with the same calls live.

// ---------- helpers building raw claude-CLI stream-json shapes ----------
// (same builders as agentStore.test.ts, so the round-trip test replays the
// shapes the store is already proven against)

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

/** A realistic full result event, as the CLI emits it (usage included). */
function usageResult(): AgentEvent {
  return {
    type: "result",
    is_error: false,
    num_turns: 1,
    total_cost_usd: 0.02,
    duration_ms: 1000,
    result: "All done.",
    usage: { input_tokens: 100, output_tokens: 10, cache_read_input_tokens: 5 },
  } as AgentEvent;
}

/** A recorder wired to a spy: returns the recorder plus the captured calls. */
function spiedRecorder(meta = { sketchDir: "/home/u/sketch", startedAt: "2026-08-05T17:02:31.000Z" }) {
  const sent: { file: string; line: string }[] = [];
  const rec = new ChatRecorder();
  rec.start("2026-08-05T14-02-31.ndjson", meta, (file, line) => {
    sent.push({ file, line });
    return Promise.resolve();
  });
  return { rec, sent };
}

// ---------- chatFileName ----------

describe("chatFileName", () => {
  it("formats a local-time Date as YYYY-MM-DDTHH-MM-SS.ndjson", () => {
    // new Date(2026, 7, 5, ...) is LOCAL time — month index 7 is August.
    expect(chatFileName(new Date(2026, 7, 5, 14, 2, 31))).toBe(
      "2026-08-05T14-02-31.ndjson",
    );
  });

  it("zero-pads every component", () => {
    expect(chatFileName(new Date(2026, 0, 3, 4, 5, 6))).toBe(
      "2026-01-03T04-05-06.ndjson",
    );
  });
});

// ---------- ChatRecorder ----------

describe("ChatRecorder", () => {
  it("is inactive until started; record before start is a silent no-op", () => {
    const sent: { file: string; line: string }[] = [];
    const rec = new ChatRecorder();
    expect(rec.active).toBe(false);
    expect(() => rec.record({ op: "userSent", text: "hi" })).not.toThrow();
    expect(sent).toEqual([]);
  });

  it("start alone writes nothing — empty sessions must never create a file", () => {
    const { rec, sent } = spiedRecorder();
    expect(rec.active).toBe(true);
    expect(sent).toEqual([]);
  });

  it("the first record flushes the meta line first, then the op, to the started file", () => {
    const { rec, sent } = spiedRecorder();
    rec.record({ op: "userSent", text: "fix the wifi reconnect loop" });

    expect(sent).toEqual([
      {
        file: "2026-08-05T14-02-31.ndjson",
        line: '{"op":"meta","sketchDir":"/home/u/sketch","startedAt":"2026-08-05T17:02:31.000Z"}',
      },
      {
        file: "2026-08-05T14-02-31.ndjson",
        line: '{"op":"userSent","text":"fix the wifi reconnect loop"}',
      },
    ]);
  });

  it("subsequent records append exactly one serialized line each", () => {
    const { rec, sent } = spiedRecorder();
    rec.record({ op: "userSent", text: "hi" });
    rec.record({ op: "sessionStarted", pid: 901395 });
    rec.record({ op: "push", ev: { type: "stderr", line: "warn" } });
    rec.record({ op: "closed", reason: "the agent process ended", pid: 901395 });

    expect(sent.map((s) => s.line)).toEqual([
      '{"op":"meta","sketchDir":"/home/u/sketch","startedAt":"2026-08-05T17:02:31.000Z"}',
      '{"op":"userSent","text":"hi"}',
      '{"op":"sessionStarted","pid":901395}',
      '{"op":"push","ev":{"type":"stderr","line":"warn"}}',
      '{"op":"closed","reason":"the agent process ended","pid":901395}',
    ]);
  });

  it("stop forgets the file: active flips off and later records are no-ops", () => {
    const { rec, sent } = spiedRecorder();
    rec.record({ op: "userSent", text: "hi" });
    rec.stop();
    expect(rec.active).toBe(false);
    rec.record({ op: "closed", reason: "late" });
    expect(sent).toHaveLength(2); // meta + userSent only
  });

  it("a rejecting send never surfaces into the chat flow", async () => {
    const rec = new ChatRecorder();
    rec.start(
      "f.ndjson",
      { sketchDir: "/s", startedAt: "t" },
      () => Promise.reject(new Error("disk full")),
    );
    expect(() => rec.record({ op: "userSent", text: "hi" })).not.toThrow();
    // Let the rejection land; an unhandled rejection would fail the test.
    await new Promise((r) => setTimeout(r, 0));
  });

  it("a synchronously-throwing send never surfaces either", () => {
    const rec = new ChatRecorder();
    rec.start("f.ndjson", { sketchDir: "/s", startedAt: "t" }, () => {
      throw new Error("invoke exploded");
    });
    expect(() => rec.record({ op: "userSent", text: "hi" })).not.toThrow();
  });
});

// ---------- replayChat ----------

describe("replayChat", () => {
  it("replaying a recorded round trip deep-equals driving a fresh store live", () => {
    // Freeze the clock: snapshots now carry wall-time stamps (turnStartedAt,
    // tool startedAt, rawLog ts) and the two stores are driven milliseconds
    // apart — the equality under test is semantic, not chronometric.
    const now = vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
    // The live session: user message, a tool call resolving ok, the turn's
    // result — recorded through the recorder exactly as App.tsx would.
    const ops: ChatOp[] = [
      { op: "userSent", text: "read the sketch" },
      { op: "sessionStarted", pid: 42 },
      { op: "push", ev: toolUse("t1", "Read", { file_path: "/s/Blink.ino" }) },
      { op: "push", ev: toolResult("t1", "file contents", false) },
      { op: "push", ev: usageResult() },
      { op: "closed", reason: "the agent process ended", pid: 42 },
    ];

    const { rec, sent } = spiedRecorder();
    for (const op of ops) rec.record(op);
    const lines = sent.map((s) => s.line); // meta line included, as on disk

    const live = new AgentStore();
    live.userSent("read the sketch");
    live.sessionStarted(42);
    live.push(toolUse("t1", "Read", { file_path: "/s/Blink.ino" }));
    live.push(toolResult("t1", "file contents", false));
    live.push(usageResult());
    live.closed("the agent process ended", 42);

    const replayed = replayChat(lines);
    expect(replayed).toBeInstanceOf(AgentStore);
    expect(replayed.snapshot()).toEqual(live.snapshot());
    now.mockRestore();
  });

  it("skips corrupt lines and unknown ops without throwing", () => {
    const lines = [
      '{"op":"userSent","text":"hi"}',
      "{oops", // corrupt JSON
      '{"op":"wat"}', // unknown op
      '"just a string"', // valid JSON, not an op object
      '{"op":"push","ev":{"type":"stderr","line":"warn"}}',
    ];
    let store: AgentStore | undefined;
    expect(() => {
      store = replayChat(lines);
    }).not.toThrow();
    expect(store?.snapshot().messages).toEqual([
      { kind: "user", text: "hi" },
      { kind: "stderr", line: "warn" },
    ]);
  });

  it("a project-switch teardown replays as an honest session end", () => {
    // The exact op teardownAgentSession writes when the user switches
    // projects mid-session: no pid (the recorder is closing, not reacting
    // to a child EOF), free-form reason.
    const lines = [
      '{"op":"userSent","text":"hi"}',
      '{"op":"sessionStarted","pid":7}',
      '{"op":"closed","reason":"project switched"}',
    ];
    const snap = replayChat(lines).snapshot();
    expect(snap.status).toBe("ended");
    expect(snap.closedReason).toBe("project switched");
  });

  it("ignores the meta line and empty lines: alone they yield a fresh store", () => {
    const lines = [
      '{"op":"meta","sketchDir":"/s","startedAt":"t"}',
      "",
    ];
    expect(replayChat(lines).snapshot()).toEqual(new AgentStore().snapshot());
  });
});
