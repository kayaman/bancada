// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import StatusBar from "../StatusBar";
import type { Activity } from "../../statusLine";

const T0 = 1_700_000_000_000;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(T0);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const base = {
  activity: null,
  lastResult: null,
  project: null,
  portName: null,
  busy: false,
  measuredFraction: null,
  estimateMs: null,
};

const compiling: Activity = {
  key: "compile",
  label: "Compiling…",
  startedAt: T0 - 7_000,
};

const text = () => document.querySelector(".statusbar-text")?.textContent;
const bar = () => screen.getByRole("progressbar");

describe("StatusBar — text", () => {
  it("greets, then names the project, then reports the last result", () => {
    const a = render(<StatusBar {...base} />);
    expect(text()).toBe("Bancada ready — open a project folder.");
    a.unmount();

    const b = render(
      <StatusBar {...base} project="blink" portName="Uno · /dev/ttyACM0" />,
    );
    expect(text()).toBe("blink · Uno · /dev/ttyACM0");
    b.unmount();

    render(
      <StatusBar
        {...base}
        project="blink"
        lastResult={{ ok: true, label: "Compile", durationMs: 4_500, at: T0 }}
      />,
    );
    expect(text()).toBe("✓ Compile in 0:04");
  });

  it("turns the bar red only when the last result failed", () => {
    const ok = render(
      <StatusBar
        {...base}
        lastResult={{ ok: true, label: "Compile", durationMs: 1_000, at: T0 }}
      />,
    );
    expect(document.querySelector("footer")!.classList.contains("error")).toBe(
      false,
    );
    ok.unmount();

    render(
      <StatusBar
        {...base}
        lastResult={{ ok: false, label: "Upload", durationMs: 12_000, at: T0 }}
      />,
    );
    const footer = document.querySelector("footer")!;
    expect(footer.classList.contains("statusbar")).toBe(true);
    expect(footer.classList.contains("error")).toBe(true);
  });
});

describe("StatusBar — the running clock", () => {
  it("ticks while an activity is live", () => {
    render(<StatusBar {...base} activity={compiling} busy />);
    expect(text()).toBe("Compiling… 0:07");

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(text()).toBe("Compiling… 0:08");

    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(text()).toBe("Compiling… 0:10");
  });

  it("shows the remembered duration alongside the clock", () => {
    render(
      <StatusBar {...base} activity={compiling} busy estimateMs={65_000} />,
    );
    expect(text()).toBe("Compiling… 0:07 (usually ~1:05)");
  });

  it("arms no interval at rest, and clears it on unmount", () => {
    const idle = render(<StatusBar {...base} />);
    expect(vi.getTimerCount()).toBe(0);
    idle.unmount();

    const live = render(<StatusBar {...base} activity={compiling} busy />);
    expect(vi.getTimerCount()).toBe(1);
    live.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("re-reads the clock at once when an activity starts", () => {
    const { rerender } = render(<StatusBar {...base} />);
    // Half an hour of idling: the ticker was not running, so `now` is stale.
    act(() => {
      vi.advanceTimersByTime(1_800_000);
    });
    rerender(
      <StatusBar
        {...base}
        busy
        activity={{ key: "upload", label: "Uploading…", startedAt: Date.now() }}
      />,
    );
    expect(text()).toBe("Uploading… 0:00");
  });
});

describe("StatusBar — progress", () => {
  it("keeps the bar in the DOM even at rest, claiming no value", () => {
    render(<StatusBar {...base} />);
    expect(bar().getAttribute("aria-label")).toBe("Build progress");
    expect(bar().getAttribute("aria-valuenow")).toBe(null);
    expect(bar().querySelector(".fill.none")).not.toBe(null);
  });

  it("announces a number only when the number was measured", () => {
    const m = render(
      <StatusBar {...base} busy activity={compiling} measuredFraction={0.42} />,
    );
    expect(bar().getAttribute("aria-valuenow")).toBe("42");
    expect(bar().getAttribute("aria-valuetext")).toBe(null);
    expect(
      (bar().querySelector(".fill.measured") as HTMLElement).style.width,
    ).toBe("42%");
    m.unmount();

    // No measured fraction, but a remembered duration: 7 s into a build that
    // took 20 s last time. The component derives this itself now — the only
    // inputs are the activity's `startedAt` and `estimateMs`.
    const e = render(
      <StatusBar {...base} busy activity={compiling} estimateMs={20_000} />,
    );
    expect(bar().getAttribute("aria-valuenow")).toBe(null);
    expect(bar().getAttribute("aria-valuetext")).toBe("estimated");
    expect(
      (bar().querySelector(".fill.estimate") as HTMLElement).style.width,
    ).toBe("35%");
    e.unmount();

    render(<StatusBar {...base} busy activity={compiling} />);
    expect(bar().getAttribute("aria-valuenow")).toBe(null);
    expect(bar().getAttribute("aria-valuetext")).toBe(null);
    // width is left to the CSS animation, not written inline
    expect(
      (bar().querySelector(".fill.indeterminate") as HTMLElement).style.width,
    ).toBe("");
  });

  it("widens the estimate bar on its own as the clock runs", () => {
    // The point of deriving the fraction here rather than taking it as a
    // prop: nothing above this component re-renders on a timer, so a
    // parent-computed fraction would sit frozen at 35% for the whole build.
    render(
      <StatusBar {...base} busy activity={compiling} estimateMs={20_000} />,
    );
    const fill = () => bar().querySelector(".fill.estimate") as HTMLElement;
    expect(fill().style.width).toBe("35%");

    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(fill().style.width).toBe("60%");

    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(fill().style.width).toBe("85%");
  });

  it("never lets the estimate bar claim the build has finished", () => {
    // `estimateFraction` caps at 0.95: a build running long must not sit at
    // 100% for minutes, which reads as a hang rather than as an overrun.
    render(
      <StatusBar {...base} busy activity={compiling} estimateMs={20_000} />,
    );
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(
      (bar().querySelector(".fill.estimate") as HTMLElement).style.width,
    ).toBe("95%");
  });

  it("draws no estimate bar without a remembered duration", () => {
    render(<StatusBar {...base} busy activity={compiling} />);
    expect(bar().querySelector(".fill.indeterminate")).not.toBe(null);
    expect(bar().getAttribute("aria-valuetext")).toBe(null);
  });

  it("draws nothing while not busy, whatever fraction it is handed", () => {
    render(<StatusBar {...base} measuredFraction={0.9} />);
    expect(bar().querySelector(".fill.none")).not.toBe(null);
    expect(bar().getAttribute("aria-valuenow")).toBe(null);
  });

  it("prefers the measured fraction over the estimate when both exist", () => {
    render(
      <StatusBar
        {...base}
        busy
        activity={compiling}
        measuredFraction={0.42}
        estimateMs={20_000}
      />,
    );
    expect(bar().getAttribute("aria-valuenow")).toBe("42");
    expect(bar().querySelector(".fill.estimate")).toBe(null);
  });
});
