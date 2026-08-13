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

  it("keeps asking when the port will not open yet", () => {
    // A *failed start* emits no serial://closed, so recovery hung off that
    // event alone left capture dead after every flash: the port takes a
    // couple of seconds to re-enumerate and the one attempt at 1200 ms
    // simply threw into an empty catch.
    const start = appSource.indexOf("const startMonitorQuiet");
    const body = appSource.slice(start, appSource.indexOf("\n  }, [", start));
    expect(body).toContain("catch");
    const caught = body.indexOf("} catch {");
    expect(caught).toBeGreaterThan(-1);
    expect(
      body.slice(caught),
      "a failed start must schedule a retry, not swallow the error",
    ).toContain("scheduleRecapture()");
    // And the same when there is no port to open yet.
    expect(body).toMatch(/if \(!selectedPort\) \{[\s\S]*?scheduleRecapture\(\)/);
  });

  it("re-arms the standing request after a flash", () => {
    // The flash path clears the intent to free the port for esptool, so the
    // post-flash restart has to put it back — otherwise the ladder sees no
    // standing request and never chases the returning port.
    const start = appSource.indexOf("const upload = async");
    const body = appSource.slice(start, appSource.indexOf("\n  };", start));
    expect(body, "post-flash restart must re-arm capture").toContain(
      "requestCapture()",
    );
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
