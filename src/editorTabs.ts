// Tab management for the editor: open/close tabs, handle renames and deletes.
// The editor has a tab strip showing the currently open files.

/** Basename only — the full rel_path belongs in the tooltip, not the strip. */
export function tabLabel(relPath: string): string {
  return relPath.split("/").pop() ?? relPath;
}

/** Append rel if absent; never reorders or duplicates. */
export function openTab(tabs: string[], rel: string): string[] {
  if (tabs.includes(rel)) {
    return tabs;
  }
  return [...tabs, rel];
}

export interface CloseResult {
  tabs: string[];
  /** File to activate after the close; null = editor goes empty. */
  nextActive: string | null;
}

/** Remove rel. If rel !== active, active is unchanged. If rel === active,
 *  activate the right neighbor, else the left one, else null. If rel is
 *  not in tabs, return tabs unchanged with nextActive = active. */
export function closeTab(
  tabs: string[],
  rel: string,
  active: string | null
): CloseResult {
  const index = tabs.indexOf(rel);
  if (index === -1) {
    // Tab not found, return unchanged
    return { tabs, nextActive: active };
  }

  const newTabs = tabs.filter((_, i) => i !== index);

  // If closing a non-active tab, keep active
  if (rel !== active) {
    return { tabs: newTabs, nextActive: active };
  }

  // Closing the active tab - pick a neighbor
  if (newTabs.length === 0) {
    // No tabs left
    return { tabs: newTabs, nextActive: null };
  }

  // Right neighbor exists
  if (index < newTabs.length) {
    return { tabs: newTabs, nextActive: newTabs[index] };
  }

  // Right neighbor doesn't exist, use left neighbor (always exists if length > 0)
  return { tabs: newTabs, nextActive: newTabs[newTabs.length - 1] };
}

/** Keep `keep` plus any tab in `dirty` (unsaved buffers are never closed
 *  in bulk). Order preserved. */
export function closeOthers(
  tabs: string[],
  keep: string,
  dirty: ReadonlySet<string>
): string[] {
  return tabs.filter((tab) => tab === keep || dirty.has(tab));
}

/** Close everything except dirty tabs. */
export function closeAll(tabs: string[], dirty: ReadonlySet<string>): string[] {
  return tabs.filter((tab) => dirty.has(tab));
}

/** Rename fixup: a tab equal to `from` becomes `to`; when wasDir, a tab
 *  under `from`/ gets its prefix rewritten to `to`/. */
export function renameTabs(
  tabs: string[],
  from: string,
  to: string,
  wasDir: boolean
): string[] {
  if (!wasDir) {
    // File rename: exact match only
    return tabs.map((tab) => (tab === from ? to : tab));
  }

  // Directory rename: update prefix for tabs under from/
  const prefix = from + "/";
  return tabs.map((tab) => {
    if (tab.startsWith(prefix)) {
      return to + "/" + tab.slice(prefix.length);
    }
    return tab;
  });
}

/** Deletion fixup: drop the tab equal to `rel`; when wasDir, drop tabs under `rel`/. */
export function deleteTabs(
  tabs: string[],
  rel: string,
  wasDir: boolean
): string[] {
  if (!wasDir) {
    // File delete: exact match only
    return tabs.filter((tab) => tab !== rel);
  }

  // Directory delete: remove tabs under rel/
  const prefix = rel + "/";
  return tabs.filter((tab) => !tab.startsWith(prefix));
}
