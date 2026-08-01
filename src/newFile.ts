// Validation for creating a file inside the open sketch.
//
// The backend (`write_sketch_file`) already refuses traversal, but it
// silently overwrites an existing file — the frontend must catch that, and
// give friendlier messages for the rest.

import type { SketchFile } from "./api";

export type NewFileCheck =
  | { ok: true; relPath: string }
  | { ok: false; reason: string };

export function checkNewFile(raw: string, existing: SketchFile[]): NewFileCheck {
  const relPath = raw.trim();
  if (!relPath) return { ok: false, reason: "name the file to create" };
  if (relPath.startsWith("/"))
    return { ok: false, reason: "use a path inside the sketch, not an absolute one" };
  if (relPath.endsWith("/"))
    return { ok: false, reason: "that names a folder — folders appear when a file needs them" };
  const segments = relPath.split("/");
  if (segments.some((s) => s === ".."))
    return { ok: false, reason: "the path cannot leave the sketch (..)" };
  if (segments.some((s) => s.trim() === ""))
    return { ok: false, reason: "the path has an empty segment" };
  if (existing.some((f) => f.rel_path === relPath))
    return { ok: false, reason: `${relPath} already exists` };
  return { ok: true, relPath };
}
