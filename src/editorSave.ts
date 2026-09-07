/** Serialize disk writes so an older save can never finish after a newer one. */
export function createSaveQueue() {
  let tail: Promise<unknown> = Promise.resolve();
  return <T>(save: () => Promise<T>): Promise<T> => {
    const result = tail.then(save);
    tail = result.catch(() => {});
    return result;
  };
}

/** Save a finite snapshot; edits arriving during I/O remain dirty. */
export async function saveBuffers(
  buffers: Map<string, string>,
  write: (path: string, text: string) => Promise<unknown>,
  skip: ReadonlySet<string> = new Set(),
  paths?: readonly string[],
): Promise<void> {
  const snapshot = [...buffers].filter(([path]) => !paths || paths.includes(path));
  for (const [path, text] of snapshot) {
    if (skip.has(path)) continue;
    await write(path, text);
    if (buffers.get(path) === text) buffers.delete(path);
  }
}

/** A dismissal or failed/incomplete save must never authorize losing work. */
export async function mayLeaveEditor(
  hasChanges: () => boolean,
  choose: () => Promise<string>,
  save: () => Promise<boolean>,
): Promise<boolean> {
  if (!hasChanges()) return true;
  const choice = await choose();
  if (choice === "Discard") return true;
  if (choice !== "Save") return false;
  return (await save()) && !hasChanges();
}
