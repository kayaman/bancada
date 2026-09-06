import { describe, expect, it } from "vitest";
import {
  buildBlockedReason,
  projectButtonLabel,
  projectMenuItems,
  resolveBuildTarget,
  retargetBlockedReason,
  setTargetBlockedReason,
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

describe("setTargetBlockedReason", () => {
  const base = { sketchDir: "/p", busy: false, idfAvailable: true };

  it("allows the change when everything is in place", () => {
    expect(setTargetBlockedReason(base)).toBeNull();
  });

  it("reports the most fundamental obstacle first", () => {
    // With nothing open, a missing toolchain is not the thing to say.
    expect(
      setTargetBlockedReason({ ...base, sketchDir: null, idfAvailable: false }),
    ).toBe("open a project first");
    expect(setTargetBlockedReason({ ...base, idfAvailable: false })).toBe(
      "ESP-IDF is not available on this machine",
    );
    expect(setTargetBlockedReason({ ...base, busy: true })).toBe(
      "a build is already running",
    );
  });
});

describe("resolveBuildTarget", () => {
  const arduino = {
    kind: "arduino" as const,
    profile: null,
    detectedFqbn: null,
    idfTarget: null,
  };

  it("prefers the sketch.yaml profile over the detected board", () => {
    expect(
      resolveBuildTarget({ ...arduino, profile: "uno", detectedFqbn: "arduino:avr:nano" }),
    ).toEqual({ target: { profile: "uno" } });
  });

  it("falls back to the board detected on the port", () => {
    expect(resolveBuildTarget({ ...arduino, detectedFqbn: "arduino:avr:nano" })).toEqual({
      target: { fqbn: "arduino:avr:nano" },
    });
  });

  it("asks for a profile when a bridge port reports no board", () => {
    expect(resolveBuildTarget(arduino)).toEqual({
      error:
        "No sketch.yaml profile, and this port reports no board identity (USB bridge) — create a profile to set the board.",
    });
  });

  it("treats an unrecognised folder as Arduino, like the backend does", () => {
    expect(resolveBuildTarget({ ...arduino, kind: "unknown" })).toEqual({
      error:
        "No sketch.yaml profile, and this port reports no board identity (USB bridge) — create a profile to set the board.",
    });
  });

  it("needs no profile or board for an ESP-IDF project with a target", () => {
    // The chip target lives in sdkconfig and the backend reads it there; the
    // port being a bare USB bridge is the normal case for a devkit, not a
    // reason to refuse. sketch.yaml is an Arduino concept and never comes up.
    expect(
      resolveBuildTarget({ kind: "idf", profile: null, detectedFqbn: null, idfTarget: "esp32s3" }),
    ).toEqual({ target: {} });
  });

  it("ignores a stale profile or detected board on an ESP-IDF project", () => {
    expect(
      resolveBuildTarget({
        kind: "idf",
        profile: "uno",
        detectedFqbn: "esp32:esp32:esp32s3",
        idfTarget: "esp32s3",
      }),
    ).toEqual({ target: {} });
  });

  it("asks for a chip target, not a profile, on an ESP-IDF project without one", () => {
    expect(
      resolveBuildTarget({ kind: "idf", profile: null, detectedFqbn: null, idfTarget: null }),
    ).toEqual({
      error: "This ESP-IDF project has no target set — choose a chip in the toolbar before building.",
    });
  });
});
