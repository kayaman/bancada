import { describe, it, expect } from "vitest";
import {
  stripAnsi,
  parseDiagnosticLine,
  parseMemoryLine,
  parseBuildOutput,
  relativeToSketch,
  jumpTarget,
  shortenToolchainPath,
  summaryLabel,
  badgeCount,
  filterRows,
  formatBytes,
} from "../diagnostics";
import type { BuildSummary, Diagnostic, Row } from "../diagnostics";
import {
  AVR_ERRORS,
  AVR_ERRORS_SKETCH_DIR,
  AVR_FATAL_INCLUDE,
  AVR_FATAL_SKETCH_DIR,
  AVR_LINKER_UNDEFINED_REFERENCE,
  AVR_OK_MEMORY,
  ESP32_ERRORS_WITH_INCLUDE_CHAIN,
  ESP32_SKETCH_DIR,
  GIT_SYNC_NOISE,
} from "./fixtures/buildOutput";

const kinds = (rows: readonly Row[]) => rows.map((r) => r.kind);

const emptySummary = (over: Partial<BuildSummary> = {}): BuildSummary => ({
  errors: 0,
  warnings: 0,
  notes: 0,
  memory: null,
  buildFailed: false,
  uploadFailed: false,
  ...over,
});

describe("stripAnsi", () => {
  it("drops SGR sequences and leaves the payload", () => {
    // The "Used platform" table is the only coloured line arduino-cli emits.
    expect(stripAnsi("\u001b[92mUsed platform\u001b[0m \u001b[92mVersion\u001b[0m")).toBe(
      "Used platform Version",
    );
  });

  it("is a no-op on plain text", () => {
    expect(stripAnsi("Sketch uses 924 bytes")).toBe("Sketch uses 924 bytes");
  });
});

describe("parseDiagnosticLine", () => {
  it("parses an error with line and column", () => {
    expect(
      parseDiagnosticLine("/home/x/Blink/Blink.ino:13:3: error: 'foo' was not declared"),
    ).toEqual({
      loc: { path: "/home/x/Blink/Blink.ino", line: 13, col: 3 },
      severity: "error",
      message: "'foo' was not declared",
    });
  });

  it("parses a warning", () => {
    const d = parseDiagnosticLine(
      "/home/x/Blink/Blink.ino:10:13: warning: comparison between signed and unsigned integer expressions [-Wsign-compare]",
    );
    expect(d?.severity).toBe("warning");
    expect(d?.loc.line).toBe(10);
    expect(d?.loc.col).toBe(13);
  });

  it("parses a note", () => {
    expect(parseDiagnosticLine("/usr/include/x.h:1283:7: note: candidate: 'push_back'")?.severity).toBe(
      "note",
    );
  });

  it("folds `fatal error` into error and keeps a message full of colons", () => {
    expect(
      parseDiagnosticLine("/home/x/Broken/Broken.ino:1:10: fatal error: Nope.h: No such file or directory"),
    ).toEqual({
      loc: { path: "/home/x/Broken/Broken.ino", line: 1, col: 10 },
      severity: "error",
      message: "Nope.h: No such file or directory",
    });
  });

  it("treats the column as optional", () => {
    expect(parseDiagnosticLine("/home/x/a.ino:4: error: boom")?.loc).toEqual({
      path: "/home/x/a.ino",
      line: 4,
      col: null,
    });
  });

  it("strips ANSI before matching", () => {
    expect(
      parseDiagnosticLine("\u001b[1m/home/x/a.ino:4:2:\u001b[0m \u001b[31merror:\u001b[0m boom"),
    ).toEqual({
      loc: { path: "/home/x/a.ino", line: 4, col: 2 },
      severity: "error",
      message: "boom",
    });
  });

  it("keeps a Windows drive letter with the path", () => {
    expect(parseDiagnosticLine("C:\\Users\\x\\Blink\\Blink.ino:12:5: error: boom")?.loc.path).toBe(
      "C:\\Users\\x\\Blink\\Blink.ino",
    );
  });

  it("returns null for a line with no severity", () => {
    expect(parseDiagnosticLine("/home/x/a.ino:4: undefined reference to `helper()'")).toBeNull();
    expect(parseDiagnosticLine("collect2: error: ld returned 1 exit status")).toBeNull();
    expect(parseDiagnosticLine("Fast-forward")).toBeNull();
  });
});

