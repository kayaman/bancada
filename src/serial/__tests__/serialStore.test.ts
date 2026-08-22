import { describe, expect, it } from "vitest";
import {
  SERIAL_CAP,
  SERIAL_TRIM_TO,
  SerialStore,
  exportText,
  filterRows,
  type SerialEntry,
} from "../serialStore";

const ts = 1_700_000_000_000;

/** Push `n` numbered stdout lines, one ms apart. */
function fill(store: SerialStore, n: number, from = 1): void {
  for (let i = 0; i < n; i++) store.push("stdout", `line ${from + i}`, ts + i);
}

describe("SerialStore ordering", () => {
  it("keeps rows oldest→newest with monotonic seq", () => {
    const s = new SerialStore();
    s.push("stdout", "a", ts);
    s.push("stderr", "b", ts + 1);
    s.push("tx", "c", ts + 2);
    const snap = s.snapshot();
    expect(snap.rows.map((r) => r.text)).toEqual(["a", "b", "c"]);
    expect(snap.rows.map((r) => r.seq)).toEqual([1, 2, 3]);
    expect(snap.rows.map((r) => r.stream)).toEqual(["stdout", "stderr", "tx"]);
    expect(snap.total).toBe(3);
    expect(snap.dropped).toBe(0);
  });

  it("bumps version on every push", () => {
    const s = new SerialStore();
    const v0 = s.version;
    s.push("stdout", "a", ts);
    expect(s.version).toBeGreaterThan(v0);
  });

  it("ships the production cap and trim target", () => {
    expect(SERIAL_CAP).toBe(5000);
    expect(SERIAL_TRIM_TO).toBe(4000);
  });
});

describe("SerialStore cap", () => {
  it("trims in one amortised splice, counting what it dropped", () => {
    const s = new SerialStore(5, 3);
    fill(s, 5);
    // At the cap, nothing has gone yet.
    expect(s.snapshot().rows).toHaveLength(5);
    expect(s.dropped).toBe(0);

    s.push("stdout", "line 6", ts + 5);
    // Past the cap: trim down to 3, dropping the three oldest.
    const snap = s.snapshot();
    expect(snap.rows.map((r) => r.text)).toEqual(["line 4", "line 5", "line 6"]);
    expect(snap.dropped).toBe(3);
    expect(snap.total).toBe(6);
    // seq keeps counting from the beginning of time, not from the window.
    expect(snap.rows.map((r) => r.seq)).toEqual([4, 5, 6]);
  });

  it("survives eviction while paused", () => {
    const s = new SerialStore(5, 3);
    fill(s, 3);
    s.setPaused(true);
    fill(s, 10, 4);
    const snap = s.snapshot();
    // The frozen view can only show rows still in the ring.
    expect(snap.paused).toBe(true);
    expect(snap.rows.every((r) => r.seq <= 3)).toBe(true);
    expect(snap.bufferedWhilePaused).toBe(10);
  });
});

describe("SerialStore pause", () => {
  it("freezes the view and counts what arrived behind it", () => {
    const s = new SerialStore();
    fill(s, 2);
    s.setPaused(true);
    fill(s, 3, 3);
    const snap = s.snapshot();
    expect(snap.rows.map((r) => r.text)).toEqual(["line 1", "line 2"]);
    expect(snap.bufferedWhilePaused).toBe(3);
    // Total keeps counting: the lines are kept, just not shown.
    expect(snap.total).toBe(5);
  });

  it("resume unfreezes and zeroes the buffered count", () => {
    const s = new SerialStore();
    fill(s, 2);
    s.setPaused(true);
    fill(s, 3, 3);
    s.setPaused(false);
    const snap = s.snapshot();
    expect(snap.rows).toHaveLength(5);
    expect(snap.bufferedWhilePaused).toBe(0);
    expect(snap.paused).toBe(false);
  });

  it("bumps version once for a real change and not at all for a repeat", () => {
    const s = new SerialStore();
    const v0 = s.version;
    s.setPaused(true);
    const v1 = s.version;
    expect(v1).toBe(v0 + 1);
    s.setPaused(true);
    expect(s.version).toBe(v1);
  });
});

