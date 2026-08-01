// Which project files get an editor tab, and how a tab is captioned.
//
// The tab strip exists so the open files stay reachable when the sidebar is
// showing another panel (Boards, Fleet, Libraries) or is collapsed — the
// editor is single-buffer, so without it there is no visible way to switch
// files. Sketches are small, so every file gets a tab rather than only the
// ones opened so far.

import type { SketchFile } from "./api";

/** Every file of the sketch, in tree order; directories carry no tab. */
export function fileTabs(files: SketchFile[]): SketchFile[] {
  return files.filter((f) => !f.is_dir);
}

/** Basename only — the full rel_path belongs in the tooltip, not the strip. */
export function tabLabel(relPath: string): string {
  return relPath.split("/").pop() ?? relPath;
}
