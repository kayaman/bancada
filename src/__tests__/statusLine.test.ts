import { describe, expect, it } from "vitest";
import {
  type Activity,
  formatElapsed,
  progressMode,
  statusLineText,
} from "../statusLine";

const T0 = 1_700_000_000_000;

const idle = {
  activity: null,
  now: T0,
  lastResult: null,
  project: null,
  portName: null,
};

describe("formatElapsed", () => {
  it("floors to whole seconds and pads them", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(999)).toBe("0:00");
    expect(formatElapsed(7_400)).toBe("0:07");
    expect(formatElapsed(59_999)).toBe("0:59");
    expect(formatElapsed(60_000)).toBe("1:00");
    expect(formatElapsed(65_000)).toBe("1:05");
    expect(formatElapsed(3_723_000)).toBe("62:03");
  });

  it("never shows a negative clock", () => {
    expect(formatElapsed(-1)).toBe("0:00");
    expect(formatElapsed(-90_000)).toBe("0:00");
  });
});

describe("statusLineText — active", () => {
  const compiling: Activity = {
    key: "compile",
    label: "Compiling…",
    startedAt: T0,
  };

  it("shows the label and the running clock", () => {
    expect(
      statusLineText({ ...idle, activity: compiling, now: T0 + 7_000 }),
    ).toEqual({ text: "Compiling… 0:07", isError: false });
  });

  it("adds an honest 'usually' hint when a past run is remembered", () => {
    expect(
      statusLineText({
        ...idle,
        activity: compiling,
        now: T0 + 7_000,
        estimateMs: 65_000,
      }),
    ).toEqual({ text: "Compiling… 0:07 (usually ~1:05)", isError: false });
  });

  it("omits the hint when there is no estimate", () => {
    for (const estimateMs of [null, undefined]) {
      expect(
        statusLineText({
          ...idle,
          activity: compiling,
          now: T0 + 1_000,
          estimateMs,
        }).text,
      ).toBe("Compiling… 0:01");
    }
  });

  it("outranks a remembered result and a project", () => {
    expect(
      statusLineText({
        activity: compiling,
        now: T0 + 2_000,
        lastResult: { ok: false, label: "Upload", durationMs: 3_000, at: T0 },
        project: "blink",
        portName: "Uno · /dev/ttyACM0",
      }),
    ).toEqual({ text: "Compiling… 0:02", isError: false });
  });
});

describe("statusLineText — idle", () => {
  it("reports the last result, and flags only a failure", () => {
    expect(
      statusLineText({
        ...idle,
        lastResult: { ok: true, label: "Compile", durationMs: 4_500, at: T0 },
      }),
    ).toEqual({ text: "✓ Compile in 0:04", isError: false });

    expect(
      statusLineText({
        ...idle,
        lastResult: { ok: false, label: "Upload", durationMs: 12_000, at: T0 },
        project: "blink",
      }),
    ).toEqual({ text: "✗ Upload after 0:12 — see Build", isError: true });
  });

  it("falls back to the project and its port", () => {
    expect(
      statusLineText({
        ...idle,
        project: "blink",
        portName: "Uno · /dev/ttyACM0",
      }),
    ).toEqual({ text: "blink · Uno · /dev/ttyACM0", isError: false });
  });

  it("says so when the project has no port", () => {
    expect(statusLineText({ ...idle, project: "blink" }).text).toBe(
      "blink · no port",
    );
    expect(
      statusLineText({ ...idle, project: "blink", portName: null }).text,
    ).toBe("blink · no port");
  });

  it("keeps the launch greeting when there is nothing at all", () => {
    expect(statusLineText(idle)).toEqual({
      text: "Bancada ready — open a project folder.",
      isError: false,
    });
  });
});

describe("progressMode", () => {
  it("shows nothing at rest, whatever the fractions say", () => {
    expect(progressMode(false, 0.5, 0.5)).toEqual({ mode: "none", fraction: 0 });
    expect(progressMode(false, null, null)).toEqual({
      mode: "none",
      fraction: 0,
    });
  });

  it("prefers a measured fraction, clamped to 0..1", () => {
    expect(progressMode(true, 0.42, 0.9)).toEqual({
      mode: "measured",
      fraction: 0.42,
    });
    expect(progressMode(true, 0, null)).toEqual({
      mode: "measured",
      fraction: 0,
    });
    expect(progressMode(true, 1.4, null)).toEqual({
      mode: "measured",
      fraction: 1,
    });
    expect(progressMode(true, -0.2, null)).toEqual({
      mode: "measured",
      fraction: 0,
    });
  });

  it("falls back to the estimate, clamped the same way", () => {
    expect(progressMode(true, null, 0.3)).toEqual({
      mode: "estimate",
      fraction: 0.3,
    });
    expect(progressMode(true, null, 2)).toEqual({
      mode: "estimate",
      fraction: 1,
    });
  });

  it("is indeterminate when busy with neither", () => {
    expect(progressMode(true, null, null)).toEqual({
      mode: "indeterminate",
      fraction: 0,
    });
  });
});
