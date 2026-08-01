# Profile Bootstrap and New-File Creation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a sketch opened without `sketch.yaml` gain a profile from the toolbar, and let new files be created from the file tree and the editor tab strip.

**Architecture:** Yaml creation goes through a new `SketchProject::init_profile` in the core crate (one source of truth for the yaml format), exposed as a Tauri command. File creation reuses the existing `write_sketch_file` command; the frontend adds validation (pure module) and two `＋` affordances sharing one inline-input component.

**Tech Stack:** Rust (core crate, serde_yaml, tempfile tests), Tauri 2 commands, React 18 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-08-01-profile-bootstrap-and-new-files-design.md`

## Global Constraints

- Frontend interactive logic lives in pure modules with vitest tests (`editorTabs.ts` pattern); components stay thin. There is no React test harness — do not add one.
- Never overwrite: a duplicate profile name and an existing file path are both errors surfaced via `notify(msg, true)`.
- Commit messages: imperative first line, explanatory body, trailer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Run commands from the repo root `/home/kayaman/Projects/bancada`.

---

### Task 1: `SketchProject::init_profile` in core

**Files:**
- Modify: `core/src/sketch.rs` (method after `save_yaml`, ~line 123; tests in the existing `mod tests`)

**Interfaces:**
- Consumes: existing `SketchProject::{open, load_yaml, save_yaml}`, `SketchYaml`, `Profile`, `Error::Other`.
- Produces: `pub fn init_profile(&self, profile_name: &str, fqbn: &str) -> Result<SketchYaml>` — used by Task 2.

- [ ] **Step 1: Write the failing tests** (inside `mod tests` in `core/src/sketch.rs`)

```rust
    // ---------- init_profile ----------

    #[test]
    fn init_profile_creates_yaml_and_sets_default() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();

        let y = proj.init_profile("weather", "esp32:esp32:esp32s3").unwrap();

        assert!(tmp.path().join("sketch.yaml").is_file());
        assert_eq!(y.profiles["weather"].fqbn, "esp32:esp32:esp32s3");
        assert_eq!(y.default_profile.as_deref(), Some("weather"));
        // And it round-trips from disk, not just in memory.
        assert_eq!(proj.load_yaml().unwrap(), y);
    }

    #[test]
    fn init_profile_rejects_a_duplicate_name() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        proj.init_profile("weather", "esp32:esp32:esp32s3").unwrap();

        let err = proj.init_profile("weather", "esp32:esp32:esp32c6");
        assert!(err.is_err(), "a second `weather` must not overwrite the first");
        // The original is untouched.
        assert_eq!(
            proj.load_yaml().unwrap().profiles["weather"].fqbn,
            "esp32:esp32:esp32s3"
        );
    }

    #[test]
    fn init_profile_keeps_existing_default_and_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("sketch.yaml"),
            "default_profile: base\nprofiles:\n  base:\n    fqbn: esp32:esp32:esp32\n",
        )
        .unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();

        let y = proj.init_profile("c6", "esp32:esp32:esp32c6").unwrap();

        assert_eq!(y.default_profile.as_deref(), Some("base"), "default must not move");
        assert_eq!(y.profiles["base"].fqbn, "esp32:esp32:esp32");
        assert_eq!(y.profiles["c6"].fqbn, "esp32:esp32:esp32c6");
    }

    #[test]
    fn init_profile_rejects_blank_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = SketchProject::open(tmp.path()).unwrap();
        assert!(proj.init_profile("  ", "esp32:esp32:esp32").is_err());
        assert!(proj.init_profile("weather", "  ").is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bancada-core init_profile`
Expected: compile error — `init_profile` not found.

- [ ] **Step 3: Write the implementation** (in `impl SketchProject`, directly after `save_yaml`)

```rust
    /// Create `profile_name` pointing at `fqbn`, creating sketch.yaml when
    /// absent. Never overwrites: a profile that already exists is an error.
    /// The sketch's first profile also becomes `default_profile`, so gaining
    /// a profile gives Verify/Flash a working default immediately.
    pub fn init_profile(&self, profile_name: &str, fqbn: &str) -> Result<SketchYaml> {
        let name = profile_name.trim();
        if name.is_empty() {
            return Err(Error::Other("a profile needs a name".into()));
        }
        let fqbn = fqbn.trim();
        if fqbn.is_empty() {
            return Err(Error::Other("a profile needs a board (FQBN)".into()));
        }
        let mut y = self.load_yaml()?;
        if y.profiles.contains_key(name) {
            return Err(Error::Other(format!("profile `{name}` already exists")));
        }
        y.profiles.insert(
            name.to_string(),
            Profile {
                fqbn: fqbn.to_string(),
                ..Default::default()
            },
        );
        if y.default_profile.is_none() {
            y.default_profile = Some(name.to_string());
        }
        self.save_yaml(&y)?;
        Ok(y)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p bancada-core init_profile`
Expected: 4 passed. Then `cargo test -p bancada-core` — everything else still green.

- [ ] **Step 5: Commit**

```bash
git add core/src/sketch.rs
git commit -m "Add SketchProject::init_profile — bootstrap sketch.yaml with a first profile"
```

---

### Task 2: Tauri command and api.ts wrapper

**Files:**
- Modify: `src-tauri/src/lib.rs` (command near `write_sketch_file` ~line 141; register in the `generate_handler![...]` list next to `load_sketch_yaml`)
- Modify: `src/api.ts` (wrapper next to `loadSketchYaml`)

**Interfaces:**
- Consumes: Task 1's `init_profile`.
- Produces: TS `initProfile(sketchDir: string, profile: string, fqbn: string): Promise<SketchYaml>` — used by Task 5.

- [ ] **Step 1: Add the command.** Open the existing `load_sketch_yaml` command in `src-tauri/src/lib.rs` and copy its exact `SketchProject` open expression (path type and error mapping included). The new command:

```rust
#[tauri::command]
fn init_profile(
    sketch_dir: String,
    profile: String,
    fqbn: String,
) -> Result<bancada_core::sketch::SketchYaml, String> {
    // Open the project exactly the way load_sketch_yaml does.
    let proj = bancada_core::sketch::SketchProject::open(Path::new(&sketch_dir))
        .map_err(err_str)?;
    proj.init_profile(&profile, &fqbn).map_err(err_str)
}
```

Register `init_profile,` in `generate_handler![...]` beside `load_sketch_yaml`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p bancada`
Expected: clean.

- [ ] **Step 3: Add the api.ts wrapper** (after `loadSketchYaml`):

```ts
/** Create sketch.yaml (when absent) with a first profile for `fqbn`. */
export const initProfile = (sketchDir: string, profile: string, fqbn: string) =>
  invoke<SketchYaml>("init_profile", { sketchDir, profile, fqbn });
```

- [ ] **Step 4: Verify types**

Run: `npx tsc --noEmit`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src/api.ts
git commit -m "Expose init_profile as a Tauri command with an api.ts wrapper"
```

---

### Task 3: `newFile.ts` validation module

**Files:**
- Create: `src/newFile.ts`
- Test: `src/__tests__/newFile.test.ts`

**Interfaces:**
- Consumes: `SketchFile` from `src/api.ts`.
- Produces: `checkNewFile(raw: string, existing: SketchFile[]): NewFileCheck` where `NewFileCheck = { ok: true; relPath: string } | { ok: false; reason: string }` — used by Task 4.

- [ ] **Step 1: Write the failing tests**

```ts
import { describe, expect, it } from "vitest";
import { checkNewFile } from "../newFile";
import type { SketchFile } from "../api";

const f = (rel_path: string, is_dir = false): SketchFile => ({ rel_path, is_dir });

describe("checkNewFile", () => {
  const existing = [f("A.ino"), f("data", true), f("data/config.h")];

  it("accepts a fresh root name, trimmed", () => {
    expect(checkNewFile("  util.h ", existing)).toEqual({ ok: true, relPath: "util.h" });
  });

  it("accepts a fresh subpath", () => {
    expect(checkNewFile("data/secrets.h", existing)).toEqual({
      ok: true,
      relPath: "data/secrets.h",
    });
  });

  it("rejects empty input", () => {
    expect(checkNewFile("   ", existing).ok).toBe(false);
  });

  it("rejects an absolute path", () => {
    expect(checkNewFile("/etc/passwd", existing).ok).toBe(false);
  });

  it("rejects .. anywhere in the path", () => {
    expect(checkNewFile("../outside.h", existing).ok).toBe(false);
    expect(checkNewFile("data/../../out.h", existing).ok).toBe(false);
  });

  it("rejects a trailing slash — that names a folder", () => {
    expect(checkNewFile("data/", existing).ok).toBe(false);
  });

  it("rejects empty path segments", () => {
    expect(checkNewFile("data//x.h", existing).ok).toBe(false);
  });

  it("refuses to shadow an existing file", () => {
    const r = checkNewFile("data/config.h", existing);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("already exists");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/__tests__/newFile.test.ts`
Expected: FAIL — module `../newFile` not found.

- [ ] **Step 3: Implement**

```ts
// Validation for creating a file inside the open sketch.
//
// The backend (`write_sketch_file`) already refuses traversal, but it
// silently overwrites an existing file — the frontend must catch that, and
// give friendlier messages for the rest.

import type { SketchFile } from "./api";

export type NewFileCheck =
  | { ok: true; relPath: string }
  | { ok: false; reason: string };

export function checkNewFile(raw: string, existing: SketchFile[]): NewFileCheck {
  const relPath = raw.trim();
  if (!relPath) return { ok: false, reason: "name the file to create" };
  if (relPath.startsWith("/"))
    return { ok: false, reason: "use a path inside the sketch, not an absolute one" };
  if (relPath.endsWith("/"))
    return { ok: false, reason: "that names a folder — folders appear when a file needs them" };
  const segments = relPath.split("/");
  if (segments.some((s) => s === ".."))
    return { ok: false, reason: "the path cannot leave the sketch (..)" };
  if (segments.some((s) => s.trim() === ""))
    return { ok: false, reason: "the path has an empty segment" };
  if (existing.some((f) => f.rel_path === relPath))
    return { ok: false, reason: `${relPath} already exists` };
  return { ok: true, relPath };
}
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run src/__tests__/newFile.test.ts`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/newFile.ts src/__tests__/newFile.test.ts
git commit -m "Add checkNewFile — frontend validation for creating sketch files"
```

---

### Task 4: New-file UI — inline input in tree and tab strip

**Files:**
- Create: `src/components/NewFileInput.tsx`
- Modify: `src/components/FileTree.tsx`, `src/components/EditorTabs.tsx`, `src/App.tsx`, `src/styles.css`

**Interfaces:**
- Consumes: Task 3's `checkNewFile`; existing `api.writeSketchFile`, `api.listSketchFiles`, App's `openFileInEditor`, `notify`.
- Produces: `NewFileInput` component with props `{ title: string; onSubmit: (raw: string) => boolean }` (returns `false` to keep the input open); `onCreate?: (raw: string) => boolean` prop on `FileTree` and `EditorTabs`.

- [ ] **Step 1: Create `src/components/NewFileInput.tsx`**

```tsx
import { useState } from "react";

interface Props {
  title: string;
  /** Return true when handled — the input closes; false keeps it open. */
  onSubmit: (raw: string) => boolean;
}

/** A ＋ that turns into a filename input in place. Enter submits, Escape or
 *  clicking away cancels. */
export default function NewFileInput({ title, onSubmit }: Props) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  const close = () => {
    setOpen(false);
    setValue("");
  };
  if (!open) {
    return (
      <button className="btn icon" title={title} onClick={() => setOpen(true)}>
        ＋
      </button>
    );
  }
  return (
    <input
      className="input new-file-input"
      autoFocus
      placeholder="new file, e.g. config.h"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && onSubmit(value)) close();
        if (e.key === "Escape") close();
      }}
      onBlur={close}
    />
  );
}
```

- [ ] **Step 2: Add the App-side create flow.** In `src/App.tsx`, next to `openFileInEditor` (~line 322), add — and import `checkNewFile` from `./newFile`:

```tsx
  /** Validate, create empty, refresh the list and open — the file must show
   *  up in tree and tabs immediately or creation looks like it failed. */
  const createNewFile = (raw: string): boolean => {
    if (!sketchDir) return false;
    const check = checkNewFile(raw, files);
    if (!check.ok) {
      notify(check.reason, true);
      return false;
    }
    (async () => {
      try {
        await api.writeSketchFile(sketchDir, check.relPath, "");
        setFiles(await api.listSketchFiles(sketchDir));
        await openFileInEditor(sketchDir, check.relPath);
        notify(`Created ${check.relPath}`);
      } catch (e) {
        notify(String(e), true);
      }
    })();
    return true;
  };
```

- [ ] **Step 3: Thread `onCreate` into both components.**

`FileTree.tsx` — add to `Props`: `onCreate?: (raw: string) => boolean;` and import `NewFileInput`. Render a header row as the first child of `.file-tree`:

```tsx
      {onCreate && (
        <div className="tree-new">
          <NewFileInput title="New file in this sketch" onSubmit={onCreate} />
        </div>
      )}
```

`EditorTabs.tsx` — add to `Props`: `onCreate?: (raw: string) => boolean;` and import `NewFileInput`. Render right after the `tabs.map(...)` block (before the empty-state span):

```tsx
      {onCreate && <NewFileInput title="New file in this sketch" onSubmit={onCreate} />}
```

In `App.tsx`, pass `onCreate={createNewFile}` to both `<FileTree …>` and `<EditorTabs …>`.

- [ ] **Step 4: Styles.** In `src/styles.css`, after the `.editor-tabs` block:

```css
.new-file-input {
  width: 160px;
  font-size: 11px;
  font-family: var(--font-mono);
}

.tree-new {
  padding: 2px 8px;
}
```

- [ ] **Step 5: Verify**

Run: `npx tsc --noEmit && npx vitest run`
Expected: clean, all tests pass. In the running app (HMR): ＋ appears at the end of the tab strip and atop the file tree; creating `notes.md` opens an empty buffer; creating `A.ino` again (existing name) toasts an error and keeps the input open.

- [ ] **Step 6: Commit**

```bash
git add src/components/NewFileInput.tsx src/components/FileTree.tsx src/components/EditorTabs.tsx src/App.tsx src/styles.css
git commit -m "Add file creation from the file tree and the editor tab strip"
```

---

### Task 5: Profile bootstrap UI — toolbar button and form

**Files:**
- Create: `src/profileInit.ts`, `src/components/ProfileInit.tsx`
- Test: `src/__tests__/profileInit.test.ts`
- Modify: `src/components/Toolbar.tsx`, `src/App.tsx`, `src/styles.css`

**Interfaces:**
- Consumes: Task 2's `initProfile`; existing `listAllBoards`, `BoardOption`, `SketchYaml`, App's `detectedFqbn`, `notify`.
- Produces: `profileNameForFqbn(fqbn: string): string`; `ProfileInit` component; `onCreateProfile: () => void` prop on `Toolbar`.

- [ ] **Step 1: Write the failing tests** (`src/__tests__/profileInit.test.ts`)

```ts
import { describe, expect, it } from "vitest";
import { profileNameForFqbn } from "../profileInit";

// Mirrors core's profile_name_for_fqbn (core/src/project.rs) — same inputs,
// same outputs, so the suggested name matches what the backend would derive.
describe("profileNameForFqbn", () => {
  it("uses the board segment", () => {
    expect(profileNameForFqbn("esp32:esp32:esp32s3")).toBe("esp32s3");
  });

  it("sanitizes illegal characters to underscores", () => {
    expect(profileNameForFqbn("esp32:esp32:esp32 s3!")).toBe("esp32_s3");
  });

  it("falls back to the sanitized whole when there is no board segment", () => {
    expect(profileNameForFqbn("esp32")).toBe("esp32");
  });

  it("falls back to `default` when nothing usable remains", () => {
    expect(profileNameForFqbn(":::")).toBe("default");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/__tests__/profileInit.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `src/profileInit.ts`**

```ts
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
```

Run: `npx vitest run src/__tests__/profileInit.test.ts` — expected: 4 passed.

- [ ] **Step 4: Create `src/components/ProfileInit.tsx`** (board grouping copied from `NewProject.tsx`'s picker)

```tsx
import { useEffect, useMemo, useState } from "react";
import {
  initProfile,
  listAllBoards,
  type BoardOption,
  type SketchYaml,
} from "../api";
import { profileNameForFqbn } from "../profileInit";

interface Props {
  sketchDir: string;
  /** FQBN detected on the selected port, preselected when known. */
  detectedFqbn: string | null;
  onCreated: (yaml: SketchYaml, profile: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

/** One-row form under the toolbar: pick a board, name the profile, create
 *  sketch.yaml. Shown only while the sketch has no profiles. */
export default function ProfileInit({
  sketchDir,
  detectedFqbn,
  onCreated,
  onCancel,
  notify,
}: Props) {
  const [boards, setBoards] = useState<BoardOption[]>([]);
  const [fqbn, setFqbn] = useState(detectedFqbn ?? "");
  const [name, setName] = useState(
    detectedFqbn ? profileNameForFqbn(detectedFqbn) : "",
  );
  const [nameTouched, setNameTouched] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    listAllBoards()
      .then(setBoards)
      .catch((e) => notify(String(e), true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const groups = useMemo(() => {
    const g = new Map<string, BoardOption[]>();
    for (const b of boards) {
      const list = g.get(b.platform_name) ?? [];
      list.push(b);
      g.set(b.platform_name, list);
    }
    return [...g.entries()];
  }, [boards]);

  const pick = (f: string) => {
    setFqbn(f);
    if (!nameTouched) setName(f ? profileNameForFqbn(f) : "");
  };

  const create = async () => {
    setBusy(true);
    try {
      const yaml = await initProfile(sketchDir, name.trim(), fqbn);
      notify(`✓ Profile “${name.trim()}” written to sketch.yaml`);
      onCreated(yaml, name.trim());
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="profile-init">
      <span className="profile-init-label">New sketch.yaml profile:</span>
      <select
        className="select"
        value={fqbn}
        onChange={(e) => pick(e.target.value)}
        title="Board for this profile"
      >
        <option value="">choose a board…</option>
        {groups.map(([platform, list]) => (
          <optgroup key={platform} label={platform}>
            {list.map((b) => (
              <option key={b.fqbn} value={b.fqbn}>
                {b.name}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
      <input
        className="input"
        value={name}
        placeholder="profile name"
        onChange={(e) => {
          setName(e.target.value);
          setNameTouched(true);
        }}
      />
      <button
        className="btn primary"
        disabled={busy || !fqbn || !name.trim()}
        onClick={create}
      >
        Create
      </button>
      <button className="btn" disabled={busy} onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}
```

- [ ] **Step 5: Toolbar.** In `src/components/Toolbar.tsx` add `onCreateProfile: () => void;` to `Props`, and replace the profile `<select>` block with:

```tsx
      {props.sketchDir && profiles.length === 0 ? (
        <button
          className="btn"
          onClick={props.onCreateProfile}
          title="This sketch has no sketch.yaml profile — create one"
        >
          ＋ Create profile…
        </button>
      ) : (
        <select
          className="select"
          value={props.profile ?? ""}
          onChange={(e) => props.onSelectProfile(e.target.value)}
          disabled={profiles.length === 0}
          title="Build profile (sketch.yaml)"
        >
          {profiles.length === 0 && <option value="">no sketch.yaml profile</option>}
          {profiles.map((p) => {
            const board = props.sketchYaml?.profiles?.[p]?.fqbn.split(":")[2];
            return (
              <option key={p} value={p}>
                {board ? `${p} — ${board}` : p}
              </option>
            );
          })}
        </select>
      )}
```

- [ ] **Step 6: App wiring.** In `src/App.tsx`:

State, next to `creatingProject`:

```tsx
  const [creatingProfile, setCreatingProfile] = useState(false);
```

Pass to Toolbar: `onCreateProfile={() => setCreatingProfile(true)}`.

Render directly below `<Toolbar …/>` (import `ProfileInit`):

```tsx
      {creatingProfile && sketchDir && (
        <ProfileInit
          sketchDir={sketchDir}
          detectedFqbn={detectedFqbn() ?? null}
          onCreated={(yaml, prof) => {
            setSketchYaml(yaml);
            setProfile(prof);
            setCreatingProfile(false);
          }}
          onCancel={() => setCreatingProfile(false)}
          notify={notify}
        />
      )}
```

Also close the form when the sketch changes: in `loadSketch`, alongside the other `set*` resets, add `setCreatingProfile(false);`.

- [ ] **Step 7: Styles.** In `src/styles.css`, after the `.toolbar` block:

```css
/* One-row profile-bootstrap form, anchored under the toolbar. */
.profile-init {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border);
}

.profile-init-label {
  font-size: 12px;
  color: var(--text-dim);
}
```

- [ ] **Step 8: Verify**

Run: `npx tsc --noEmit && npx vitest run && cargo test -p bancada-core`
Expected: all clean. In the running app: open a sketch without sketch.yaml (e.g. create a scratch one) → toolbar shows `＋ Create profile…`; creating writes sketch.yaml, the dropdown replaces the button showing the new profile selected, and Verify becomes profile-aware. A second create attempt with the same name errors without touching the file.

- [ ] **Step 9: Commit**

```bash
git add src/profileInit.ts src/__tests__/profileInit.test.ts src/components/ProfileInit.tsx src/components/Toolbar.tsx src/App.tsx src/styles.css
git commit -m "Add profile bootstrap — create sketch.yaml from the toolbar"
```
