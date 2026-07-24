import { describe, expect, it } from "vitest";
import { parseLine } from "../textParser";

describe("parseLine", () => {
  it("parses label:value pairs", () => {
    expect(parseLine("temp:23.5 hum:60")).toEqual([
      { name: "temp", value: 23.5 },
      { name: "hum", value: 60 },
    ]);
  });

  it("parses bare numbers with positional names", () => {
    expect(parseLine("1 2.5 -3")).toEqual([
      { name: "ch1", value: 1 },
      { name: "ch2", value: 2.5 },
      { name: "ch3", value: -3 },
    ]);
  });

  it("supports comma and tab separators", () => {
    expect(parseLine("10,20\t30")).toEqual([
      { name: "ch1", value: 10 },
      { name: "ch2", value: 20 },
      { name: "ch3", value: 30 },
    ]);
  });

  it("handles negative and exponent float formats", () => {
    expect(parseLine("x:-1.5e-3 y:+2E2 .5")).toEqual([
      { name: "x", value: -0.0015 },
      { name: "y", value: 200 },
      { name: "ch1", value: 0.5 },
    ]);
  });

  it("ignores non-numeric tokens within a line", () => {
    expect(parseLine("hello 42 world:oops t:7")).toEqual([
      { name: "ch1", value: 42 },
      { name: "t", value: 7 },
    ]);
  });

  it("returns [] for garbage lines", () => {
    expect(parseLine("")).toEqual([]);
    expect(parseLine("boot: ready!")).toEqual([]);
    expect(parseLine("ESP-ROM:esp32s3-20210327")).toEqual([]);
  });

  it("mixed labels and bares: bares are numbered independently", () => {
    expect(parseLine("a:1 5 b:2 6")).toEqual([
      { name: "a", value: 1 },
      { name: "ch1", value: 5 },
      { name: "b", value: 2 },
      { name: "ch2", value: 6 },
    ]);
  });
});
