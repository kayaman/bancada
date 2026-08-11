import { describe, expect, it } from "vitest";
import { activityLabel } from "../activity";
import type { AgentMessage } from "../agentStore";

const tool = (
  name: string,
  status: "running" | "ok" | "error",
  input: unknown = {},
): AgentMessage => ({ kind: "tool", id: name, name, input, status });

const base = {
  status: "running" as const,
  verifyRunning: false,
  uploadRunning: false,
  streaming: false,
  turnActive: true,
  messages: [] as AgentMessage[],
};

describe("activityLabel", () => {
  it("is null unless the session is running or starting", () => {
    expect(activityLabel({ ...base, status: "idle" })).toBeNull();
    expect(activityLabel({ ...base, status: "ended" })).toBeNull();
    expect(activityLabel({ ...base, status: "starting" })).not.toBeNull();
  });

  it("is null between turns even while the session runs", () => {
    expect(activityLabel({ ...base, turnActive: false })).toBeNull();
    expect(
      activityLabel({ ...base, turnActive: false, streaming: true }),
    ).toBeNull();
  });

  it("an in-flight upload outranks even verify", () => {
    const a = activityLabel({
      ...base,
      turnActive: false,
      uploadRunning: true,
      verifyRunning: true,
      streaming: true,
    });
    expect(a).toBe("📡 upload (flashing)…");
  });

  it("verify outranks everything, including the turnActive gate", () => {
    const a = activityLabel({
      ...base,
      turnActive: false,
      verifyRunning: true,
      streaming: true,
      messages: [tool("Edit", "running", { file_path: "/x/soil.ino" })],
    });
    expect(a).toBe("🔨 verify (compiling)…");
  });

  it("names the newest running tool with the full file path", () => {
    const a = activityLabel({
      ...base,
      messages: [
        tool("Read", "ok", { file_path: "/x/a.ino" }),
        tool("Edit", "running", { file_path: "/home/x/soil/soil.ino" }),
      ],
    });
    expect(a).toBe("⚙ Edit /home/x/soil/soil.ino…");
  });

  it("uses the pattern for Grep and Glob", () => {
    const a = activityLabel({
      ...base,
      messages: [tool("Grep", "running", { pattern: "Serial.begin" })],
    });
    expect(a).toBe("⚙ Grep Serial.begin…");
  });

  it("falls back to the bare tool name when there is no hint", () => {
    const a = activityLabel({
      ...base,
      messages: [tool("mcp__bancada__verify", "running")],
    });
    expect(a).toBe("⚙ mcp__bancada__verify…");
  });

  it("shows writing while text streams and no tool runs", () => {
    const a = activityLabel({
      ...base,
      streaming: true,
      messages: [tool("Edit", "ok", { file_path: "/x/soil.ino" })],
    });
    expect(a).toBe("✍ writing…");
  });

  it("defaults to thinking", () => {
    expect(activityLabel(base)).toBe("thinking…");
  });

  it("appends elapsed seconds when timestamps are known", () => {
    const msgs = [
      {
        ...tool("Edit", "running", { file_path: "/x/soil.ino" }),
        startedAt: 1000,
      },
    ];
    expect(activityLabel({ ...base, messages: msgs, now: 13400 })).toBe(
      "⚙ Edit /x/soil.ino… 12s",
    );
    expect(
      activityLabel({ ...base, streaming: true, turnStartedAt: 1000, now: 4000 }),
    ).toBe("✍ writing… 3s");
    expect(activityLabel({ ...base, turnStartedAt: 1000, now: 3100 })).toBe(
      "thinking… 2s",
    );
  });

  it("omits elapsed under one second or without timestamps", () => {
    expect(activityLabel({ ...base, turnStartedAt: 1000, now: 1500 })).toBe(
      "thinking…",
    );
    expect(activityLabel(base)).toBe("thinking…");
  });
});
