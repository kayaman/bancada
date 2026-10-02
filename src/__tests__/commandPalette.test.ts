import { describe, expect, it } from "vitest";
import {
  type Command,
  filterCommands,
  fuzzyScore,
  moveIndex,
} from "../commandPalette";

const cmd = (label: string, group = "Go to"): Command => ({
  id: label,
  label,
  group,
  run: () => {},
});

describe("fuzzyScore", () => {
  it("matches subsequences and rejects others", () => {
    expect(fuzzyScore("srl", "Serial")).not.toBeNull();
    expect(fuzzyScore("xyz", "Serial")).toBeNull();
  });
  it("scores empty query as 0", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
  });
  it("prefers tighter matches", () => {
    expect(fuzzyScore("ser", "Serial")!).toBeLessThan(fuzzyScore("ser", "sxxexxr")!);
  });
});

describe("filterCommands", () => {
  const all = [cmd("Build"), cmd("Serial"), cmd("Scope"), cmd("Save", "File")];
  it("returns everything for a blank query", () => {
    expect(filterCommands(all, "  ")).toEqual(all);
  });
  it("filters and ranks", () => {
    expect(filterCommands(all, "sc").map((c) => c.label)[0]).toBe("Scope");
    expect(filterCommands(all, "zzz")).toEqual([]);
  });
  it("matches against the group too", () => {
    expect(filterCommands(all, "file").map((c) => c.label)).toEqual(["Save"]);
  });
});

describe("moveIndex", () => {
  it("wraps both ways", () => {
    expect(moveIndex(0, -1, 3)).toBe(2);
    expect(moveIndex(2, 1, 3)).toBe(0);
  });
  it("is safe on an empty list", () => {
    expect(moveIndex(0, 1, 0)).toBe(0);
  });
});
