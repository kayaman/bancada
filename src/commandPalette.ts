// Command palette model: a flat list of commands and a pure fuzzy filter.
// No React; the palette component and App.tsx build the list and render it.

export interface Command {
  id: string;
  label: string;
  /** Grouping shown dimmed beside the label, e.g. "Go to". */
  group: string;
  /** Display-only shortcut hint, e.g. "Ctrl+S". */
  shortcut?: string;
  /** Disabled commands stay listed (discoverable) but cannot run. */
  disabled?: boolean;
  run: () => void;
}

/** Subsequence match score; null when `query` is not a subsequence of `text`.
 *  Lower is better: gaps cost, a prefix or word-start match is cheaper. */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 0;
  let ti = 0;
  let score = 0;
  let last = -1;
  for (const ch of q) {
    const at = t.indexOf(ch, ti);
    if (at < 0) return null;
    const wordStart = at === 0 || t[at - 1] === " ";
    score += last < 0 ? at : at - last - 1;
    if (!wordStart) score += 1;
    last = at;
    ti = at + 1;
  }
  return score;
}

/** Commands matching `query`, best first; stable for equal scores. */
export function filterCommands(commands: Command[], query: string): Command[] {
  const q = query.trim();
  if (!q) return commands;
  return commands
    .map((c, i) => ({ c, i, s: fuzzyScore(q, `${c.group} ${c.label}`) }))
    .filter((x): x is { c: Command; i: number; s: number } => x.s !== null)
    .sort((a, b) => a.s - b.s || a.i - b.i)
    .map((x) => x.c);
}

/** Move the highlighted row by `delta`, wrapping, over `count` rows. */
export function moveIndex(index: number, delta: number, count: number): number {
  if (count <= 0) return 0;
  return (((index + delta) % count) + count) % count;
}
