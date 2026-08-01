import { describe, expect, it } from "vitest";
import { fileTabs, tabLabel } from "../editorTabs";
import type { SketchFile } from "../api";

const f = (rel_path: string, is_dir = false): SketchFile => ({
  rel_path,
  is_dir,
});

describe("fileTabs", () => {
  it("keeps files in tree order", () => {
    const files = [f("A.ino"), f("sketch.yaml")];
    expect(fileTabs(files).map((x) => x.rel_path)).toEqual([
      "A.ino",
      "sketch.yaml",
    ]);
  });

  it("drops directories — a tab must open a file", () => {
    const files = [f("A.ino"), f("data", true), f("data/config.h")];
    expect(fileTabs(files).map((x) => x.rel_path)).toEqual([
      "A.ino",
      "data/config.h",
    ]);
  });

  it("is empty when no sketch is open", () => {
    expect(fileTabs([])).toEqual([]);
  });
});

describe("tabLabel", () => {
  it("shows the bare name for a root file", () => {
    expect(tabLabel("A.ino")).toBe("A.ino");
  });

  it("shows only the basename for a nested file", () => {
    expect(tabLabel("data/config.h")).toBe("config.h");
  });
});
