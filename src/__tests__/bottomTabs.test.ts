import { describe, expect, it } from "vitest";
import {
  BOTTOM_TABS,
  SEPARATOR_AFTER,
  TAB_LABEL,
  tabRow,
  type BottomTab,
} from "../bottomTabs";

const item = (row: ReturnType<typeof tabRow>, tab: BottomTab) => {
  const found = row.find((i) => i.tab === tab);
  if (!found) throw new Error(`no row item for ${tab}`);
  return found;
};

describe("BOTTOM_TABS / TAB_LABEL / SEPARATOR_AFTER", () => {
  it("is exactly the seven tabs, in bench order", () => {
    expect([...BOTTOM_TABS]).toEqual([
      "build",
      "serial",
      "scope",
      "mqtt",
      "ws",
      "web",
      "agent",
    ]);
  });

  it("labels cover every tab", () => {
    for (const t of BOTTOM_TABS) expect(TAB_LABEL[t]).toBeTruthy();
    expect(Object.keys(TAB_LABEL).sort()).toEqual([...BOTTOM_TABS].sort());
  });

  it("separators sit only after serial, scope and web", () => {
    expect([...SEPARATOR_AFTER].sort()).toEqual(["scope", "serial", "web"]);
    for (const t of BOTTOM_TABS) {
      expect(SEPARATOR_AFTER.has(t)).toBe(
        t === "serial" || t === "scope" || t === "web",
      );
    }
  });
});

describe("tabRow", () => {
  it("yields every tab in BOTTOM_TABS order with its label", () => {
    const row = tabRow("build", {});
    expect(row.map((i) => i.tab)).toEqual([...BOTTOM_TABS]);
    expect(row.map((i) => i.label)).toEqual(
      BOTTOM_TABS.map((t) => TAB_LABEL[t]),
    );
  });

  it("marks exactly one item active — the one asked for", () => {
    for (const active of BOTTOM_TABS) {
      const row = tabRow(active, {});
      expect(row.filter((i) => i.active).map((i) => i.tab)).toEqual([active]);
    }
  });

  it("carries separatorAfter from SEPARATOR_AFTER", () => {
    const row = tabRow("build", {});
    expect(row.filter((i) => i.separatorAfter).map((i) => i.tab)).toEqual([
      "serial",
      "scope",
      "web",
    ]);
  });

  it("dots flagged inactive tabs", () => {
    const row = tabRow("build", { serial: true, mqtt: true });
    expect(item(row, "serial").dot).toBe(true);
    expect(item(row, "mqtt").dot).toBe(true);
    expect(item(row, "scope").dot).toBe(false);
  });

  it("never dots the active tab, even when flagged", () => {
    const row = tabRow("serial", { serial: true, scope: true });
    expect(item(row, "serial").dot).toBe(false);
    expect(item(row, "scope").dot).toBe(true);
  });

  it("false/undefined unseen flags do not dot", () => {
    const row = tabRow("build", { serial: false, scope: undefined });
    expect(item(row, "serial").dot).toBe(false);
    expect(item(row, "scope").dot).toBe(false);
  });

  it("badge is null without badges, null at 0, the number above 0", () => {
    expect(item(tabRow("serial", {}), "build").badge).toBe(null);
    expect(item(tabRow("serial", {}, {}), "build").badge).toBe(null);
    expect(item(tabRow("serial", {}, { build: 0 }), "build").badge).toBe(null);
    expect(item(tabRow("serial", {}, { build: 3 }), "build").badge).toBe(3);
  });

  it("badges the active tab too", () => {
    expect(item(tabRow("build", {}, { build: 2 }), "build").badge).toBe(2);
  });

  it("ignores unknown keys in unseen and badges", () => {
    const row = tabRow(
      "build",
      { nope: true } as Partial<Record<BottomTab, boolean>>,
      { nope: 9 } as Partial<Record<BottomTab, number>>,
    );
    expect(row.map((i) => i.tab)).toEqual([...BOTTOM_TABS]);
    expect(row.some((i) => i.dot)).toBe(false);
    expect(row.every((i) => i.badge === null)).toBe(true);
  });
});
