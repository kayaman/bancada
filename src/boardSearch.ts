// Filtering and grouping for the board pickers. `board listall` returns
// hundreds of boards; scrolling a <select> is not a way to find one.

import type { BoardOption } from "./api";

/** Boards whose name, FQBN or platform contains every query token. */
export function filterBoards(boards: BoardOption[], query: string): BoardOption[] {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return boards;
  return boards.filter((b) => {
    const hay = `${b.name} ${b.fqbn} ${b.platform_name}`.toLowerCase();
    return tokens.every((t) => hay.includes(t));
  });
}

/** Rows for the open results listbox: matches + their optgroup headers,
 *  clamped to [3, 10] — 3 keeps the "no boards match" state from looking
 *  like a sliver, 10 keeps the overlay from swallowing the dialog. */
export function listboxRows(matchCount: number, groupCount: number): number {
  return Math.min(10, Math.max(3, matchCount + groupCount));
}

/** Boards grouped by platform, in first-seen order — optgroup-ready. */
export function groupByPlatform(boards: BoardOption[]): [string, BoardOption[]][] {
  const g = new Map<string, BoardOption[]>();
  for (const b of boards) {
    const list = g.get(b.platform_name) ?? [];
    list.push(b);
    g.set(b.platform_name, list);
  }
  return [...g.entries()];
}
