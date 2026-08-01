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
