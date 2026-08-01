import { describe, expect, it } from "vitest";
import { diffForToolInput, unifiedDiff } from "../diff";

describe("unifiedDiff", () => {
  it("replace-one-line: del then add around a single changed line", () => {
    expect(unifiedDiff("a\nb\nc", "a\nX\nc")).toEqual([
      { kind: "ctx", text: "a" },
      { kind: "del", text: "b" },
      { kind: "add", text: "X" },
      { kind: "ctx", text: "c" },
    ]);
  });

  it("pure insert: no deletions", () => {
    expect(unifiedDiff("a\nb", "a\nx\nb")).toEqual([
      { kind: "ctx", text: "a" },
      { kind: "add", text: "x" },
      { kind: "ctx", text: "b" },
    ]);
  });

  it("pure delete: no additions", () => {
    expect(unifiedDiff("a\nb\nc", "a\nc")).toEqual([
      { kind: "ctx", text: "a" },
      { kind: "del", text: "b" },
      { kind: "ctx", text: "c" },
    ]);
  });

  it("identical strings: every line is context", () => {
    expect(unifiedDiff("a\nb\nc", "a\nb\nc")).toEqual([
      { kind: "ctx", text: "a" },
      { kind: "ctx", text: "b" },
      { kind: "ctx", text: "c" },
    ]);
  });

  it("multi-hunk: two independent changed regions, each keeping - before +", () => {
    const lines = unifiedDiff("1\n2\n3\n4\n5\n6", "1\nX\n3\n4\nY\n6");
    // Two separate hunks, unrelated to each other, with ctx lines around and
    // between them; within each hunk the deletion precedes the insertion.
    expect(lines).toEqual([
      { kind: "ctx", text: "1" },
      { kind: "del", text: "2" },
      { kind: "add", text: "X" },
      { kind: "ctx", text: "3" },
      { kind: "ctx", text: "4" },
      { kind: "del", text: "5" },
      { kind: "add", text: "Y" },
      { kind: "ctx", text: "6" },
    ]);
  });

  it("empty old string against non-empty new string is pure additions (Write-tool shape)", () => {
    expect(unifiedDiff("", "line1\nline2")).toEqual([
      { kind: "add", text: "line1" },
      { kind: "add", text: "line2" },
    ]);
  });

  it("two empty strings produce no lines at all", () => {
    expect(unifiedDiff("", "")).toEqual([]);
  });
});

describe("diffForToolInput", () => {
  it("Edit: diffs old_string -> new_string", () => {
    const input = { old_string: "a\nb", new_string: "a\nX" };
    expect(diffForToolInput("Edit", input)).toEqual(unifiedDiff("a\nb", "a\nX"));
  });

  it("Write: whole content renders as pure additions, no old_string involved", () => {
    const input = { content: "line1\nline2" };
    expect(diffForToolInput("Write", input)).toEqual([
      { kind: "add", text: "line1" },
      { kind: "add", text: "line2" },
    ]);
  });

  it("Edit with a missing old_string/new_string field returns null", () => {
    expect(diffForToolInput("Edit", { old_string: "a" })).toBeNull();
    expect(diffForToolInput("Edit", {})).toBeNull();
  });

  it("Write with a missing content field returns null", () => {
    expect(diffForToolInput("Write", {})).toBeNull();
  });

  it("any other tool (e.g. verify, Read) has no diff card", () => {
    expect(diffForToolInput("verify", {})).toBeNull();
    expect(diffForToolInput("verify", undefined)).toBeNull();
    expect(diffForToolInput("Read", { file_path: "/s/Blink.ino" })).toBeNull();
  });

  it("a non-object input is tolerated, not thrown on", () => {
    expect(diffForToolInput("Edit", "not an object")).toBeNull();
    expect(diffForToolInput("Edit", null)).toBeNull();
  });
});
