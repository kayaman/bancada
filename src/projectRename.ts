// Pure helpers for the Rename Project pane: pre-validation and the path math
// the pane needs to describe what the rename will do.
//
// core::project::validate_project_name refuses the same names, and the
// backend does the move — this exists for friendlier messages without a
// round trip.

import type { Check } from "./check";

export type { Check };

export type RenamePlan = {
  destDir: string; // sibling of the current dir, with the new name
  oldIno: string; // "<old>.ino"
  newIno: string; // "<new>.ino"
};

/** Last segment of a sketch dir, ignoring trailing slashes. */
function dirName(sketchDir: string): string {
  const dir = sketchDir.replace(/\/+$/, "");
  const cut = dir.lastIndexOf("/");
  return cut < 0 ? dir : dir.slice(cut + 1);
}

/** Mirrors core::project::validate_project_name for friendlier messages.
 *
 *  Spaces are refused rather than converted: the name becomes both the folder
 *  and the main `.ino` basename, so silently rewriting it would change the
 *  project's identity. */
export function checkProjectName(name: string, currentDir: string): Check {
  const n = name.trim();
  if (!n) return { ok: false, reason: "name the project" };
  if (n === dirName(currentDir))
    return { ok: false, reason: "that is already the project's name" };
  if (n.includes("/") || n.includes("\\"))
    return {
      ok: false,
      reason: "a project name cannot contain a path separator — the folder stays where it is",
    };
  if (n.length > 63)
    return {
      ok: false,
      reason: `a project name is 63 characters or fewer (got ${n.length})`,
    };
  if (n.startsWith("."))
    return {
      ok: false,
      reason:
        "a project name cannot start with `.` — a dotted folder is hidden and arduino-cli skips it",
    };
  const bad = [...n].find((c) => !/[A-Za-z0-9_.-]/.test(c));
  if (bad !== undefined) {
    const hint = bad === " " ? " — use `_` or `-` instead of spaces" : "";
    return {
      ok: false,
      reason: `a project name may only contain letters, digits, '_', '.' and '-' — found '${bad}'${hint}`,
    };
  }
  if (!/[A-Za-z0-9]/.test(n[0]))
    return { ok: false, reason: "a project name must start with a letter or a digit" };
  return { ok: true };
}

/** Where the rename lands and which `.ino` moves with it. */
export function renamePlan(sketchDir: string, newName: string): RenamePlan {
  const name = newName.trim();
  const dir = sketchDir.replace(/\/+$/, "");
  const cut = dir.lastIndexOf("/");
  const old = cut < 0 ? dir : dir.slice(cut + 1);
  return {
    destDir: cut < 0 ? name : dir.slice(0, cut + 1) + name,
    oldIno: `${old}.ino`,
    newIno: `${name}.ino`,
  };
}
