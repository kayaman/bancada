// Plain-JS message store for the observability panels (MQTT / WebSocket).
// No React and no wall clock: time is injected via `push(msg.ts)` and
// `tick(now)` so every behavior is deterministic under test.
//
// Consumption pattern (ScopeView decoupling): the channel callback only calls
// `push`; panels poll at ~4 Hz and re-render only when `version` changed.

export interface ObsMsg {
  topic: string;
  payload: string;
  b64: boolean;
  retain: boolean;
  qos: number;
  ts: number;
}

export interface TopicStat {
  /** Total messages ever seen on this topic (monotonic until clear()). */
  count: number;
  /** Last payload seen on this topic. */
  last: string;
  /** Timestamp of the last message on this topic. */
  lastTs: number;
  /** Messages in the trailing 10 s window / 10. */
  ratePerSec: number;
}

const RATE_WINDOW_MS = 10_000;

interface TopicEntry {
  stat: TopicStat;
  /** Timestamps of messages inside the sliding rate window (ascending). */
  window: number[];
}

export class ObsStore {
  private readonly cap: number;
  private msgs: ObsMsg[] = [];
  private topics = new Map<string, TopicEntry>();
  private ver = 0;
  private pausedFlag = false;
  private buffered = 0;
  /** Latest injected time seen (max of push ts / tick now). */
  private lastNow = 0;

  constructor(cap = 500) {
    if (!Number.isInteger(cap) || cap <= 0) {
      throw new Error(`ObsStore: bad capacity ${cap}`);
    }
    this.cap = cap;
  }

  /** Bumps on any visible change (push, pause toggle, rate change, clear). */
  get version(): number {
    return this.ver;
  }

  /**
   * Ingest a message. While paused the message is counted into
   * `bufferedWhilePaused` and NOT appended to the ring (it is dropped, not
   * queued — see setPaused), but the per-topic stats still update. The ring
   * evicts its oldest entry once past capacity.
   */
  push(msg: ObsMsg): void {
    if (msg.ts > this.lastNow) this.lastNow = msg.ts;

    // Per-topic stats update regardless of pause.
    let entry = this.topics.get(msg.topic);
    if (!entry) {
      entry = {
        stat: { count: 0, last: "", lastTs: 0, ratePerSec: 0 },
        window: [],
      };
      this.topics.set(msg.topic, entry);
    }
    entry.stat.count++;
    entry.stat.last = msg.payload;
    entry.stat.lastTs = msg.ts;
    entry.window.push(msg.ts);
    this.pruneWindow(entry, this.lastNow);
    entry.stat.ratePerSec = entry.window.length / (RATE_WINDOW_MS / 1000);

    if (this.pausedFlag) {
      this.buffered++;
    } else {
      this.msgs.push(msg);
      if (this.msgs.length > this.cap) this.msgs.shift();
    }
    this.ver++;
  }

  /**
   * Prune the 10 s sliding rate windows against `now` (defaults to the
   * latest injected time). Bumps `version` only if some rate changed.
   */
  tick(now?: number): void {
    const t = now ?? this.lastNow;
    if (t > this.lastNow) this.lastNow = t;
    let changed = false;
    for (const entry of this.topics.values()) {
      this.pruneWindow(entry, t);
      const rate = entry.window.length / (RATE_WINDOW_MS / 1000);
      if (rate !== entry.stat.ratePerSec) {
        entry.stat.ratePerSec = rate;
        changed = true;
      }
    }
    if (changed) this.ver++;
  }

  /**
   * Read-time view. `filter` is a case-insensitive substring matched against
   * each message's topic OR payload; it narrows `msgs` only. `msgs` are
   * oldest→newest; `topics` always lists every known topic sorted by name.
   */
  snapshot(filter?: string): {
    msgs: ObsMsg[];
    topics: [string, TopicStat][];
    bufferedWhilePaused: number;
    paused: boolean;
  } {
    let msgs = this.msgs.slice();
    if (filter) {
      const f = filter.toLowerCase();
      msgs = msgs.filter(
        (m) =>
          m.topic.toLowerCase().includes(f) ||
          m.payload.toLowerCase().includes(f),
      );
    }
    const topics: [string, TopicStat][] = [...this.topics.entries()]
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
      .map(([name, e]) => [name, { ...e.stat }]);
    return {
      msgs,
      topics,
      bufferedWhilePaused: this.buffered,
      paused: this.pausedFlag,
    };
  }

  /**
   * Pause/resume the feed. Unpausing clears `bufferedWhilePaused`; the
   * messages counted while paused were NOT retained anywhere — the counter
   * only tells the user how many they missed. No-op (no version bump) when
   * the state doesn't change.
   */
  setPaused(p: boolean): void {
    if (p === this.pausedFlag) return;
    this.pausedFlag = p;
    if (!p) this.buffered = 0;
    this.ver++;
  }

  /** Drop all messages, topic stats and the paused-buffer count. */
  clear(): void {
    this.msgs = [];
    this.topics.clear();
    this.buffered = 0;
    this.ver++;
  }

  private pruneWindow(entry: TopicEntry, now: number): void {
    const cutoff = now - RATE_WINDOW_MS;
    let drop = 0;
    const w = entry.window;
    while (drop < w.length && w[drop] <= cutoff) drop++;
    if (drop > 0) w.splice(0, drop);
  }
}
