import { describe, expect, it } from "vitest";
import { checkNewEntry, checkNewFile } from "../newFile";
import type { SketchFile } from "../api";

const f = (rel_path: string, is_dir = false): SketchFile => ({ rel_path, is_dir });

describe("checkNewEntry", () => {
  const existing = [f("A.ino"), f("data", true), f("data/config.h")];

  it("resolves the name against the parent dir", () => {
    expect(checkNewEntry("secrets.h", "data", existing)).toEqual({
      ok: true,
      relPath: "data/secrets.h",
    });
  });

  it("empty parent means the sketch root", () => {
    expect(checkNewEntry("util.h", "", existing)).toEqual({ ok: true, relPath: "util.h" });
  });

  it("tolerates a trailing slash — folders are first-class now", () => {
    expect(checkNewEntry("assets/", "", existing)).toEqual({ ok: true, relPath: "assets" });
  });

  it("rejects collisions inside the parent", () => {
    const r = checkNewEntry("config.h", "data", existing);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("already exists");
  });

  it("still rejects traversal, absolute paths and empty segments", () => {
    expect(checkNewEntry("../out.h", "data", existing).ok).toBe(false);
    expect(checkNewEntry("/abs.h", "", existing).ok).toBe(false);
    expect(checkNewEntry("a//b.h", "", existing).ok).toBe(false);
    expect(checkNewEntry("  ", "data", existing).ok).toBe(false);
  });
});

describe("checkNewFile", () => {
  const existing = [f("A.ino"), f("data", true), f("data/config.h")];

  it("accepts a fresh root name, trimmed", () => {
    expect(checkNewFile("  util.h ", existing)).toEqual({ ok: true, relPath: "util.h" });
  });

  it("accepts a fresh subpath", () => {
    expect(checkNewFile("data/secrets.h", existing)).toEqual({
      ok: true,
      relPath: "data/secrets.h",
    });
  });

  it("rejects empty input", () => {
    expect(checkNewFile("   ", existing).ok).toBe(false);
  });

  it("rejects an absolute path", () => {
    expect(checkNewFile("/etc/passwd", existing).ok).toBe(false);
  });

  it("rejects .. anywhere in the path", () => {
    expect(checkNewFile("../outside.h", existing).ok).toBe(false);
    expect(checkNewFile("data/../../out.h", existing).ok).toBe(false);
  });

  it("rejects a trailing slash — that names a folder", () => {
    expect(checkNewFile("data/", existing).ok).toBe(false);
  });

  it("rejects empty path segments", () => {
    expect(checkNewFile("data//x.h", existing).ok).toBe(false);
  });

  it("refuses to shadow an existing file", () => {
    const r = checkNewFile("data/config.h", existing);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("already exists");
  });
});
