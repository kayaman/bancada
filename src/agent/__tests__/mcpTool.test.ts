import { describe, expect, it } from "vitest";
import { mcpCallSubject, parseMcpName, shortToolName } from "../mcpTool";

describe("parseMcpName", () => {
  it("splits a server from its tool", () => {
    expect(parseMcpName("mcp__espressif-docs__search_espressif_sources")).toEqual({
      server: "espressif-docs",
      tool: "search_espressif_sources",
    });
  });

  it("keeps underscores inside a tool name", () => {
    // `search_espressif_sources` is one name, not three segments. Only the
    // DOUBLE underscore is a separator.
    expect(parseMcpName("mcp__bancada__serial_read")?.tool).toBe("serial_read");
  });

  it("rejoins a tool name that itself contains a double underscore", () => {
    // Unlikely but not forbidden; losing the tail would name the wrong tool.
    expect(parseMcpName("mcp__srv__odd__name")).toEqual({
      server: "srv",
      tool: "odd__name",
    });
  });

  it("is null for anything that is not an MCP tool", () => {
    for (const n of ["Read", "Edit", "WebFetch", "mcp__onlyserver", "", "mcp__"]) {
      expect(parseMcpName(n), n).toBeNull();
    }
  });
});

describe("mcpCallSubject", () => {
  it("prefers the query, which is what a docs search is about", () => {
    expect(
      mcpCallSubject({ query: "I2C pull-ups on ESP32-C6", language: "en" }),
    ).toBe("I2C pull-ups on ESP32-C6");
  });

  it("falls back to another meaningful field before giving up", () => {
    expect(mcpCallSubject({ gpio: 8 })).toBe("gpio: 8");
    expect(mcpCallSubject({ url: "https://x/y" })).toBe("https://x/y");
  });

  it("is empty for a tool that takes no arguments", () => {
    // `verify` and `upload` take none by design — an empty subject renders as
    // no subject line rather than as "{}".
    expect(mcpCallSubject({})).toBe("");
    expect(mcpCallSubject(null)).toBe("");
    expect(mcpCallSubject("nonsense")).toBe("");
  });
});

describe("shortToolName", () => {
  it("drops the mcp prefix so the footer can name the tool", () => {
    expect(shortToolName("mcp__espressif-docs__search_espressif_sources")).toBe(
      "search_espressif_sources",
    );
  });

  it("leaves a built-in alone", () => {
    expect(shortToolName("Read")).toBe("Read");
  });
});
