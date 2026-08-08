import { describe, expect, it } from "vitest";
import { filterBoards, groupByPlatform, listboxRows } from "../boardSearch";
import type { BoardOption } from "../api";

const b = (fqbn: string, name: string, platform_name: string): BoardOption => ({
  fqbn,
  name,
  platform_id: fqbn.split(":").slice(0, 2).join(":"),
  platform_name,
});

const boards = [
  b("esp32:esp32:esp32s3", "ESP32S3 Dev Module", "esp32"),
  b("esp32:esp32:esp32c3", "ESP32C3 Dev Module", "esp32"),
  b("arduino:avr:uno", "Arduino Uno", "Arduino AVR Boards"),
  b("arduino:avr:nano", "Arduino Nano", "Arduino AVR Boards"),
];

describe("filterBoards", () => {
  it("returns everything for an empty or blank query", () => {
    expect(filterBoards(boards, "")).toEqual(boards);
    expect(filterBoards(boards, "   ")).toEqual(boards);
  });

  it("matches by name, case-insensitively", () => {
    expect(filterBoards(boards, "UNO")).toEqual([boards[2]]);
  });

  it("matches by fqbn segment", () => {
    expect(filterBoards(boards, "esp32s3")).toEqual([boards[0]]);
  });

  it("matches by platform name", () => {
    expect(filterBoards(boards, "AVR")).toEqual([boards[2], boards[3]]);
  });

  it("requires every token to match somewhere (AND)", () => {
    expect(filterBoards(boards, "esp32 s3")).toEqual([boards[0]]);
    expect(filterBoards(boards, "esp32 uno")).toEqual([]);
  });
});

describe("listboxRows", () => {
  it("counts matches plus group headers", () => {
    expect(listboxRows(4, 2)).toBe(6);
  });

  it("never below 3 (room for the no-match row)", () => {
    expect(listboxRows(0, 0)).toBe(3);
    expect(listboxRows(1, 1)).toBe(3);
  });

  it("caps at 10", () => {
    expect(listboxRows(200, 30)).toBe(10);
  });
});

describe("groupByPlatform", () => {
  it("groups in first-seen platform order", () => {
    expect(groupByPlatform(boards)).toEqual([
      ["esp32", [boards[0], boards[1]]],
      ["Arduino AVR Boards", [boards[2], boards[3]]],
    ]);
  });

  it("returns no groups for no boards", () => {
    expect(groupByPlatform([])).toEqual([]);
  });
});
