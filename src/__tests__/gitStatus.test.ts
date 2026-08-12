import { describe, expect, it } from "vitest";
import {
  flashTaggingNote,
  parentName,
  pillLabel,
  popoverMode,
  suggestedMessage,
  syncDisabledReason,
} from "../gitStatus";
import type { ChangedPath, RepoState } from "../api";

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
  it("is null with no state (no project open — no placeholder pill)", () => {
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
  it("marks a detached HEAD instead of clean/changed", () => {
    expect(pillLabel(root({ detached: true }))).toBe("detached");
    expect(
      pillLabel(root({ detached: true, dirty: [{ path: "a.ino", status: ".M" }] })),
    ).toBe("detached · 1 changed");
    expect(
      pillLabel(root({ detached: true, ahead: 2, behind: 1 })),
    ).toBe("detached ↑2 ↓1");
    expect(
      pillLabel(
        root({
          detached: true,
          dirty: [{ path: "a.ino", status: ".M" }],
          ahead: 2,
        }),
      ),
    ).toBe("detached · 1 changed ↑2");
  });
});

describe("suggestedMessage", () => {
  const path = (p: string): ChangedPath => ({ path: p, status: ".M" });
  it("mirrors core::git::suggested_message", () => {
    expect(suggestedMessage([])).toBe("checkpoint");
    expect(suggestedMessage([path("a.ino")])).toBe("checkpoint: a.ino");
    expect(suggestedMessage([path("a.ino"), path("b.h")])).toBe(
      "checkpoint: a.ino, b.h",
    );
    expect(
      suggestedMessage([path("a.ino"), path("b.h"), path("c.cpp")]),
    ).toBe("checkpoint: a.ino, b.h (+1)");
    expect(
      suggestedMessage([
        path("a.ino"),
        path("b.h"),
        path("c.cpp"),
        path("d.txt"),
      ]),
    ).toBe("checkpoint: a.ino, b.h (+2)");
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

describe("flashTaggingNote", () => {
  it("says nothing at the repo root, where flashes are tagged", () => {
    expect(flashTaggingNote(root())).toBeNull();
    expect(flashTaggingNote(root({ dirty: [{ path: "a.ino", status: ".M" }] }))).toBeNull();
  });
  it("says nothing with no project open", () => {
    expect(flashTaggingNote(null)).toBeNull();
  });
  it("points an untracked project at init", () => {
    const note = flashTaggingNote({ kind: "no_git" });
    expect(note).toMatch(/aren't tagged/);
    expect(note).toMatch(/initialize a repository/i);
  });
  it("names the parent repo for a nested project", () => {
    const note = flashTaggingNote({ kind: "nested", root: "/home/u/Projects", dirty: [] });
    expect(note).toMatch(/aren't tagged/);
    expect(note).toContain("Projects");
  });
});

describe("parentName", () => {
  it("takes the last path segment", () => {
    expect(parentName("/home/u/Projects")).toBe("Projects");
    expect(parentName("/")).toBe("/");
  });
});
