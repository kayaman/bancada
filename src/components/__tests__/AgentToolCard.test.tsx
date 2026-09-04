// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MessageView } from "../AgentPanel";
import type { AgentMessage } from "../../agent/agentStore";

afterEach(cleanup);

function toolMsg(over: Partial<Extract<AgentMessage, { kind: "tool" }>> = {}) {
  return {
    kind: "tool" as const,
    id: "t1",
    name: "mcp__espressif-docs__search_espressif_sources",
    input: { query: "I2C pull-ups on ESP32-C6", language: "en" },
    status: "ok" as const,
    result: "chunk 1\nchunk 2",
    ...over,
  };
}

function show(msg: AgentMessage) {
  render(<MessageView msg={msg} openBottomTab={vi.fn()} onOpenTurn={vi.fn()} />);
}

describe("MCP tool cards in the Assistant log", () => {
  it("names the server, the tool and the query without being expanded", () => {
    // The whole point: five consecutive doc searches must be tellable apart
    // at a glance, which they are not if the query hides inside the JSON.
    show(toolMsg());
    expect(screen.getByText("espressif-docs")).toBeTruthy();
    expect(screen.getByText("search_espressif_sources")).toBeTruthy();
    expect(screen.getByText("I2C pull-ups on ESP32-C6")).toBeTruthy();
  });

  it("does not print the raw mcp__ wire name", () => {
    show(toolMsg());
    expect(
      screen.queryByText(/mcp__espressif-docs__/),
    ).toBeNull();
  });

  it("distinguishes running, ok and error", () => {
    const { rerender } = render(
      <MessageView msg={toolMsg({ status: "running" })} openBottomTab={vi.fn()} onOpenTurn={vi.fn()} />,
    );
    expect(screen.getByText("⟳")).toBeTruthy();
    rerender(<MessageView msg={toolMsg({ status: "error" })} openBottomTab={vi.fn()} onOpenTurn={vi.fn()} />);
    expect(screen.getByText("✗")).toBeTruthy();
    rerender(<MessageView msg={toolMsg({ status: "ok" })} openBottomTab={vi.fn()} onOpenTurn={vi.fn()} />);
    expect(screen.getByText("✓")).toBeTruthy();
  });

  it("renders a no-argument MCP tool without an empty subject", () => {
    // board_pinout with no gpio takes {} — a subject line reading "{}" would
    // be worse than none. The expanded JSON still shows it, which is correct;
    // what must not exist is the summary's subject span.
    const { container } = render(
      <MessageView
        msg={toolMsg({ name: "mcp__bancada__board_pinout", input: {} })}
        openBottomTab={vi.fn()}
        onOpenTurn={vi.fn()}
      />,
    );
    expect(screen.getByText("bancada")).toBeTruthy();
    expect(screen.getByText("board_pinout")).toBeTruthy();
    expect(container.querySelector(".agent-mcp-subject")).toBeNull();
  });

  it("does render a subject span when there is one", () => {
    const { container } = render(
      <MessageView msg={toolMsg()} openBottomTab={vi.fn()} onOpenTurn={vi.fn()} />,
    );
    expect(container.querySelector(".agent-mcp-subject")?.textContent).toBe(
      "I2C pull-ups on ESP32-C6",
    );
  });

  it("leaves bancada's own specific cards alone", () => {
    // verify has its own card and must not be swallowed by the MCP branch.
    show(toolMsg({ name: "mcp__bancada__verify", input: {}, result: "success: true\nexit_code: 0\n\nok" }));
    expect(screen.getByText(/Verify passed/)).toBeTruthy();
    expect(screen.queryByText("verify")).toBeNull();
  });

  it("still shows a built-in through the generic card", () => {
    show(toolMsg({ name: "Grep", input: { pattern: "TODO" } }));
    expect(screen.getByText(/Grep\(pattern: TODO\)/)).toBeTruthy();
  });
});
