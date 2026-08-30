import { describe, expect, it } from "vitest";

import { auditTheme } from "../themeAudit";
import { DEFAULT_THEME_ID, allThemes, themeById } from "../themes";
import { loadThemePrefs } from "../themePrefs";
import { COLOR_TOKENS, type Theme } from "../tokens";
import {
  IMPORTED_THEMES_KEY,
  MAX_IMPORTED,
  addImportedThemes,
  importThemeSource,
  loadImportedThemes,
  removeImportedTheme,
  saveImportedThemes,
} from "../importedThemes";
import type { StorageLike } from "../themePrefs";
import { AWFUL, DARK_PLUS } from "./fixtures/vscodeThemes";

function fakeStore(seed?: Record<string, string>): StorageLike & {
  map: Map<string, string>;
} {
  const map = new Map<string, string>(Object.entries(seed ?? {}));
  return {
    map,
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
  };
}

const stub = (id: string): Theme => ({
  id,
  name: id,
  appearance: "dark",
  colors: Object.fromEntries(COLOR_TOKENS.map((t) => [t, "#808080"])) as Theme["colors"],
});

describe("importThemeSource", () => {
  it("turns a source document into a renderable palette", () => {
    const out = importThemeSource("x", "Dark+", JSON.stringify(DARK_PLUS))!;
    expect(out).not.toBeNull();
    expect(out.theme.id).toBe("x");
    for (const token of COLOR_TOKENS) {
      expect(out.theme.colors[token], token).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it("prefers the package's label over the document's own name", () => {
    // `contributes.themes[].label` is what the user sees in VS Code, so it is
    // the name they will look for here.
    const out = importThemeSource("x", "Neon Night", JSON.stringify(DARK_PLUS))!;
    expect(out.theme.name).toBe("Neon Night");
  });

  it("repairs an unreadable theme rather than refusing it", () => {
    // The user picked this theme. "Here it is, with 6 contrast failures
    // fixed" respects that in a way "no" does not.
    const out = importThemeSource("x", "Awful", JSON.stringify(AWFUL))!;
    expect(out.violations.length).toBeGreaterThan(0);
    expect(auditTheme(out.theme)).toEqual([]);
  });

  it("reports nothing to repair for a well-made theme", () => {
    const out = importThemeSource("x", "Dark+", JSON.stringify(DARK_PLUS))!;
    expect(auditTheme(out.theme)).toEqual([]);
  });

  it("returns null for input that is not a theme document", () => {
    expect(importThemeSource("x", "l", "not json")).toBeNull();
    expect(importThemeSource("x", "l", "null")).toBeNull();
    expect(importThemeSource("x", "l", "[]")).not.toBeNull(); // an array is an object
    expect(importThemeSource("x", "l", "42")).toBeNull();
  });
});

describe("imported theme storage", () => {
  it("round-trips", () => {
    const s = fakeStore();
    saveImportedThemes(s, [stub("a"), stub("b")]);
    expect(loadImportedThemes(s).map((t) => t.id)).toEqual(["a", "b"]);
  });

  it("returns nothing for an empty or corrupt store", () => {
    expect(loadImportedThemes(fakeStore())).toEqual([]);
    for (const junk of ["{", "null", '"x"', "{}"]) {
      expect(loadImportedThemes(fakeStore({ [IMPORTED_THEMES_KEY]: junk })), junk).toEqual([]);
    }
  });

  it("drops entries that no longer look like themes", () => {
    // The stored shape is ours, but localStorage is the user's.
    const s = fakeStore({
      [IMPORTED_THEMES_KEY]: JSON.stringify([
        stub("good"),
        { id: "bad" },
        null,
        { id: "x", name: "x", appearance: "chartreuse", colors: {} },
      ]),
    });
    expect(loadImportedThemes(s).map((t) => t.id)).toEqual(["good"]);
  });

  it("survives a store that throws", () => {
    const hostile: StorageLike = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
      removeItem: () => {},
    };
    expect(loadImportedThemes(hostile)).toEqual([]);
    expect(() => saveImportedThemes(hostile, [stub("a")])).not.toThrow();
  });
});

describe("addImportedThemes", () => {
  it("puts new themes first", () => {
    const out = addImportedThemes([stub("old")], [stub("new")]);
    expect(out.map((t) => t.id)).toEqual(["new", "old"]);
  });

  it("replaces in place rather than duplicating on re-import", () => {
    // The id carries publisher, package and label, so re-importing the same
    // .vsix is an update, not a second copy.
    const first = addImportedThemes([], [stub("acme.neon:Dark")]);
    const again = addImportedThemes(first, [stub("acme.neon:Dark")]);
    expect(again).toHaveLength(1);
  });

  it("caps the list", () => {
    const many = Array.from({ length: MAX_IMPORTED + 5 }, (_, i) => stub(`t${i}`));
    expect(addImportedThemes([], many)).toHaveLength(MAX_IMPORTED);
  });
});

describe("removeImportedTheme", () => {
  it("removes by id and leaves the rest", () => {
    const out = removeImportedTheme([stub("a"), stub("b")], "a");
    expect(out.map((t) => t.id)).toEqual(["b"]);
  });
});

describe("imported themes are first-class in lookup", () => {
  it("resolves an imported id", () => {
    const imported = [stub("vsix:acme.neon:Dark")];
    expect(themeById("vsix:acme.neon:Dark", imported).id).toBe("vsix:acme.neon:Dark");
  });

  it("still falls back for an id that is gone", () => {
    expect(themeById("vsix:removed", []).id).toBe(DEFAULT_THEME_ID);
  });

  it("lists built-ins before imports", () => {
    const all = allThemes([stub("z")]);
    expect(all[0].id).toBe(DEFAULT_THEME_ID);
    expect(all[all.length - 1].id).toBe("z");
  });

  it("keeps a saved imported theme selected across a restart", () => {
    // Without passing the imported list to loadThemePrefs, an imported theme
    // reads as an unknown id and resets to the default on every launch — the
    // import would appear to not stick.
    const imported = [stub("vsix:acme.neon:Dark")];
    const s = fakeStore({
      "bancada.theme": JSON.stringify({
        themeId: "vsix:acme.neon:Dark",
        density: "normal",
      }),
    });
    expect(loadThemePrefs(s, imported).themeId).toBe("vsix:acme.neon:Dark");
    // ...and still falls back when that theme has since been removed.
    expect(loadThemePrefs(s, []).themeId).toBe(DEFAULT_THEME_ID);
  });
});
