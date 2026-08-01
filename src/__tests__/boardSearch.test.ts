import { describe, expect, it } from "vitest";
import { filterBoards, groupByPlatform, visibleBoards } from "../boardSearch";
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

describe("visibleBoards", () => {
  it("pins the selected board when the filter would drop it", () => {
    expect(visibleBoards(boards, "uno", "esp32:esp32:esp32s3")).toEqual([
      boards[0],
      boards[2],
    ]);
  });

  it("does not duplicate a selected board that already matches", () => {
    expect(visibleBoards(boards, "uno", "arduino:avr:uno")).toEqual([boards[2]]);
  });

  it("is plain filtering when nothing is selected", () => {
    expect(visibleBoards(boards, "nano", "")).toEqual([boards[3]]);
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
