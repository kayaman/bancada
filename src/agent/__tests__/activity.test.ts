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
  streaming: false,
  messages: [] as AgentMessage[],
};

describe("activityLabel", () => {
  it("is null unless the session is running or starting", () => {
    expect(activityLabel({ ...base, status: "idle" })).toBeNull();
    expect(activityLabel({ ...base, status: "ended" })).toBeNull();
    expect(activityLabel({ ...base, status: "starting" })).not.toBeNull();
  });

  it("verify outranks everything", () => {
    const a = activityLabel({
      ...base,
      verifyRunning: true,
      streaming: true,
      messages: [tool("Edit", "running", { file_path: "/x/soil.ino" })],
    });
    expect(a).toBe("🔨 verify (compiling)…");
  });

  it("names the newest running tool with the file basename", () => {
    const a = activityLabel({
      ...base,
      messages: [
        tool("Read", "ok", { file_path: "/x/a.ino" }),
        tool("Edit", "running", { file_path: "/home/x/soil/soil.ino" }),
      ],
    });
    expect(a).toBe("⚙ Edit soil.ino…");
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
});
