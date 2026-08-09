import { describe, expect, it } from "vitest";
import { parentName, pillLabel, popoverMode, syncDisabledReason } from "../gitStatus";
import type { RepoState } from "../api";

const root = (over: Partial<Extract<RepoState, { kind: "root" }>> = {}): RepoState => ({
  kind: "root",
  branch: "main",
  detached: false,
  dirty: [],
  remote: "git@github.com:m/x.git",
  has_upstream: true,
  ahead: 0,
  behind: 0,
  tracked_secrets: [],
  suggested_message: "checkpoint",
  ...over,
});

describe("pillLabel", () => {
  it("is null with no state (no sketch open — no placeholder pill)", () => {
    expect(pillLabel(null)).toBeNull();
  });
  it("shows clean, changed, and ahead/behind states", () => {
    expect(pillLabel(root())).toBe("✓ clean");
    expect(
      pillLabel(root({ dirty: [{ path: "a.ino", status: ".M" }] })),
    ).toBe("1 changed");
    expect(pillLabel(root({ ahead: 2, behind: 1 }))).toBe("✓ clean ↑2 ↓1");
    expect(pillLabel(root({ ahead: 2 }))).toBe("✓ clean ↑2");
  });
  it("names the other states", () => {
    expect(pillLabel({ kind: "no_git" })).toBe("no git");
    expect(
      pillLabel({ kind: "nested", root: "/home/u/Projects", dirty: [] }),
    ).toBe("tracked by Projects");
  });
});

describe("popoverMode", () => {
  it("routes each state to its popover", () => {
    expect(popoverMode({ kind: "no_git" })).toBe("init");
    expect(popoverMode({ kind: "nested", root: "/p", dirty: [] })).toBe("nested");
    expect(popoverMode(root({ remote: null }))).toBe("setup_remote");
    expect(popoverMode(root())).toBe("actions");
  });
});

describe("syncDisabledReason", () => {
  it("explains a dirty tree and a detached head", () => {
    expect(
      syncDisabledReason(root({ dirty: [{ path: "a", status: ".M" }] })),
    ).toMatch(/commit first/i);
    expect(syncDisabledReason(root({ detached: true }))).toMatch(/detached/i);
    expect(syncDisabledReason(root())).toBeNull();
  });
});

describe("parentName", () => {
  it("takes the last path segment", () => {
    expect(parentName("/home/u/Projects")).toBe("Projects");
    expect(parentName("/")).toBe("/");
  });
});
