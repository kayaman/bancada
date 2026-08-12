// One shape for "is this allowed, and if not why".
//
// Four modules had grown their own copy of this union — `PathCheck` in
// explorerOps, `NewFileCheck` in newFile, and two separate `Check`s in
// publishRepo and projectRename that collided on the name, so importing both
// into one file needed an alias. They were byte-identical apart from
// newFile's payload on success.
//
// The `reason` is **user-facing prose**, shown verbatim in a tooltip, a
// warning box or the status bar. Write it as a sentence fragment a person
// would say, not as an error code.

export type Check = { ok: true } | { ok: false; reason: string };

/** The blocking reason, or null when there is none. */
export function reasonOf(check: Check): string | null {
  return check.ok ? null : check.reason;
}
