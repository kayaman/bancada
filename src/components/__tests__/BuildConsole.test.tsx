// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import BuildConsole from "../BuildConsole";
import { parseBuildOutput } from "../../diagnostics";
import type { OutputLine } from "../../api";
import { AVR_ERRORS, GIT_SYNC_NOISE } from "../../__tests__/fixtures/buildOutput";

afterEach(cleanup);

const noop = () => {};

/** One sketch-local error and one warning from deep inside the esp32 core —
 *  the two cases that must render differently. */
const MIXED: readonly OutputLine[] = [
  {
    stream: "stderr",
    line: "/home/x/Blink/Blink.ino:3:1: error: 'foo' was not declared in this scope",
  },
  {
    stream: "stderr",
    line: "/home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11/cores/esp32/HardwareSerial.h:49:5: warning: unused parameter 'x' [-Wunused-parameter]",
  },
];

const rows = (container: HTMLElement) =>
  Array.from(container.querySelectorAll(".console-row"));

const renderMixed = (over: Partial<Parameters<typeof BuildConsole>[0]> = {}) =>
  render(
    <BuildConsole
      model={parseBuildOutput(MIXED)}
      sketchDir="/home/x/Blink"
      onJump={noop}
      onClear={noop}
      {...over}
    />,
  );

describe("BuildConsole", () => {
  it("renders one row per line of build output", () => {
    const { container } = render(
      <BuildConsole
        model={parseBuildOutput(AVR_ERRORS)}
        sketchDir={null}
        onJump={noop}
        onClear={noop}
      />,
    );
    expect(rows(container)).toHaveLength(AVR_ERRORS.length);
  });

  it("makes a sketch-local diagnostic a real button that jumps", () => {
    const onJump = vi.fn();
    const { container } = renderMixed({ onJump });

    const first = rows(container)[0];
    expect(first.tagName).toBe("BUTTON");
    expect(first.className).toContain("diag");
    expect(first.className).toContain("error");
    expect(first.className).toContain("clickable");
    expect(first.getAttribute("title")).toBe("/home/x/Blink/Blink.ino");
    expect(first.textContent).toBe(
      "Blink.ino:3:1: error: 'foo' was not declared in this scope",
    );

    fireEvent.click(first);
    expect(onJump).toHaveBeenCalledWith({ rel: "Blink.ino", line: 3, col: 1 });
  });

  it("leaves a toolchain diagnostic unclickable and shortens its path", () => {
    const { container } = renderMixed();

    const second = rows(container)[1];
    expect(second.tagName).toBe("DIV");
    expect(second.className).not.toContain("clickable");
    expect(second.getAttribute("title")).toBe(
      "/home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11/cores/esp32/HardwareSerial.h",
    );
    expect(second.textContent).toBe(
      "esp32:esp32@3.3.11/cores/esp32/HardwareSerial.h:49:5: warning: unused parameter 'x' [-Wunused-parameter]",
    );
  });

  it("refuses to jump to a file the editor does not have open", () => {
    const { container } = renderMixed({ knownFiles: new Set(["other.cpp"]) });
    expect(rows(container)[0].tagName).toBe("DIV");
  });

  it("announces the summary in a live region", () => {
    renderMixed();
    const strip = screen.getByRole("status");
    expect(strip.textContent).toContain("✗ 1 error · 1 warning");
    expect(strip.className).toContain("build-summary");
    expect(strip.className).toContain("error");
    expect(strip.getAttribute("aria-live")).toBe("polite");
  });

  it("filters down to errors when the toggle is pressed", () => {
    const { container } = renderMixed();
    const toggle = screen.getByRole("button", { name: "errors only" });

    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    expect(rows(container)).toHaveLength(2);

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(toggle.className).toContain("toggled");
    expect(rows(container)).toHaveLength(1);
    expect(rows(container)[0].className).toContain("error");
  });

  it("releases the filter when the next build has no errors left", () => {
    // Otherwise fixing the code leaves an almost-empty console behind a
    // toggle that is now disabled, so it cannot be un-pressed.
    const { container, rerender } = renderMixed();
    fireEvent.click(screen.getByRole("button", { name: "errors only" }));
    expect(rows(container)).toHaveLength(1);

    rerender(
      <BuildConsole
        model={parseBuildOutput(GIT_SYNC_NOISE)}
        sketchDir="/home/x/Blink"
        onJump={noop}
        onClear={noop}
      />,
    );
    expect(
      screen.getByRole("button", { name: "errors only" }).getAttribute("aria-pressed"),
    ).toBe("false");
    expect(rows(container)).toHaveLength(GIT_SYNC_NOISE.length);
  });

  it("disables the toggle when there is nothing to filter to", () => {
    render(
      <BuildConsole
        model={parseBuildOutput(GIT_SYNC_NOISE)}
        sketchDir="/home/x/Blink"
        onJump={noop}
        onClear={noop}
      />,
    );
    expect(
      (screen.getByRole("button", { name: "errors only" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("wires Clear", () => {
    const onClear = vi.fn();
    renderMixed({ onClear });
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("says nothing about a stream that carried no build", () => {
    // `build://line` is shared — sketch sync must not produce a summary strip,
    // but the controls row stays so the bar does not jump later.
    const { container } = render(
      <BuildConsole
        model={parseBuildOutput(GIT_SYNC_NOISE)}
        sketchDir="/home/x/Blink"
        onJump={noop}
        onClear={noop}
      />,
    );
    expect(screen.queryByRole("status")).toBeNull();
    expect(container.querySelector(".build-summary")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Clear" })).toBeTruthy();
  });

  it("puts the rows in a log region", () => {
    const { container } = renderMixed();
    const log = container.querySelector(".console-scroll");
    expect(log?.getAttribute("role")).toBe("log");
    expect(log?.getAttribute("aria-live")).toBe("off");
  });

  it("does not warn-colour plain stderr the way the serial console does", () => {
    const { container } = render(
      <BuildConsole
        model={parseBuildOutput([{ stream: "stderr", line: "just some text" }])}
        sketchDir={null}
        onJump={noop}
        onClear={noop}
      />,
    );
    expect(rows(container)[0].className).toBe("console-row raw");
  });
});
