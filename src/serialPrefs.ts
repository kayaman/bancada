// Serial Monitor preferences: the baud list, the line-ending vocabulary, the
// sketch-baud sniffer, and the `localStorage` shape behind both.
//
// Pure and storage-injected on purpose. The monitor's most annoying habit was
// forgetting the baud on every launch, so the rules for "which baud" are
// worth pinning in tests rather than scattering through a component.

/** The rates the picker offers. 74880 is the ESP8266 boot-ROM rate — without
 *  it the reset banner is mojibake, which is exactly when you need to read
 *  it. The sketch's own rate is added to the picker when it is not here. */
export const BAUD_RATES = [
  9600, 19200, 57600, 74880, 115200, 230400, 250000, 500000, 921600, 1000000,
  2000000,
] as const;

export const DEFAULT_BAUD = 115200;

export type LineEnding = "none" | "nl" | "cr" | "nlcr";

export const LINE_ENDINGS: readonly LineEnding[] = ["none", "nl", "cr", "nlcr"];

export const LINE_ENDING_LABEL: Record<LineEnding, string> = {
  none: "No line ending",
  nl: "Newline",
  cr: "Carriage return",
  nlcr: "Both NL & CR",
};

/** The bytes an ending contributes. "Both NL & CR" sends CR then LF, the
 *  order the Arduino IDE's identically-named option uses — several AT-command
 *  firmwares accept nothing else. */
export function lineEndingBytes(e: LineEnding): string {
  switch (e) {
    case "none":
      return "";
    case "nl":
      return "\n";
    case "cr":
      return "\r";
    case "nlcr":
      return "\r\n";
  }
}

/** What actually goes on the wire: the backend writes this verbatim. */
export function withLineEnding(data: string, e: LineEnding): string {
  return data + lineEndingBytes(e);
}

// ---------- sketch baud detection ----------

/** Strip comments so a commented-out `Serial.begin` cannot vote.
 *
 *  Best-effort and deliberately not string-literal aware: a `"//"` inside a
 *  string literal will be treated as a comment start. Getting that right
 *  needs a C tokenizer, and the cost of being wrong here is a baud the user
 *  overrides in one click — not worth the machinery. */
function stripComments(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .split("\n")
    .map((line) => line.replace(/\/\/.*$/, ""))
    .join("\n");
}

/** A C integer literal → number. Tolerates `UL`/`L`/`u` suffixes and C++14
 *  `'` digit separators; anything else is not a literal. */
