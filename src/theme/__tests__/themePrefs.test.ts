import { describe, expect, it } from "vitest";

import { DEFAULT_DENSITY } from "../density";
import { DEFAULT_THEME_ID } from "../themes";
import {
  DEFAULT_THEME_PREFS,
  THEME_PREFS_KEY,
  loadThemePrefs,
  saveThemePrefs,
  type StorageLike,
} from "../themePrefs";

/** A Map standing in for localStorage, per the serialPrefs.ts convention. */
function fakeStore(seed?: Record<string, string>): StorageLike & { map: Map<string, string> } {
  const map = new Map<string, string>(Object.entries(seed ?? {}));
  return {
    map,
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
  };
}

describe("loadThemePrefs", () => {
  it("returns the defaults for an empty store", () => {
    expect(loadThemePrefs(fakeStore())).toEqual(DEFAULT_THEME_PREFS);
  });

  it("round-trips a saved preference", () => {
    const s = fakeStore();
    saveThemePrefs(s, { themeId: "bancada-light", density: "comfortable" });
    expect(loadThemePrefs(s)).toEqual({
      themeId: "bancada-light",
      density: "comfortable",
    });
  });

  it("falls back rather than throwing on corrupt JSON", () => {
    // This runs before the first paint. There is no useful behaviour for a
    // corrupt record other than the default theme — certainly not a crash
    // that leaves the window blank.
    for (const junk of ["{", "null", "[]", '"a string"', "7", ""]) {
      const s = fakeStore({ [THEME_PREFS_KEY]: junk });
      expect(loadThemePrefs(s), junk).toEqual(DEFAULT_THEME_PREFS);
    }
  });

  it("keeps the valid half of a half-valid record", () => {
    // An unknown theme id is the expected shape of "this theme was removed",
    // or later "this imported .vsix is no longer installed". Losing the
    // density too would be gratuitous.
    const s = fakeStore({
      [THEME_PREFS_KEY]: JSON.stringify({
        themeId: "some-uninstalled-vsix",
        density: "comfortable",
      }),
    });
    expect(loadThemePrefs(s)).toEqual({
      themeId: DEFAULT_THEME_ID,
      density: "comfortable",
    });
  });

  it("rejects an unknown density", () => {
    const s = fakeStore({
      [THEME_PREFS_KEY]: JSON.stringify({ themeId: "bancada-light", density: "cosy" }),
    });
    expect(loadThemePrefs(s)).toEqual({
      themeId: "bancada-light",
      density: DEFAULT_DENSITY,
    });
  });

  it("survives a store that throws on read", () => {
    // Private-mode and locked-down WebKit both do this.
    const hostile: StorageLike = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {},
      removeItem: () => {},
    };
    expect(loadThemePrefs(hostile)).toEqual(DEFAULT_THEME_PREFS);
  });
});

describe("saveThemePrefs", () => {
  it("writes under the documented key", () => {
    const s = fakeStore();
    saveThemePrefs(s, { themeId: "bancada-contrast", density: "normal" });
    expect(s.map.has(THEME_PREFS_KEY)).toBe(true);
  });

  it("does not throw when the store refuses to write", () => {
    // A full or disabled store is not a reason to refuse to change the theme
    // for this session; it just will not be remembered.
    const hostile: StorageLike = {
      getItem: () => null,
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
      removeItem: () => {},
    };
    expect(() =>
      saveThemePrefs(hostile, { themeId: "bancada-light", density: "normal" }),
    ).not.toThrow();
  });
});
