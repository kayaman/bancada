import { describe, expect, it } from "vitest";
import { alarmConsequence } from "../alarmCopy";

// This is the only safety text a real user ever sees. The first version
// printed "Nothing outside this project was changed." on EVERY alarm, which
// was false for path_escape (the layer-4 backstop fires *after* the tool_use,
// so the write may already have landed) and meaningless for unexpected_tools.
// These tests exist to keep reassurance from creeping back in.

const KINDS = ["path_escape", "unexpected_tools", "something_new"] as const;

describe("alarmConsequence", () => {
  it("tells the truth about path_escape: the write may have completed", () => {
    const text = alarmConsequence("path_escape");
    expect(text).toMatch(/may have completed/i);
    expect(text).toMatch(/inspect/i);
    expect(text).toMatch(/stopped/i);
  });

  it("describes unexpected_tools as a capability problem, not a file one", () => {
    const text = alarmConsequence("unexpected_tools");
    expect(text).toMatch(/capabilit/i);
    expect(text).toMatch(/not bounded/i);
    // It must not make any claim about what was or was not written — that
    // alarm has no information about files.
    expect(text).not.toMatch(/nothing .* (was )?(changed|written)/i);
  });

  it("never claims nothing was changed, for any kind", () => {
    for (const kind of KINDS) {
      const text = alarmConsequence(kind);
      expect(
        text,
        `${kind} must not reassure the user that nothing changed`,
      ).not.toMatch(/nothing (outside|was|else)/i);
      expect(text).not.toMatch(/no files were/i);
      expect(text).not.toMatch(/safely (blocked|stopped|prevented)/i);
    }
  });

  it("gives each kind its own wording", () => {
    expect(alarmConsequence("path_escape")).not.toBe(
      alarmConsequence("unexpected_tools"),
    );
  });

  it("says something useful for a kind it does not know", () => {
    // A future host-side alarm kind must not render an empty paragraph.
    const text = alarmConsequence("a_kind_added_later");
    expect(text.length).toBeGreaterThan(40);
    expect(text).toMatch(/review the transcript/i);
  });
});
