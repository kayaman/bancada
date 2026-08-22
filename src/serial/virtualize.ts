// Windowing arithmetic for the serial log. Pure, so the awkward parts — the
// clamps, and the "nothing has been measured yet" case — are testable
// without a layout engine.
//
// Why at all: the log holds up to 5000 rows, and a board at 921600 baud
// rewrites them faster than the browser can lay out that many nodes. Only
// what the viewport can show (plus a little) reaches the DOM; a spacer of the
// full height keeps the scrollbar honest.

/** Row height in px — must match `--serial-row-h` in `styles.css`. Rows are a
 *  fixed height precisely so this stays arithmetic instead of measurement. */
export const ROW_HEIGHT = 18;

/** Rows rendered beyond each edge, so a fast scroll does not flash blank. */
export const OVERSCAN = 8;

export function visibleRange(
  scrollTop: number,
  viewportHeight: number,
  rowCount: number,
  rowHeight = ROW_HEIGHT,
  overscan = OVERSCAN,
): { start: number; end: number; offsetY: number; totalHeight: number } {
  const totalHeight = rowCount * rowHeight;
  if (rowCount <= 0) return { start: 0, end: 0, offsetY: 0, totalHeight: 0 };

  // Before the first paint (and under jsdom) the container reports height 0.
  // Rendering nothing there would leave the panel blank until something
  // resized it, so show a token window from the top instead.
  if (viewportHeight <= 0) {
    return {
      start: 0,
      end: Math.min(rowCount, 2 * overscan),
      offsetY: 0,
      totalHeight,
    };
  }

  const first = Math.floor(Math.max(0, scrollTop) / rowHeight);
  const onScreen = Math.ceil(viewportHeight / rowHeight);
  const start = Math.max(0, Math.min(first - overscan, rowCount));
  const end = Math.min(rowCount, first + onScreen + overscan);
  return { start, end, offsetY: start * rowHeight, totalHeight };
}

/** Is the scroll position close enough to the bottom that autoscroll should
 *  stay on? `slack` is the tolerance for a wheel notch or a fractional
 *  `scrollTop` — without it, autoscroll switches itself off on its own writes. */
export function isNearBottom(
  scrollTop: number,
  viewportHeight: number,
  totalHeight: number,
  slack = 40,
): boolean {
  return scrollTop + viewportHeight >= totalHeight - slack;
}
