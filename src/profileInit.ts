// TS mirror of core's profile_name_for_fqbn (core/src/project.rs), so the
// form's suggested name equals what the backend derives for the same board.

export function profileNameForFqbn(fqbn: string): string {
  const sanitize = (s: string) =>
    s.replace(/[^A-Za-z0-9_-]/g, "_").replace(/^_+|_+$/g, "");
  const board = (fqbn.split(":")[2] ?? "").trim();
  const candidate = sanitize(board);
  if (candidate) return candidate;
  return sanitize(fqbn.trim()) || "default";
}

export type ProfileFormMode = "bootstrap" | "add" | "retarget";

export type SubmitPlan =
  | { kind: "retarget"; profile: string; fqbn: string }
  | { kind: "create"; profile: string; fqbn: string; copyLibsFrom?: string };

/** Pure decision behind ProfileInit's submit button: what command to call and
 *  with what arguments, or null when the form isn't valid yet. Mirrors the
 *  three modes' rules — retarget targets the *selected* profile (not the
 *  name field, which retarget doesn't show); add carries copyLibsFrom from
 *  the currently selected profile; bootstrap never copies libraries. */
export function submitPlan(
  mode: ProfileFormMode,
  currentProfile: string | null,
  name: string,
  fqbn: string,
): SubmitPlan | null {
  if (!fqbn.trim()) return null;
  if (mode === "retarget") {
    if (!currentProfile) return null;
    return { kind: "retarget", profile: currentProfile, fqbn };
  }
  const profile = name.trim();
  if (!profile) return null;
  return {
    kind: "create",
    profile,
    fqbn,
    copyLibsFrom: mode === "add" ? (currentProfile ?? undefined) : undefined,
  };
}

/** Board picker's preselected value: retarget offers the profile's current
 *  board, bootstrap/add offer the board detected on the selected port. */
export function initialFqbn(
  mode: ProfileFormMode,
  currentFqbn: string | null,
  detectedFqbn: string | null,
): string {
  return (mode === "retarget" ? currentFqbn : detectedFqbn) ?? "";
}

/** First three `:`-segments of an fqbn (vendor:arch:board), trimmed — the
 *  part that identifies the board itself, ignoring any trailing options. */
function baseBoard(fqbn: string): string {
  return fqbn
    .trim()
    .split(":")
    .slice(0, 3)
    .map((s) => s.trim())
    .join(":");
}

/** The fqbn a retarget should submit: re-picking the same base board keeps
 *  the current fqbn verbatim (board options ride on it and the picker's list
 *  values never carry options); a different board uses the picked value. */
export function effectiveRetargetFqbn(picked: string, currentFqbn: string | null): string {
  const current = currentFqbn?.trim();
  if (!current) return picked;
  return baseBoard(picked) === baseBoard(current) ? current : picked;
}