describe("parseMemoryLine", () => {
  it("parses the flash line", () => {
    expect(
      parseMemoryLine("Sketch uses 924 bytes (2%) of program storage space. Maximum is 32256 bytes."),
    ).toEqual({ flashBytes: 924, flashPct: 2, flashMax: 32256 });
  });

  it("parses the RAM line and strips thousands separators", () => {
    expect(
      parseMemoryLine(
        "Global variables use 1,234 bytes (60%) of dynamic memory, leaving 814 bytes for local variables. Maximum is 2,048 bytes.",
      ),
    ).toEqual({ ramBytes: 1234, ramPct: 60, ramMax: 2048 });
  });

  it("parses the maximum-less RAM line some cores print", () => {
    expect(parseMemoryLine("Global variables use 21456 bytes of dynamic memory.")).toEqual({
      ramBytes: 21456,
    });
  });

  it("returns null for anything else", () => {
    expect(parseMemoryLine("Fast-forward")).toBeNull();
  });
});

describe("parseBuildOutput — AVR_ERRORS (real avr-gcc 7.3.0, bare caret gutter)", () => {
  const m = parseBuildOutput(AVR_ERRORS);

  it("emits one row per input line", () => {
    expect(m.rows).toHaveLength(AVR_ERRORS.length);
    expect(m.rows.map((r) => r.index)).toEqual(AVR_ERRORS.map((_, i) => i));
  });

  it("classifies every row", () => {
    expect(kinds(m.rows)).toEqual([
      "detail", // "…ino: In function 'void loop()':" — re-pointed at the warning
      "diag", // the -Wsign-compare warning
      "detail", // echoed source line
      "detail", // bare caret line (no gcc>=9 gutter on avr-gcc 7.3.0)
      "diag", // the undeclared-identifier error
      "detail",
      "detail",
      "raw", // stdout blank line
      "raw", // "Used platform" table header
      "raw", // "arduino:avr 1.8.8 …"
      "status", // "Error during build: exit status 1"
    ]);
  });

  it("attaches the function context and the snippet to the right diagnostics", () => {
    const [warning, error] = m.diagnostics;
    expect(warning.severity).toBe("warning");
    expect(warning.loc).toEqual({
      path: `${AVR_ERRORS_SKETCH_DIR}/BrokenNoInc.ino`,
      line: 10,
      col: 13,
    });
    expect(warning.context).toEqual([
      `${AVR_ERRORS_SKETCH_DIR}/BrokenNoInc.ino: In function 'void loop()':`,
    ]);
    expect(warning.detail).toEqual(["   if (count < limit) {", "       ~~~~~~^~~~~~~"]);

    expect(error.severity).toBe("error");
    expect(error.loc?.line).toBe(13);
    expect(error.context).toEqual([]);
    expect(error.detail).toEqual(["   undeclaredHelper();", "   ^~~~~~~~~~~~~~~~"]);

    const details = m.rows.filter((r) => r.kind === "detail");
    expect(details.map((r) => (r as { of: number }).of)).toEqual([0, 0, 0, 1, 1]);
  });

  it("strips ANSI from the raw rows", () => {
    expect((m.rows[8] as { text: string }).text).toBe("Used platform Version Path");
  });

  it("counts one error and one warning and marks the build failed", () => {
    expect(m.summary).toEqual(emptySummary({ errors: 1, warnings: 1, buildFailed: true }));
  });

  it("summarises as an error strip", () => {
    expect(summaryLabel(m.summary)).toEqual({ text: "✗ 1 error · 1 warning", tone: "error" });
  });
});

