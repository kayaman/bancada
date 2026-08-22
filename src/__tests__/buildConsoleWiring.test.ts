import { describe, expect, it } from "vitest";
// Vite's `?raw` — App.tsx's own source. Same escape hatch conflicts.test.ts
// and portHandoff.test.ts use: the repo has no harness that mounts App, and
// the wiring is the whole deliverable here, so it is pinned at source level
// rather than left untested.
import appSource from "../App.tsx?raw";

// What this is pinning: `diagnostics.ts`, `editorGoto.ts` and `BuildConsole`
// are all separately and thoroughly tested — and all three are inert unless
// App.tsx actually hands them the editor handle, the parsed model and the
// error count. Every assertion below is a wire that, if cut, breaks a
// user-visible feature while every other test in the suite stays green.
describe("App.tsx wiring: the build console, the badge and the editor jump", () => {
  const src: string = appSource;

  /** The source between two markers, so an assertion can say "inside this
   *  branch" instead of "somewhere in a 2600-line file". */
  function between(from: string, to: string): string {
    const a = src.indexOf(from);
    expect(a, `${from} not found in App.tsx`).toBeGreaterThan(-1);
    const b = src.indexOf(to, a);
    expect(b, `${to} not found after ${from}`).toBeGreaterThan(-1);
    return src.slice(a, b);
  }

  it("gives the build tab the parsed console, not the raw one", () => {
    // The raw `Console` is still imported — the serial tab uses it — so the
    // meaningful assertion is that the *build* branch no longer does.
    expect(src).toContain("<BuildConsole");
    expect(src).not.toContain("<Console lines={buildLines}");
  });

  it("feeds BuildConsole the parsed model, the sketch dir and the file set", () => {
    const el = between("<BuildConsole", "/>");
    expect(el).toContain("model={buildModel}");
    expect(el).toContain("sketchDir={sketchDir}");
    expect(el).toContain("knownFiles={knownFiles}");
    expect(el).toContain("onJump={jumpToDiagnostic}");
  });

  it("holds a handle on the editor so a diagnostic can move the cursor", () => {
    // Without the ref, `jumpToDiagnostic` has no view and every click on a
    // diagnostic silently does nothing.
    const el = between("<CodeMirror", "editable={!!openFile}");
    expect(el).toContain("ref={editorRef}");
  });

  it("counts the errors onto the Build tab", () => {
    const el = between("<BottomTabBar", "/>");
    expect(el).toContain("badges={{ build: badgeCount(buildModel.summary) }}");
  });

  it("clears the console when the assistant starts a build of its own", () => {
    // Agent parity: `verify()`/`upload()` reset `buildLines`, so the agent's
    // equivalents must too — otherwise the badge keeps reporting the errors
    // of whatever compiled last, over output that no longer contains them.
    const verifyBranch = between(
      'if (ev.type === "verify_started") {',
      'beginActivity("agent_compile"',
    );
    expect(verifyBranch).toContain("setBuildLines([]);");
    const uploadBranch = between(
      'if (ev.type === "upload_started") {',
      'beginActivity("agent_upload"',
    );
    expect(uploadBranch).toContain("setBuildLines([]);");
  });
});
