// Driving the CodeMirror view from a compiler diagnostic.
//
// `EditorView` comes from `@uiw/react-codemirror`, which re-exports all of
// `@codemirror/view` — no new dependency, and one import path for the editor.
import { EditorView } from "@uiw/react-codemirror";

/** The slice of `EditorState.doc` this module needs, so the arithmetic can be
 *  tested without a DOM or a real editor. */
export interface DocLike {
  lines: number;
  line(n: number): { from: number; length: number };
}

/**
 * 1-based line/col (gcc's convention) → a 0-based document offset.
 *
 * Every input is clamped rather than trusted: a diagnostic can outlive the
 * edit that invalidated it, and CodeMirror throws on an out-of-range line.
 */
export function posForLineCol(
  doc: DocLike,
  line: number,
  col: number | null,
): number {
  const n = Math.min(Math.max(line, 1), doc.lines);
  const l = doc.line(n);
  if (col === null) return l.from;
  return l.from + Math.min(Math.max(col - 1, 0), l.length);
}

/** Move the cursor to a diagnostic's location, centre it, and take focus. */
export function gotoLine(
  view: EditorView,
  line: number,
  col: number | null,
): void {
  const pos = posForLineCol(view.state.doc, line, col);
  view.dispatch({
    selection: { anchor: pos },
    effects: EditorView.scrollIntoView(pos, { y: "center" }),
  });
  view.focus();
}
