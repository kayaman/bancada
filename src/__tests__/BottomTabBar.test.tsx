// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import BottomTabBar from "../components/BottomTabBar";

afterEach(cleanup);

const noop = () => {};

/** The tab buttons, in DOM order (the maximize button has no .tab class). */
const tabButtons = () =>
  screen
    .getAllByRole("button")
    .filter((b) => b.classList.contains("tab"));

describe("BottomTabBar", () => {
  it("scopes to the active division — Hardware sees Assistant plus every live/device tool", () => {
    render(
      <BottomTabBar
        active="serial"
        unseen={{}}
        division="hardware"
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons().map((b) => b.textContent)).toEqual([
      "Assistant",
      "Serial",
      "Scope",
      "MQTT",
      "WS",
      "Web",
      "BOM",
      "Diagram",
    ]);
  });

  it("scopes to the active division — Software sees only Assistant and Build", () => {
    render(
      <BottomTabBar
        active="build"
        unseen={{}}
        division="software"
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons().map((b) => b.textContent)).toEqual([
      "Assistant",
      "Build",
    ]);
  });

  it("scopes to the active division — Enclosure sees only Assistant, never an empty-looking row", () => {
    render(
      <BottomTabBar
        active="agent"
        unseen={{}}
        division="enclosure"
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons().map((b) => b.textContent)).toEqual(["Assistant"]);
  });

  it("clicking a tab opens it", () => {
    const onOpen = vi.fn();
    render(
      <BottomTabBar
        active="serial"
        unseen={{}}
        division="hardware"
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
        division="hardware"
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    const mqtt = screen.getByRole("button", { name: "MQTT" });
    expect(mqtt.getAttribute("aria-current")).toBe("true");
    expect(mqtt.classList.contains("active")).toBe(true);
    const serial = screen.getByRole("button", { name: "Serial" });
    expect(serial.getAttribute("aria-current")).toBe(null);
    expect(serial.classList.contains("active")).toBe(false);
    expect(
      tabButtons().filter((b) => b.getAttribute("aria-current") === "true")
        .length,
    ).toBe(1);
  });

  it("dots an unseen inactive tab", () => {
    render(
      <BottomTabBar
        active="agent"
        unseen={{ serial: true }}
        division="hardware"
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
        division="hardware"
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
        active="serial"
        unseen={{}}
        division="hardware"
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(screen.getAllByRole("separator").length).toBe(3);
  });

  it("puts each separator immediately after the tab it closes off", () => {
    render(
      <BottomTabBar
        active="serial"
        unseen={{}}
        division="hardware"
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    const after = (label: string) =>
      screen.getByRole("button", { name: label }).nextElementSibling;
    for (const label of ["Serial", "Scope", "Web"]) {
      expect(after(label)?.getAttribute("role")).toBe("separator");
    }
    // ...and nowhere else: Assistant, MQTT and WS are not group boundaries.
    for (const label of ["Assistant", "MQTT", "WS"]) {
      expect(after(label)?.getAttribute("role")).not.toBe("separator");
    }
  });

  it("shows a badge count, named so it does not run into the tab label", () => {
    const { unmount } = render(
      <BottomTabBar
        active="agent"
        unseen={{}}
        division="software"
        badges={{ build: 3 }}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    const badge = screen.getByLabelText("3 errors");
    expect(badge.classList.contains("tab-badge")).toBe(true);
    expect(badge.textContent).toBe("3");
    const build = tabButtons()[1];
    expect(build.contains(badge)).toBe(true);
    // Without the badge's own label the button would announce as "Build3".
    expect(screen.getByRole("button", { name: "Build 3 errors" })).toBe(build);
    unmount();

    render(
      <BottomTabBar
        active="agent"
        unseen={{}}
        division="software"
        badges={{ build: 0 }}
        onOpen={noop}
        maximized={false}
        onToggleMaximize={noop}
      />,
    );
    expect(tabButtons()[1].querySelector(".tab-badge")).toBe(null);
  });

  it("names the maximize button for what the click will do", () => {
    const onToggleMaximize = vi.fn();
    const { unmount } = render(
      <BottomTabBar
        active="agent"
        unseen={{}}
        division="software"
        onOpen={noop}
        maximized={false}
        onToggleMaximize={onToggleMaximize}
      />,
    );
    const max = screen.getByRole("button", { name: "Maximize panel" });
    expect(max.getAttribute("title")).toBe("Maximize panel");
    max.click();
    expect(onToggleMaximize).toHaveBeenCalledTimes(1);
    unmount();

    render(
      <BottomTabBar
        active="agent"
        unseen={{}}
        division="software"
        onOpen={noop}
        maximized={true}
        onToggleMaximize={noop}
      />,
    );
    const restore = screen.getByRole("button", { name: "Restore panel" });
    expect(restore.getAttribute("title")).toBe("Restore panel");
  });
});
