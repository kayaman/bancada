// Splits assistant message text into plain-text and fenced-code segments.
//
// No markdown dependency exists in the repo and adding a renderer+sanitizer
// is out of character (see spec §Files, src/agent/fences.ts entry) — the
// panel renders plain text plus mono code blocks and nothing more. This
// helper only finds the ``` fence boundaries; it does not interpret
// anything else markdown does (headings, lists, inline code, ...).

export interface Segment {
  kind: "text" | "code";
  lang?: string;
  content: string;
}

const FENCE = "```";

/**
 * Splits on lines that start with ``` (optionally followed by a language
 * tag). Tolerant of an unclosed trailing fence: if the text ends while still
 * inside a code block, everything from the opening fence to the end of the
 * text is treated as code rather than being dropped or left as plain text.
 */
export function splitFences(text: string): Segment[] {
  const lines = text.split("\n");
  const segments: Segment[] = [];
  let textBuf: string[] = [];
  let i = 0;

  const flushText = () => {
    if (textBuf.length === 0) return;
    const content = textBuf.join("\n");
    if (content.length > 0) segments.push({ kind: "text", content });
    textBuf = [];
  };

  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trimStart();
    if (!trimmed.startsWith(FENCE)) {
      textBuf.push(line);
      i++;
      continue;
    }

    flushText();
    const lang = trimmed.slice(FENCE.length).trim();
    i++;

    const codeBuf: string[] = [];
    while (i < lines.length && !lines[i].trimStart().startsWith(FENCE)) {
      codeBuf.push(lines[i]);
      i++;
    }
    if (i < lines.length) i++; // consume the closing fence; EOF = unclosed, rest already collected

    segments.push({
      kind: "code",
      ...(lang ? { lang } : {}),
      content: codeBuf.join("\n"),
    });
  }

  flushText();
  return segments;
}
