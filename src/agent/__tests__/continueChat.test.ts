import { describe, expect, it } from "vitest";
import { distillFacts } from "../continueChat";
import type { AgentSnapshot } from "../agentStore";

// distillFacts is pure and defensive: its only input is a saved chat's
// snapshot, which on the resume path can be an arbitrarily large or
// malformed replay of untrusted disk data. Every test builds just enough of
// an AgentSnapshot to exercise one section — `emptySnapshot()` fills in the
// rest so each test only names what it cares about.

function emptySnapshot(over: Partial<AgentSnapshot> = {}): AgentSnapshot {
  return {
    messages: [],
    status: "idle",
    verifyRunning: false,
    uploadRunning: false,
    streaming: false,
    turnActive: false,
    rawLog: [],
    sessionUsage: { costUsd: 0, inputTokens: 0, outputTokens: 0 },
    ...over,
  };
}

describe("distillFacts: empty snapshot", () => {
  it("returns an empty string — no headers for sections with nothing to say", () => {
    expect(distillFacts(emptySnapshot())).toBe("");
  });
});

describe("distillFacts: Recent requests", () => {
  it("lists the last 3 user texts, oldest first, truncated at 300 chars", () => {
    const long = "x".repeat(400);
    const snap = emptySnapshot({
      messages: [
        { kind: "user", text: "one" },
        { kind: "user", text: "two" },
        { kind: "user", text: "three" },
        { kind: "user", text: long },
      ],
    });
    const out = distillFacts(snap);
    expect(out).toContain("Recent requests:");
    expect(out).not.toContain("- one");
    expect(out).toContain("- two");
    expect(out).toContain("- three");
    // the 4th (most recent) request is truncated to 300 chars + ellipsis
    const truncated = "x".repeat(300) + "…";
    expect(out).toContain(truncated);
    expect(out).not.toContain(long);
  });

  it("fewer than 3 user texts still produces the section", () => {
    const snap = emptySnapshot({ messages: [{ kind: "user", text: "only one" }] });
    expect(distillFacts(snap)).toContain("Recent requests:\n- only one");
  });
});

describe("distillFacts: Last assistant answer", () => {
  it("takes the LAST assistant text, truncated at 600 chars", () => {
    const long = "y".repeat(700);
    const snap = emptySnapshot({
      messages: [
        { kind: "assistant", text: "first answer" },
        { kind: "assistant", text: long },
      ],
    });
    const out = distillFacts(snap);
    expect(out).toContain("Last assistant answer:");
    expect(out).not.toContain("first answer");
    expect(out).toContain("y".repeat(600) + "…");
  });

  it("an empty final assistant text is skipped, not rendered as a blank section", () => {
    const snap = emptySnapshot({ messages: [{ kind: "assistant", text: "" }] });
    expect(distillFacts(snap)).not.toContain("Last assistant answer:");
  });
});

describe("distillFacts: Files touched", () => {
  it("collects unique file_path values from Edit/Write tool cards, capped at 15", () => {
    const messages: AgentSnapshot["messages"] = [];
    for (let i = 0; i < 20; i++) {
      messages.push({
        kind: "tool",
        id: `t${i}`,
        name: i % 2 === 0 ? "Edit" : "Write",
        input: { file_path: `/proj/file${i}.ino` },
        status: "ok",
      });
    }
    // a duplicate path must not count twice
    messages.push({
      kind: "tool",
      id: "dup",
      name: "Edit",
      input: { file_path: "/proj/file0.ino" },
      status: "ok",
    });
    const out = distillFacts(emptySnapshot({ messages }));
    expect(out).toContain("Files touched:");
    const fileLines = out
      .split("\n")
      .filter((l) => l.startsWith("- /proj/file"));
    expect(fileLines).toHaveLength(15);
    expect(new Set(fileLines).size).toBe(15); // no duplicates
  });

  it("ignores tool cards from other tools (Read, Grep, mcp__bancada__verify)", () => {
    const snap = emptySnapshot({
      messages: [
        { kind: "tool", id: "1", name: "Read", input: { file_path: "/a.ino" }, status: "ok" },
        { kind: "tool", id: "2", name: "Grep", input: { file_path: "/b.ino" }, status: "ok" },
      ],
    });
    expect(distillFacts(snap)).not.toContain("Files touched:");
  });

  it("malformed tool cards (missing/non-string/non-object input) don't throw and are skipped", () => {
    const snap = emptySnapshot({
      messages: [
        { kind: "tool", id: "1", name: "Edit", input: undefined, status: "ok" },
        { kind: "tool", id: "2", name: "Write", input: "not an object", status: "ok" },
        { kind: "tool", id: "3", name: "Edit", input: {}, status: "ok" },
        { kind: "tool", id: "4", name: "Edit", input: { file_path: 42 }, status: "ok" },
        { kind: "tool", id: "5", name: "Write", input: { file_path: "/ok.ino" }, status: "ok" },
      ],
    });
    let out = "";
    expect(() => {
      out = distillFacts(snap);
    }).not.toThrow();
    expect(out).toContain("Files touched:\n- /ok.ino");
  });
});

