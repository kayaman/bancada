// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import BottomTabBar from "../components/BottomTabBar";

afterEach(cleanup);

const noop = () => {};

/** The seven tab buttons, in DOM order (the maximize button is excluded by name). */
const tabButtons = () =>
  screen
    .getAllByRole("button")
    .filter((b) => b.classList.contains("tab"));

describe("BottomTabBar", () => {
  it("renders the seven tabs in bench order", () => {
    render(
      <BottomTabBar
        active="build"
        unseen={{}}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons().map((b) => b.textContent)).toEqual([
      "Build",
      "Serial",
      "Scope",
      "MQTT",
      "WS",
      "Web",
      "Assistant",
    ]);
  });

  it("clicking a tab opens it", () => {
    const onOpen = vi.fn();
    render(
      <BottomTabBar
        active="build"
        unseen={{}}
        onOpen={onOpen}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    screen.getByRole("button", { name: "Scope" }).click();
    expect(onOpen).toHaveBeenCalledWith("scope");
  });

  it("marks the active tab for sight and for assistive tech", () => {
    render(
      <BottomTabBar
        active="mqtt"
        unseen={{}}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    const mqtt = screen.getByRole("button", { name: "MQTT" });
    expect(mqtt.getAttribute("aria-current")).toBe("true");
    expect(mqtt.classList.contains("active")).toBe(true);
    const build = screen.getByRole("button", { name: "Build" });
    expect(build.getAttribute("aria-current")).toBe(null);
    expect(build.classList.contains("active")).toBe(false);
    expect(
      tabButtons().filter((b) => b.getAttribute("aria-current") === "true")
        .length,
    ).toBe(1);
  });

  it("dots an unseen inactive tab", () => {
    render(
      <BottomTabBar
        active="build"
        unseen={{ serial: true }}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    const serial = tabButtons()[1];
    expect(serial.querySelector(".tab-dot")).not.toBe(null);
    expect(tabButtons()[0].querySelector(".tab-dot")).toBe(null);
  });

  it("does not dot the tab you are looking at", () => {
    render(
      <BottomTabBar
        active="serial"
        unseen={{ serial: true }}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons()[1].querySelector(".tab-dot")).toBe(null);
  });

  it("draws the three former group boundaries as separators", () => {
    render(
      <BottomTabBar
        active="build"
        unseen={{}}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(screen.getAllByRole("separator").length).toBe(3);
  });

  it("shows a badge count, and nothing at zero", () => {
    const { unmount } = render(
      <BottomTabBar
        active="serial"
        unseen={{}}
        badges={{ build: 3 }}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons()[0].querySelector(".tab-badge")?.textContent).toBe("3");
    unmount();

    render(
      <BottomTabBar
        active="serial"
        unseen={{}}
        badges={{ build: 0 }}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons()[0].querySelector(".tab-badge")).toBe(null);
  });

  it("names the maximize button for what the click will do", () => {
    const onToggleMaximize = vi.fn();
    const { unmount } = render(
      <BottomTabBar
        active="build"
        unseen={{}}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={onToggleMaximize}
      />,
    );
    const max = screen.getByRole("button", { name: "Maximize panel" });
    max.click();
    expect(onToggleMaximize).toHaveBeenCalledTimes(1);
    unmount();

    render(
      <BottomTabBar
        active="build"
        unseen={{}}
        onOpen={noop}
        maximized={true}
        onToggleMaximize={noop}
      />,
    );
    expect(screen.getByRole("button", { name: "Restore panel" })).toBeTruthy();
  });
});
