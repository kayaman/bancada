import { describe, expect, it } from "vitest";
import { formatAccel, isTypingTarget, matchesAccel, parseAccel } from "../keys";

/** A minimal stand-in for KeyboardEvent — matchesAccel only reads these five. */
const ev = (
  key: string,
  mods: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {},
) => ({
  key,
  ctrlKey: mods.ctrl ?? false,
  metaKey: mods.meta ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
});

describe("parseAccel", () => {
  it("parses a bare key", () => {
    expect(parseAccel("F5")).toEqual({
      ctrl: false,
      shift: false,
      alt: false,
      key: "f5",
    });
  });

  it("parses modifiers in any order and is case-insensitive", () => {
    const expected = { ctrl: true, shift: true, alt: false, key: "n" };
    expect(parseAccel("Ctrl+Shift+N")).toEqual(expected);
    expect(parseAccel("shift+ctrl+n")).toEqual(expected);
    expect(parseAccel("CTRL + SHIFT + n")).toEqual(expected);
  });

  it("treats Cmd and Meta as Ctrl so one definition serves both platforms", () => {
    expect(parseAccel("Cmd+S")).toEqual(parseAccel("Ctrl+S"));
    expect(parseAccel("Meta+S")).toEqual(parseAccel("Ctrl+S"));
  });

  it("rejects malformed specs rather than throwing", () => {
    expect(parseAccel("")).toBeNull();
    expect(parseAccel("+")).toBeNull();
    expect(parseAccel("Ctrl")).toBeNull(); // modifier with no key
    expect(parseAccel("Ctrl+Shift")).toBeNull();
    expect(parseAccel("Ctrl+A+B")).toBeNull(); // two keys
  });
});

describe("formatAccel", () => {
  it("round-trips the specs used in the menu definition", () => {
    for (const spec of [
      "Ctrl+S",
      "Ctrl+Shift+S",
      "Ctrl+Shift+N",
      "Ctrl+O",
      "Ctrl+R",
      "Ctrl+U",
      "Ctrl+M",
      "Ctrl+B",
      "Ctrl+J",
      "Ctrl+Shift+J",
      "Ctrl+Q",
      "F5",
    ]) {
      expect(formatAccel(parseAccel(spec)!)).toBe(spec);
    }
  });

  it("orders modifiers consistently regardless of input order", () => {
    expect(formatAccel(parseAccel("shift+ctrl+alt+p")!)).toBe("Ctrl+Shift+Alt+P");
  });

  it("spells out named keys", () => {
    expect(formatAccel(parseAccel("Escape")!)).toBe("Esc");
    expect(formatAccel(parseAccel("ArrowDown")!)).toBe("ArrowDown");
    expect(formatAccel(parseAccel("Space")!)).toBe("Space");
  });
});

describe("matchesAccel", () => {
  const ctrlS = parseAccel("Ctrl+S")!;
  const ctrlShiftS = parseAccel("Ctrl+Shift+S")!;

  it("matches the intended combination", () => {
    expect(matchesAccel(ev("s", { ctrl: true }), ctrlS)).toBe(true);
  });

  it("accepts Cmd in place of Ctrl", () => {
    expect(matchesAccel(ev("s", { meta: true }), ctrlS)).toBe(true);
  });

  it("matches the upper-case key a held Shift produces", () => {
    expect(matchesAccel(ev("S", { ctrl: true, shift: true }), ctrlShiftS)).toBe(true);
  });

  // The regression that matters: Save must not fire when the user asks for
  // Save All, and vice versa.
  it("distinguishes Ctrl+S from Ctrl+Shift+S", () => {
    expect(matchesAccel(ev("S", { ctrl: true, shift: true }), ctrlS)).toBe(false);
    expect(matchesAccel(ev("s", { ctrl: true }), ctrlShiftS)).toBe(false);
  });

  it("requires the modifier", () => {
    expect(matchesAccel(ev("s"), ctrlS)).toBe(false);
  });

  it("rejects extra modifiers", () => {
    expect(matchesAccel(ev("s", { ctrl: true, alt: true }), ctrlS)).toBe(false);
  });

  it("matches unmodified function keys", () => {
    expect(matchesAccel(ev("F5"), parseAccel("F5")!)).toBe(true);
    expect(matchesAccel(ev("F5", { ctrl: true }), parseAccel("F5")!)).toBe(false);
  });
});

describe("isTypingTarget", () => {
  it("is true for text entry and selects", () => {
    for (const tagName of ["INPUT", "TEXTAREA", "SELECT"]) {
      expect(isTypingTarget({ tagName })).toBe(true);
    }
  });

  it("is true for contenteditable, which is how CodeMirror presents itself", () => {
    expect(isTypingTarget({ tagName: "DIV", isContentEditable: true })).toBe(true);
  });

  it("is false for ordinary elements and for non-elements", () => {
    expect(isTypingTarget({ tagName: "DIV" })).toBe(false);
    expect(isTypingTarget({ tagName: "BUTTON" })).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
    expect(isTypingTarget(undefined)).toBe(false);
  });
});
