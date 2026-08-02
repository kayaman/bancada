import { describe, expect, it } from "vitest";
import type { SketchFile } from "../api";
import {
  affectedByDelete,
  checkRename,
  createAnchorDir,
  isDescendant,
  isNonEmptyDir,
  pathAfterRename,
  protectedPaths,
} from "../explorerOps";

const f = (rel_path: string, is_dir = false): SketchFile => ({ rel_path, is_dir });

const FILES: SketchFile[] = [
  f("demo.ino"),
  f("sketch.yaml"),
  f("src", true),
  f("src/util.h"),
  f("assets", true),
];

describe("pathAfterRename", () => {
  it("maps the renamed file itself", () => {
    expect(pathAfterRename("src/util.h", "src/util.h", "src/h.h", false)).toBe("src/h.h");
  });

  it("maps paths inside a renamed dir", () => {
    expect(pathAfterRename("src/util.h", "src", "lib", true)).toBe("lib/util.h");
  });

  it("returns null for unaffected paths", () => {
    expect(pathAfterRename("demo.ino", "src", "lib", true)).toBe(null);
    // prefix sibling must not match
    expect(pathAfterRename("src2/x.h", "src", "lib", true)).toBe(null);
  });
});

describe("affectedByDelete", () => {
  it("matches the entry itself and dir contents", () => {
    expect(affectedByDelete("src/util.h", "src/util.h", false)).toBe(true);
    expect(affectedByDelete("src/util.h", "src", true)).toBe(true);
    expect(affectedByDelete("demo.ino", "src", true)).toBe(false);
    expect(affectedByDelete("src2/x.h", "src", true)).toBe(false);
  });
});

describe("protectedPaths", () => {
  it("derives the main ino from the sketch dir basename", () => {
    expect(protectedPaths("/home/x/demo")).toEqual(new Set(["demo.ino", "sketch.yaml"]));
  });

  it("handles trailing slash on the sketch dir", () => {
    expect(protectedPaths("/home/x/demo/")).toEqual(new Set(["demo.ino", "sketch.yaml"]));
  });
});

describe("isNonEmptyDir", () => {
  it("is true when any entry lives under the dir", () => {
    expect(isNonEmptyDir(FILES, "src")).toBe(true);
    expect(isNonEmptyDir(FILES, "assets")).toBe(false);
  });
});

describe("createAnchorDir", () => {
  it("uses the selected dir itself", () => {
    expect(createAnchorDir("src", FILES)).toBe("src");
  });

  it("uses a selected file's parent", () => {
    expect(createAnchorDir("src/util.h", FILES)).toBe("src");
    expect(createAnchorDir("demo.ino", FILES)).toBe("");
  });

  it("falls back to the root for no or stale selection", () => {
    expect(createAnchorDir(null, FILES)).toBe("");
    expect(createAnchorDir("gone/x.h", FILES)).toBe("");
  });
});

describe("isDescendant", () => {
  it("is true only for genuine descendants", () => {
    expect(isDescendant("src/a/b", "src")).toBe(true);
    expect(isDescendant("src", "src")).toBe(false);
    expect(isDescendant("src2/x", "src")).toBe(false);
  });
});

describe("checkRename", () => {
  const check = (from: string, to: string) => checkRename(from, to, FILES, "/home/x/demo");

  it("accepts a plain rename and a move", () => {
    expect(check("src/util.h", "src/helpers.h")).toEqual({ ok: true });
    expect(check("src/util.h", "lib/util.h")).toEqual({ ok: true });
  });

  it("rejects unchanged, protected, collision, descendant and bad paths", () => {
    expect(check("src/util.h", "src/util.h").ok).toBe(false);
    expect(check("demo.ino", "x.ino").ok).toBe(false);
    expect(check("src/util.h", "demo.ino").ok).toBe(false); // collision
    expect(check("src", "src/inner").ok).toBe(false);
    expect(check("src/util.h", "/abs").ok).toBe(false);
    expect(check("src/util.h", "a//b").ok).toBe(false);
    expect(check("src/util.h", "../out").ok).toBe(false);
  });

  it("rejects an empty target", () => {
    expect(check("src/util.h", "   ").ok).toBe(false);
  });

  it("rejects a genuine collision with a non-protected entry", () => {
    // "assets" exists and is not protected — this must reach the
    // already-exists guard, not be masked by the protected-target one.
    const r = check("src/util.h", "assets");
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("already exists");
  });

  it("rejects renaming onto a protected name", () => {
    // sketch.yaml exists so it is also a collision — but even if it were
    // deleted, taking a protected name must be blocked.
    const noYaml = FILES.filter((x) => x.rel_path !== "sketch.yaml");
    expect(checkRename("src/util.h", "sketch.yaml", noYaml, "/home/x/demo").ok).toBe(false);
  });

  it("allows a case-only rename despite the case-insensitive collision", () => {
    expect(check("src/util.h", "src/Util.h")).toEqual({ ok: true });
  });
});
