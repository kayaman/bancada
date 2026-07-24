// Arduino Serial Plotter line parser.
//
// Accepts `label:value` (and `label=value`) pairs and bare numbers, separated
// by spaces, commas and/or tabs. Bare numbers get positional names "ch1",
// "ch2", ... Non-numeric tokens (and fully non-numeric lines) are ignored
// gracefully, so debug prose like "[sensor] raw=42 out of range" still
// yields the `raw` channel.

export interface ParsedValue {
  name: string;
  value: number;
}

// int/float with optional sign, optional exponent; nothing else in the token.
const NUM_RE = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/;

function parseNumber(tok: string): number | null {
  if (!NUM_RE.test(tok)) return null;
  const v = Number(tok);
  return Number.isFinite(v) ? v : null;
}

/** Parse one serial line into named values. Returns [] for unusable lines. */
export function parseLine(line: string): ParsedValue[] {
  const out: ParsedValue[] = [];
  if (!line) return out;
  const tokens = line.split(/[\s,]+/);
  let bareIndex = 0;
  for (const raw of tokens) {
    const tok = raw.trim();
    if (tok.length === 0) continue;
    let sep = tok.indexOf(":");
    if (sep <= 0 || sep >= tok.length - 1) sep = tok.indexOf("=");
    if (sep > 0 && sep < tok.length - 1) {
      const name = tok.slice(0, sep).trim();
      const v = parseNumber(tok.slice(sep + 1).trim());
      if (name.length > 0 && v !== null) out.push({ name, value: v });
      continue;
    }
    const v = parseNumber(tok);
    if (v !== null) {
      bareIndex++;
      out.push({ name: `ch${bareIndex}`, value: v });
    }
    // anything else: ignore token
  }
  return out;
}
