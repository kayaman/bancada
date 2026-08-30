// Which theme and density the user picked, and the `localStorage` shape
// behind them.
//
// localStorage rather than settings.json, for the same reason the serial
// monitor's baud and line-ending live there: these are UI preferences, not
// project state, and they must be readable *synchronously* at startup. A theme
// fetched over Tauri IPC arrives a frame or two after the window paints, which
// is long enough to see the default palette flash past on the way to the one
// you chose — worst on the light theme, where the flash is a dark window.
//
// Storage-injected and pure, matching serialPrefs.ts, so the fallback
// behaviour can be tested without a DOM.

import { DEFAULT_DENSITY, isDensity, type Density } from "./density";
import { BUILTIN_THEMES, DEFAULT_THEME_ID } from "./themes";

/** The slice of `Storage` these helpers use, so tests can hand in a Map. */
export type StorageLike = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export const THEME_PREFS_KEY = "bancada.theme";

export type ThemePrefs = {
  themeId: string;
  density: Density;
};

export const DEFAULT_THEME_PREFS: ThemePrefs = {
  themeId: DEFAULT_THEME_ID,
  density: DEFAULT_DENSITY,
};

function knownTheme(id: unknown, extra: readonly { id: string }[]): id is string {
  return (
    typeof id === "string" &&
    (BUILTIN_THEMES.some((t) => t.id === id) || extra.some((t) => t.id === id))
  );
}

/** Reads the saved preference, falling back per-field.
 *
 *  Never throws. This runs before the app renders, and there is no useful
 *  behaviour for "the theme file was corrupt" other than the default theme —
 *  certainly not a blank window. A half-valid record keeps the half that is
 *  valid: a recognised density with an unknown theme id (one that was removed,
 *  or an imported theme that is no longer installed) still gets its density. */
export function loadThemePrefs(
  s: StorageLike,
  /** Imported themes, so a saved import is not treated as an unknown id and
   *  reset to the default on every launch. */
  imported: readonly { id: string }[] = [],
): ThemePrefs {
  let raw: unknown;
  try {
    const text = s.getItem(THEME_PREFS_KEY);
    if (!text) return { ...DEFAULT_THEME_PREFS };
    raw = JSON.parse(text);
  } catch {
    return { ...DEFAULT_THEME_PREFS };
  }
  if (typeof raw !== "object" || raw === null) return { ...DEFAULT_THEME_PREFS };
  const rec = raw as Record<string, unknown>;
  return {
    themeId: knownTheme(rec.themeId, imported) ? rec.themeId : DEFAULT_THEME_ID,
    density: isDensity(rec.density) ? rec.density : DEFAULT_DENSITY,
  };
}

/** Persists the preference. Silent on failure — a full or disabled store is
 *  not a reason to refuse to change the theme for this session. */
export function saveThemePrefs(s: StorageLike, p: ThemePrefs): void {
  try {
    s.setItem(THEME_PREFS_KEY, JSON.stringify(p));
  } catch {
    /* ignore */
  }
}
