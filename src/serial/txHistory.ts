// Shell-style recall for the Serial Monitor's send box. Immutable: every
// operation returns a new `TxHistory`, so the component can hold it in
// `useState` and React sees a real change.
//
// Modelled on a shell's history rather than a dropdown because that is the
// muscle memory the box competes with — ↑ for "the AT command I just sent".

/** Entries kept. A bench session's worth of commands, not a transcript. */
export const TX_HISTORY_CAP = 50;

export interface TxHistory {
  /** Oldest → newest. */
  items: string[];
  /** Index into `items`; `cursor === items.length` means "not recalling". */
  cursor: number;
  /** What was half-typed when recall started, restored by ↓ past the newest. */
  draft: string;
}

export function emptyHistory(): TxHistory {
  return { items: [], cursor: 0, draft: "" };
}

/** Record a sent line. Blank lines and an immediate repeat are not worth a
 *  slot; a repeat further back is (it is a different point in the session). */
export function pushHistory(h: TxHistory, line: string): TxHistory {
  if (!line.trim()) return h;
  const items = h.items.at(-1) === line ? h.items.slice() : [...h.items, line];
  if (items.length > TX_HISTORY_CAP) items.splice(0, items.length - TX_HISTORY_CAP);
  // Sending always ends a recall: the cursor returns past the newest entry.
  return { items, cursor: items.length, draft: "" };
}

/**
 * Walk the history. `currentInput` is what is in the box right now — it
 * becomes the saved draft on the first ↑, and ↓ past the newest entry puts it
 * back. ↑ at the oldest entry stays there rather than wrapping or clearing.
 */
export function recall(
  h: TxHistory,
  dir: "up" | "down",
  currentInput: string,
): { history: TxHistory; value: string } {
  if (h.items.length === 0) return { history: h, value: currentInput };

  if (dir === "up") {
    const atNewest = h.cursor >= h.items.length;
    const draft = atNewest ? currentInput : h.draft;
    const cursor = Math.max(0, h.cursor - 1);
    return { history: { ...h, cursor, draft }, value: h.items[cursor] };
  }

  // Already past the newest: nothing below to walk to.
  if (h.cursor >= h.items.length) return { history: h, value: currentInput };
  const cursor = h.cursor + 1;
  if (cursor >= h.items.length) {
    return { history: { ...h, cursor: h.items.length }, value: h.draft };
  }
  return { history: { ...h, cursor }, value: h.items[cursor] };
}
