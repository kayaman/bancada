// Turns the raw `build://line` stream into something a console can render as
// diagnostics instead of dead text.
//
// Two constraints shape everything here. First, `build://line` is SHARED —
// compile, upload, core install, sketch sync and the agent's MCP verify all
// push through it — so nothing may be inferred from "a build ran"; the summary
// is derived from content only, and stays null for traffic that carries no
// diagnostics. Second, arduino-cli injects `#line N "<abs>/Sketch.ino"` into
// the merged .ino.cpp, so gcc reports the ORIGINAL absolute .ino path and a
// location that maps straight back onto the editor buffer.
//
// See src/__tests__/fixtures/buildOutput.ts for the captured output every
// regex below was written against.

import type { OutputLine } from "./api";

export type Severity = "error" | "warning" | "note";

export interface SourceLoc {
  path: string;
  line: number;
  col: number | null;
}

export interface Diagnostic {
  id: number;
  severity: Severity;
  loc: SourceLoc | null;
  message: string;
  /** `In file included from …` / `…: In function 'f()':` lines above it. */
  context: string[];
  /** Echoed source line, caret line, `compilation terminated.`, … */
  detail: string[];
}

export type Row =
  | { kind: "diag"; index: number; text: string; diag: Diagnostic }
  | { kind: "detail"; index: number; text: string; of: number; tone: Severity }
  | { kind: "memory"; index: number; text: string }
  | { kind: "status"; index: number; text: string; tone: "error" | "info" }
  | {
      kind: "raw";
      index: number;
      text: string;
      stream: "stdout" | "stderr";
      tone: "error" | "warn" | null;
    };

export interface MemorySummary {
  flashBytes: number;
  flashPct: number | null;
  flashMax: number | null;
  ramBytes: number | null;
  ramPct: number | null;
  ramMax: number | null;
}

export interface BuildSummary {
  errors: number;
  warnings: number;
  notes: number;
  memory: MemorySummary | null;
  buildFailed: boolean;
  uploadFailed: boolean;
}

export interface BuildModel {
  rows: Row[];
  diagnostics: Diagnostic[];
  summary: BuildSummary;
}

/* ---------- line grammars ---------- */

