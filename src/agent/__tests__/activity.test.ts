import { describe, expect, it } from "vitest";
import {
  agentActivity,
  agentActivityParts,
  formatAgentActivity,
  STALL_AFTER_MS,
  type ActivityInput,
} from "../activity";
import type { AgentMessage } from "../agentStore";

const tool = (
  name: string,
  status: "running" | "ok" | "error",
  input: unknown = {},
  startedAt?: number,
): AgentMessage => ({ kind: "tool", id: name, name, input, status, startedAt });

const user = (text = "do a thing"): AgentMessage => ({ kind: "user", text });

const base: ActivityInput = {
  status: "running",
  verifyRunning: false,
  uploadRunning: false,
  streaming: false,
  turnActive: true,
  messages: [],
};

describe("agentActivity — the phase is always decidable", () => {
  it("is offline before anything has started", () => {
    const a = agentActivity({ ...base, status: "idle" });
    expect(a.phase).toBe("offline");
    expect(a.state).toBe("Not started");
    expect(a.detail).toBeUndefined();
  });

  it("is ended once the session has ended", () => {
    const a = agentActivity({ ...base, status: "ended" });
    expect(a.phase).toBe("ended");
    expect(a.state).toBe("Session ended");
  });

  it("is starting while the session spins up with no turn in flight", () => {
    const a = agentActivity({ ...base, status: "starting", turnActive: false });
    expect(a.phase).toBe("starting");
    expect(a.state).toBe("Starting…");
  });

  it("is ready when the session is alive between turns", () => {
    const a = agentActivity({ ...base, turnActive: false });
    expect(a.phase).toBe("ready");
    expect(a.state).toBe("Ready");
    expect(a.detail).toBeUndefined();
  });

  it("is working whenever a turn is in flight", () => {
    const a = agentActivity(base);
    expect(a.phase).toBe("working");
    expect(a.state).toBe("Working");
  });

  it("is working during a verify that outlives the turn", () => {
    const a = agentActivity({ ...base, turnActive: false, verifyRunning: true });
    expect(a.phase).toBe("working");
  });
});

describe("agentActivity — what it is doing", () => {
  it("ranks an in-flight upload above even verify", () => {
    const a = agentActivity({
      ...base,
      uploadRunning: true,
      verifyRunning: true,
      streaming: true,
    });
    expect(a.detail).toBe("📡 upload (flashing)");
  });

  it("ranks verify above a running tool and streamed text", () => {
    const a = agentActivity({
      ...base,
      verifyRunning: true,
      streaming: true,
      messages: [tool("Edit", "running", { file_path: "/x/soil.ino" })],
    });
    expect(a.detail).toBe("🔨 verify (compiling)");
  });

  it("names the newest running tool with its full file path", () => {
    const a = agentActivity({
      ...base,
      messages: [
        tool("Read", "ok", { file_path: "/x/a.ino" }),
        tool("Edit", "running", { file_path: "/home/x/soil/soil.ino" }),
      ],
    });
    expect(a.detail).toBe("⚙ Edit /home/x/soil/soil.ino");
  });

  it("uses the pattern for Grep and Glob", () => {
    const a = agentActivity({
      ...base,
      messages: [tool("Grep", "running", { pattern: "Serial.begin" })],
    });
    expect(a.detail).toBe("⚙ Grep Serial.begin");
  });

  it("uses the command for Bash", () => {
    const a = agentActivity({
      ...base,
      messages: [tool("Bash", "running", { command: "arduino-cli board list" })],
    });
    expect(a.detail).toBe("⚙ Bash arduino-cli board list");
  });

  it("uses the url for WebFetch", () => {
    const a = agentActivity({
      ...base,
      messages: [tool("WebFetch", "running", { url: "https://example.com/x" })],
    });
    expect(a.detail).toBe("⚙ WebFetch https://example.com/x");
  });

  it("uses the port for the bancada serial tools", () => {
    const a = agentActivity({
      ...base,
      messages: [
        tool("mcp__bancada__serial_read", "running", { port: "/dev/ttyACM0" }),
      ],
    });
    // Short name, not the wire name: the `mcp__bancada__` prefix is identical
    // on every one of these and spends the line's width saying nothing, while
    // the port — the part that differs — is what gets ellipsised away.
    expect(a.detail).toBe("⚙ serial_read /dev/ttyACM0");
  });

  it("names a documentation search by what it searched for", () => {
    const a = agentActivity({
      ...base,
      messages: [
        tool("mcp__espressif-docs__search_espressif_sources", "running", {
          query: "I2C pull-ups",
          language: "en",
        }),
      ],
    });
    expect(a.detail).toBe("⚙ search_espressif_sources I2C pull-ups");
  });

  it("falls back to the bare tool name when there is no hint", () => {
    const a = agentActivity({
      ...base,
      messages: [tool("TodoWrite", "running")],
    });
    expect(a.detail).toBe("⚙ TodoWrite");
  });

  it("says writing while text streams and no tool runs", () => {
    const a = agentActivity({
      ...base,
      streaming: true,
      messages: [tool("Edit", "ok", { file_path: "/x/soil.ino" })],
    });
    expect(a.detail).toBe("✍ writing");
  });

  it("defaults to thinking", () => {
    expect(agentActivity(base).detail).toBe("thinking");
  });
});

