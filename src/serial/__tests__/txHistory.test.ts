import { describe, expect, it } from "vitest";
import {
  TX_HISTORY_CAP,
  emptyHistory,
  pushHistory,
  recall,
} from "../txHistory";

describe("pushHistory", () => {
  it("appends, newest last, and leaves the cursor past the end", () => {
    const h = pushHistory(pushHistory(emptyHistory(), "a"), "b");
    expect(h.items).toEqual(["a", "b"]);
    expect(h.cursor).toBe(h.items.length);
  });

  it("skips empty and whitespace-only lines", () => {
    let h = emptyHistory();
    h = pushHistory(h, "");
    h = pushHistory(h, "   ");
    expect(h.items).toEqual([]);
  });

  it("dedupes only consecutive repeats", () => {
    let h = emptyHistory();
    h = pushHistory(h, "AT");
    h = pushHistory(h, "AT");
    expect(h.items).toEqual(["AT"]);
    h = pushHistory(h, "AT+GMR");
    h = pushHistory(h, "AT");
    expect(h.items).toEqual(["AT", "AT+GMR", "AT"]);
  });

  it("caps at the newest TX_HISTORY_CAP entries", () => {
    let h = emptyHistory();
    for (let i = 0; i < TX_HISTORY_CAP + 10; i++) h = pushHistory(h, `cmd${i}`);
    expect(h.items).toHaveLength(TX_HISTORY_CAP);
    expect(h.items[0]).toBe("cmd10");
    expect(h.items.at(-1)).toBe(`cmd${TX_HISTORY_CAP + 9}`);
  });

  it("resets a recall in progress", () => {
    let h = pushHistory(pushHistory(emptyHistory(), "a"), "b");
    h = recall(h, "up", "").history;
    expect(h.cursor).toBeLessThan(h.items.length);
    h = pushHistory(h, "c");
    expect(h.cursor).toBe(h.items.length);
  });

  it("does not mutate the input", () => {
    const h = emptyHistory();
    pushHistory(h, "a");
    expect(h.items).toEqual([]);
  });
});

describe("recall", () => {
  const seeded = () =>
    ["one", "two", "three"].reduce(pushHistory, emptyHistory());

  it("walks backwards from the newest", () => {
    let r = recall(seeded(), "up", "");
    expect(r.value).toBe("three");
    r = recall(r.history, "up", r.value);
    expect(r.value).toBe("two");
    r = recall(r.history, "up", r.value);
    expect(r.value).toBe("one");
  });

  it("stays on the oldest entry at the top", () => {
    let h = seeded();
    for (let i = 0; i < 5; i++) h = recall(h, "up", "").history;
    expect(recall(h, "up", "one").value).toBe("one");
  });

  it("saves the draft on the first ↑ and restores it past the newest", () => {
    let r = recall(seeded(), "up", "half-typed");
    expect(r.value).toBe("three");
    r = recall(r.history, "down", r.value);
    expect(r.value).toBe("half-typed");
    expect(r.history.cursor).toBe(r.history.items.length);
  });

  it("↓ while not recalling is a no-op that keeps the input", () => {
    const r = recall(seeded(), "down", "typing");
    expect(r.value).toBe("typing");
    expect(r.history.cursor).toBe(r.history.items.length);
  });

  it("on an empty history keeps the current input", () => {
    expect(recall(emptyHistory(), "up", "typing").value).toBe("typing");
    expect(recall(emptyHistory(), "down", "typing").value).toBe("typing");
  });
});
