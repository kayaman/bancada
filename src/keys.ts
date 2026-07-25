/**
 * Keyboard accelerators for the menu bar: parse the display strings the menu
 * definition uses, render them back for the menu row, and match them against
 * real key events.
 *
 * Pure and framework-free so it can be unit tested; the menu bar and the global
 * key handler are the only consumers.
 *
 * Ctrl and Cmd are deliberately the same modifier here. Accelerators are
 * authored as `Ctrl+...` and matched against either, so one definition serves
 * both platforms without a per-platform table.
 */

export interface Accel {
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  /** Normalised to lower case, e.g. `"s"`, `"f5"`, `"arrowdown"`. */
  key: string;
}

/**
 * Parse `"Ctrl+Shift+N"` into an `Accel`, or `null` when the string is not a
 * well-formed accelerator (no key, an unknown modifier, or more than one key).
 *
 * Returning `null` rather than throwing keeps a typo in a menu definition from
 * taking the whole window down; the caller renders no shortcut and moves on.
 */
export function parseAccel(spec: string): Accel | null {
  const parts = spec
    .split("+")
    .map((p) => p.trim())
    .filter((p) => p.length > 0);
  if (parts.length === 0) return null;

  const accel: Accel = { ctrl: false, shift: false, alt: false, key: "" };
  for (const part of parts) {
    switch (part.toLowerCase()) {
      case "ctrl":
      case "cmd":
      case "control":
      case "meta":
        accel.ctrl = true;
        break;
      case "shift":
        accel.shift = true;
        break;
      case "alt":
      case "option":
        accel.alt = true;
        break;
      default:
        // Anything not a modifier is the key — and there can be only one.
        if (accel.key) return null;
        accel.key = part.toLowerCase();
    }
  }
  return accel.key ? accel : null;
}

/** Render an accelerator for display, e.g. `{ctrl,shift,key:"n"}` → `"Ctrl+Shift+N"`. */
export function formatAccel(accel: Accel): string {
  const parts: string[] = [];
  if (accel.ctrl) parts.push("Ctrl");
  if (accel.shift) parts.push("Shift");
  if (accel.alt) parts.push("Alt");
  parts.push(displayKey(accel.key));
  return parts.join("+");
}

/** `"s"` → `"S"`, `"f5"` → `"F5"`, `"arrowdown"` → `"ArrowDown"`. */
function displayKey(key: string): string {
  if (/^f\d{1,2}$/.test(key)) return key.toUpperCase();
  const named: Record<string, string> = {
    arrowup: "ArrowUp",
    arrowdown: "ArrowDown",
    arrowleft: "ArrowLeft",
    arrowright: "ArrowRight",
    escape: "Esc",
    enter: "Enter",
    " ": "Space",
    space: "Space",
  };
  return named[key] ?? key.charAt(0).toUpperCase() + key.slice(1);
}

/**
 * Does this key event fire the accelerator?
 *
 * Modifiers must match exactly — `Ctrl+S` does not fire on `Ctrl+Shift+S`, so
 * Save and Save All can coexist. The key comparison is case-insensitive because
 * a held Shift turns `e.key` into `"S"`.
 */
export function matchesAccel(
  e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">,
  accel: Accel,
): boolean {
  return (
    (e.ctrlKey || e.metaKey) === accel.ctrl &&
    e.shiftKey === accel.shift &&
    e.altKey === accel.alt &&
    e.key.toLowerCase() === accel.key
  );
}

/**
 * True when the event came from somewhere that owns its own keystrokes — a text
 * field, a select, or the code editor (CodeMirror presents as contenteditable).
 *
 * Unmodified keys must never be stolen from those. Modified ones (`Ctrl+S`) are
 * still fair game, which is why this is a separate question from `matchesAccel`.
 *
 * Structurally typed rather than `instanceof HTMLElement` so it is testable
 * without a DOM — the test suite runs in node.
 */
export function isTypingTarget(target: unknown): boolean {
  if (!target || typeof target !== "object") return false;
  const el = target as { tagName?: unknown; isContentEditable?: unknown };
  if (el.isContentEditable === true) return true;
  return (
    typeof el.tagName === "string" &&
    ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName)
  );
}
