import { describe, it, expect } from "vitest";
import { posForLineCol } from "../editorGoto";
import type { DocLike } from "../editorGoto";

/** Stand-in for CodeMirror's `EditorState.doc` — three lines of 10, 4 and 7. */
const doc: DocLike = {
  lines: 3,
  line(n: number) {
    const spec = [
      { from: 0, length: 10 },
      { from: 11, length: 4 },
      { from: 16, length: 7 },
    ][n - 1];
    if (!spec) throw new RangeError(`no line ${n}`);
    return spec;
  },
};

describe("posForLineCol", () => {
  it("maps 1-based line/col onto a document offset", () => {
    expect(posForLineCol(doc, 1, 1)).toBe(0);
    expect(posForLineCol(doc, 2, 1)).toBe(11);
    expect(posForLineCol(doc, 2, 3)).toBe(13);
    expect(posForLineCol(doc, 3, 7)).toBe(22);
  });

  it("puts the cursor at the line start when there is no column", () => {
    // gcc omits the column on linker-adjacent diagnostics.
    expect(posForLineCol(doc, 2, null)).toBe(11);
  });

  it("clamps a column past the end of the line to the line end", () => {
    expect(posForLineCol(doc, 2, 99)).toBe(15);
  });

  it("clamps a line past the end of the document to the last line", () => {
    // A stale diagnostic against an edited buffer must not throw.
    expect(posForLineCol(doc, 99, 1)).toBe(16);
    expect(posForLineCol(doc, 99, 99)).toBe(23);
  });

  it("clamps a line below 1 to the first line", () => {
    expect(posForLineCol(doc, 0, 1)).toBe(0);
    expect(posForLineCol(doc, -5, null)).toBe(0);
  });

  it("clamps a column below 1 to the line start", () => {
    expect(posForLineCol(doc, 2, 0)).toBe(11);
  });
});
