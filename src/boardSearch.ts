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

/**
 * The filtered list, with the selected board pinned when the filter would
 * drop it — a <select> whose value has no matching <option> silently shows
 * the wrong entry.
 */
export function visibleBoards(
  boards: BoardOption[],
  query: string,
  selectedFqbn: string,
): BoardOption[] {
  const shown = filterBoards(boards, query);
  if (!selectedFqbn || shown.some((b) => b.fqbn === selectedFqbn)) return shown;
  const selected = boards.find((b) => b.fqbn === selectedFqbn);
  return selected ? [selected, ...shown] : shown;
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
