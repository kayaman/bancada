import { describe, expect, it } from "vitest";
import {
  tabLabel,
  openTab,
  closeTab,
  closeOthers,
  closeAll,
  renameTabs,
  deleteTabs,
} from "../editorTabs";

describe("tabLabel", () => {
  it("shows the bare name for a root file", () => {
    expect(tabLabel("A.ino")).toBe("A.ino");
  });

  it("shows only the basename for a nested file", () => {
    expect(tabLabel("data/config.h")).toBe("config.h");
  });
});

describe("openTab", () => {
  it("appends a tab that is not present", () => {
    const tabs = ["A.ino"];
    expect(openTab(tabs, "B.h")).toEqual(["A.ino", "B.h"]);
  });

  it("does not duplicate an already-open tab", () => {
    const tabs = ["A.ino", "B.h"];
    expect(openTab(tabs, "B.h")).toEqual(["A.ino", "B.h"]);
  });

  it("appends to an empty tab list", () => {
    expect(openTab([], "A.ino")).toEqual(["A.ino"]);
  });

  it("preserves order when the tab already exists", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    expect(openTab(tabs, "B.h")).toEqual(["A.ino", "B.h", "C.cpp"]);
  });

  it("does not reorder on append", () => {
    const tabs = ["A.ino", "B.h"];
    expect(openTab(tabs, "C.cpp")).toEqual(["A.ino", "B.h", "C.cpp"]);
  });
});

describe("closeTab", () => {
  it("closes a non-active tab and keeps active unchanged", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const result = closeTab(tabs, "B.h", "A.ino");
    expect(result).toEqual({
      tabs: ["A.ino", "C.cpp"],
      nextActive: "A.ino",
    });
  });

  it("closes an absent tab and returns unchanged tabs with active unchanged", () => {
    const tabs = ["A.ino", "B.h"];
    const result = closeTab(tabs, "missing.h", "A.ino");
    expect(result).toEqual({
      tabs: ["A.ino", "B.h"],
      nextActive: "A.ino",
    });
  });

  it("closes the active middle tab and activates the right neighbor", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const result = closeTab(tabs, "B.h", "B.h");
    expect(result).toEqual({
      tabs: ["A.ino", "C.cpp"],
      nextActive: "C.cpp",
    });
  });

  it("closes the active last tab and activates the left neighbor", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const result = closeTab(tabs, "C.cpp", "C.cpp");
    expect(result).toEqual({
      tabs: ["A.ino", "B.h"],
      nextActive: "B.h",
    });
  });

  it("closes the only tab and returns null as nextActive", () => {
    const tabs = ["A.ino"];
    const result = closeTab(tabs, "A.ino", "A.ino");
    expect(result).toEqual({
      tabs: [],
      nextActive: null,
    });
  });

  it("closes the active first tab and activates the right neighbor", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const result = closeTab(tabs, "A.ino", "A.ino");
    expect(result).toEqual({
      tabs: ["B.h", "C.cpp"],
      nextActive: "B.h",
    });
  });

  it("handles closing when active is null", () => {
    const tabs = ["A.ino", "B.h"];
    const result = closeTab(tabs, "A.ino", null);
    expect(result).toEqual({
      tabs: ["B.h"],
      nextActive: null,
    });
  });
});

describe("closeOthers", () => {
  it("keeps the target tab and all dirty tabs", () => {
    const tabs = ["A.ino", "B.h", "C.cpp", "D.cpp"];
    const dirty = new Set(["B.h", "D.cpp"]);
    expect(closeOthers(tabs, "A.ino", dirty)).toEqual(["A.ino", "B.h", "D.cpp"]);
  });

  it("closes non-dirty tabs except the target", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const dirty = new Set<string>();
    expect(closeOthers(tabs, "A.ino", dirty)).toEqual(["A.ino"]);
  });

  it("preserves tab order", () => {
    const tabs = ["C.cpp", "A.ino", "B.h", "D.cpp"];
    const dirty = new Set(["B.h"]);
    expect(closeOthers(tabs, "A.ino", dirty)).toEqual(["A.ino", "B.h"]);
  });

  it("keeps only the target when nothing is dirty", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const dirty = new Set<string>();
    expect(closeOthers(tabs, "B.h", dirty)).toEqual(["B.h"]);
  });

  it("handles empty dirty set", () => {
    const tabs = ["A.ino", "B.h"];
    expect(closeOthers(tabs, "A.ino", new Set())).toEqual(["A.ino"]);
  });
});

