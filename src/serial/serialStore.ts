// Plain-JS line store for the Serial Monitor. No React and no wall clock:
// timestamps arrive with `push`, so every behaviour here is deterministic
// under test — the same contract `ObsStore` keeps for the MQTT/WS feed.
//
// Consumption pattern (Tier 3 polled store, `docs/architecture/frontend.md`
// §2): the `serial://line` listener only calls `push`; the panel polls at
// 10 Hz and re-renders only when `version` changed. A board printing at
// 921600 baud would otherwise schedule a React render per line.

import { hms } from "../timeFormat";

export type SerialStream = "stdout" | "stderr" | "tx" | "info";

export interface SerialEntry {
  /** Monotonic since the last `clear()`; also the React key. */
  seq: number;
  ts: number;
  stream: SerialStream;
  text: string;
}

/** Rows kept before a trim. Chosen to survive a long boot log. */
export const SERIAL_CAP = 5000;
/** Rows kept *after* a trim. The gap is what makes the trim amortised: one
 *  `splice` per 1000 lines instead of a `shift` per line. */
export const SERIAL_TRIM_TO = 4000;

interface Memo {
  version: number;
  filter: string;
  rows: SerialEntry[];
}

export class SerialStore {
  private rows: SerialEntry[] = [];
  private ver = 0;
  private seq = 0;
  private totalCount = 0;
  private droppedCount = 0;
  private pausedFlag = false;
  /** The rows the view is pinned to while paused, or `null` when running.
   *  A *copy*, not a seq watermark: the ring keeps evicting behind a pause,
   *  and a watermark view would empty out the very screen the user paused to
   *  read. Bounded by the cap, so the copy costs at most one ring. */
  private frozen: SerialEntry[] | null = null;
  /** Pushes since the pause began — counted, not derived, because the rows
   *  it counts may already be gone from the ring. */
  private bufferedCount = 0;
  private memo: Memo | null = null;

  constructor(
    private readonly cap = SERIAL_CAP,
    private readonly trimTo = SERIAL_TRIM_TO,
  ) {
    if (!Number.isInteger(cap) || cap <= 0) {
      throw new Error(`SerialStore: bad capacity ${cap}`);
    }
    if (!Number.isInteger(trimTo) || trimTo <= 0 || trimTo > cap) {
      throw new Error(`SerialStore: bad trim target ${trimTo}`);
    }
  }

  /** Bumps on push, on a real pause change, and on clear. */
  get version(): number {
    return this.ver;
  }

  /** Lines ever pushed since the last `clear()`. */
  get total(): number {
    return this.totalCount;
  }

  /** Lines the cap evicted since the last `clear()`. */
  get dropped(): number {
    return this.droppedCount;
  }

  get paused(): boolean {
    return this.pausedFlag;
  }

  /**
   * Ingest one line. Unlike `ObsStore`, a paused monitor still *keeps* its
   * lines — a pause here is "stop the view moving so I can read", not "stop
   * listening"; the board is mid-sentence and those lines are the point.
   */
  push(stream: SerialStream, text: string, ts: number): void {
    this.rows.push({ seq: ++this.seq, ts, stream, text });
    this.totalCount++;
    if (this.pausedFlag) this.bufferedCount++;
    if (this.rows.length > this.cap) {
      const cut = this.rows.length - this.trimTo;
      this.rows.splice(0, cut);
      this.droppedCount += cut;
    }
    this.ver++;
  }

  /** Freeze/unfreeze the view. No-op — and no version bump — when unchanged,
   *  so a component polling `version` does not re-render for nothing. */
  setPaused(p: boolean): void {
    if (p === this.pausedFlag) return;
    this.pausedFlag = p;
    this.frozen = p ? this.rows.slice() : null;
    this.bufferedCount = 0;
    this.ver++;
  }

  /** Drop every row and both counters. The paused flag survives: clearing is
   *  a way to make a frozen screen readable, not a way to resume. */
  clear(): void {
    this.rows = [];
    this.seq = 0;
    this.totalCount = 0;
    this.droppedCount = 0;
    // Clearing a frozen screen empties the frozen screen too, and restarts
    // the buffered count — otherwise Resume would advertise lines the user
    // just threw away.
    if (this.pausedFlag) this.frozen = [];
    this.bufferedCount = 0;
    this.memo = null;
    this.ver++;
  }

  /**
   * Read-time view. While paused it stops at `pauseSeq`, and
   * `bufferedWhilePaused` counts what has arrived behind it.
   *
   * `rows` is memoised per (version, filter): the panel polls ten times a
   * second, and an unstable array reference would defeat every downstream
   * `useMemo` and re-render the whole log on every idle tick.
   */
  snapshot(filter?: string): {
    rows: SerialEntry[];
    /** Rows matching the filter. */
    matched: number;
    /** Rows the view could show with no filter (the frozen window). */
    visibleTotal: number;
    total: number;
    dropped: number;
    paused: boolean;
    bufferedWhilePaused: number;
  } {
    const f = filter ?? "";
    const visible = this.frozen ?? this.rows;

    let rows: SerialEntry[];
    if (this.memo && this.memo.version === this.ver && this.memo.filter === f) {
      rows = this.memo.rows;
    } else {
      const filtered = filterRows(visible, f);
      // `filterRows` hands back its input when the filter is blank, and that
      // input may be the live ring. Copy in that case: a snapshot the next
      // `push` mutates underneath the caller is not a snapshot.
      rows = filtered === this.rows ? this.rows.slice() : filtered;
      this.memo = { version: this.ver, filter: f, rows };
    }

    return {
      rows,
      matched: rows.length,
      visibleTotal: visible.length,
      total: this.totalCount,
      dropped: this.droppedCount,
      paused: this.pausedFlag,
      bufferedWhilePaused: this.pausedFlag ? this.bufferedCount : 0,
    };
  }
}

/** Case-insensitive substring over the line text. A blank filter is no
 *  filter, and returns the input array itself so the memo above stays cheap. */
export function filterRows(rows: SerialEntry[], filter: string): SerialEntry[] {
  const f = filter.trim().toLowerCase();
  if (!f) return rows;
  return rows.filter((r) => r.text.toLowerCase().includes(f));
}

/**
 * The rows as a text file: what is on screen, in screen order, with the same
 * markers the log draws so a saved capture reads like the panel did.
 * `info` rows are already em-dash wrapped when they came from an export
 * round-trip, so wrap only what is not.
 */
export function exportText(
  rows: SerialEntry[],
  opts: { timestamps: boolean },
): string {
  return rows
    .map((r) => {
      let line = r.text;
      if (r.stream === "tx") line = `❯ ${line}`;
      else if (r.stream === "info" && !/^—.*—$/.test(line.trim())) {
        line = `— ${line} —`;
      }
      return opts.timestamps ? `${hms(r.ts)} ${line}` : line;
    })
    .join("\n");
}
