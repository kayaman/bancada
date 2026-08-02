import { beforeEach, describe, expect, it } from "vitest";
import type { SketchFile } from "../api";
import { useExplorerStore } from "../explorerStore";

const f = (rel_path: string, is_dir = false): SketchFile => ({ rel_path, is_dir });

const SAMPLE: SketchFile[] = [
  f("demo.ino"),
  f("src", true),
  f("src/util.h"),
  f("src/sensors", true),
  f("src/sensors/Env.cpp"),
];

const store = () => useExplorerStore.getState();

beforeEach(() => {
  useExplorerStore.getState().reset();
});

describe("setFiles", () => {
  it("stores the listing and prunes stale expansion and selection", () => {
    store().setFiles(SAMPLE);
    store().toggleExpanded("src");
    store().select("src/util.h");
    store().setFiles([f("demo.ino")]); // src is gone
    expect(store().files).toEqual([f("demo.ino")]);
    expect(store().expanded.has("src")).toBe(false);
    expect(store().selected).toBe(null);
  });

  it("keeps selection when the file still exists", () => {
    store().setFiles(SAMPLE);
    store().select("demo.ino");
    store().setFiles(SAMPLE);
    expect(store().selected).toBe("demo.ino");
  });
});

describe("setFilesAfterRename", () => {
  it("remaps expansion and selection across the rename", () => {
    store().setFiles(SAMPLE);
    store().toggleExpanded("src");
    store().toggleExpanded("src/sensors");
    store().select("src/sensors/Env.cpp");
    const renamed: SketchFile[] = [
      f("demo.ino"),
      f("lib", true),
      f("lib/util.h"),
      f("lib/sensors", true),
      f("lib/sensors/Env.cpp"),
    ];
    store().setFilesAfterRename(renamed, "src", "lib");
    expect(store().expanded).toEqual(new Set(["lib", "lib/sensors"]));
    expect(store().selected).toBe("lib/sensors/Env.cpp");
  });
});

describe("setFilesAfterRename without a selection", () => {
  it("keeps selected null", () => {
    store().setFiles(SAMPLE);
    store().setFilesAfterRename(SAMPLE, "src", "lib");
    expect(store().selected).toBe(null);
  });
});

describe("drag state", () => {
  it("clearing the drag also clears the drop target", () => {
    store().setDragging("src/util.h");
    store().setDropTarget("src/sensors");
    store().setDragging(null);
    expect(store().dragging).toBe(null);
    expect(store().dropTarget).toBe(null);
  });
});

describe("expansion", () => {
  it("toggleExpanded flips a dir open and closed", () => {
    store().setFiles(SAMPLE);
    store().toggleExpanded("src");
    expect(store().expanded.has("src")).toBe(true);
    store().toggleExpanded("src");
    expect(store().expanded.has("src")).toBe(false);
  });

  it("expandTo opens every ancestor of a path", () => {
    store().setFiles(SAMPLE);
    store().expandTo("src/sensors/Env.cpp");
    expect(store().expanded.has("src")).toBe(true);
    expect(store().expanded.has("src/sensors")).toBe(true);
  });
});

describe("transient ui state", () => {
  it("rename, create, drag and context-menu state round-trip", () => {
    store().startRename("src/util.h");
    expect(store().renaming).toBe("src/util.h");
    store().cancelRename();
    expect(store().renaming).toBe(null);

    store().startCreate("src", "dir");
    expect(store().creating).toEqual({ parentDir: "src", kind: "dir" });
    store().cancelCreate();
    expect(store().creating).toBe(null);

    store().setDragging("src/util.h");
    store().setDropTarget("src/sensors");
    expect(store().dragging).toBe("src/util.h");
    expect(store().dropTarget).toBe("src/sensors");

    store().openContextMenu(10, 20, "src");
    expect(store().contextMenu).toEqual({ x: 10, y: 20, target: "src" });
    store().closeContextMenu();
    expect(store().contextMenu).toBe(null);
  });
});
