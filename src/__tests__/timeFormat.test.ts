import { describe, expect, it } from "vitest";
import { fileStamp, hms } from "../timeFormat";

// Both formatters render *local* time, so every expectation is built from the
// same Date the function is given — the assertions hold in any timezone.

const two = (n: number) => String(n).padStart(2, "0");

describe("hms", () => {
  it("renders HH:MM:SS.mmm in local time", () => {
    const d = new Date(2026, 7, 22, 9, 5, 3, 7);
    expect(hms(d.getTime())).toBe(
      `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}.007`,
    );
  });

  it("zero-pads every field", () => {
    const d = new Date(2026, 0, 1, 0, 0, 0, 0);
    expect(hms(d.getTime())).toMatch(/^\d{2}:\d{2}:\d{2}\.\d{3}$/);
    expect(hms(d.getTime()).endsWith(".000")).toBe(true);
  });

  it("keeps three millisecond digits past 99", () => {
    const d = new Date(2026, 7, 22, 12, 0, 0, 999);
    expect(hms(d.getTime())).toBe(
      `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}.999`,
    );
  });
});

describe("fileStamp", () => {
  it("renders YYYYMMDD-HHMMSS in local time", () => {
    const d = new Date(2026, 7, 22, 14, 3, 9);
    expect(fileStamp(d)).toBe(
      `${d.getFullYear()}${two(d.getMonth() + 1)}${two(d.getDate())}-` +
        `${two(d.getHours())}${two(d.getMinutes())}${two(d.getSeconds())}`,
    );
  });

  it("is filename-safe — digits and one dash, nothing else", () => {
    expect(fileStamp(new Date(2026, 0, 2, 3, 4, 5))).toMatch(/^\d{8}-\d{6}$/);
  });
});
