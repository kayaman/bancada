import { describe, expect, it } from "vitest";
// Vite's `?raw` — App.tsx's own source. Same escape hatch conflicts.test.ts
// uses: these are ordering invariants inside a component the repo has no
// harness to render, and they are exactly the kind that break silently.
import appSource from "../App.tsx?raw";

/**
 * Only one thing may hold the serial port at a time, and esptool must always
 * win. These two rules were both broken by the monitor-recovery ladder and
 * the symptom was a flash failing with "No more data to read from the serial
 * port" — nothing about the monitor at all.
 */
describe("serial port handoff", () => {
  it("never lets automatic capture start during a flash", () => {
    // The ladder decides a second or more before it acts, so checking only
    // at scheduling time is not enough — a flash can start in between. The
    // guard has to be where the port is actually opened.
    const start = appSource.indexOf("const startMonitorQuiet");
    expect(start, "startMonitorQuiet not found in App.tsx").toBeGreaterThan(-1);
    const body = appSource.slice(start, appSource.indexOf("\n  }, [", start));

    const guard = body.indexOf("busyRef.current");
    const open = body.indexOf("api.startMonitor(");
    expect(guard, "startMonitorQuiet must consult busyRef").toBeGreaterThan(-1);
    expect(open).toBeGreaterThan(-1);
    expect(guard, "the busy guard must precede opening the port").toBeLessThan(open);
    expect(body).toContain("agentFlashingRef.current");
  });

  it("clears the standing capture request on an explicit stop", () => {
    // The flash path frees the port via toggleMonitor, not stopMonitorIfOn.
    // Without clearing the intent here the ladder treated the stop as an
    // unexpected close and took the port straight back off esptool.
    const start = appSource.indexOf("const toggleMonitor");
    expect(start, "toggleMonitor not found in App.tsx").toBeGreaterThan(-1);
    const body = appSource.slice(start, appSource.indexOf("\n  };", start));

    const cleared = body.indexOf("monitorWantedRef.current = false");
    const stopped = body.indexOf("api.stopMonitor()");
    expect(cleared, "toggleMonitor must clear the capture intent").toBeGreaterThan(-1);
    expect(stopped).toBeGreaterThan(-1);
    expect(cleared, "intent must be cleared before stopping").toBeLessThan(stopped);
  });

  it("stops the monitor before the flash begins", () => {
    const start = appSource.indexOf("const upload = async");
    expect(start).toBeGreaterThan(-1);
    const body = appSource.slice(start, appSource.indexOf("\n  };", start));
    const freed = body.indexOf("toggleMonitor()");
    const flashed = body.indexOf("api.uploadSketch(");
    expect(freed, "upload must free the port").toBeGreaterThan(-1);
    expect(freed).toBeLessThan(flashed);
  });
});