describe("agentActivity — how far along", () => {
  it("counts the tools run since the last user message", () => {
    const a = agentActivity({
      ...base,
      messages: [
        tool("Read", "ok"),
        user(),
        tool("Grep", "ok"),
        tool("Read", "ok"),
        tool("Edit", "running"),
      ],
    });
    expect(a.step).toBe(3);
  });

  it("omits the step count before the turn has run a tool", () => {
    expect(agentActivity({ ...base, messages: [user()] }).step).toBeUndefined();
  });

  it("omits the step count when no turn is in flight", () => {
    const a = agentActivity({
      ...base,
      turnActive: false,
      messages: [user(), tool("Read", "ok")],
    });
    expect(a.step).toBeUndefined();
  });
});

describe("agentActivity — elapsed", () => {
  it("times a running tool from its own start, as M:SS", () => {
    const a = agentActivity({
      ...base,
      messages: [tool("Edit", "running", { file_path: "/x/soil.ino" }, 1000)],
      now: 73_400,
    });
    expect(a.elapsed).toBe("1:12");
  });

  it("times writing and thinking from the turn's start", () => {
    expect(
      agentActivity({ ...base, streaming: true, turnStartedAt: 1000, now: 4000 })
        .elapsed,
    ).toBe("0:03");
    expect(
      agentActivity({ ...base, turnStartedAt: 1000, now: 3100 }).elapsed,
    ).toBe("0:02");
  });

  it("has no elapsed without timestamps", () => {
    expect(agentActivity(base).elapsed).toBeUndefined();
  });

  it("never times a verify or upload off the turn clock", () => {
    // The turn may be minutes old while the compile is seconds old — showing
    // the turn's clock next to "verify" would claim a duration that is false.
    const a = agentActivity({
      ...base,
      verifyRunning: true,
      turnStartedAt: 1000,
      now: 90_000,
    });
    expect(a.elapsed).toBeUndefined();
  });
});

