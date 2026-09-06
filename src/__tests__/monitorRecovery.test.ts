import { describe, expect, it } from "vitest";
import { MAX_RECAPTURE_ATTEMPTS, giveUpMarker, recapturePlan } from "../monitorRecovery";

const state = (over: Partial<Parameters<typeof recapturePlan>[0]> = {}) => ({
  wanted: true,
  busy: false,
  attempt: 0,
  ...over,
});

describe("recapturePlan", () => {
  it("chases a lost port, backing off", () => {
    // The bug this exists for: a native-USB board re-enumerates on reset and
    // nothing brought the monitor back.
    expect(recapturePlan(state({ attempt: 0 }))).toEqual({ retry: true, delayMs: 1000 });
    expect(recapturePlan(state({ attempt: 1 }))).toEqual({ retry: true, delayMs: 2000 });
    expect(recapturePlan(state({ attempt: 2 }))).toEqual({ retry: true, delayMs: 4000 });
  });

  it("gives up rather than chasing a board that has been carried off", () => {
    expect(recapturePlan(state({ attempt: MAX_RECAPTURE_ATTEMPTS }))).toEqual({
      retry: false,
      delayMs: 0,
    });
    expect(recapturePlan(state({ attempt: MAX_RECAPTURE_ATTEMPTS + 3 })).retry).toBe(false);
  });

  it("does not fight an explicit stop", () => {
    // The user's Stop button, the scope taking the port, and the pre-flash
    // handoff all go through the same explicit path. Retrying would steal
    // the port back from whoever just asked for it.
    expect(recapturePlan(state({ wanted: false })).retry).toBe(false);
  });

  it("stays out of the way while a flash owns the port", () => {
    // The flash restarts the monitor itself once the board is back; a second
    // claimant here would race it for a port esptool is still driving.
    expect(recapturePlan(state({ busy: true })).retry).toBe(false);
  });

  it("spans longer than a reset takes to re-enumerate", () => {
    // ~2 s on a native-USB board. The ladder must comfortably outlast it or
    // the fix does not fix anything.
    let total = 0;
    for (let a = 0; a < MAX_RECAPTURE_ATTEMPTS; a++) {
      total += recapturePlan(state({ attempt: a })).delayMs;
    }
    expect(total).toBeGreaterThan(10_000);
  });
});

describe("giveUpMarker", () => {
  it("names the reason the port would not open", () => {
    // On the bench: the user was added to dialout, the session predated it,
    // and five silent retries ended in a marker that blamed nothing.
    expect(giveUpMarker("could not open /dev/ttyACM0 at 115200 baud: Permission denied")).toBe(
      "— gave up re-opening the port: could not open /dev/ttyACM0 at 115200 baud: Permission denied —",
    );
  });

  it("stays bare when nothing was ever reported", () => {
    expect(giveUpMarker(null)).toBe("— gave up re-opening the port —");
    expect(giveUpMarker("  ")).toBe("— gave up re-opening the port —");
  });
});
