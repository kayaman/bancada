// Pure tree math for the explorer: nesting, visibility, expansion upkeep.
// Kept free of React/store imports so it tests under the node-env vitest.

import type { SketchFile } from "./api";

export interface TreeNode {
  relPath: string;
  name: string;
  isDir: boolean;
  children: TreeNode[];
}

/** Nested tree from the flat listing: dirs first, then case-insensitive by
 *  name. Tolerates listings missing intermediate dir entries. */
export function buildTree(files: SketchFile[]): TreeNode[] {
  const byPath = new Map<string, TreeNode>();
  const roots: TreeNode[] = [];

  const nodeFor = (relPath: string, isDir: boolean): TreeNode => {
    let node = byPath.get(relPath);
    if (node) {
      node.isDir ||= isDir;
      return node;
    }
    node = {
      relPath,
      name: relPath.split("/").pop() ?? relPath,
      isDir,
      children: [],
    };
    byPath.set(relPath, node);
    const slash = relPath.lastIndexOf("/");
    if (slash === -1) roots.push(node);
    else nodeFor(relPath.slice(0, slash), true).children.push(node);
    return node;
  };

  for (const f of files) nodeFor(f.rel_path, f.is_dir);

  const sortRec = (nodes: TreeNode[]) => {
    nodes.sort(
      (a, b) =>
        Number(b.isDir) - Number(a.isDir) ||
        a.name.localeCompare(b.name, undefined, { sensitivity: "base" }) ||
        a.name.localeCompare(b.name),
    );
    for (const n of nodes) sortRec(n.children);
  };
  sortRec(roots);
  return roots;
}

/** Rows to render: children appear only under expanded dirs. */
export function visibleNodes(
  tree: TreeNode[],
  expanded: Set<string>,
): { node: TreeNode; depth: number }[] {
  const out: { node: TreeNode; depth: number }[] = [];
  const visit = (nodes: TreeNode[], depth: number) => {
    for (const node of nodes) {
      out.push({ node, depth });
      if (node.isDir && expanded.has(node.relPath)) {
        visit(node.children, depth + 1);
      }
    }
  };
  visit(tree, 0);
  return out;
}

/** Every ancestor dir of `relPath`, shallowest first. */
export function ancestorsOf(relPath: string): string[] {
  const segs = relPath.split("/");
  return segs.slice(0, -1).map((_, i) => segs.slice(0, i + 1).join("/"));
}

/** Drop expansion entries whose dir is gone from the listing. */
export function pruneExpanded(
  expanded: Set<string>,
  files: SketchFile[],
): Set<string> {
  const dirs = new Set(files.filter((f) => f.is_dir).map((f) => f.rel_path));
  return new Set([...expanded].filter((p) => dirs.has(p)));
}

/** Rewrite set entries across a rename: the path itself and everything
 *  under it move from `from` to `to`; siblings sharing a prefix don't. */
export function remapSet(set: Set<string>, from: string, to: string): Set<string> {
  const out = new Set<string>();
  for (const p of set) {
    if (p === from) out.add(to);
    else if (p.startsWith(from + "/")) out.add(to + p.slice(from.length));
    else out.add(p);
  }
  return out;
}
