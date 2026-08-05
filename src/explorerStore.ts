// Explorer UI state. Scope is deliberate: tree state only. Buffers, the
// dirty set and the open file stay in App.tsx, and this store never calls
// the api — fs operations are orchestrated by App handlers, which feed the
// refreshed listing back in via setFiles / setFilesAfterRename.

import { create } from "zustand";
import type { SketchFile } from "./api";
import { ancestorsOf, pruneExpanded, remapSet } from "./fileTreeModel";

interface ExplorerState {
  files: SketchFile[];
  expanded: Set<string>;
  /** Tree highlight — independent of the file open in the editor. */
  selected: string | null;
  /** rel_path under inline rename. */
  renaming: string | null;
  creating: { parentDir: string; kind: "file" | "dir" } | null;
  dragging: string | null;
  /** Hovered drop dir; "" means the sketch root. */
  dropTarget: string | null;
  contextMenu: { x: number; y: number; target: string | null } | null;

  setFiles(files: SketchFile[]): void;
  setFilesAfterRename(files: SketchFile[], from: string, to: string): void;
  toggleExpanded(dir: string): void;
  expandTo(relPath: string): void;
  select(relPath: string | null): void;
  startRename(relPath: string): void;
  cancelRename(): void;
  startCreate(parentDir: string, kind: "file" | "dir"): void;
  cancelCreate(): void;
  setDragging(relPath: string | null): void;
  setDropTarget(dir: string | null): void;
  openContextMenu(x: number, y: number, target: string | null): void;
  closeContextMenu(): void;
  reset(): void;
}

const initial = {
  files: [] as SketchFile[],
  expanded: new Set<string>(),
  selected: null as string | null,
  renaming: null as string | null,
  creating: null as ExplorerState["creating"],
  dragging: null as string | null,
  dropTarget: null as string | null,
  contextMenu: null as ExplorerState["contextMenu"],
};

const stillListed = (files: SketchFile[], p: string | null) =>
  p !== null && files.some((f) => f.rel_path === p) ? p : null;

export const useExplorerStore = create<ExplorerState>((set) => ({
  ...initial,

  setFiles: (files) =>
    set((s) => ({
      files,
      expanded: pruneExpanded(s.expanded, files),
      selected: stillListed(files, s.selected),
      renaming: stillListed(files, s.renaming),
    })),

  setFilesAfterRename: (files, from, to) =>
    set((s) => ({
      files,
      expanded: pruneExpanded(remapSet(s.expanded, from, to), files),
      selected: stillListed(
        files,
        s.selected ? [...remapSet(new Set([s.selected]), from, to)][0] : null,
      ),
      renaming: null,
    })),

  toggleExpanded: (dir) =>
    set((s) => {
      const expanded = new Set(s.expanded);
      if (!expanded.delete(dir)) expanded.add(dir);
      return { expanded };
    }),

  expandTo: (relPath) =>
    set((s) => ({ expanded: new Set([...s.expanded, ...ancestorsOf(relPath)]) })),

  select: (relPath) => set({ selected: relPath }),
  startRename: (relPath) => set({ renaming: relPath, contextMenu: null }),
  cancelRename: () => set({ renaming: null }),
  startCreate: (parentDir, kind) =>
    set({ creating: { parentDir, kind }, contextMenu: null }),
  cancelCreate: () => set({ creating: null }),
  setDragging: (relPath) =>
    set(relPath === null ? { dragging: null, dropTarget: null } : { dragging: relPath }),
  setDropTarget: (dir) => set({ dropTarget: dir }),
  openContextMenu: (x, y, target) => set({ contextMenu: { x, y, target } }),
  closeContextMenu: () => set({ contextMenu: null }),
  reset: () => set({ ...initial, expanded: new Set(), }),
}));
