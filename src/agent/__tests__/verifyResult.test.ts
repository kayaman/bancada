import { describe, expect, it } from "vitest";
import { parseVerifyResult } from "../verifyResult";

// The strings below are written to match `run_verify` in
// src-tauri/src/lib.rs exactly:
//
//     format!("success: {}\nexit_code: {}\n\n{summary}", success, exit_code)
//
// where `summary` comes from `core::agent::summarize_build_output`, whose
// sections are "--- stderr ---" / "--- stdout ---". Keeping real-shaped
// fixtures here is the point of extracting this module: the contract used to
// be held together by two comments pointing at each other.

describe("parseVerifyResult", () => {
  it("reads a passing build the way run_verify formats it", () => {
    const r = parseVerifyResult(
      "success: true\nexit_code: 0\n\n--- stdout ---\nSketch uses 1234 bytes\n",
    );
    expect(r.success).toBe(true);
    expect(r.exitCode).toBe(0);
    expect(r.summary).toBe("--- stdout ---\nSketch uses 1234 bytes\n");
  });

  it("reads a failing build and keeps the compiler error in the summary", () => {
    const r = parseVerifyResult(
      "success: false\nexit_code: 1\n\n--- stderr ---\n" +
        "Blink.ino:3:1: error: expected ';' before '}' token\n",
    );
    expect(r.success).toBe(false);
    expect(r.exitCode).toBe(1);
    expect(r.summary).toContain("expected ';'");
  });

  it("handles a negative exit code (a signalled arduino-cli)", () => {
    // `RunResult::exit_code` is `status.code().unwrap_or(-1)`.
    const r = parseVerifyResult("success: false\nexit_code: -1\n\nboom\n");
    expect(r.exitCode).toBe(-1);
    expect(r.success).toBe(false);
  });

  it("handles an empty summary (a build with no output at all)", () => {
    const r = parseVerifyResult("success: true\nexit_code: 0\n\n");
    expect(r.success).toBe(true);
    expect(r.summary).toBe("");
  });

  it("survives a missing exit_code line rather than losing the verdict", () => {
    const r = parseVerifyResult("success: true\n\nsomething");
    expect(r.success).toBe(true);
    expect(r.exitCode).toBeUndefined();
    expect(r.summary).toBe("something");
  });

  it("reports `undefined` and the raw text for a shape it does not recognise", () => {
    // What the *other* tool-result paths look like: the gate-busy message,
    // the arduino-cli-missing message, and the cancelled-session message all
    // arrive as plain prose with isError set instead.
    for (const text of [
      "build already in progress",
      "verify could not run: tool not found: arduino-cli",
      "the agent session was stopped before this build could start",
      "",
    ]) {
      const r = parseVerifyResult(text);
      expect(r.success).toBeUndefined();
      expect(r.summary).toBe(text);
    }
  });

  it("does not mistake a summary line for the success line", () => {
    // A compiler warning quoting the word would otherwise flip the verdict.
    const r = parseVerifyResult(
      "success: false\nexit_code: 2\n\nnote: success: true was expected\n",
    );
    expect(r.success).toBe(false);
    expect(r.exitCode).toBe(2);
    expect(r.summary).toContain("note: success: true was expected");
  });

  it("tolerates extra blank lines between the header and the body", () => {
    const r = parseVerifyResult("success: true\nexit_code: 0\n\n\n\nbody");
    expect(r.summary).toBe("body");
  });
});