describe("parseBuildOutput — AVR_FATAL_INCLUDE", () => {
  const m = parseBuildOutput(AVR_FATAL_INCLUDE);

  it("folds `compilation terminated.` into the fatal diagnostic", () => {
    expect(kinds(m.rows)).toEqual([
      "diag",
      "detail",
      "detail",
      "detail", // "compilation terminated."
      "raw",
      "raw",
      "raw",
      "status",
    ]);
    expect(m.diagnostics).toHaveLength(1);
    expect(m.diagnostics[0]).toMatchObject({
      severity: "error",
      message: "Nope.h: No such file or directory",
      loc: { path: `${AVR_FATAL_SKETCH_DIR}/Broken.ino`, line: 1, col: 10 },
    });
    expect(m.diagnostics[0].detail).toContain("compilation terminated.");
  });

  it("counts the fatal error once", () => {
    expect(m.summary.errors).toBe(1);
    expect(m.summary.buildFailed).toBe(true);
    expect(summaryLabel(m.summary)).toEqual({ text: "✗ 1 error", tone: "error" });
  });
});

describe("parseBuildOutput — ESP32_ERRORS_WITH_INCLUDE_CHAIN (real gcc 14.2.0 gutter)", () => {
  const m = parseBuildOutput(ESP32_ERRORS_WITH_INCLUDE_CHAIN);

  it("emits one row per input line", () => {
    expect(m.rows).toHaveLength(ESP32_ERRORS_WITH_INCLUDE_CHAIN.length);
  });

  it("re-points the whole `In file included from` chain at the note that follows it", () => {
    // Rows 4..8 are the chain; it arrives AFTER the first error and belongs to
    // the stl_vector.h note at row 9, not to the error above it.
    const chain = m.rows.slice(4, 9);
    expect(chain.every((r) => r.kind === "detail")).toBe(true);
    expect(chain.map((r) => (r as { of: number }).of)).toEqual([1, 1, 1, 1, 1]);
    expect(m.diagnostics[1].severity).toBe("note");
    expect(m.diagnostics[1].context).toHaveLength(5);
    expect(m.diagnostics[1].context[2]).toContain("cores/esp32/HardwareSerial.h:49,");
  });

  it("keeps the numbered gutter, the caret gutter and the label lines as detail", () => {
    // "      |                      |" and "      |   const char*" match no
    // gutter regex; they are kept because they are indented stderr under an
    // open diagnostic.
    expect(kinds(m.rows).slice(14, 18)).toEqual(["detail", "detail", "detail", "detail"]);
    expect(m.diagnostics[3].detail).toEqual([
      '    7 |   readings.push_back("not an int");',
      "      |                      ^~~~~~~~~~~~",
      "      |                      |",
      "      |                      const char*",
    ]);
  });

  it("counts three errors and four notes", () => {
    expect(m.summary).toEqual(emptySummary({ errors: 3, notes: 4, buildFailed: true }));
    expect(summaryLabel(m.summary)).toEqual({ text: "✗ 3 errors", tone: "error" });
  });

  it("locates the sketch-local errors", () => {
    expect(m.diagnostics[0].loc).toEqual({
      path: `${ESP32_SKETCH_DIR}/Chain.ino`,
      line: 7,
      col: 21,
    });
  });
});

describe("parseBuildOutput — AVR_LINKER_UNDEFINED_REFERENCE", () => {
  const m = parseBuildOutput(AVR_LINKER_UNDEFINED_REFERENCE);

  it("counts the undefined reference once, as a location-less error", () => {
    expect(kinds(m.rows)).toEqual([
      "detail", // "/tmp/cc….ltrans.o: In function `setup':"
      "diag", // "…Linker.ino:4: undefined reference to `helper()'"
      "raw", // collect2
      "raw",
      "raw",
      "raw",
      "status",
    ]);
    expect(m.diagnostics).toHaveLength(1);
    expect(m.diagnostics[0].loc).toBeNull();
    expect(m.diagnostics[0].severity).toBe("error");
    expect(m.summary.errors).toBe(1);
  });

  it("tones collect2 without counting it", () => {
    expect(m.rows[2]).toMatchObject({ kind: "raw", tone: "error" });
  });
});