function parseIntLiteral(raw: string): number | null {
  let t = raw.trim().replace(/'/g, "");
  // `#define BAUD (115200)` — the parens are the usual defensive habit, and
  // they are not part of the number.
  while (t.startsWith("(") && t.endsWith(")")) t = t.slice(1, -1).trim();
  const m = /^(\d+)[uUlL]*$/.exec(t);
  if (!m) return null;
  const n = Number(m[1]);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

/** `#define NAME 250000` and `const`/`constexpr NAME = 500000;`, gathered
 *  across every source. One level only: a name defined as another name is
 *  left unresolved (and therefore votes for nothing). */
function collectConstants(sources: string[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const src of sources) {
    for (const m of src.matchAll(/^\s*#define\s+(\w+)\s+([^\s/]+)/gm)) {
      const n = parseIntLiteral(m[2]);
      if (n !== null) out.set(m[1], n);
    }
    for (const m of src.matchAll(
      /\bconst(?:expr)?\b[\w\s]*?\b(\w+)\s*=\s*([^;]+);/g,
    )) {
      const n = parseIntLiteral(m[2]);
      if (n !== null) out.set(m[1], n);
    }
  }
  return out;
}

/**
 * The baud a sketch opens `Serial` at, or `null` when that is not a single
 * clear answer (no call, an unresolvable argument, or two calls disagreeing).
 *
 * Only `Serial` counts: `Serial1`/`Serial2` are other UARTs, usually wired to
 * a peripheral rather than to the USB the monitor is reading.
 */
export function detectBaud(sources: string[]): number | null {
  const clean = sources.map(stripComments);
  const consts = collectConstants(clean);
  const found = new Set<number>();
  for (const src of clean) {
    // The literal `.` right after `Serial` is what keeps `Serial1.begin`
    // and `Serial2.begin` out: they are other UARTs, and matching them would
    // make a two-UART sketch look like it disagreed with itself.
    for (const m of src.matchAll(/\bSerial\.begin\(\s*([^,)]+)/g)) {
      const arg = m[1].trim();
      const lit = parseIntLiteral(arg);
      if (lit !== null) {
        found.add(lit);
        continue;
      }
      const named = /^\w+$/.test(arg) ? consts.get(arg) : undefined;
      if (named !== undefined) found.add(named);
      // An argument that resolves to nothing is simply not a vote.
    }
  }
  return found.size === 1 ? [...found][0] : null;
}

// ---------- storage ----------

/** The slice of `Storage` these helpers use, so tests can hand in a Map. */
export type StorageLike = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export const BAUD_KEY = "bancada.serial.baud";
export const UI_KEY = "bancada.serial.ui";

function readJson(s: StorageLike, key: string): unknown {
  try {
    const raw = s.getItem(key);
    return raw === null ? null : JSON.parse(raw);
  } catch {
    // Unparseable or a storage that threw (private mode, quota): the caller
    // gets defaults, which is always better than a panel that will not mount.
    return null;
  }
}

/** Per-sketch baud choices, keyed by sketch dir. Anything that is not a
 *  `{ string: number }` map is discarded wholesale. */
export function loadBaudOverrides(s: StorageLike): Record<string, number> {
  const raw = readJson(s, BAUD_KEY);
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
  const out: Record<string, number> = {};
  for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
    if (typeof v === "number" && Number.isFinite(v) && v > 0) out[k] = v;
  }
  return out;
}

/** Set (or, with `null`, clear) one sketch's override. Returns the new map so
 *  the caller can hold it in state without a second read. */
export function saveBaudOverride(
  s: StorageLike,
  sketchDir: string,
  baud: number | null,
): Record<string, number> {
  const map = loadBaudOverrides(s);
  if (baud === null) delete map[sketchDir];
  else map[sketchDir] = baud;
  try {
    s.setItem(BAUD_KEY, JSON.stringify(map));
  } catch {
    // A full or blocked storage must not take the monitor down with it.
  }
  return map;
}

export interface SerialUiPrefs {
  lineEnding: LineEnding;
  timestamps: boolean;
  autoscroll: boolean;
}

export const DEFAULT_UI_PREFS: SerialUiPrefs = {
  lineEnding: "nl",
  timestamps: false,
  autoscroll: true,
};

/** Toolbar prefs, field by field: one bad field falls back to its default
 *  rather than discarding the other two. */
export function loadUiPrefs(s: StorageLike): SerialUiPrefs {
  const raw = readJson(s, UI_KEY);
  if (!raw || typeof raw !== "object") return { ...DEFAULT_UI_PREFS };
  const o = raw as Record<string, unknown>;
  const ending = o.lineEnding;
  return {
    lineEnding: LINE_ENDINGS.includes(ending as LineEnding)
      ? (ending as LineEnding)
      : DEFAULT_UI_PREFS.lineEnding,
    timestamps:
      o.timestamps === undefined
        ? DEFAULT_UI_PREFS.timestamps
        : Boolean(o.timestamps),
    autoscroll:
      o.autoscroll === undefined
        ? DEFAULT_UI_PREFS.autoscroll
        : Boolean(o.autoscroll),
  };
}

export function saveUiPrefs(s: StorageLike, p: SerialUiPrefs): void {
  try {
    s.setItem(UI_KEY, JSON.stringify(p));
  } catch {
    // As above: prefs are a convenience, never a hard dependency.
  }
}

/** Where the baud in the toolbar came from — the "Use sketch's N" button only
 *  appears when the user has overridden a rate the sketch disagrees with. */
export type BaudSource = "override" | "sketch" | "default";

export function effectiveBaud(
  override: number | undefined,
  detected: number | null,
): { baud: number; source: BaudSource } {
  if (override !== undefined) return { baud: override, source: "override" };
  if (detected !== null) return { baud: detected, source: "sketch" };
  return { baud: DEFAULT_BAUD, source: "default" };
}
