// Pure helpers for explorer operations: pre-validation (friendlier, no
// round trip — core remains the authority) and the path math App.tsx uses
// to keep buffers, dirty flags and the open file coherent across ops.

import type { SketchFile } from "./api";
import type { Check } from "./check";

export type { Check };

/** New path for `p` after renaming `from` → `to`; null when unaffected. */
export function pathAfterRename(
  p: string,
  from: string,
  to: string,
  wasDir: boolean,
): string | null {
  if (p === from) return to;
  if (wasDir && p.startsWith(from + "/")) return to + p.slice(from.length);
  return null;
}

/** Is `p` the deleted entry, or inside a deleted dir? */
export function affectedByDelete(p: string, deleted: string, wasDir: boolean): boolean {
  return p === deleted || (wasDir && p.startsWith(deleted + "/"));
}

/** The rel paths the sketch cannot build without. Mirrors core's
 *  is_protected — keep the two in sync. */
export function protectedPaths(sketchDir: string): Set<string> {
  const base = sketchDir.replace(/\/+$/, "").split("/").pop() ?? "";
  return new Set([`${base}.ino`, "sketch.yaml"]);
}

/** Any listed entry under `dir/`? Blind to SKIP_DIRS contents (build/,
 *  .git/…) — acceptable: deletes are trash-recoverable. */
export function isNonEmptyDir(files: SketchFile[], dir: string): boolean {
  return files.some((f) => f.rel_path.startsWith(dir + "/"));
}

/** Where the explorer header's New File / New Folder buttons create:
 *  the selected dir, a selected file's parent, else the sketch root. */
export function createAnchorDir(
  selected: string | null,
  files: SketchFile[],
): string {
  if (!selected) return "";
  const entry = files.find((f) => f.rel_path === selected);
  if (!entry) return "";
  if (entry.is_dir) return selected;
  return selected.split("/").slice(0, -1).join("/");
}

export function isDescendant(child: string, dir: string): boolean {
  return child.startsWith(dir + "/");
}

/** Frontend pre-validation for rename-as-move. Order mirrors core's guard
 *  block: rule violations before disk state. */
export function checkRename(
  from: string,
  to: string,
  files: SketchFile[],
  sketchDir: string,
): Check {
  const t = to.trim();
  if (!t) return { ok: false, reason: "name the new path" };
  if (t === from) return { ok: false, reason: "the path is unchanged" };
  if (t.startsWith("/"))
    return { ok: false, reason: "use a path inside the project, not an absolute one" };
  const segments = t.split("/");
  if (segments.some((s) => s === ".."))
    return { ok: false, reason: "the path cannot leave the project (..)" };
  if (segments.some((s) => s.trim() === ""))
    return { ok: false, reason: "the path has an empty segment" };
  const prot = protectedPaths(sketchDir);
  if (prot.has(from))
    return { ok: false, reason: `${from} is required by the build and cannot be renamed` };
  if (prot.has(t))
    return { ok: false, reason: `${t} is reserved — the build needs it as-is` };
  if (isDescendant(t, from))
    return { ok: false, reason: `cannot move ${from} inside itself` };
  if (files.some((f) => f.rel_path === t))
    return { ok: false, reason: `${t} already exists` };
  return { ok: true };
}