describe("closeAll", () => {
  it("closes all non-dirty tabs", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const dirty = new Set(["B.h"]);
    expect(closeAll(tabs, dirty)).toEqual(["B.h"]);
  });

  it("returns empty array when nothing is dirty", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    expect(closeAll(tabs, new Set())).toEqual([]);
  });

  it("preserves order of remaining dirty tabs", () => {
    const tabs = ["A.ino", "B.h", "C.cpp", "D.cpp"];
    const dirty = new Set(["B.h", "D.cpp"]);
    expect(closeAll(tabs, dirty)).toEqual(["B.h", "D.cpp"]);
  });

  it("keeps all tabs when everything is dirty", () => {
    const tabs = ["A.ino", "B.h", "C.cpp"];
    const dirty = new Set(["A.ino", "B.h", "C.cpp"]);
    expect(closeAll(tabs, dirty)).toEqual(["A.ino", "B.h", "C.cpp"]);
  });
});

describe("renameTabs", () => {
  it("renames a tab that matches the from path exactly", () => {
    const tabs = ["A.ino", "config.h", "C.cpp"];
    expect(renameTabs(tabs, "config.h", "settings.h", false)).toEqual([
      "A.ino",
      "settings.h",
      "C.cpp",
    ]);
  });

  it("does not rename a similarly-named tab that is not a directory prefix match", () => {
    // webx is not under web/ directory, so should not be renamed when web→w2
    const tabs = ["webx.h", "web/index.html"];
    expect(renameTabs(tabs, "web", "w2", true)).toEqual(["webx.h", "w2/index.html"]);
  });

  it("renames all tabs under a renamed directory", () => {
    const tabs = ["A.ino", "data/config.h", "data/info.txt", "other.h"];
    expect(renameTabs(tabs, "data", "config", true)).toEqual([
      "A.ino",
      "config/config.h",
      "config/info.txt",
      "other.h",
    ]);
  });

  it("preserves tab order during rename", () => {
    const tabs = ["data/z.txt", "A.ino", "data/a.txt"];
    expect(renameTabs(tabs, "data", "config", true)).toEqual([
      "config/z.txt",
      "A.ino",
      "config/a.txt",
    ]);
  });

  it("handles file rename when wasDir is false", () => {
    const tabs = ["A.ino", "config.h", "C.cpp"];
    expect(renameTabs(tabs, "config.h", "settings.h", false)).toEqual([
      "A.ino",
      "settings.h",
      "C.cpp",
    ]);
  });

  it("renames only exact matches when wasDir is false", () => {
    const tabs = ["config.h", "config/data.h"];
    expect(renameTabs(tabs, "config.h", "settings.h", false)).toEqual([
      "settings.h",
      "config/data.h",
    ]);
  });
});

describe("deleteTabs", () => {
  it("deletes a tab matching the rel path exactly", () => {
    const tabs = ["A.ino", "config.h", "C.cpp"];
    expect(deleteTabs(tabs, "config.h", false)).toEqual(["A.ino", "C.cpp"]);
  });

  it("deletes all tabs under a directory", () => {
    const tabs = ["A.ino", "data/config.h", "data/info.txt", "other.h"];
    expect(deleteTabs(tabs, "data", true)).toEqual(["A.ino", "other.h"]);
  });

  it("does not delete similarly-named non-directory tabs", () => {
    // datax is not under data/ directory
    const tabs = ["datax.txt", "data/config.h"];
    expect(deleteTabs(tabs, "data", true)).toEqual(["datax.txt"]);
  });

  it("preserves tab order after deletion", () => {
    const tabs = ["data/z.txt", "A.ino", "data/a.txt", "B.h"];
    expect(deleteTabs(tabs, "data", true)).toEqual(["A.ino", "B.h"]);
  });

  it("does nothing when the tab to delete is absent", () => {
    const tabs = ["A.ino", "B.h"];
    expect(deleteTabs(tabs, "missing.h", false)).toEqual(["A.ino", "B.h"]);
  });

  it("handles deleting a file when wasDir is false", () => {
    const tabs = ["A.ino", "config.h", "C.cpp"];
    expect(deleteTabs(tabs, "config.h", false)).toEqual(["A.ino", "C.cpp"]);
  });
});