describe("SerialStore clear", () => {
  it("drops rows and counters but keeps the paused flag", () => {
    const s = new SerialStore();
    fill(s, 3);
    s.setPaused(true);
    fill(s, 2, 4);
    s.clear();
    const snap = s.snapshot();
    expect(snap.rows).toEqual([]);
    expect(snap.total).toBe(0);
    expect(snap.dropped).toBe(0);
    expect(snap.bufferedWhilePaused).toBe(0);
    expect(snap.paused).toBe(true);
  });

  it("bumps version", () => {
    const s = new SerialStore();
    fill(s, 1);
    const v = s.version;
    s.clear();
    expect(s.version).toBeGreaterThan(v);
  });
});

describe("SerialStore filtering", () => {
  it("narrows rows and reports both counts", () => {
    const s = new SerialStore();
    s.push("stdout", "temp=21", ts);
    s.push("stdout", "hum=40", ts + 1);
    s.push("stdout", "TEMP=22", ts + 2);
    const snap = s.snapshot("temp");
    expect(snap.rows.map((r) => r.text)).toEqual(["temp=21", "TEMP=22"]);
    expect(snap.matched).toBe(2);
    expect(snap.visibleTotal).toBe(3);
    expect(snap.total).toBe(3);
  });

  it("filters over the frozen view, not the live one", () => {
    const s = new SerialStore();
    s.push("stdout", "temp=1", ts);
    s.setPaused(true);
    s.push("stdout", "temp=2", ts + 1);
    const snap = s.snapshot("temp");
    expect(snap.rows.map((r) => r.text)).toEqual(["temp=1"]);
    expect(snap.matched).toBe(1);
    expect(snap.visibleTotal).toBe(1);
  });
});

describe("SerialStore memoisation", () => {
  it("returns the same rows array for the same version and filter", () => {
    const s = new SerialStore();
    fill(s, 3);
    const a = s.snapshot();
    const b = s.snapshot();
    expect(b.rows).toBe(a.rows);
  });

  it("invalidates on a push", () => {
    const s = new SerialStore();
    fill(s, 3);
    const a = s.snapshot();
    s.push("stdout", "line 4", ts + 3);
    expect(s.snapshot().rows).not.toBe(a.rows);
  });

  it("invalidates on a filter change", () => {
    const s = new SerialStore();
    s.push("stdout", "temp", ts);
    const a = s.snapshot();
    const b = s.snapshot("temp");
    expect(b.rows).not.toBe(a.rows);
    expect(s.snapshot("temp").rows).toBe(b.rows);
  });
});

describe("filterRows", () => {
  const rows: SerialEntry[] = [
    { seq: 1, ts, stream: "stdout", text: "Alpha" },
    { seq: 2, ts, stream: "stdout", text: "beta" },
  ];

  it("matches case-insensitively", () => {
    expect(filterRows(rows, "ALPHA").map((r) => r.seq)).toEqual([1]);
  });

  it("treats an empty or whitespace filter as no filter", () => {
    expect(filterRows(rows, "")).toEqual(rows);
    expect(filterRows(rows, "   ")).toEqual(rows);
  });
});

describe("exportText", () => {
  const rows: SerialEntry[] = [
    { seq: 1, ts, stream: "stdout", text: "hello" },
    { seq: 2, ts, stream: "tx", text: "AT" },
    { seq: 3, ts, stream: "info", text: "monitor started" },
    { seq: 4, ts, stream: "info", text: "— already wrapped —" },
  ];

  it("marks tx and info rows, without timestamps", () => {
    expect(exportText(rows, { timestamps: false })).toBe(
      ["hello", "❯ AT", "— monitor started —", "— already wrapped —"].join("\n"),
    );
  });

  it("prefixes every line with hms when asked", () => {
    const out = exportText(rows, { timestamps: true }).split("\n");
    expect(out).toHaveLength(4);
    for (const line of out) expect(line).toMatch(/^\d{2}:\d{2}:\d{2}\.\d{3} /);
    expect(out[1].endsWith("❯ AT")).toBe(true);
  });

  it("renders an empty selection as an empty string", () => {
    expect(exportText([], { timestamps: true })).toBe("");
  });
});
