// Pure summarizer for "Continue this chat"'s fallback path: when native
// `--resume <session_id>` can't recover the original CLI transcript (no
// session_id was ever recorded, or the CLI has since forgotten it),
// `distillFacts` boils a saved chat's `AgentSnapshot` down into a bounded
// plain-text block that rides the fresh session's system prompt, so the new
// child still has *some* memory of what came before.
//
// Deliberately dependency-free beyond the snapshot's own types: the input is
// untrusted disk data replayed through `applyChatOps` (see chatLog.ts), so
// nothing here may throw regardless of how large or malformed it is, and the
// output is hard-capped so it can never blow out a system prompt.

import type { AgentSnapshot } from "./agentStore";

const HARD_CAP = 2048;
const MAX_USER_TEXTS = 3;
const USER_TEXT_TRUNCATE = 300;
const ASSISTANT_TEXT_TRUNCATE = 600;
const MAX_FILES = 15;

/** Truncate `s` to at most `max` chars, appending "…" when it was cut. */
function truncate(s: string, max: number): string {
  return s.length > max ? s.slice(0, max) + "…" : s;
}

/**
 * Distill `snap` into a bounded facts block. Sections are included only
 * when they have something to say (an empty snapshot yields ""); non-empty
 * sections are joined with a blank line. The whole result is hard-capped at
 * 2048 chars, truncated at a char boundary with a trailing "…".
 */
export function distillFacts(snap: AgentSnapshot): string {
  const sections: string[] = [];
  const messages = Array.isArray(snap.messages) ? snap.messages : [];

  // ---- Recent requests: last 3 user texts, oldest first ----
  const userTexts: string[] = [];
  for (const m of messages) {
    if (!m || m.kind !== "user" || typeof m.text !== "string") continue;
    userTexts.push(m.text);
  }
  const recentUserTexts = userTexts.slice(-MAX_USER_TEXTS);
  if (recentUserTexts.length > 0) {
    sections.push(
      "Recent requests:\n" +
        recentUserTexts
          .map((t) => `- ${truncate(t, USER_TEXT_TRUNCATE)}`)
          .join("\n"),
    );
  }

  // ---- Last assistant answer ----
  let lastAssistantText: string | undefined;
  for (const m of messages) {
    if (!m || m.kind !== "assistant" || typeof m.text !== "string") continue;
    if (m.text.length === 0) continue;
    lastAssistantText = m.text;
  }
  if (lastAssistantText !== undefined) {
    sections.push(
      "Last assistant answer:\n" + truncate(lastAssistantText, ASSISTANT_TEXT_TRUNCATE),
    );
  }

  // ---- Files touched: unique file_path values from Edit/Write tool cards ----
  const files: string[] = [];
  for (const m of messages) {
    if (files.length >= MAX_FILES) break;
    if (!m || m.kind !== "tool") continue;
    if (m.name !== "Edit" && m.name !== "Write") continue;
    const input = m.input;
    if (typeof input !== "object" || input === null) continue;
    const fp = (input as Record<string, unknown>).file_path;
    if (typeof fp !== "string" || fp.length === 0) continue;
    if (files.includes(fp)) continue;
    files.push(fp);
  }
  if (files.length > 0) {
    sections.push("Files touched:\n" + files.map((f) => `- ${f}`).join("\n"));
  }

  // ---- Last build/upload: latest verify/upload outcome from the ledger ----
  let lastBuild: { label: "verify" | "upload"; status: string } | undefined;
  for (const m of messages) {
    if (!m || m.kind !== "turn_end") continue;
    const tools = Array.isArray(m.tools) ? m.tools : [];
    for (const t of tools) {
      if (!t || typeof t.name !== "string" || typeof t.status !== "string") continue;
      const lower = t.name.toLowerCase();
      if (lower.includes("verify")) lastBuild = { label: "verify", status: t.status };
      else if (lower.includes("upload")) lastBuild = { label: "upload", status: t.status };
    }
  }
  if (lastBuild) {
    sections.push(`Last build/upload: ${lastBuild.label} ${lastBuild.status}`);
  }

  let out = sections.join("\n\n");
  if (out.length > HARD_CAP) out = out.slice(0, HARD_CAP - 1) + "…";
  return out;
}
