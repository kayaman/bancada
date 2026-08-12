import { describe, expect, it } from "vitest";
import { projectButtonLabel, projectMenuItems } from "../toolbarModel";

describe("projectButtonLabel", () => {
  it("names the open project by its folder", () => {
    expect(projectButtonLabel("/home/u/Projects/led-test")).toBe("led-test");
  });

  it("tolerates a trailing slash", () => {
    expect(projectButtonLabel("/home/u/Projects/led-test/")).toBe("led-test");
  });

  it("invites an open when nothing is open", () => {
    expect(projectButtonLabel(null)).toBe("Open project");
  });

  it("invites an open rather than showing an empty button", () => {
    // A path that yields no basename must not render a nameless button.
    expect(projectButtonLabel("/")).toBe("Open project");
  });
});

describe("projectMenuItems", () => {
  const ids = (dir: string | null) => projectMenuItems({ sketchDir: dir }).map((i) => i.id);

  it("offers the same four actions either way, in order", () => {
    expect(ids("/s")).toEqual(["open", "new", "duplicate", "rename"]);
    expect(ids(null)).toEqual(["open", "new", "duplicate", "rename"]);
  });

  it("enables everything when a project is open", () => {
    const disabled = projectMenuItems({ sketchDir: "/s" }).filter((i) => i.disabledReason);
    expect(disabled).toEqual([]);
  });

  it("disables only Rename with nothing open, and says why", () => {
    const items = projectMenuItems({ sketchDir: null });
    const disabled = items.filter((i) => i.disabledReason);
    expect(disabled.map((i) => i.id)).toEqual(["rename"]);
    expect(disabled[0].disabledReason).toBe("open a project first");
  });

  it("keeps Duplicate usable with nothing open", () => {
    // Its pane has always had its own source picker; gating it here would
    // silently remove a capability rather than clarify one.
    const dup = projectMenuItems({ sketchDir: null }).find((i) => i.id === "duplicate");
    expect(dup?.disabledReason).toBeUndefined();
  });

  it("shows the real Ctrl+O accelerator on Open, and invents no others", () => {
    const items = projectMenuItems({ sketchDir: "/s" });
    expect(items.find((i) => i.id === "open")?.accel).toBe("Ctrl+O");
    expect(items.filter((i) => i.accel).map((i) => i.id)).toEqual(["open"]);
  });
});
