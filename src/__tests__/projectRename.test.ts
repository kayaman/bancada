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

describe("ESP-IDF projects rename by different rules", () => {
  // The backend forks on detect_kind, so the pane has to as well — a name it
  // accepts and the backend refuses is a round trip ending in an error the
  // user could have been shown instantly.
  it("refuses a leading digit, which Arduino allows", () => {
    expect(checkProjectName("2fast", cur, "arduino").ok).toBe(true);
    const idf = checkProjectName("2fast", cur, "idf");
    expect(idf.ok).toBe(false);
    expect(idf.ok === false && idf.reason).toMatch(/digit/);
  });

  it("refuses a dot, which is legal in a sketch folder", () => {
    expect(checkProjectName("my.app", cur, "arduino").ok).toBe(true);
    expect(checkProjectName("my.app", cur, "idf").ok).toBe(false);
  });

  it("still accepts the names both paradigms allow", () => {
    for (const n of ["blink_node", "sensor-node", "app2"]) {
      expect(checkProjectName(n, cur, "idf").ok, n).toBe(true);
      expect(checkProjectName(n, cur, "arduino").ok, n).toBe(true);
    }
  });

  it("allows the 64th character ESP-IDF permits", () => {
    // MAX_NAME_LEN is 64 for CMake, 63 for arduino-lint.
    const n = "a".repeat(64);
    expect(checkProjectName(n, cur, "idf").ok).toBe(true);
    expect(checkProjectName(n, cur, "arduino").ok).toBe(false);
  });

  it("describes the CMake name rather than a .ino that does not move", () => {
    // The pane used to promise "old.ino → new.ino" for every project. For an
    // ESP-IDF one no source is renamed at all; what changes is the project()
    // call, and showing the wrong thing is worse than showing less.
    const plan = renamePlan("/p/old", "new", "idf");
    expect(plan.destDir).toBe("/p/new");
    expect(plan.oldIno).toBeUndefined();
    expect(plan.cmakeName).toBe("project(new)");
  });

  it("still describes the .ino move for a sketch", () => {
    const plan = renamePlan("/p/old", "new", "arduino");
    expect(plan.oldIno).toBe("old.ino");
    expect(plan.newIno).toBe("new.ino");
    expect(plan.cmakeName).toBeUndefined();
  });
});
