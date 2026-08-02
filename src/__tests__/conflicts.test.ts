import { describe, expect, it } from "vitest";
// Vite's `?raw` import — the file's own source as a string. Used instead of
// `node:fs` so this needs no @types/node, and instead of a component test
// harness the repo does not have (see the block comment below).
import appSource from "../App.tsx?raw";
import { blockedByConflict, conflictMessage } from "../conflicts";

describe("blockedByConflict", () => {
  it("does not block when there is no conflict", () => {
    expect(blockedByConflict([], "Compile")).toEqual({ blocked: false });
  });

  it("blocks and names the action and the file", () => {
    const b = blockedByConflict(["Blink.ino"], "Upload");
    expect(b.blocked).toBe(true);
    expect(b.message).toContain("Upload cancelled");
    expect(b.message).toContain("Blink.ino");
  });

  it("names every conflicted file", () => {
    const b = blockedByConflict(["a.ino", "src/b.h"], "Compile");
    expect(b.message).toContain("a.ino");
    expect(b.message).toContain("src/b.h");
  });

  it("tells the user how to resolve it", () => {
    // There is no discard action in this editor, so Ctrl+S (keep mine) or
    // reload (take the assistant's) are the only two exits — the message has
    // to say so or the banner is a dead end.
    const msg = conflictMessage(["Blink.ino"]);
    expect(msg).toContain("Ctrl+S");
    expect(msg).toMatch(/reload/i);
  });

  it("reads correctly for one file and for several", () => {
    expect(conflictMessage(["a.ino"])).toContain("Open it");
    expect(conflictMessage(["a.ino", "b.ino"])).toContain("Open each file");
  });
});

// The guard is only worth anything if the dangerous call sites actually
// consult it. App.tsx has no component-test harness in this repo (every
// other test here covers a pure module), so this asserts the wiring at the
// source level rather than leaving the most consequential half of the fix
// untested.
//
// What it is pinning: `saveAll` refusing to flush a conflicted buffer was NOT
// enough on its own. Its warning was a transient notify that the next line
// ("Compiling…") overwrote, and neither caller aborted — so Verify compiled,
// and Upload FLASHED A BOARD WITH, the assistant's on-disk version while the
// user's unsaved edits were invisible. Both must return early.
describe("App.tsx wiring: build paths refuse while a conflict is outstanding", () => {
  const src: string = appSource;

  /** The body of `const <name> = async (…) => { … }` up to the next
   *  top-level `\n  };` — enough to see the guard and the api call in
   *  order. */
  function body(name: string): string {
    const start = src.search(
      new RegExp(`const ${name} = async \\([^)]*\\) => \\{`),
    );
    expect(start, `${name} not found in App.tsx`).toBeGreaterThan(-1);
    const end = src.indexOf("\n  };", start);
    return src.slice(start, end);
  }

  it("verify() refuses before it compiles", () => {
    const b = body("verify");
    const guard = b.indexOf("refuseOnConflict(");
    const call = b.indexOf("api.compileSketch(");
    expect(guard, "verify() must consult the conflict guard").toBeGreaterThan(-1);
    expect(call).toBeGreaterThan(-1);
    expect(guard).toBeLessThan(call);
    expect(b).toContain("if (refuseOnConflict(");
    expect(b).toMatch(/if \(refuseOnConflict\([^)]*\)\) return;/);
  });

  it("upload() refuses before it builds or flashes", () => {
    const b = body("upload");
    const guard = b.indexOf("refuseOnConflict(");
    const call = b.indexOf("api.uploadSketch(");
    expect(guard, "upload() must consult the conflict guard").toBeGreaterThan(-1);
    expect(call).toBeGreaterThan(-1);
    expect(guard).toBeLessThan(call);
    expect(b).toMatch(/if \(refuseOnConflict\([^)]*\)\) return;/);
  });

  it("upload() refuses before it stops the serial monitor", () => {
    // Ordering detail worth pinning: bailing after `toggleMonitor()` would
    // leave the user's monitor closed by a command that then did nothing.
    const b = body("upload");
    expect(b.indexOf("refuseOnConflict(")).toBeLessThan(
      b.indexOf("toggleMonitor()"),
    );
  });

  it("sendToAgent() still refuses too", () => {
    const b = body("sendToAgent");
    expect(b).toContain("agentConflictsRef.current.size > 0");
  });

  it("the conflict banner is rendered from persistent state, not a notify", () => {
    // The status bar is transient by design; the banner is what survives
    // "Compiling…" and tells the user why the build refused.
    expect(src).toContain('className="conflict-banner"');
    expect(src).toContain("conflicts.length > 0 &&");
  });
});