describe("parseBuildOutput — AVR_OK_MEMORY", () => {
  const m = parseBuildOutput(AVR_OK_MEMORY);

  it("reads both memory lines", () => {
    expect(kinds(m.rows)).toEqual(["memory", "memory"]);
    expect(m.summary.memory).toEqual({
      flashBytes: 924,
      flashPct: 2,
      flashMax: 32256,
      ramBytes: 9,
      ramPct: 0,
      ramMax: 2048,
    });
    expect(m.summary.buildFailed).toBe(false);
  });

  it("summarises as a success strip", () => {
    expect(summaryLabel(m.summary)).toEqual({
      text: "✓ Compile OK · 924 bytes (2%) flash · 9 bytes (0%) RAM",
      tone: "success",
    });
  });
});

describe("parseBuildOutput — GIT_SYNC_NOISE", () => {
  const m = parseBuildOutput(GIT_SYNC_NOISE);

  it("stays silent on non-build traffic sharing the stream", () => {
    expect(kinds(m.rows)).toEqual(new Array(GIT_SYNC_NOISE.length).fill("raw"));
    expect(m.rows.every((r) => (r as { tone: unknown }).tone === null)).toBe(true);
    expect(m.diagnostics).toEqual([]);
    expect(summaryLabel(m.summary)).toBeNull();
    expect(badgeCount(m.summary)).toBe(0);
  });
});

describe("parseBuildOutput — synthetic shapes the real captures do not produce", () => {
  it("demotes an orphan context run to raw rows when no diagnostic follows", () => {
    const m = parseBuildOutput([
      { stream: "stderr", line: "In file included from /a/b/c.h:3," },
      { stream: "stderr", line: "                 from /a/b/d.cpp:1:" },
      { stream: "stdout", line: "Done." },
    ]);
    expect(kinds(m.rows)).toEqual(["raw", "raw", "raw"]);
    expect(m.rows[0]).toMatchObject({ tone: null });
  });

  it("recognises a bare `exit status` trailer", () => {
    const m = parseBuildOutput([{ stream: "stderr", line: "exit status 1" }]);
    expect(m.rows[0]).toMatchObject({ kind: "status", tone: "error" });
    expect(m.summary.buildFailed).toBe(true);
  });

  it("treats `exit status 0` as information, not a failure", () => {
    const m = parseBuildOutput([{ stream: "stderr", line: "exit status 0" }]);
    expect(m.rows[0]).toMatchObject({ kind: "status", tone: "info" });
    expect(m.summary.buildFailed).toBe(false);
  });

  it("recognises the newer `Compilation error:` trailer", () => {
    const m = parseBuildOutput([
      { stream: "stderr", line: "Compilation error: 'foo' was not declared in this scope" },
    ]);
    expect(m.rows[0].kind).toBe("status");
    expect(m.summary.buildFailed).toBe(true);
  });

  it("recognises an upload failure", () => {
    const m = parseBuildOutput([
      { stream: "stderr", line: "Failed uploading: uploading error: exit status 1" },
    ]);
    expect(m.summary.uploadFailed).toBe(true);
    expect(m.summary.buildFailed).toBe(false);
  });

  it("tones a CLI warning without counting it as a compiler warning", () => {
    const m = parseBuildOutput([
      { stream: "stderr", line: "WARNING: library Foo claims to run on avr architecture" },
    ]);
    expect(m.rows[0]).toMatchObject({ kind: "raw", tone: "warn" });
    expect(m.summary.warnings).toBe(0);
  });

  it("counts a region-overflow linker error", () => {
    const m = parseBuildOutput([
      { stream: "stderr", line: "/usr/bin/ld: region `iram0_0_seg' overflowed by 1024 bytes" },
    ]);
    expect(m.summary.errors).toBe(1);
    expect(m.diagnostics[0].loc).toBeNull();
  });
});

