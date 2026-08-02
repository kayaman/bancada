import { describe, expect, it } from "vitest";
import type { SketchFile } from "../api";
import {
  ancestorsOf,
  buildTree,
  pruneExpanded,
  remapSet,
  visibleNodes,
} from "../fileTreeModel";

const f = (rel_path: string, is_dir = false): SketchFile => ({ rel_path, is_dir });

const SAMPLE: SketchFile[] = [
  f("demo.ino"),
  f("sketch.yaml"),
  f("src", true),
  f("src/util.h"),
  f("src/sensors", true),
  f("src/sensors/Env.cpp"),
  f("assets", true), // empty dir
];

describe("buildTree", () => {
  it("nests children under their directories", () => {
    const tree = buildTree(SAMPLE);
    const src = tree.find((n) => n.relPath === "src");
    expect(src?.isDir).toBe(true);
    expect(src?.children.map((c) => c.relPath)).toEqual([
      "src/sensors",
      "src/util.h",
    ]);
  });

  it("sorts dirs before files, then case-insensitively by name", () => {
    const tree = buildTree([f("b.txt"), f("A.txt"), f("zdir", true), f("adir", true)]);
    expect(tree.map((n) => n.name)).toEqual(["adir", "zdir", "A.txt", "b.txt"]);
  });

  it("orders case-insensitive name ties deterministically", () => {
    // "A.txt" and "a.txt" tie in the base-sensitivity pass; the exact
    // tiebreaker must give the same order on every build.
    const once = buildTree([f("a.txt"), f("A.txt")]).map((n) => n.name);
    const twice = buildTree([f("A.txt"), f("a.txt")]).map((n) => n.name);
    expect([...once].sort()).toEqual(["A.txt", "a.txt"]);
    expect(once).toEqual(twice);
  });

  it("represents empty dirs as leaf dir nodes", () => {
    const tree = buildTree(SAMPLE);
    const assets = tree.find((n) => n.relPath === "assets");
    expect(assets?.isDir).toBe(true);
    expect(assets?.children).toEqual([]);
  });

  it("creates implicit nodes for missing intermediate dirs", () => {
    const tree = buildTree([f("deep/nested/leaf.txt")]);
    expect(tree[0].relPath).toBe("deep");
    expect(tree[0].children[0].relPath).toBe("deep/nested");
    expect(tree[0].children[0].children[0].relPath).toBe("deep/nested/leaf.txt");
  });
});

describe("visibleNodes", () => {
  it("hides children of collapsed dirs", () => {
    const rows = visibleNodes(buildTree(SAMPLE), new Set());
    expect(rows.map((r) => r.node.relPath)).toEqual([
      "assets",
      "src",
      "demo.ino",
      "sketch.yaml",
    ]);
  });

  it("shows children of expanded dirs with depth", () => {
    const rows = visibleNodes(buildTree(SAMPLE), new Set(["src"]));
    const srcUtil = rows.find((r) => r.node.relPath === "src/util.h");
    expect(srcUtil?.depth).toBe(1);
    // sensors is visible but its child is not (sensors not expanded)
    expect(rows.some((r) => r.node.relPath === "src/sensors")).toBe(true);
    expect(rows.some((r) => r.node.relPath === "src/sensors/Env.cpp")).toBe(false);
  });
});

describe("ancestorsOf", () => {
  it("lists every ancestor dir shallowest-first", () => {
    expect(ancestorsOf("src/sensors/Env.cpp")).toEqual(["src", "src/sensors"]);
  });

  it("is empty for top-level paths", () => {
    expect(ancestorsOf("demo.ino")).toEqual([]);
  });
});

describe("pruneExpanded", () => {
  it("drops expansion entries for dirs that no longer exist", () => {
    const kept = pruneExpanded(new Set(["src", "gone"]), SAMPLE);
    expect([...kept]).toEqual(["src"]);
  });
});

describe("remapSet", () => {
  it("rewrites the renamed path and everything under it", () => {
    const out = remapSet(new Set(["src", "src/sensors", "assets"]), "src", "lib");
    expect(out).toEqual(new Set(["lib", "lib/sensors", "assets"]));
  });

  it("does not touch sibling paths sharing a prefix", () => {
    const out = remapSet(new Set(["src2/x"]), "src", "lib");
    expect(out).toEqual(new Set(["src2/x"]));
  });
});
