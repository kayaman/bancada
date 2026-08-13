import { describe, expect, it } from "vitest";
// Vite's `?raw` — the component's own source. Same escape hatch
// portHandoff.test.ts uses: vitest runs in the node environment with no
// harness to render a component, and these are the invariants that break
// without producing a visible symptom.
import fleetSource from "../components/FleetManager.tsx?raw";

/**
 * A board's project offer costs a subprocess, and the panel it lives in is
 * re-synced by a 2 s port scan. The two facts meet badly.
 */
describe("fleet flash record", () => {
  it("keys the drift lookup on the record, not on the boards array", () => {
    // `fleetSync` hands back a freshly built array on every scan, so a
    // dependency on `boards` re-runs `git rev-list` twice a minute for as
    // long as a card stays expanded — invisible except as disk chatter.
    const start = fleetSource.indexOf("projectDrift(selectedRec");
    expect(start, "the drift fetch should read selectedRec").toBeGreaterThan(-1);

    const deps = fleetSource.slice(start, fleetSource.indexOf("]);", start) + 3);
    expect(deps).toContain("selectedRec?.project_dir");
    expect(deps).toContain("selectedRec?.commit");
    expect(deps, "must not depend on the whole boards array").not.toMatch(
      /\}, \[[^\]]*\bboards\b/,
    );
  });

  it("only fetches drift for the card the user opened", () => {
    // Not for every board on every render: the whole reason selection exists.
    const effects = fleetSource.split("projectDrift(").length - 1;
    expect(effects, "exactly one call site").toBe(1);
    expect(fleetSource).toContain("if (!selectedRec)");
  });

  it("checks the cost of opening at click time, not at render time", () => {
    // A file dirtied after the card was expanded still has to count, so the
    // guard reads a function rather than a captured boolean.
    const start = fleetSource.indexOf("const openProject");
    expect(start).toBeGreaterThan(-1);
    const body = fleetSource.slice(start, fleetSource.indexOf("\n  };", start));
    expect(body).toContain("openCost()");
    const arm = body.indexOf("setArmed(entry.id)");
    const open = body.indexOf("onOpenProject(");
    expect(arm, "arms before opening").toBeGreaterThan(-1);
    expect(open).toBeGreaterThan(-1);
    expect(arm, "the arming return must precede the open").toBeLessThan(open);
  });

  it("disables Open when the recorded folder is gone", () => {
    // The disabled-controls rule: refuse with the reason in the title rather
    // than fail on click.
    expect(fleetSource).toContain('drift?.kind === "missing"');
    expect(fleetSource).toContain("disabled={working || missing}");
  });
});
