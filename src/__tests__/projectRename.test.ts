import { describe, expect, it } from "vitest";
import { checkProjectName, renamePlan } from "../projectRename";

const cur = "/home/u/Projects/Blink";

describe("checkProjectName", () => {
  it("accepts the charset core allows, trimmed", () => {
    expect(checkProjectName("blink-2", cur)).toEqual({ ok: true });
    expect(checkProjectName("  sala_v2.1  ", cur)).toEqual({ ok: true });
    expect(checkProjectName("a".repeat(63), cur)).toEqual({ ok: true });
  });

  it("rejects an empty name", () => {
    const r = checkProjectName("   ", cur);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toBe("name the project");
  });

  it("rejects the current name — trailing slashes and whitespace included", () => {
    expect(checkProjectName("Blink", cur).ok).toBe(false);
    expect(checkProjectName(" Blink ", "/home/u/Projects/Blink/").ok).toBe(false);
    const r = checkProjectName("Blink", cur);
    if (!r.ok) expect(r.reason).toMatch(/already the project's name/i);
  });

  it("rejects a path separator, either slash", () => {
    const r = checkProjectName("Projects/Blink2", cur);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("path separator");
    expect(checkProjectName("a\\b", cur).ok).toBe(false);
  });

  it("rejects over 63 characters (arduino-lint's limit), and says how many", () => {
    const r = checkProjectName("a".repeat(64), cur);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("63 characters or fewer (got 64)");
  });

  it("rejects a dotted name — arduino-cli skips hidden folders", () => {
    const r = checkProjectName(".hidden", cur);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/hidden/);
  });

  it("refuses spaces rather than converting them, and hints", () => {
    const r = checkProjectName("my blink", cur);
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.reason).toContain("found ' '");
      expect(r.reason).toContain("instead of spaces");
    }
  });

  it("names any other disallowed character", () => {
    const r = checkProjectName("blinké", cur);
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.reason).toContain("found 'é'");
      expect(r.reason).not.toContain("spaces");
    }
  });

  it("requires an alphanumeric first character", () => {
    const r = checkProjectName("-blink", cur);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/start with a letter or a digit/);
    expect(checkProjectName("_blink", cur).ok).toBe(false);
  });
});

describe("renamePlan", () => {
  it("lands beside the current dir and moves the main .ino", () => {
    expect(renamePlan("/home/u/Projects/Blink", "Blink2")).toEqual({
      destDir: "/home/u/Projects/Blink2",
      oldIno: "Blink.ino",
      newIno: "Blink2.ino",
    });
  });

  it("ignores trailing slashes and trims the new name", () => {
    expect(renamePlan("/home/u/Projects/Blink/", "  Blink2 ")).toEqual({
      destDir: "/home/u/Projects/Blink2",
      oldIno: "Blink.ino",
      newIno: "Blink2.ino",
    });
  });

  it("handles a dir directly under root", () => {
    expect(renamePlan("/Blink", "Blink2").destDir).toBe("/Blink2");
  });

  it("handles a bare relative name", () => {
    expect(renamePlan("Blink", "Blink2")).toEqual({
      destDir: "Blink2",
      oldIno: "Blink.ino",
      newIno: "Blink2.ino",
    });
  });
});