describe("distillFacts: Last build/upload", () => {
  it("reports the latest verify outcome from a turn_end ledger row", () => {
    const snap = emptySnapshot({
      messages: [
        {
          kind: "turn_end",
          tools: [{ name: "mcp__bancada__verify", status: "ok" }],
        },
      ],
    });
    expect(distillFacts(snap)).toContain("Last build/upload: verify ok");
  });

  it("reports the latest upload outcome, and later rows win over earlier ones", () => {
    const snap = emptySnapshot({
      messages: [
        { kind: "turn_end", tools: [{ name: "mcp__bancada__verify", status: "ok" }] },
        { kind: "turn_end", tools: [{ name: "mcp__bancada__upload", status: "error" }] },
      ],
    });
    expect(distillFacts(snap)).toContain("Last build/upload: upload error");
  });

  it("no section when there are no ledger (turn_end) rows at all", () => {
    const snap = emptySnapshot({ messages: [{ kind: "user", text: "hi" }] });
    expect(distillFacts(snap)).not.toContain("Last build/upload");
  });

  it("no section when ledger rows exist but none mention verify/upload", () => {
    const snap = emptySnapshot({
      messages: [{ kind: "turn_end", tools: [{ name: "Edit", status: "ok" }] }],
    });
    expect(distillFacts(snap)).not.toContain("Last build/upload");
  });

  it("malformed turn_end rows (missing/non-array tools) don't throw", () => {
    const snap = emptySnapshot({
      messages: [
        { kind: "turn_end", tools: undefined as unknown as [] },
        { kind: "turn_end", tools: [{ name: "mcp__bancada__verify", status: "ok" }] },
      ],
    });
    let out = "";
    expect(() => {
      out = distillFacts(snap);
    }).not.toThrow();
    expect(out).toContain("Last build/upload: verify ok");
  });
});

describe("distillFacts: hard cap", () => {
  it("never exceeds 2048 chars, even on a huge fixture, and truncates with a trailing ellipsis", () => {
    // Individual sections are already bounded (3 requests, 1 answer, 15
    // files) EXCEPT a file_path itself has no per-item truncation — so 15
    // long paths is what actually forces the 2048 hard cap to engage.
    const messages: AgentSnapshot["messages"] = [];
    for (let i = 0; i < 500; i++) {
      messages.push({ kind: "user", text: `request number ${i} `.repeat(20) });
      messages.push({ kind: "assistant", text: `answer number ${i} `.repeat(20) });
      messages.push({
        kind: "tool",
        id: `t${i}`,
        name: "Edit",
        input: { file_path: `/huge/deeply/nested/project/path/`.repeat(5) + `file${i}.ino` },
        status: "ok",
      });
      messages.push({
        kind: "turn_end",
        tools: [{ name: "mcp__bancada__verify", status: i % 2 === 0 ? "ok" : "error" }],
      });
    }
    const out = distillFacts(emptySnapshot({ messages }));
    expect(out.length).toBeLessThanOrEqual(2048);
    expect(out.endsWith("…")).toBe(true);
  });

  it("does not truncate (no trailing ellipsis forced) when the content is well under the cap", () => {
    const snap = emptySnapshot({ messages: [{ kind: "user", text: "short" }] });
    const out = distillFacts(snap);
    expect(out).toBe("Recent requests:\n- short");
  });
});

describe("distillFacts: joins non-empty sections with blank-line separation", () => {
  it("combines multiple sections readably", () => {
    const snap = emptySnapshot({
      messages: [
        { kind: "user", text: "fix the reconnect loop" },
        { kind: "assistant", text: "Done — see the diff." },
        {
          kind: "tool",
          id: "t1",
          name: "Edit",
          input: { file_path: "/x/wifi.ino" },
          status: "ok",
        },
        { kind: "turn_end", tools: [{ name: "mcp__bancada__verify", status: "ok" }] },
      ],
    });
    const out = distillFacts(snap);
    expect(out).toBe(
      [
        "Recent requests:\n- fix the reconnect loop",
        "Last assistant answer:\nDone — see the diff.",
        "Files touched:\n- /x/wifi.ino",
        "Last build/upload: verify ok",
      ].join("\n\n"),
    );
  });
});
