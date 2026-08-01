// Unified line diff for Edit/Write tool cards in the Assistant panel.
//
// User decision (binding, spec §Decisions #8): unified diffs, full strings
// (no elision), `-` lines then `+` lines per hunk, unchanged lines as
// context. This is a small hand-rolled LCS-based line diff — no new deps.

export type DiffLine =
  | { kind: "ctx"; text: string }
  | { kind: "del"; text: string }
  | { kind: "add"; text: string };

/**
 * Splits on `\n`. An empty string yields zero lines (not one empty line) so
 * diffing against an empty `oldStr` (the Write-tool case) never manufactures
 * a spurious deleted blank line.
 */
function splitLines(s: string): string[] {
  return s === "" ? [] : s.split("\n");
}

/**
 * Classic LCS line diff: a `[oldLines.length+1][newLines.length+1]` DP table
 * of suffix-LCS lengths, then a forward walk that emits a deletion whenever
 * the table favours consuming the old line first — the standard bias that
 * groups `-` lines before the `+` lines of the same hunk.
 */
export function unifiedDiff(oldStr: string, newStr: string): DiffLine[] {
  const oldLines = splitLines(oldStr);
  const newLines = splitLines(newStr);
  const n = oldLines.length;
  const m = newLines.length;

  // dp[i][j] = LCS length of oldLines[i:] and newLines[j:].
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] =
        oldLines[i] === newLines[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (oldLines[i] === newLines[j]) {
      out.push({ kind: "ctx", text: oldLines[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: "del", text: oldLines[i] });
      i++;
    } else {
      out.push({ kind: "add", text: newLines[j] });
      j++;
    }
  }
  while (i < n) {
    out.push({ kind: "del", text: oldLines[i] });
    i++;
  }
  while (j < m) {
    out.push({ kind: "add", text: newLines[j] });
    j++;
  }
  return out;
}

/**
 * Maps a tool_use `input` to a diff, for the two tools that touch file
 * content. `Edit` diffs `old_string` → `new_string`; `Write` has no old
 * content, so its whole `content` renders as pure additions. Any other tool
 * (including `verify`, which takes no arguments) has no diff card.
 */
export function diffForToolInput(name: string, input: unknown): DiffLine[] | null {
  if (!isRecord(input)) return null;
  if (name === "Edit") {
    const oldString = input.old_string;
    const newString = input.new_string;
    if (typeof oldString === "string" && typeof newString === "string") {
      return unifiedDiff(oldString, newString);
    }
    return null;
  }
  if (name === "Write") {
    const content = input.content;
    if (typeof content === "string") {
      return unifiedDiff("", content);
    }
    return null;
  }
  return null;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}