describe("agentActivity — staleness is the 'has it hung' answer", () => {
  const quiet = { ...base, turnStartedAt: 0, lastEventAt: 10_000 };

  it("stays working while events keep arriving", () => {
    const a = agentActivity({
      ...quiet,
      now: 10_000 + STALL_AFTER_MS - 1,
    });
    expect(a.phase).toBe("working");
    expect(a.stale).toBeUndefined();
  });

  it("goes stalled once nothing has arrived for the threshold", () => {
    const a = agentActivity({ ...quiet, now: 10_000 + STALL_AFTER_MS });
    expect(a.phase).toBe("stalled");
    expect(a.state).toBe("Working");
    expect(a.stale).toBe(`no output for ${formatThreshold()}`);
  });

  it("measures quiet from the turn start when it is the later of the two", () => {
    // A fresh turn whose first event has not landed yet must not inherit the
    // previous turn's last-event time and read as instantly stalled.
    const a = agentActivity({
      ...base,
      lastEventAt: 0,
      turnStartedAt: 100_000,
      now: 100_000 + 1000,
    });
    expect(a.phase).toBe("working");
    expect(a.stale).toBeUndefined();
  });

  it("never calls a verify or upload stalled — a silent compiler is normal", () => {
    const a = agentActivity({
      ...quiet,
      verifyRunning: true,
      now: 10_000 + STALL_AFTER_MS * 5,
    });
    expect(a.phase).toBe("working");
    expect(a.stale).toBeUndefined();
  });

  it("never marks an idle session stalled", () => {
    const a = agentActivity({
      ...quiet,
      turnActive: false,
      now: 10_000 + STALL_AFTER_MS * 5,
    });
    expect(a.phase).toBe("ready");
    expect(a.stale).toBeUndefined();
  });
});

function formatThreshold(): string {
  const s = Math.floor(STALL_AFTER_MS / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

describe("formatAgentActivity", () => {
  it("is just the state when there is nothing else to say", () => {
    expect(formatAgentActivity(agentActivity({ ...base, turnActive: false }))).toBe(
      "Ready",
    );
    expect(formatAgentActivity(agentActivity({ ...base, status: "idle" }))).toBe(
      "Not started",
    );
  });

  it("joins state, detail, step and elapsed with middots", () => {
    const a = agentActivity({
      ...base,
      messages: [user(), tool("Read", "ok"), tool("Edit", "running", { file_path: "/x/soil.ino" }, 1000)],
      now: 4000,
    });
    expect(formatAgentActivity(a)).toBe(
      "Working · ⚙ Edit /x/soil.ino · step 2 · 0:03",
    );
  });

  it("puts the staleness last, where it reads as a caveat", () => {
    const a = agentActivity({
      ...base,
      turnStartedAt: 0,
      lastEventAt: 0,
      now: 252_000,
    });
    expect(formatAgentActivity(a)).toBe(
      "Working · thinking · 4:12 · no output for 4:12",
    );
  });
});

describe("agentActivityParts — what gets sacrificed when the row is narrow", () => {
  const working = agentActivity({
    ...base,
    messages: [user(), tool("Edit", "running", { file_path: "/very/long/path/soil.ino" }, 1000)],
    turnStartedAt: 0,
    lastEventAt: 0,
    now: 100_000,
  });

  it("always reassembles into exactly the formatted line", () => {
    const p = agentActivityParts(working);
    expect(p.head + p.detail + p.tail).toBe(formatAgentActivity(working));
  });

  it("puts the whole path in the shrinkable middle and nothing else there", () => {
    const p = agentActivityParts(working);
    expect(p.detail).toContain("/very/long/path/soil.ino");
    expect(p.head).not.toContain("/very/long/path");
    expect(p.tail).not.toContain("/very/long/path");
  });

  it("keeps the state word and the staleness caveat out of harm's way", () => {
    const p = agentActivityParts(working);
    expect(p.head).toBe("Working");
    expect(p.tail).toContain("no output for");
    expect(p.tail).toContain("step 1");
  });

  it("renames the state for a caller that needs a subject", () => {
    expect(agentActivityParts(working, "Assistant").head).toBe("Assistant");
  });

  it("has an empty middle when there is nothing to shrink", () => {
    const p = agentActivityParts(agentActivity({ ...base, turnActive: false }));
    expect(p).toEqual({ head: "Ready", detail: "", tail: "" });
  });
});
