import { describe, expect, it } from "vitest";
import {
  BOTTOM_TABS,
  SEPARATOR_AFTER,
  TAB_DIVISION,
  TAB_LABEL,
  tabRow,
  type BottomTab,
  type SideDivision,
} from "../bottomTabs";

const item = (row: ReturnType<typeof tabRow>, tab: BottomTab) => {
  const found = row.find((i) => i.tab === tab);
  if (!found) throw new Error(`no row item for ${tab}`);
  return found;
};

describe("BOTTOM_TABS / TAB_LABEL / SEPARATOR_AFTER / TAB_DIVISION", () => {
  it("is exactly the nine tabs, Assistant first, in bench order", () => {
    expect([...BOTTOM_TABS]).toEqual([
      "agent",
      "build",
      "serial",
      "scope",
      "mqtt",
      "ws",
      "web",
      "bom",
      "diagram",
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

  it("divisions cover every tab — Assistant global, Build software, everything else hardware", () => {
    expect(Object.keys(TAB_DIVISION).sort()).toEqual([...BOTTOM_TABS].sort());
    expect(TAB_DIVISION.agent).toBe("global");
    expect(TAB_DIVISION.build).toBe("software");
    for (const t of ["serial", "scope", "mqtt", "ws", "web", "bom", "diagram"] as const) {
      expect(TAB_DIVISION[t]).toBe("hardware");
    }
  });
});

describe("tabRow", () => {
  it("yields every tab in BOTTOM_TABS order with its label, when asked for every division", () => {
    for (const division of ["software", "hardware", "enclosure"] as SideDivision[]) {
      const row = tabRow("agent", {}, division);
      for (const i of row) expect(i.label).toBe(TAB_LABEL[i.tab]);
    }
  });

  it("scopes to global + the active division — Software sees only Assistant and Build", () => {
    const row = tabRow("agent", {}, "software");
    expect(row.map((i) => i.tab)).toEqual(["agent", "build"]);
  });

  it("scopes to global + the active division — Hardware sees Assistant plus every live/device tool", () => {
    const row = tabRow("agent", {}, "hardware");
    expect(row.map((i) => i.tab)).toEqual([
      "agent",
      "serial",
      "scope",
      "mqtt",
      "ws",
      "web",
      "bom",
      "diagram",
    ]);
  });

  it("scopes to global + the active division — Enclosure sees only Assistant (it has no tools of its own)", () => {
    const row = tabRow("agent", {}, "enclosure");
    expect(row.map((i) => i.tab)).toEqual(["agent"]);
  });

  it("marks exactly one item active — the one asked for, when it's visible in the division", () => {
    for (const active of BOTTOM_TABS) {
      const division = TAB_DIVISION[active] === "global" ? "hardware" : TAB_DIVISION[active];
      const row = tabRow(active, {}, division as SideDivision);
      expect(row.filter((i) => i.active).map((i) => i.tab)).toEqual([active]);
    }
  });

  it("carries separatorAfter from SEPARATOR_AFTER", () => {
    const row = tabRow("agent", {}, "hardware");
    expect(row.filter((i) => i.separatorAfter).map((i) => i.tab)).toEqual([
      "serial",
      "scope",
      "web",
    ]);
  });

  it("dots flagged inactive tabs", () => {
    const row = tabRow("build", { serial: true, mqtt: true }, "hardware");
    expect(item(row, "serial").dot).toBe(true);
    expect(item(row, "mqtt").dot).toBe(true);
    expect(item(row, "scope").dot).toBe(false);
  });

  it("never dots the active tab, even when flagged", () => {
    const row = tabRow("serial", { serial: true, scope: true }, "hardware");
    expect(item(row, "serial").dot).toBe(false);
    expect(item(row, "scope").dot).toBe(true);
  });

  it("false/undefined unseen flags do not dot", () => {
    const row = tabRow("build", { serial: false, scope: undefined }, "hardware");
    expect(item(row, "serial").dot).toBe(false);
    expect(item(row, "scope").dot).toBe(false);
  });

  it("badge is null without badges, null at 0, the number above 0", () => {
    expect(item(tabRow("build", {}, "software"), "build").badge).toBe(null);
    expect(item(tabRow("build", {}, "software", {}), "build").badge).toBe(null);
    expect(item(tabRow("build", {}, "software", { build: 0 }), "build").badge).toBe(null);
    expect(item(tabRow("build", {}, "software", { build: 3 }), "build").badge).toBe(3);
  });

  it("badges the active tab too", () => {
    expect(item(tabRow("build", {}, "software", { build: 2 }), "build").badge).toBe(2);
  });

  it("ignores unknown keys in unseen and badges", () => {
    const row = tabRow(
      "build",
      { nope: true } as Partial<Record<BottomTab, boolean>>,
      "software",
      { nope: 9 } as Partial<Record<BottomTab, number>>,
    );
    expect(row.map((i) => i.tab)).toEqual(["agent", "build"]);
    expect(row.some((i) => i.dot)).toBe(false);
    expect(row.every((i) => i.badge === null)).toBe(true);
  });
});
