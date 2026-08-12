import { describe, expect, it } from "vitest";
import {
  buildBlockedReason,
  projectButtonLabel,
  projectMenuItems,
  retargetBlockedReason,
} from "../toolbarModel";

describe("buildBlockedReason", () => {
  const ready = { sketchDir: "/s", selectedPort: "/dev/ttyACM0", busy: false };

  it("is null when everything is ready", () => {
    expect(buildBlockedReason("verify", ready)).toBeNull();
    expect(buildBlockedReason("flash", ready)).toBeNull();
  });

  it("asks for a project before anything else", () => {
    // With nothing open, a missing port is not the useful thing to say.
    const cold = { sketchDir: null, selectedPort: null, busy: false };
    expect(buildBlockedReason("verify", cold)).toBe("open a project first");
    expect(buildBlockedReason("flash", cold)).toBe("open a project first");
  });

  it("reports a running build", () => {
    expect(buildBlockedReason("verify", { ...ready, busy: true })).toBe(
      "a build is already running",
    );
  });

  it("wants a port for Flash only", () => {
    const noPort = { ...ready, selectedPort: null };
    expect(buildBlockedReason("flash", noPort)).toBe("select a serial port");
    expect(buildBlockedReason("verify", noPort)).toBeNull();
  });
});

describe("retargetBlockedReason", () => {
  it("is null once a profile is selected", () => {
    expect(retargetBlockedReason(["esp32s3"], "esp32s3")).toBeNull();
  });

  it("distinguishes no profiles from none selected", () => {
    expect(retargetBlockedReason([], null)).toBe(
      "this project has no sketch.yaml profile yet",
    );
    expect(retargetBlockedReason(["esp32s3"], null)).toBe("select a profile first");
  });
});

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