describe("summaryLabel", () => {
  it("is null when there is nothing to say", () => {
    expect(summaryLabel(emptySummary())).toBeNull();
  });

  it("pluralises errors and warnings", () => {
    expect(summaryLabel(emptySummary({ errors: 1 }))?.text).toBe("✗ 1 error");
    expect(summaryLabel(emptySummary({ errors: 2 }))?.text).toBe("✗ 2 errors");
    expect(summaryLabel(emptySummary({ errors: 2, warnings: 1 }))?.text).toBe(
      "✗ 2 errors · 1 warning",
    );
    expect(summaryLabel(emptySummary({ errors: 1, warnings: 3 }))?.text).toBe(
      "✗ 1 error · 3 warnings",
    );
  });

  const memory = {
    flashBytes: 234512,
    flashPct: 17,
    flashMax: 1310720,
    ramBytes: 21456,
    ramPct: 6,
    ramMax: 327680,
  };

  it("reports an upload failure over a good compile", () => {
    expect(summaryLabel(emptySummary({ uploadFailed: true, memory }))).toEqual({
      text: "✗ Upload failed · Compile OK · 234,512 bytes (17%) flash · 21,456 bytes (6%) RAM",
      tone: "error",
    });
  });

  it("reports a bare upload failure when nothing compiled", () => {
    expect(summaryLabel(emptySummary({ uploadFailed: true }))).toEqual({
      text: "✗ Upload failed",
      tone: "error",
    });
  });

  it("reports a build failure that produced no diagnostics", () => {
    expect(summaryLabel(emptySummary({ buildFailed: true }))).toEqual({
      text: "✗ Build failed",
      tone: "error",
    });
  });

  it("reports memory on success", () => {
    expect(summaryLabel(emptySummary({ memory }))).toEqual({
      text: "✓ Compile OK · 234,512 bytes (17%) flash · 21,456 bytes (6%) RAM",
      tone: "success",
    });
  });

  it("keeps the success wording but the warn tone when warnings survived", () => {
    expect(summaryLabel(emptySummary({ warnings: 2, memory }))).toEqual({
      text: "✓ Compile OK · 2 warnings · 234,512 bytes (17%) flash · 21,456 bytes (6%) RAM",
      tone: "warn",
    });
  });

  it("omits the RAM half when the core does not report it", () => {
    expect(
      summaryLabel(
        emptySummary({
          memory: { ...memory, ramBytes: null, ramPct: null, ramMax: null },
        }),
      )?.text,
    ).toBe("✓ Compile OK · 234,512 bytes (17%) flash");
  });

  it("falls back to a warning-only strip", () => {
    expect(summaryLabel(emptySummary({ warnings: 1 }))).toEqual({
      text: "⚠ 1 warning",
      tone: "warn",
    });
  });
});

describe("badgeCount", () => {
  it("is the error count, so notes and warnings never light the tab", () => {
    expect(badgeCount(emptySummary({ errors: 3, warnings: 9, notes: 4 }))).toBe(3);
    expect(badgeCount(emptySummary({ warnings: 9 }))).toBe(0);
  });
});

describe("filterRows", () => {
  const m = parseBuildOutput(AVR_ERRORS);

  it("is the identity when the filter is off", () => {
    expect(filterRows(m.rows, false)).toEqual(m.rows);
  });

  it("keeps error diagnostics with their details, plus status and memory", () => {
    const kept = filterRows(m.rows, true);
    expect(kinds(kept)).toEqual(["diag", "detail", "detail", "status"]);
    expect((kept[0] as { diag: Diagnostic }).diag.severity).toBe("error");
  });

  it("keeps memory rows so the numbers survive the filter", () => {
    const ok = parseBuildOutput(AVR_OK_MEMORY);
    expect(filterRows(ok.rows, true)).toHaveLength(2);
  });
});

