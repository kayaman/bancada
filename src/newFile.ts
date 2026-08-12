// Validation for creating entries inside the open sketch.
//
// The backend commands guard traversal and collisions too — this exists
// for friendlier messages without a round trip.

import type { SketchFile } from "./api";

export type NewFileCheck =
  | { ok: true; relPath: string }
  | { ok: false; reason: string };

/** Validate a new file/folder name typed relative to `parentDir` ("" for
 *  the sketch root). A trailing slash is tolerated — folders are
 *  first-class explorer entries now. */
export function checkNewEntry(
  raw: string,
  parentDir: string,
  existing: SketchFile[],
): NewFileCheck {
  const name = raw.trim().replace(/\/+$/, "");
  if (!name) return { ok: false, reason: "name the entry to create" };
  if (name.startsWith("/"))
    return { ok: false, reason: "use a path inside the project, not an absolute one" };
  const relPath = parentDir ? `${parentDir}/${name}` : name;
  const segments = relPath.split("/");
  if (segments.some((s) => s === ".."))
    return { ok: false, reason: "the path cannot leave the project (..)" };
  if (segments.some((s) => s.trim() === ""))
    return { ok: false, reason: "the path has an empty segment" };
  if (existing.some((f) => f.rel_path === relPath))
    return { ok: false, reason: `${relPath} already exists` };
  return { ok: true, relPath };
}

/** File-only variant used by the editor tab strip's ＋ input. */
export function checkNewFile(raw: string, existing: SketchFile[]): NewFileCheck {
  if (raw.trim().endsWith("/"))
    return { ok: false, reason: "that names a folder — use New Folder in the explorer" };
  return checkNewEntry(raw, "", existing);
}
