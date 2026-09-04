// Pure helpers for the Rename Project pane: pre-validation and the path math
// the pane needs to describe what the rename will do.
//
// core::project::validate_project_name refuses the same names, and the
// backend does the move — this exists for friendlier messages without a
// round trip.

import type { Check } from "./check";

export type { Check };

/** Which paradigm's rules apply. `unknown` is treated as Arduino, matching
 *  the backend, where `detect_kind`'s Unknown falls through to the sketch
 *  path. */
export type RenameKind = "arduino" | "idf" | "unknown";

export type RenamePlan = {
  destDir: string; // sibling of the current dir, with the new name
  /** The main sketch that moves with the folder — Arduino only. An ESP-IDF
   *  project renames no source at all: ESP-IDF names its files in
   *  `main/CMakeLists.txt` and does not care what they are called. */
  oldIno?: string;
  newIno?: string;
  /** What the CMake project call becomes — ESP-IDF only. That call *is* the
   *  project's name as far as the build is concerned; `<name>.elf` comes from
   *  it, which is why it moves with the directory. */
  cmakeName?: string;
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
export function checkProjectName(
  name: string,
  currentDir: string,
  kind: RenameKind = "arduino",
): Check {
  const n = name.trim();
  if (!n) return { ok: false, reason: "name the project" };
  if (n === dirName(currentDir))
    return { ok: false, reason: "that is already the project's name" };
  if (n.includes("/") || n.includes("\\"))
    return {
      ok: false,
      reason: "a project name cannot contain a path separator — the folder stays where it is",
    };
  // 63 is arduino-lint's sketch-folder limit; 64 is `idfproject`'s, which is
  // not a CMake constraint at all but the point where the name — an ELF
  // filename and an NVS namespace in one template — becomes a footgun.
  const maxLen = kind === "idf" ? 64 : 63;
  if (n.length > maxLen)
    return {
      ok: false,
      reason: `a project name is ${maxLen} characters or fewer (got ${n.length})`,
    };
  if (n.startsWith("."))
    return {
      ok: false,
      reason:
        "a project name cannot start with `.` — a dotted folder is hidden and arduino-cli skips it",
    };
  const bad = [...n].find((c) =>
    kind === "idf" ? !/[A-Za-z0-9_-]/.test(c) : !/[A-Za-z0-9_.-]/.test(c),
  );
  if (bad !== undefined) {
    const hint = bad === " " ? " — use `_` or `-` instead of spaces" : "";
    return {
      ok: false,
      reason:
        kind === "idf"
          ? `an ESP-IDF project name may only contain letters, digits, '_' and '-' — found '${bad}'${hint}`
          : `a project name may only contain letters, digits, '_', '.' and '-' — found '${bad}'${hint}`,
    };
  }
  // The one rule that genuinely inverts: a CMake project name becomes a
  // target and a C identifier prefix, so it may not start with a digit —
  // which an Arduino sketch folder may.
  if (kind === "idf" && /[0-9-]/.test(n[0]))
    return {
      ok: false,
      reason:
        "an ESP-IDF project name must not start with a digit or '-' — it becomes a CMake target and a C identifier prefix",
    };
  if (!/[A-Za-z0-9]/.test(n[0]))
    return { ok: false, reason: "a project name must start with a letter or a digit" };
  return { ok: true };
}

/** Where the rename lands, and what moves with it — which differs by
 *  paradigm, so the pane describes the operation that will actually happen. */
export function renamePlan(
  sketchDir: string,
  newName: string,
  kind: RenameKind = "arduino",
): RenamePlan {
  const name = newName.trim();
  const dir = sketchDir.replace(/\/+$/, "");
  const cut = dir.lastIndexOf("/");
  const old = cut < 0 ? dir : dir.slice(cut + 1);
  const destDir = cut < 0 ? name : dir.slice(0, cut + 1) + name;
  return kind === "idf"
    ? { destDir, cmakeName: `project(${name})` }
    : { destDir, oldIno: `${old}.ino`, newIno: `${name}.ino` };
}