describe("relativeToSketch", () => {
  it("relativises a path under the sketch directory", () => {
    expect(relativeToSketch("/a/b/c/x.ino", "/a/b/c")).toBe("x.ino");
    expect(relativeToSketch("/a/b/c/sub/x.cpp", "/a/b/c")).toBe("sub/x.cpp");
  });

  it("tolerates a trailing slash on the sketch directory", () => {
    expect(relativeToSketch("/a/b/c/x.ino", "/a/b/c/")).toBe("x.ino");
  });

  it("does not treat a sibling directory as a prefix match", () => {
    expect(relativeToSketch("/a/b/c2/x.ino", "/a/b/c")).toBeNull();
  });

  it("returns null for the sketch directory itself", () => {
    expect(relativeToSketch("/a/b/c", "/a/b/c")).toBeNull();
  });

  it("returns null for a toolchain path", () => {
    expect(
      relativeToSketch("/home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11/x.h", "/a/b/c"),
    ).toBeNull();
  });

  it("passes a bare relative path through", () => {
    expect(relativeToSketch("Blink.ino", "/a/b/c")).toBe("Blink.ino");
  });
});

describe("jumpTarget", () => {
  const local: Diagnostic = {
    id: 0,
    severity: "error",
    loc: { path: "/home/x/Blink/Blink.ino", line: 3, col: 1 },
    message: "boom",
    context: [],
    detail: [],
  };
  const toolchain: Diagnostic = {
    ...local,
    id: 1,
    loc: {
      path: "/home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11/cores/esp32/HardwareSerial.h",
      line: 49,
      col: null,
    },
  };
  const nowhere: Diagnostic = { ...local, id: 2, loc: null };

  it("resolves a sketch-local diagnostic", () => {
    expect(jumpTarget(local, "/home/x/Blink")).toEqual({ rel: "Blink.ino", line: 3, col: 1 });
  });

  it("refuses a diagnostic with no location", () => {
    expect(jumpTarget(nowhere, "/home/x/Blink")).toBeNull();
  });

  it("refuses a diagnostic outside the sketch", () => {
    expect(jumpTarget(toolchain, "/home/x/Blink")).toBeNull();
  });

  it("refuses everything when the sketch directory is unknown", () => {
    expect(jumpTarget(local, null)).toBeNull();
  });

  it("refuses a file the editor does not know about", () => {
    expect(jumpTarget(local, "/home/x/Blink", new Set(["other.cpp"]))).toBeNull();
    expect(jumpTarget(local, "/home/x/Blink", new Set(["Blink.ino"]))).toEqual({
      rel: "Blink.ino",
      line: 3,
      col: 1,
    });
  });
});

describe("shortenToolchainPath", () => {
  it("names a core by fqbn-ish package:arch@version", () => {
    expect(
      shortenToolchainPath(
        "/home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11/cores/esp32/HardwareSerial.h",
      ),
    ).toBe("esp32:esp32@3.3.11/cores/esp32/HardwareSerial.h");
  });

  it("names a toolchain by package tools/name@version", () => {
    expect(
      shortenToolchainPath(
        "/home/kayaman/.arduino15/packages/esp32/tools/esp-x32/2601/xtensa-esp-elf/include/c++/14.2.0/bits/stl_vector.h",
      ),
    ).toBe("esp32 tools/esp-x32@2601/xtensa-esp-elf/include/c++/14.2.0/bits/stl_vector.h");
  });

  it("names a library by its folder", () => {
    expect(shortenToolchainPath("/home/x/Arduino/libraries/Adafruit_GFX/Adafruit_GFX.h")).toBe(
      "Adafruit_GFX/Adafruit_GFX.h",
    );
  });

  it("elides everything but the last three segments of anything else", () => {
    expect(
      shortenToolchainPath("/home/kayaman/.cache/arduino/sketches/DDBBFF/sketch/Chain.ino.cpp"),
    ).toBe("…/DDBBFF/sketch/Chain.ino.cpp");
  });

  it("leaves a short path alone", () => {
    expect(shortenToolchainPath("Blink.ino")).toBe("Blink.ino");
    expect(shortenToolchainPath("/a/b/c.ino")).toBe("/a/b/c.ino");
  });
});

describe("formatBytes", () => {
  it("groups thousands", () => {
    expect(formatBytes(234512)).toBe("234,512");
    expect(formatBytes(924)).toBe("924");
    expect(formatBytes(0)).toBe("0");
    expect(formatBytes(1310720)).toBe("1,310,720");
  });
});
