import { describe, expect, it, vi } from "vitest";
import { createSaveQueue, mayLeaveEditor, saveBuffers } from "../editorSave";
import appSource from "../App.tsx?raw";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("saving editor buffers", () => {
  it("keeps text typed while a disk write is pending", async () => {
    const buffers = new Map([["main.ino", "old"]]);
    const disk = deferred();
    const write = vi.fn(() => disk.promise);
    const saving = saveBuffers(buffers, write);
    buffers.set("main.ino", "new");
    disk.resolve();
    await saving;
    expect(write).toHaveBeenCalledWith("main.ino", "old");
    expect(buffers.get("main.ino")).toBe("new");
  });

  it("clears successful writes but keeps failed and unattempted files", async () => {
    const buffers = new Map([["a", "A"], ["b", "B"], ["c", "C"]]);
    await expect(saveBuffers(buffers, async (path) => {
      if (path === "b") throw new Error("disk full");
    })).rejects.toThrow("disk full");
    expect([...buffers.keys()]).toEqual(["b", "c"]);
  });

  it("does not save newly opened files or overwrite conflicted files", async () => {
    const buffers = new Map([["a", "A"], ["conflict", "mine"]]);
    const write = vi.fn(async () => { buffers.set("new", "new text"); });
    await saveBuffers(buffers, write, new Set(["conflict"]));
    expect(write).toHaveBeenCalledTimes(1);
    expect([...buffers.keys()]).toEqual(["conflict", "new"]);
  });

  it("saving the current file leaves other tabs dirty", async () => {
    const buffers = new Map([["a", "A"], ["b", "B"]]);
    await saveBuffers(buffers, async () => {}, new Set(), ["a"]);
    expect([...buffers.keys()]).toEqual(["b"]);
  });

  it("serializes overlapping saves and writes the newest text last", async () => {
    const queue = createSaveQueue();
    const buffers = new Map([["a", "old"]]);
    const firstWrite = deferred();
    const started = deferred();
    const writes: string[] = [];
    const first = queue(() => saveBuffers(buffers, async (_, text) => {
      started.resolve();
      await firstWrite.promise;
      writes.push(text);
    }));
    await started.promise;
    buffers.set("a", "new");
    const second = queue(() => saveBuffers(buffers, async (_, text) => {
      writes.push(text);
    }));
    expect(writes).toEqual([]);
    firstWrite.resolve();
    await Promise.all([first, second]);
    expect(writes).toEqual(["old", "new"]);
    expect(buffers.size).toBe(0);
  });

  it("a failed write does not poison subsequent saves", async () => {
    const queue = createSaveQueue();
    await expect(queue(async () => { throw new Error("offline"); })).rejects.toThrow();
    await expect(queue(async () => "saved")).resolves.toBe("saved");
  });

  it("a save of an old project cannot clear a new project's same-named file", async () => {
    const oldBuffers = new Map([["main.ino", "old project"]]);
    const pending = deferred();
    const saving = saveBuffers(oldBuffers, () => pending.promise);
    const currentBuffers = new Map([["main.ino", "new project"]]);
    pending.resolve();
    await saving;
    expect(currentBuffers.get("main.ino")).toBe("new project");
  });
});

describe("leaving an editor", () => {
  it("does not prompt for a clean project", async () => {
    const choose = vi.fn();
    expect(await mayLeaveEditor(() => false, choose, vi.fn())).toBe(true);
    expect(choose).not.toHaveBeenCalled();
  });

  it.each(["Cancel", "", "unexpected"])("%s preserves changes without saving", async (choice) => {
    const save = vi.fn();
    expect(await mayLeaveEditor(() => true, async () => choice, save)).toBe(false);
    expect(save).not.toHaveBeenCalled();
  });

  it("Discard permits leaving without writing", async () => {
    const save = vi.fn();
    expect(await mayLeaveEditor(() => true, async () => "Discard", save)).toBe(true);
    expect(save).not.toHaveBeenCalled();
  });

  it("Save permits leaving only after all changes are saved", async () => {
    let dirty = true;
    expect(await mayLeaveEditor(() => dirty, async () => "Save", async () => {
      dirty = false;
      return true;
    })).toBe(true);
    expect(await mayLeaveEditor(() => true, async () => "Save", async () => true)).toBe(false);
    expect(await mayLeaveEditor(() => true, async () => "Save", async () => false)).toBe(false);
  });
});

describe("editor protection wiring", () => {
  it("checks before project buffers are reset", () => {
    const load = appSource.slice(appSource.indexOf("const loadSketch ="), appSource.indexOf("const openFileInEditor ="));
    expect(load.indexOf("await leaveEditorRef.current()")).toBeGreaterThan(-1);
    expect(load.indexOf("await leaveEditorRef.current()")).toBeLessThan(load.indexOf("buffersRef.current = new Map()"));
  });

  it("intercepts desktop close and checks before destroying the window", () => {
    const close = appSource.slice(appSource.indexOf("appWindow.onCloseRequested"), appSource.indexOf("const beforeUnload"));
    expect(close).toContain("event.preventDefault()");
    expect(close).toContain("if (await leaveEditorRef.current())");
    expect(close.indexOf("await leaveEditorRef.current()")).toBeLessThan(close.indexOf("appWindow.destroy()"));
  });

  it("all bulk-save callers stop when saving fails or leaves newer edits", () => {
    expect(appSource.match(/if \(!\(await saveAll\(\)\)\) return;/g)).toHaveLength(4);
    expect(appSource).not.toMatch(/^\s+await saveAll\(\);/m);
  });
});
