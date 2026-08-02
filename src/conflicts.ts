// Agent/user edit-conflict wording and the guard that blocks builds.
//
// A "conflict" is a file the assistant rewrote on disk while the user still
// had unsaved edits to it in the editor buffer. There is no discard action
// in this editor (a dirty buffer always wins on reopen), so the only real
// resolution is for the user to choose: Ctrl+S to keep their version, or
// reload to take the assistant's.
//
// Extracted from App.tsx because the guard is a safety rule with three call
// sites (`sendToAgent`, `verify`, `upload`) and App.tsx has no test harness —
// the repo tests pure modules (`newFile.ts`, `ports.ts`, `portWatch.ts`) and
// this belongs with them.

/** The one wording, shared by the status-bar notify and the persistent
 *  banner so the two can never drift. */
export function conflictMessage(files: string[]): string {
  const one = files.length === 1;
  return (
    `the assistant changed ${files.join(", ")} on disk while you had unsaved edits. ` +
    `Open ${one ? "it" : "each file"} and press Ctrl+S to keep your version ` +
    `(or reload the file to take the assistant's), which resolves the conflict.`
  );
}

export interface ConflictBlock {
  blocked: boolean;
  /** Present only when `blocked` — the message to show the user. */
  message?: string;
}

/**
 * Should `action` refuse to run?
 *
 * `saveAll` already declines to overwrite a conflicted buffer, but that on
 * its own was not enough for Verify and Upload: `saveAll`'s warning is a
 * transient status-bar notify that the very next line ("Compiling…")
 * overwrote, and the build then ran anyway. The user saw a compile — or a
 * **flash of a physical board** — built from the assistant's on-disk version
 * while their own unsaved edits were invisible and unmentioned.
 *
 * So the refusal is explicit and shared, and the banner that accompanies it
 * is persistent. Both build paths and the send path use this.
 */
export function blockedByConflict(
  conflicts: readonly string[],
  action: string,
): ConflictBlock {
  if (conflicts.length === 0) return { blocked: false };
  return {
    blocked: true,
    message: `${action} cancelled — ${conflictMessage([...conflicts])}`,
  };
}
