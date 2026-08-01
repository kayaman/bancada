import { describe, expect, it } from "vitest";
import {
  BottomGroup,
  BottomTab,
  GROUP_LABEL,
  GROUP_OF,
  GROUP_TABS,
  TAB_LABEL,
  groupHasUnseen,
} from "../bottomTabs";

const ALL_TABS = Object.keys(GROUP_OF) as BottomTab[];
const ALL_GROUPS = Object.keys(GROUP_TABS) as BottomGroup[];

describe("bottomTabs mapping consistency", () => {
  it("every GROUP_TABS member maps back to its group via GROUP_OF", () => {
    for (const g of ALL_GROUPS) {
      for (const t of GROUP_TABS[g]) {
        expect(GROUP_OF[t]).toBe(g);
      }
    }
  });

  it("every BottomTab appears exactly once across GROUP_TABS", () => {
    const flat = ALL_GROUPS.flatMap((g) => GROUP_TABS[g]);
    expect(flat.length).toBe(ALL_TABS.length);
    expect(new Set(flat).size).toBe(flat.length);
    expect([...flat].sort()).toEqual([...ALL_TABS].sort());
  });

  it("labels cover every group and every tab", () => {
    for (const g of ALL_GROUPS) expect(GROUP_LABEL[g]).toBeTruthy();
    for (const t of ALL_TABS) expect(TAB_LABEL[t]).toBeTruthy();
  });

  it("expected grouping (D1)", () => {
    expect(GROUP_TABS.console).toEqual(["build"]);
    expect(GROUP_TABS.debug).toEqual(["serial", "scope"]);
    expect(GROUP_TABS.obs).toEqual(["mqtt", "ws"]);
    expect(GROUP_TABS.assistant).toEqual(["agent"]);
  });
});

describe("groupHasUnseen (D2)", () => {
  it("no unseen flags → no dots anywhere", () => {
    for (const g of ALL_GROUPS) {
      for (const active of ALL_GROUPS) {
        expect(groupHasUnseen(g, active, {})).toBe(false);
      }
    }
  });

  it("dot shows when any tab in an inactive group is unseen", () => {
    expect(groupHasUnseen("debug", "console", { serial: true })).toBe(true);
    expect(groupHasUnseen("debug", "console", { scope: true })).toBe(true);
    expect(groupHasUnseen("debug", "obs", { serial: true, scope: true })).toBe(
      true,
    );
    expect(groupHasUnseen("obs", "console", { ws: true })).toBe(true);
    expect(groupHasUnseen("obs", "debug", { mqtt: true })).toBe(true);
    expect(groupHasUnseen("console", "debug", { build: true })).toBe(true);
  });

  it("the active group never rolls up, even with every tab unseen", () => {
    const all: Partial<Record<BottomTab, boolean>> = {
      build: true,
      serial: true,
      scope: true,
      mqtt: true,
      ws: true,
    };
    for (const g of ALL_GROUPS) {
      expect(groupHasUnseen(g, g, all)).toBe(false);
    }
  });

  it("unseen flags in other groups don't light this group", () => {
    expect(groupHasUnseen("debug", "console", { mqtt: true, build: true })).toBe(
      false,
    );
    expect(groupHasUnseen("obs", "console", { serial: true, scope: true })).toBe(
      false,
    );
  });

  it("false/undefined flags don't count", () => {
    expect(groupHasUnseen("debug", "console", { serial: false })).toBe(false);
    expect(groupHasUnseen("obs", "console", { mqtt: undefined })).toBe(false);
  });
});