// SGR colour codes; arduino-cli colours its "Used platform" table.
const ANSI = /\x1b\[[0-9;]*[A-Za-z]/g;
// gcc/clang: `<path>:<line>[:<col>]: <severity>: <message>`. The optional
// `[A-Za-z]:` prefix keeps a Windows drive letter with the path.
const DIAG =
  /^((?:[A-Za-z]:)?[^:\n]+?):(\d+)(?::(\d+))?:\s*(fatal error|error|warning|note):\s*(.*)$/;
// gcc include stack, printed above the diagnostic it belongs to.
const INCLUDE_CHAIN =
  /^(?:In file included from|\s+from)\s+((?:[A-Za-z]:)?[^:\n]+?):(\d+)(?::(\d+))?[:,]\s*$/;
// gcc scope banner: `<path>: In function 'void loop()':` and its relatives.
const FUNC_CONTEXT =
  /^((?:[A-Za-z]:)?[^:\n]+?)(?::\d+(?::\d+)?)?:\s+(?:In (?:(?:static )?member |lambda |)function\b|In instantiation of\b|In constructor\b|In destructor\b|At global scope\b|\s*required (?:from|by)\b)/;
// gcc>=9 echoes the source with a numbered gutter (`   12 | foo();`).
const SNIPPET_GUTTER = /^\s*\d+\s*\|/;
// …and underlines it on a gutter-only line (`      |    ^~~~`).
const CARET_GUTTER = /^\s*\|\s*[\^~ ]*$/;
// avr-gcc 7.3.0 has no gutter at all — just the caret line.
const CARET_BARE = /^\s*[\^~]+\s*$/;
// gcc, after a fatal error.
const COMPILATION_TERMINATED = /^compilation terminated\.$/;
// ld, which reports no severity keyword of its own.
const LINKER_ERROR =
  /(: undefined reference to |: multiple definition of |region `[^']+' overflowed by )/;
// gcc's driver, echoing ld's failure — already counted via LINKER_ERROR.
const COLLECT2 = /^collect2(?:\.exe)?: error: ld returned \d+ exit status$/;
// arduino-cli trailers. 1.5.0 prints only `Error during build: exit status 1`;
// the bare and `Compilation error:` forms come from other CLI generations.
const EXIT_STATUS = /^exit status (\d+)$/;
const BUILD_FAILED = /^(?:Error during build|Compilation error):\s*(.*)$/;
const UPLOAD_FAILED =
  /^(?:Failed uploading|Error during [Uu]pload|Upload error):\s*(.*)$/;
// arduino-cli's own advisories (library architecture mismatches, …).
const CLI_WARNING = /^(?:WARNING|Warning):\s/;
// arduino-cli's memory report, printed on a successful compile.
const SKETCH_USES =
  /^Sketch uses (\d[\d,]*) bytes \((\d+)%\) of program storage space\. Maximum is (\d[\d,]*) bytes\.?$/;
const GLOBALS_USE =
  /^Global variables use (\d[\d,]*) bytes \((\d+)%\) of dynamic memory, leaving (\d[\d,]*) bytes for local variables\. Maximum is (\d[\d,]*) bytes\.?$/;
const GLOBALS_USE_NOMAX =
  /^Global variables use (\d[\d,]*) bytes of dynamic memory\.?$/;

export function stripAnsi(s: string): string {
  return s.replace(ANSI, "");
}

const num = (s: string): number => Number(s.replace(/,/g, ""));

export function parseDiagnosticLine(
  text: string,
): { loc: SourceLoc; severity: Severity; message: string } | null {
  const m = DIAG.exec(stripAnsi(text));
  if (!m) return null;
  const raw = m[4];
  return {
    loc: {
      path: m[1],
      line: Number(m[2]),
      col: m[3] === undefined ? null : Number(m[3]),
    },
    // A fatal error is still an error; nothing downstream distinguishes them.
    severity: raw === "fatal error" ? "error" : (raw as Severity),
    message: m[5],
  };
}

export function parseMemoryLine(text: string): Partial<MemorySummary> | null {
  const t = stripAnsi(text);
  const flash = SKETCH_USES.exec(t);
  if (flash)
    return {
      flashBytes: num(flash[1]),
      flashPct: Number(flash[2]),
      flashMax: num(flash[3]),
    };
  const ram = GLOBALS_USE.exec(t);
  if (ram)
    return {
      ramBytes: num(ram[1]),
      ramPct: Number(ram[2]),
      ramMax: num(ram[4]),
    };
  const ramOnly = GLOBALS_USE_NOMAX.exec(t);
  if (ramOnly) return { ramBytes: num(ramOnly[1]) };
  return null;
}

/**
 * One forward pass over the stream. State is two variables: `openDiag` (the
 * diagnostic indented stderr still belongs to) and `pending` (context rows
 * already emitted that are waiting for the diagnostic they describe).
 */
export function parseBuildOutput(lines: readonly OutputLine[]): BuildModel {
  const rows: Row[] = [];
  const diagnostics: Diagnostic[] = [];
  const summary: BuildSummary = {
    errors: 0,
    warnings: 0,
    notes: 0,
    memory: null,
    buildFailed: false,
    uploadFailed: false,
  };

  let openDiag: number | null = null;
  // Indices into `rows` of context rows provisionally emitted with `of: -1`.
  let pending: number[] = [];

  /** Nothing claimed the context we buffered — it was just output. */
  const flushPending = () => {
    for (const i of pending) {
      const r = rows[i];
      rows[i] = {
        kind: "raw",
        index: r.index,
        text: r.text,
        stream: "stderr",
        tone: null,
      };
    }
    pending = [];
  };

  const openNewDiag = (
    index: number,
    text: string,
    severity: Severity,
    loc: SourceLoc | null,
    message: string,
  ) => {
    const diag: Diagnostic = {
      id: diagnostics.length,
      severity,
      loc,
      message,
      context: pending.map((i) => rows[i].text),
      detail: [],
    };
    for (const i of pending) {
      const r = rows[i];
      rows[i] = {
        kind: "detail",
        index: r.index,
        text: r.text,
        of: diag.id,
        tone: severity,
      };
    }
    pending = [];
    diagnostics.push(diag);
    rows.push({ kind: "diag", index, text, diag });
    openDiag = diag.id;
    if (severity === "error") summary.errors++;
    else if (severity === "warning") summary.warnings++;
    else summary.notes++;
  };

  const mergeMemory = (part: Partial<MemorySummary>) => {
    summary.memory = {
      flashBytes: 0,
      flashPct: null,
      flashMax: null,
      ramBytes: null,
      ramPct: null,
      ramMax: null,
      ...summary.memory,
      ...part,
    };
  };

  lines.forEach((l, index) => {
    const text = stripAnsi(l.line);

    const diag = parseDiagnosticLine(text);
    if (diag) {
      openNewDiag(index, text, diag.severity, diag.loc, diag.message);
      return;
    }

    const isContext = INCLUDE_CHAIN.test(text) || FUNC_CONTEXT.test(text);

    // Continuation of the open diagnostic: gutters, carets, the fatal-error
    // epilogue, or any other indented stderr that isn't a grammar of its own.
    if (
      openDiag !== null &&
      l.stream === "stderr" &&
      !isContext &&
      (SNIPPET_GUTTER.test(text) ||
        CARET_GUTTER.test(text) ||
        CARET_BARE.test(text) ||
        COMPILATION_TERMINATED.test(text) ||
        (/^\s/.test(text) && !LINKER_ERROR.test(text)))
    ) {
      const d = diagnostics[openDiag];
      d.detail.push(text);
      rows.push({
        kind: "detail",
        index,
        text,
        of: openDiag,
        tone: d.severity,
      });
      return;
    }

    openDiag = null;

    if (isContext) {
      pending.push(rows.length);
      rows.push({ kind: "detail", index, text, of: -1, tone: "note" });
      return;
    }

    // ld speaks no severity keyword; treat its complaint as an error with no
    // usable location (the file it names is the object file, not the sketch).
    if (LINKER_ERROR.test(text)) {
      openNewDiag(index, text, "error", null, text);
      return;
    }

    flushPending();

    if (COLLECT2.test(text)) {
      rows.push({ kind: "raw", index, text, stream: l.stream, tone: "error" });
      return;
    }
    if (CLI_WARNING.test(text)) {
      rows.push({ kind: "raw", index, text, stream: l.stream, tone: "warn" });
      return;
    }

    const mem = parseMemoryLine(text);
    if (mem) {
      mergeMemory(mem);
      rows.push({ kind: "memory", index, text });
      return;
    }

    const exit = EXIT_STATUS.exec(text);
    if (exit) {
      const failed = Number(exit[1]) !== 0;
      if (failed) summary.buildFailed = true;
      rows.push({ kind: "status", index, text, tone: failed ? "error" : "info" });
      return;
    }
    if (UPLOAD_FAILED.test(text)) {
      summary.uploadFailed = true;
      rows.push({ kind: "status", index, text, tone: "error" });
      return;
    }
    if (BUILD_FAILED.test(text)) {
      summary.buildFailed = true;
      rows.push({ kind: "status", index, text, tone: "error" });
      return;
    }

    rows.push({ kind: "raw", index, text, stream: l.stream, tone: null });
  });

  flushPending();
  return { rows, diagnostics, summary };
}

/* ---------- jumping to source ---------- */

export interface JumpTarget {
  rel: string;
  line: number;
  col: number | null;
}

const isAbsolute = (p: string) => p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p);

/**
 * "/a/b/c/x.ino" under "/a/b/c" → "x.ino"; a sibling directory → null; a bare
 * relative path is already sketch-relative and passes through.
 */
export function relativeToSketch(
  path: string,
  sketchDir: string,
): string | null {
  if (!isAbsolute(path)) return path;
  const dir = sketchDir.replace(/[/\\]+$/, "");
  if (!path.startsWith(dir + "/")) return null;
  const rel = path.slice(dir.length + 1);
  return rel === "" ? null : rel;
}

export function jumpTarget(
  d: Diagnostic,
  sketchDir: string | null,
  known?: ReadonlySet<string>,
): JumpTarget | null {
  if (!d.loc || sketchDir === null) return null;
  const rel = relativeToSketch(d.loc.path, sketchDir);
  if (rel === null) return null;
  if (known && !known.has(rel)) return null;
  return { rel, line: d.loc.line, col: d.loc.col };
}

/** Toolchain paths are ~120 chars of noise; keep the identifying tail. */
export function shortenToolchainPath(path: string): string {
  const core =
    /\/\.arduino15\/packages\/([^/]+)\/hardware\/([^/]+)\/([^/]+)\/(.*)$/.exec(
      path,
    );
  if (core) return `${core[1]}:${core[2]}@${core[3]}/${core[4]}`;
  const tools =
    /\/\.arduino15\/packages\/([^/]+)\/tools\/([^/]+)\/([^/]+)\/(.*)$/.exec(
      path,
    );
  if (tools) return `${tools[1]} tools/${tools[2]}@${tools[3]}/${tools[4]}`;
  const lib = /\/libraries\/([^/]+)\/(.*)$/.exec(path);
  if (lib) return `${lib[1]}/${lib[2]}`;
  const segs = path.split("/").filter((s) => s !== "");
  if (segs.length > 3) return "…/" + segs.slice(-3).join("/");
  return path;
}

/* ---------- summary strip ---------- */

export type SummaryTone = "error" | "warn" | "success";

export function formatBytes(n: number): string {
  // Explicit grouping rather than toLocaleString: the strip must read the same
  // whatever locale the bench machine happens to have.
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

const memoryPhrase = (m: MemorySummary): string => {
  const pct = (p: number | null) => (p === null ? "" : ` (${p}%)`);
  let s = `${formatBytes(m.flashBytes)} bytes${pct(m.flashPct)} flash`;
  if (m.ramBytes !== null)
    s += ` · ${formatBytes(m.ramBytes)} bytes${pct(m.ramPct)} RAM`;
  return s;
};

/** null means "say nothing" — the stream carried no build. */
export function summaryLabel(
  s: BuildSummary,
): { text: string; tone: SummaryTone } | null {
  if (s.errors > 0) {
    const warn = s.warnings > 0 ? ` · ${plural(s.warnings, "warning")}` : "";
    return { text: `✗ ${plural(s.errors, "error")}${warn}`, tone: "error" };
  }
  if (s.uploadFailed) {
    const ok = s.memory ? ` · Compile OK · ${memoryPhrase(s.memory)}` : "";
    return { text: `✗ Upload failed${ok}`, tone: "error" };
  }
  if (s.buildFailed && s.warnings === 0 && s.notes === 0)
    return { text: "✗ Build failed", tone: "error" };
  if (s.memory) {
    const warn = s.warnings > 0 ? ` · ${plural(s.warnings, "warning")}` : "";
    return {
      text: `✓ Compile OK${warn} · ${memoryPhrase(s.memory)}`,
      tone: s.warnings > 0 ? "warn" : "success",
    };
  }
  if (s.warnings > 0)
    return { text: `⚠ ${plural(s.warnings, "warning")}`, tone: "warn" };
  return null;
}

/** Feeds the bottom tab bar's build badge — only errors deserve a number. */
export function badgeCount(s: BuildSummary): number {
  return s.errors;
}

export function filterRows(rows: readonly Row[], errorsOnly: boolean): Row[] {
  if (!errorsOnly) return [...rows];
  const keep = new Set(
    rows.flatMap((r) =>
      r.kind === "diag" && r.diag.severity === "error" ? [r.diag.id] : [],
    ),
  );
  return rows.filter(
    (r) =>
      (r.kind === "diag" && keep.has(r.diag.id)) ||
      (r.kind === "detail" && keep.has(r.of)) ||
      r.kind === "status" ||
      r.kind === "memory",
  );
}
