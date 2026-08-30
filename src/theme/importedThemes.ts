// Imported themes: the pipeline from a file on disk to a palette in the
// picker, and where the result is kept.
//
// What is stored is the RESULT of the import — a finished `Theme`, fifteen
// hex values — not the source document. Three reasons, all of them the same
// reason: the mapping and repair are deterministic but not free, the source
// can be tens of kilobytes of JSON per theme, and re-deriving a palette at
// every launch means a theme could silently change under the user when the
// mapping is improved. Import once, keep the answer.

import { auditTheme, repairTheme, type Violation } from "./themeAudit";
import type { Theme } from "./tokens";
import type { StorageLike } from "./themePrefs";
import { mapVsCodeTheme, type VsCodeTheme } from "./vscode";

export const IMPORTED_THEMES_KEY = "bancada.themes.imported";

/** Keeps the picker usable and localStorage small. */
export const MAX_IMPORTED = 32;

export type ImportOutcome = {
  theme: Theme;
  /** Floors the theme broke as authored, and what they were moved to. */
  violations: Violation[];
};

/**
 * Turns one source document into a palette Bancada will render.
 *
 * Always succeeds if the JSON parses: a theme that defines nothing still maps
 * to a complete palette, and one that is unreadable is repaired rather than
 * refused. Refusing would be the wrong call — the user picked this theme, and
 * "your theme has 6 contrast failures, here it is with them fixed" respects
 * that in a way "no" does not.
 */
export function importThemeSource(
  id: string,
  label: string,
  json: string,
): ImportOutcome | null {
  let doc: unknown;
  try {
    doc = JSON.parse(json);
  } catch {
    return null;
  }
  if (typeof doc !== "object" || doc === null) return null;

  const raw = doc as VsCodeTheme;
  // The label from `contributes.themes[].label` is the one the user sees in
  // VS Code, so it beats the theme document's own `name` where they differ.
  const mapped = mapVsCodeTheme({ ...raw, name: label || raw.name }, id);
  const violations = auditTheme(mapped);
  const { theme } = repairTheme(mapped);
  return { theme, violations };
}

function isTheme(v: unknown): v is Theme {
  if (typeof v !== "object" || v === null) return false;
  const t = v as Record<string, unknown>;
  return (
    typeof t.id === "string" &&
    typeof t.name === "string" &&
    (t.appearance === "dark" || t.appearance === "light") &&
    typeof t.colors === "object" &&
    t.colors !== null
  );
}

/** Reads the stored imports, dropping anything that no longer looks like a
 *  theme. Never throws — this runs on the startup path. */
export function loadImportedThemes(s: StorageLike): Theme[] {
  let raw: unknown;
  try {
    const text = s.getItem(IMPORTED_THEMES_KEY);
    if (!text) return [];
    raw = JSON.parse(text);
  } catch {
    return [];
  }
  if (!Array.isArray(raw)) return [];
  return raw.filter(isTheme).slice(0, MAX_IMPORTED);
}

/** Persists the list. Silent on failure, like `saveThemePrefs`. */
export function saveImportedThemes(s: StorageLike, themes: Theme[]): void {
  try {
    s.setItem(IMPORTED_THEMES_KEY, JSON.stringify(themes.slice(0, MAX_IMPORTED)));
  } catch {
    /* ignore */
  }
}

/** Adds or replaces themes by id, newest first, capped.
 *
 *  Re-importing the same `.vsix` updates in place rather than accumulating
 *  duplicates — the id carries publisher, package and label, so the same
 *  theme from the same package is the same entry. */
export function addImportedThemes(existing: Theme[], incoming: Theme[]): Theme[] {
  const ids = new Set(incoming.map((t) => t.id));
  return [...incoming, ...existing.filter((t) => !ids.has(t.id))].slice(0, MAX_IMPORTED);
}

export function removeImportedTheme(existing: Theme[], id: string): Theme[] {
  return existing.filter((t) => t.id !== id);
}
