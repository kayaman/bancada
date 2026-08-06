# Project-Level Library Pinning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `sketch.yaml` library pins visible and manageable in the Library
Manager, close the silent no-pin install paths, and give profile-less sketches
a one-click "Pin current setup" adoption flow.

**Architecture:** `sketch.yaml` stays the single source of truth (spec:
`docs/superpowers/specs/2026-08-06-library-pinning-design.md`). Core gains a
`remove_library` editor; registry pin *writes* keep going through
`arduino-cli profile lib add` so dependency resolution stays arduino-cli's
job. The frontend gets a pure pin-view model plus a "Project" tab in
LibraryManager. Adoption parses arduino-cli's `Used library` compile report
(captured from the existing `build://line` stream) and pins each used library
— registry spec first, absolute `dir:` fallback.

**Tech Stack:** Rust (bancada-core + Tauri commands), React/TypeScript,
vitest, cargo test. Test layout: Rust unit tests inline in the module,
frontend logic tests in `src/__tests__/` (pure modules only — this repo has no
component render tests; keep components thin instead).

## Global Constraints

- Registry pins in `sketch.yaml` look like `"ArduinoJson (7.4.2)"`; local pins
  are `{ dir: path }` (`LibraryDep` in `core/src/sketch.rs:50`).
- Never hand-write a registry pin string into yaml — always go through
  `Cli::profile_lib_add` (`core/src/cli.rs:251`), which resolves dependencies.
- All yaml-writing Tauri commands return the rewritten `SketchYaml` (the
  frontend applies it via `onYamlChanged`).
- Unpin of an absent entry is a no-op returning current yaml (double-click safe).
- Marco's UI rule: no placeholder controls as spacers; empty states are
  honestly empty (`no-placeholder-ui-for-layout-invariants`).
- Run `cargo test -p bancada-core` and `npm test` (vitest) before every commit;
  `npm run build` (tsc + vite) for frontend-touching tasks.
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Shared checkout: `git status` before staging; stage only your own files.

---

### Task 1: Core `remove_library` + `registry_pin_name`

**Files:**
- Modify: `core/src/sketch.rs` (impl block ends ~line 271; tests module below)

**Interfaces:**
- Consumes: existing `SketchProject`, `LibraryDep`, `normalize()` (sketch.rs:312).
- Produces: `pub fn registry_pin_name(entry: &str) -> &str` and
  `SketchProject::remove_library(&self, profile_name: &str, dep: &LibraryDep) -> Result<SketchYaml>`.
  Task 2 and Task 9 call both.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests` in `core/src/sketch.rs`; the existing tests there show the `SAMPLE` yaml + tempdir pattern — follow it):

```rust
    fn project_with(yaml: &str) -> (tempfile::TempDir, SketchProject) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("sketch.yaml"), yaml).unwrap();
        let p = SketchProject::open(tmp.path()).unwrap();
        (tmp, p)
    }

    const PINNED: &str = "\
default_profile: uno
profiles:
  uno:
    fqbn: arduino:avr:uno
    libraries:
      - ArduinoJson (7.4.2)
      - Servo (1.2.1)
      - dir: ../libs/Local_Foo
";

    #[test]
    fn registry_pin_name_strips_the_version() {
        assert_eq!(registry_pin_name("ArduinoJson (7.4.2)"), "ArduinoJson");
        assert_eq!(registry_pin_name("Adafruit GFX Library (1.11.9)"), "Adafruit GFX Library");
        assert_eq!(registry_pin_name("NoVersion"), "NoVersion");
    }

    #[test]
    fn removes_a_registry_pin_by_name_regardless_of_version() {
        let (_tmp, p) = project_with(PINNED);
        let y = p
            .remove_library("uno", &LibraryDep::Registry("ArduinoJson (9.9.9)".into()))
            .unwrap();
        let libs = &y.profiles["uno"].libraries;
        assert_eq!(libs.len(), 2, "{libs:?}");
        assert!(libs.iter().all(|l| !matches!(l, LibraryDep::Registry(s) if s.starts_with("ArduinoJson"))));
        // The others survive untouched.
        assert!(libs.contains(&LibraryDep::Registry("Servo (1.2.1)".into())));
    }

    #[test]
    fn removes_a_local_pin_by_resolved_path() {
        let (_tmp, p) = project_with(PINNED);
        // A different spelling of the same target must still match.
        let y = p
            .remove_library("uno", &LibraryDep::Local { dir: "../libs/./Local_Foo".into() })
            .unwrap();
        assert_eq!(y.profiles["uno"].libraries.len(), 2, "{:?}", y.profiles["uno"].libraries);
        assert!(y.profiles["uno"].libraries.iter().all(|l| matches!(l, LibraryDep::Registry(_))));
    }

    #[test]
    fn removing_an_absent_pin_is_a_noop() {
        let (_tmp, p) = project_with(PINNED);
        let y = p
            .remove_library("uno", &LibraryDep::Registry("NotPinned".into()))
            .unwrap();
        assert_eq!(y.profiles["uno"].libraries.len(), 3);
    }

    #[test]
    fn remove_library_rejects_an_unknown_profile() {
        let (_tmp, p) = project_with(PINNED);
        let err = p
            .remove_library("nope", &LibraryDep::Registry("Servo".into()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no profile named"), "{err}");
    }

    #[test]
    fn a_registry_dep_never_matches_a_local_dep() {
        let (_tmp, p) = project_with(PINNED);
        // Name equal to the local folder must not remove the dir: entry.
        let y = p
            .remove_library("uno", &LibraryDep::Registry("Local_Foo".into()))
            .unwrap();
        assert_eq!(y.profiles["uno"].libraries.len(), 3);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bancada-core sketch::tests::remov -- --nocapture` (plus `registry_pin_name`)
Expected: FAIL — `remove_library` / `registry_pin_name` not found.

- [ ] **Step 3: Implement** (in `core/src/sketch.rs`; the free function next to `normalize`, the method inside `impl SketchProject` after `add_platform`):

```rust
/// The name part of a registry pin: `"ArduinoJson (7.4.2)"` → `"ArduinoJson"`.
///
/// Pin strings are written by arduino-cli, so the ` (` separator is stable;
/// a string without it is already a bare name.
pub fn registry_pin_name(entry: &str) -> &str {
    match entry.find(" (") {
        Some(i) => entry[..i].trim(),
        None => entry.trim(),
    }
}
```

```rust
    /// Remove a library dependency from a profile.
    ///
    /// Registry entries match by library name — the caller usually holds a
    /// different version string than the file does. Local entries match by
    /// resolved path, same normalization as `add_local_library_with`, so any
    /// spelling of the same target matches. Removing an absent entry is a
    /// no-op returning the current yaml: the UI can double-fire safely.
    pub fn remove_library(&self, profile_name: &str, dep: &LibraryDep) -> Result<SketchYaml> {
        let mut y = self.load_yaml()?;
        let profile = y
            .profiles
            .get_mut(profile_name)
            .ok_or_else(|| Error::Other(format!("no profile named `{profile_name}`")))?;
        profile.libraries.retain(|have| !same_library(&self.dir, have, dep));
        self.save_yaml(&y)?;
        Ok(y)
    }
```

```rust
fn same_library(sketch_dir: &Path, a: &LibraryDep, b: &LibraryDep) -> bool {
    match (a, b) {
        (LibraryDep::Registry(x), LibraryDep::Registry(y)) => {
            registry_pin_name(x) == registry_pin_name(y)
        }
        (LibraryDep::Local { dir: x }, LibraryDep::Local { dir: y }) => {
            normalize(&sketch_dir.join(x)) == normalize(&sketch_dir.join(y))
        }
        _ => false,
    }
}
```

- [ ] **Step 4: Run the full core suite**

Run: `cargo test -p bancada-core`
Expected: PASS (new tests included).

- [ ] **Step 5: Commit**

```bash
git add core/src/sketch.rs
git commit -m "feat(core): remove_library edits a profile's library pins"
```

---

### Task 2: Tauri commands `remove_library_from_profile` and `change_pinned_library_version` + api.ts wrappers

**Files:**
- Modify: `src-tauri/src/lib.rs` (place after `add_registry_library_to_profile`, ~line 401; register both in the `tauri::generate_handler![...]` list next to `add_registry_library_to_profile`, ~line 3021)
- Modify: `src/api.ts` (after `uninstallLibrary`, ~line 331)

**Interfaces:**
- Consumes: Task 1's `SketchProject::remove_library`; existing
  `Cli::profile_lib_add(dir, profile, spec)`; `err_str`; `LibraryDep`
  (serde untagged — deserializes from `"Name (1.0)"` or `{ dir: "…" }`).
- Produces: commands `remove_library_from_profile(sketch_dir, profile, dep) -> SketchYaml`
  and `change_pinned_library_version(sketch_dir, profile, name, version) -> SketchYaml`;
  TS wrappers `removeLibraryFromProfile(sketchDir, profile, dep)` and
  `changePinnedLibraryVersion(sketchDir, profile, name, version)`. Tasks 4–5 call them.

- [ ] **Step 1: Add the commands** (this layer is thin glue over Task 1's tested core, matching the untested existing commands around it — no new Rust test):

```rust
/// Remove one library pin from a profile. Absent pin → no-op (idempotent).
#[tauri::command]
async fn remove_library_from_profile(
    sketch_dir: String,
    profile: String,
    dep: bancada_core::sketch::LibraryDep,
) -> Result<SketchYaml, String> {
    tauri::async_runtime::spawn_blocking(move || {
        SketchProject::open(Path::new(&sketch_dir))
            .and_then(|p| p.remove_library(&profile, &dep))
            .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

/// Re-pin a registry library at a different version: remove the old pin, then
/// `arduino-cli profile lib add name@version` (dependency resolution stays
/// arduino-cli's job). If the add fails the yaml file is restored verbatim, so
/// a bad version leaves the old pin in place.
#[tauri::command]
async fn change_pinned_library_version(
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: String,
    name: String,
    version: String,
) -> Result<SketchYaml, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = Path::new(&sketch_dir);
        let project = SketchProject::open(dir).map_err(err_str)?;
        let yaml_path = dir.join("sketch.yaml");
        let before = std::fs::read_to_string(&yaml_path).map_err(err_str)?;
        project
            .remove_library(
                &profile,
                &bancada_core::sketch::LibraryDep::Registry(name.clone()),
            )
            .map_err(err_str)?;
        let spec = format!("{name}@{version}");
        if let Err(e) = cli.profile_lib_add(dir, &profile, &spec) {
            let _ = std::fs::write(&yaml_path, before);
            return Err(err_str(e));
        }
        project.load_yaml().map_err(err_str)
    })
    .await
    .map_err(err_str)?
}
```

- [ ] **Step 2: Register both** in `tauri::generate_handler![…]` immediately after `add_registry_library_to_profile,`:

```rust
            remove_library_from_profile,
            change_pinned_library_version,
```

- [ ] **Step 3: Add the api.ts wrappers**:

```ts
/** Remove one pin (registry or dir:) from a profile. Absent pin is a no-op. */
export const removeLibraryFromProfile = (
  sketchDir: string,
  profile: string,
  dep: LibraryDep,
) =>
  invoke<SketchYaml>("remove_library_from_profile", { sketchDir, profile, dep });
/** Re-pin a registry library at another version (remove + `profile lib add`). */
export const changePinnedLibraryVersion = (
  sketchDir: string,
  profile: string,
  name: string,
  version: string,
) =>
  invoke<SketchYaml>("change_pinned_library_version", {
    sketchDir,
    profile,
    name,
    version,
  });
```

- [ ] **Step 4: Verify it builds**

Run: `cargo check -p bancada 2>&1 | tail -5` (the Tauri crate) and `npm run build`
Expected: both clean. Also run `cargo test -p bancada-core` and `npm test` — all green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src/api.ts
git commit -m "feat: unpin and change-version commands for profile library pins"
```

---

### Task 3: Frontend pin view model (`src/pinModel.ts`)

**Files:**
- Create: `src/pinModel.ts`
- Test: `src/__tests__/pinModel.test.ts`

**Interfaces:**
- Consumes: `LibraryDep`, `Manifest` types from `src/api.ts`.
- Produces: `interface PinView { kind: "registry" | "local" | "vendored"; name: string; version?: string; dir?: string; dep: LibraryDep }`,
  `registryPinName(entry: string): string`, `pinViews(deps, manifest): PinView[]`.
  Tasks 4–5 render and match against these.

- [ ] **Step 1: Write the failing tests** (`src/__tests__/pinModel.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { pinViews, registryPinName } from "../pinModel";
import type { Manifest } from "../api";

const manifest: Manifest = {
  version: 1,
  libraries: [
    { alias: "@me/repo/HomeNode", ref: "v1.2.0", commit: "abc1234", vendor: ".bancada/libs/HomeNode" },
  ],
};

describe("registryPinName", () => {
  it("strips the version suffix", () => {
    expect(registryPinName("ArduinoJson (7.4.2)")).toBe("ArduinoJson");
    expect(registryPinName("Adafruit GFX Library (1.11.9)")).toBe("Adafruit GFX Library");
  });
  it("passes through a bare name", () => {
    expect(registryPinName("Servo")).toBe("Servo");
  });
});

describe("pinViews", () => {
  it("classifies registry pins with name and version", () => {
    const [v] = pinViews(["ArduinoJson (7.4.2)"], null);
    expect(v).toMatchObject({ kind: "registry", name: "ArduinoJson", version: "7.4.2" });
  });

  it("classifies a manifest vendor dir as vendored", () => {
    const [v] = pinViews([{ dir: ".bancada/libs/HomeNode" }], manifest);
    expect(v).toMatchObject({ kind: "vendored", name: "HomeNode", dir: ".bancada/libs/HomeNode" });
  });

  it("classifies any other dir as local, named by its last segment", () => {
    const [v] = pinViews([{ dir: "/home/x/Arduino/libraries/My_Sensor" }], manifest);
    expect(v).toMatchObject({ kind: "local", name: "My_Sensor" });
  });

  it("handles a registry pin without a version", () => {
    const [v] = pinViews(["Servo"], null);
    expect(v).toMatchObject({ kind: "registry", name: "Servo" });
    expect(v.version).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm test -- pinModel`
Expected: FAIL — module `../pinModel` not found.

- [ ] **Step 3: Implement** (`src/pinModel.ts`):

```ts
import type { LibraryDep, Manifest } from "./api";

/** One profile library pin, shaped for display. */
export interface PinView {
  kind: "registry" | "local" | "vendored";
  name: string;
  /** Registry pins only. */
  version?: string;
  /** dir: pins only — the stored path, verbatim. */
  dir?: string;
  /** The original entry, for exact round-trip to removeLibraryFromProfile. */
  dep: LibraryDep;
}

/** `"ArduinoJson (7.4.2)"` → `"ArduinoJson"`. Mirrors core's registry_pin_name. */
export const registryPinName = (entry: string): string => {
  const i = entry.indexOf(" (");
  return (i === -1 ? entry : entry.slice(0, i)).trim();
};

const registryPinVersion = (entry: string): string | undefined =>
  /\(([^)]+)\)\s*$/.exec(entry)?.[1];

export function pinViews(deps: LibraryDep[], manifest: Manifest | null): PinView[] {
  const vendorDirs = new Set(
    (manifest?.libraries ?? []).map((e) => e.vendor.replace(/^\.\//, "")),
  );
  return deps.map((dep): PinView => {
    if (typeof dep === "string") {
      return {
        kind: "registry",
        name: registryPinName(dep),
        version: registryPinVersion(dep),
        dep,
      };
    }
    const dir = dep.dir;
    const name = dir.replace(/\/+$/, "").split("/").pop() ?? dir;
    const vendored = vendorDirs.has(dir.replace(/^\.\//, ""));
    return { kind: vendored ? "vendored" : "local", name, dir, dep };
  });
}
```

- [ ] **Step 4: Run tests**

Run: `npm test -- pinModel`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pinModel.ts src/__tests__/pinModel.test.ts
git commit -m "feat(ui): pin view model for profile library entries"
```

---

### Task 4: "Project" tab in LibraryManager

**Files:**
- Modify: `src/components/LibraryManager.tsx` (tab union ~line 53, imports ~line 1–23, tab buttons ~line 271, new tab body after the `installed` block ~line 374)
- Modify: `src/App.tsx:1274` (LibraryManager props)

**Interfaces:**
- Consumes: Task 2 wrappers, Task 3 `pinViews`/`PinView`; existing
  `searchLibraries`, `addRegistryLibraryToProfile`, `ghManifest`;
  `SketchYaml` prop from App (`sketchYaml` state exists — referenced at
  `src/App.tsx:828`).
- Produces: LibraryManager `Props` gains `yaml: SketchYaml | null` and
  `fqbn: string | null` (Task 5 and Task 8 use `fqbn`). A `pins: PinView[]`
  value derived in the component that Task 5's uninstall guard reads.

- [ ] **Step 1: Extend Props and derive pins.** Add to `Props`: `yaml: SketchYaml | null; fqbn: string | null;`. Extend the react import to `import { useEffect, useMemo, useState } from "react";` and import `pinViews`, `type PinView` from `../pinModel`. In the component:

```ts
  const pins: PinView[] = useMemo(
    () =>
      profile && yaml?.profiles?.[profile]
        ? pinViews(yaml.profiles[profile].libraries ?? [], ghPinned)
        : [],
    [yaml, profile, ghPinned],
  );
```

  where `ghPinned: Manifest | null` is new state kept fresh the same way the
  `github` tab's `refreshPinned` does today (extract: `refreshPinned` sets both
  `setPinned(m.libraries)` and `setGhPinned(m)`; also call it when the
  `project` tab opens, not just `github`).

- [ ] **Step 2: Add the tab.** Tab union becomes
  `"project" | "installed" | "search" | "new" | "github"`; make `"project"`
  the initial tab when `sketchDir` is non-null, else `"installed"`. Add the
  button first in the tab row:

```tsx
        <button
          className={tab === "project" ? "tab active" : "tab"}
          onClick={() => setTab("project")}
          title="Libraries pinned in this sketch's build profile (sketch.yaml)"
        >
          Project
        </button>
```

- [ ] **Step 3: Render the tab body** (after the `installed` block):

```tsx
      {tab === "project" && (
        <div className="lib-list">
          {sketchDir && profile && pins.map((p) => (
            <div key={`${p.kind}:${p.name}:${p.dir ?? p.version ?? ""}`} className="lib-card">
              <div className="lib-head">
                <span className="lib-name">{p.name}</span>
                {p.version && <span className="lib-version">{p.version}</span>}
                {p.kind !== "registry" && <span className="lib-badge">{p.kind}</span>}
              </div>
              {p.dir && <div className="lib-sentence mono">{p.dir}</div>}
              <div className="lib-actions">
                {p.kind === "registry" && (
                  <button
                    className="btn small"
                    disabled={working}
                    onClick={() => openVersionPicker(p.name)}
                  >
                    Change version…
                  </button>
                )}
                <button
                  className="btn small danger"
                  disabled={working}
                  onClick={() => doUnpin(p)}
                >
                  Unpin
                </button>
              </div>
              {verPicker?.name === p.name && (
                <div className="lib-actions">
                  <select
                    className="select small"
                    value={verPicker.chosen}
                    onChange={(e) =>
                      setVerPicker({ ...verPicker, chosen: e.target.value })
                    }
                  >
                    {verPicker.versions.map((v) => (
                      <option key={v} value={v}>{v}</option>
                    ))}
                  </select>
                  <button
                    className="btn small primary"
                    disabled={working || verPicker.chosen === p.version}
                    onClick={() => doChangeVersion(p.name, verPicker.chosen)}
                  >
                    Apply
                  </button>
                </div>
              )}
            </div>
          ))}
          {sketchDir && profile && pins.length === 0 && (
            <div className="empty-hint">
              Nothing pinned yet — install from the Registry tab, or pin an
              installed library.
            </div>
          )}
          {sketchDir && !profile && (
            <div className="empty-hint">
              This sketch has no build profile, so nothing is pinned and builds
              use whatever the sketchbook holds.
            </div>
          )}
          {!sketchDir && (
            <div className="empty-hint">Open a sketch to see its pinned libraries.</div>
          )}
        </div>
      )}
```

  (Task 8 later adds the "Pin current setup" button into the no-profile empty
  state; leave it text-only here.)

- [ ] **Step 4: Add the handlers**:

```ts
  const [verPicker, setVerPicker] = useState<{
    name: string;
    versions: string[];
    chosen: string;
  } | null>(null);

  const doUnpin = async (p: PinView) => {
    if (!sketchDir || !profile) return;
    setWorking(true);
    try {
      const y = await removeLibraryFromProfile(sketchDir, profile, p.dep);
      onYamlChanged(y);
      notify(`Unpinned ${p.name}`);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const openVersionPicker = async (name: string) => {
    setWorking(true);
    try {
      const hits = await searchLibraries(name);
      const exact = hits.find((h) => h.name === name);
      if (!exact || exact.available_versions.length === 0) {
        notify(`${name} not found in the registry index`, true);
        return;
      }
      setVerPicker({
        name,
        versions: exact.available_versions,
        chosen: exact.available_versions[0],
      });
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const doChangeVersion = async (name: string, version: string) => {
    if (!sketchDir || !profile) return;
    setWorking(true);
    try {
      const y = await changePinnedLibraryVersion(sketchDir, profile, name, version);
      onYamlChanged(y);
      setVerPicker(null);
      notify(`✓ ${name} pinned at ${version}`);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };
```

- [ ] **Step 5: "Pin to project" on installed cards.** In the `installed` tab's
  `lib-actions` div, before the Remove button:

```tsx
                {sketchDir && profile &&
                  !pins.some((p) => p.kind === "registry" && p.name === lib.name) && (
                  <button
                    className="btn small"
                    disabled={working}
                    onClick={() => doPinInstalled(lib)}
                  >
                    Pin to project
                  </button>
                )}
```

```ts
  const doPinInstalled = async (lib: InstalledLibrary) => {
    if (!sketchDir || !profile) return;
    setWorking(true);
    try {
      const y = await addRegistryLibraryToProfile(sketchDir, profile, lib.name, lib.version);
      onYamlChanged(y);
      notify(`✓ Pinned ${lib.name} ${lib.version}`);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };
```

- [ ] **Step 6: Pass the new props from App** (`src/App.tsx:1274` area):
  `yaml={sketchYaml}` and `fqbn={detectedFqbn() ?? null}` (helper at
  `src/App.tsx:751`; match the surrounding prop style).

- [ ] **Step 7: Verify**

Run: `npm test && npm run build`
Expected: green; tsc clean. Then launch the dev app (`npm run tauri dev`),
open a sketch with pins, and check: Project tab lists pins, unpin removes the
yaml line, change-version rewrites it, "Pin to project" appears only for
unpinned installed libraries.

- [ ] **Step 8: Commit**

```bash
git add src/components/LibraryManager.tsx src/App.tsx
git commit -m "feat(ui): Project tab shows and manages sketch.yaml library pins"
```

---

### Task 5: Close the silent install/uninstall paths

**Files:**
- Modify: `src/components/LibraryManager.tsx` (`doInstall` ~line 88, `doUninstall` ~line 111)

**Interfaces:**
- Consumes: `fqbn` prop and `pins` from Task 4; existing `initProfile`
  (`src/api.ts:304`), `profileNameForFqbn` (`src/profileInit.ts`),
  `ask` from `@tauri-apps/plugin-dialog`; Task 2's `removeLibraryFromProfile`.
- Produces: nothing new for later tasks.

- [ ] **Step 1: Rewrite `doInstall`**:

```ts
  const doInstall = async (lib: IndexedLibrary) => {
    setWorking(true);
    try {
      let pinProfile = profile;
      if (sketchDir && !pinProfile) {
        const create = await ask(
          `This sketch has no build profile, so ${lib.name} cannot be pinned to it.\n\nCreate a profile now?`,
          { title: "Pin to project" },
        );
        if (create) {
          if (!fqbn) {
            notify("Select a board first — a profile needs an FQBN", true);
            return;
          }
          const name = profileNameForFqbn(fqbn);
          onYamlChanged(await initProfile(sketchDir, name, fqbn));
          pinProfile = name;
        }
      }
      await installLibrary(lib.name);
      await refreshInstalled();
      if (sketchDir && pinProfile) {
        const y = await addRegistryLibraryToProfile(
          sketchDir,
          pinProfile,
          lib.name,
          lib.latest.version,
        );
        onYamlChanged(y);
        notify(`✓ Installed ${lib.name} ${lib.latest.version} and pinned it`);
      } else {
        notify(
          `Installed ${lib.name} ${lib.latest.version} globally — not pinned to any sketch`,
        );
      }
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };
```

  Import `ask` alongside the existing `open` import from
  `@tauri-apps/plugin-dialog`, and `initProfile` from `../api`,
  `profileNameForFqbn` from `../profileInit`.

- [ ] **Step 2: Guard `doUninstall`**:

```ts
  const doUninstall = async (name: string) => {
    setWorking(true);
    try {
      const pin = pins.find((p) => p.kind === "registry" && p.name === name);
      if (sketchDir && profile && pin) {
        const alsoUnpin = await ask(
          `${name} is pinned in this sketch's profile.\n\nRemove the pin too? Keeping it is safe — profile builds fetch pinned libraries from the registry.`,
          { title: "Remove library" },
        );
        if (alsoUnpin) {
          onYamlChanged(await removeLibraryFromProfile(sketchDir, profile, pin.dep));
        }
      }
      await uninstallLibrary(name);
      notify(`Removed ${name}`);
      await refreshInstalled();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };
```

- [ ] **Step 3: Verify**

Run: `npm test && npm run build` — green. In the dev app: install with no
project open → "globally — not pinned" notice; install into a profile-less
sketch → profile offer → pin lands; uninstall a pinned library → warning with
working unpin.

- [ ] **Step 4: Commit**

```bash
git add src/components/LibraryManager.tsx
git commit -m "feat(ui): no more silent unpinned installs; uninstall warns on pinned libs"
```

---

### Task 6: Used-library report parser (`src/usedLibraries.ts`)

**Files:**
- Create: `src/usedLibraries.ts`
- Test: `src/__tests__/usedLibraries.test.ts`

**Interfaces:**
- Consumes: nothing project-specific (pure strings).
- Produces: `interface UsedLibrary { name: string; version: string; path: string }`,
  `parseUsedLibraries(lines: string[]): UsedLibrary[]`. Task 8 feeds it the
  captured `build://line` lines; Task 7's command receives its output.

- [ ] **Step 1: Write the failing tests** (fixture mirrors real arduino-cli output — whitespace-aligned columns, names may contain spaces, table ends at a blank line or the `Used platform` table):

```ts
import { describe, expect, it } from "vitest";
import { parseUsedLibraries } from "../usedLibraries";

const OUTPUT = [
  "Sketch uses 262114 bytes (20%) of program storage space.",
  "",
  "Used library          Version Path",
  "WiFi                  2.0.0   /home/k/.arduino15/packages/esp32/hardware/esp32/3.0.2/libraries/WiFi",
  "Adafruit GFX Library  1.11.9  /home/k/Arduino/libraries/Adafruit_GFX_Library",
  "ArduinoJson           7.4.2   /home/k/Arduino/libraries/ArduinoJson",
  "",
  "Used platform Version Path",
  "esp32:esp32   3.0.2   /home/k/.arduino15/packages/esp32/hardware/esp32/3.0.2",
];

describe("parseUsedLibraries", () => {
  it("extracts every row of the used-library table", () => {
    expect(parseUsedLibraries(OUTPUT)).toEqual([
      { name: "WiFi", version: "2.0.0", path: "/home/k/.arduino15/packages/esp32/hardware/esp32/3.0.2/libraries/WiFi" },
      { name: "Adafruit GFX Library", version: "1.11.9", path: "/home/k/Arduino/libraries/Adafruit_GFX_Library" },
      { name: "ArduinoJson", version: "7.4.2", path: "/home/k/Arduino/libraries/ArduinoJson" },
    ]);
  });

  it("returns empty when there is no table", () => {
    expect(parseUsedLibraries(["error: no such file"])).toEqual([]);
  });

  it("stops at the platform table even without a blank separator", () => {
    const lines = [
      "Used library Version Path",
      "Servo        1.2.1   /home/k/Arduino/libraries/Servo",
      "Used platform Version Path",
      "arduino:avr   1.8.6   /home/k/.arduino15/packages/arduino/hardware/avr/1.8.6",
    ];
    expect(parseUsedLibraries(lines)).toEqual([
      { name: "Servo", version: "1.2.1", path: "/home/k/Arduino/libraries/Servo" },
    ]);
  });

  it("keeps only rows that parse (name / version / absolute path)", () => {
    const lines = ["Used library Version Path", "garbage row", ""];
    expect(parseUsedLibraries(lines)).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm test -- usedLibraries`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement** (`src/usedLibraries.ts`):

```ts
/** One row of arduino-cli's post-compile `Used library` report. */
export interface UsedLibrary {
  name: string;
  version: string;
  path: string;
}

/**
 * Parse the `Used library / Version / Path` table arduino-cli prints after a
 * successful compile. Columns are space-aligned and the name may contain
 * spaces, so rows are read from the right: last field is the path (absolute),
 * second-to-last the version, the rest the name.
 */
export function parseUsedLibraries(lines: string[]): UsedLibrary[] {
  const start = lines.findIndex((l) => l.startsWith("Used library"));
  if (start === -1) return [];
  const out: UsedLibrary[] = [];
  for (const line of lines.slice(start + 1)) {
    if (!line.trim() || line.startsWith("Used platform")) break;
    const fields = line.trim().split(/\s{2,}|\s(?=\/)/).filter(Boolean);
    // Right-anchored: [ ...nameParts, version, path ]
    const path = fields.at(-1);
    const version = fields.at(-2);
    const name = fields.slice(0, -2).join(" ").trim();
    if (!path?.startsWith("/") || !version || !name) continue;
    out.push({ name, version, path });
  }
  return out;
}
```

- [ ] **Step 4: Run tests**

Run: `npm test -- usedLibraries`
Expected: PASS. If the split heuristic fails a case, fix the regex — the
tests define the contract.

- [ ] **Step 5: Commit**

```bash
git add src/usedLibraries.ts src/__tests__/usedLibraries.test.ts
git commit -m "feat(ui): parser for arduino-cli's used-library compile report"
```

---

### Task 7: `pin_current_setup` Tauri command + api.ts wrapper

**Files:**
- Modify: `src-tauri/src/lib.rs` (after Task 2's commands; register in `generate_handler!`)
- Modify: `src/api.ts` (after Task 2's wrappers)

**Interfaces:**
- Consumes: `Cli::profile_lib_add`, `Cli::sketchbook_libraries_dir`,
  `SketchProject::{init_profile, add_local_library_with, load_yaml}`,
  `PathStyle::Absolute`; Task 6's `UsedLibrary` shape (serialized as
  `{ name, version, path }`).
- Produces: command `pin_current_setup(sketch_dir, profile, fqbn, libs) -> PinReport`
  with `PinReport { registry: string[], local: string[], skipped: string[], errors: string[], yaml: SketchYaml }`;
  TS wrapper `pinCurrentSetup(sketchDir, profile, fqbn, libs)`. Task 8 calls it.

- [ ] **Step 1: Add the command**:

```rust
#[derive(serde::Deserialize)]
struct UsedLib {
    name: String,
    version: String,
    path: String,
}

#[derive(serde::Serialize)]
struct PinReport {
    /// Pinned as registry entries (dependencies resolved by arduino-cli).
    registry: Vec<String>,
    /// Pinned as absolute `dir:` entries — machine-local, better vendored.
    local: Vec<String>,
    /// Platform-bundled libraries — they come with the pinned core.
    skipped: Vec<String>,
    errors: Vec<String>,
    yaml: SketchYaml,
}

/// Adopt a profile-less sketch: ensure `profile` exists for `fqbn`, then pin
/// every library a successful build reported using. Sketchbook libraries are
/// pinned `name@version` from the registry; when arduino-cli rejects that
/// (not in the index), an absolute `dir:` entry pins the folder itself.
/// Platform-bundled libraries (outside the sketchbook) are skipped.
#[tauri::command]
async fn pin_current_setup(
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: String,
    fqbn: String,
    libs: Vec<UsedLib>,
) -> Result<PinReport, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = Path::new(&sketch_dir);
        let project = SketchProject::open(dir).map_err(err_str)?;
        if !project.load_yaml().map_err(err_str)?.profiles.contains_key(&profile) {
            project.init_profile(&profile, &fqbn).map_err(err_str)?;
        }
        let libs_root = cli.sketchbook_libraries_dir().map_err(err_str)?;
        let mut registry = Vec::new();
        let mut local = Vec::new();
        let mut skipped = Vec::new();
        let mut errors = Vec::new();
        for lib in libs {
            let path = Path::new(&lib.path);
            if !path.starts_with(&libs_root) {
                skipped.push(lib.name);
                continue;
            }
            let spec = format!("{}@{}", lib.name, lib.version);
            match cli.profile_lib_add(dir, &profile, &spec) {
                Ok(()) => registry.push(lib.name),
                Err(_) => {
                    match project.add_local_library_with(
                        &profile,
                        path,
                        bancada_core::sketch::PathStyle::Absolute,
                    ) {
                        Ok(_) => local.push(lib.name),
                        Err(e) => errors.push(format!("{}: {e}", lib.name)),
                    }
                }
            }
        }
        let yaml = project.load_yaml().map_err(err_str)?;
        Ok(PinReport { registry, local, skipped, errors, yaml })
    })
    .await
    .map_err(err_str)?
}
```

Register `pin_current_setup,` in `generate_handler!` next to Task 2's entries.

- [ ] **Step 2: Add the api.ts wrapper** (plus the two types):

```ts
/** One row of the used-library compile report, as sent to pin_current_setup. */
export interface UsedLibraryRow {
  name: string;
  version: string;
  path: string;
}

export interface PinReport {
  registry: string[];
  local: string[];
  skipped: string[];
  errors: string[];
  yaml: SketchYaml;
}

/**
 * Adopt a profile-less sketch: create `profile` for `fqbn` (if missing) and
 * pin every used library — registry spec first, absolute dir: fallback.
 */
export const pinCurrentSetup = (
  sketchDir: string,
  profile: string,
  fqbn: string,
  libs: UsedLibraryRow[],
) =>
  invoke<PinReport>("pin_current_setup", { sketchDir, profile, fqbn, libs });
```

- [ ] **Step 3: Verify it builds**

Run: `cargo check -p bancada 2>&1 | tail -5` and `npm run build`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src/api.ts
git commit -m "feat: pin_current_setup adopts a profile-less sketch"
```

---

### Task 8: "Pin current setup" UI

**Files:**
- Modify: `src/components/LibraryManager.tsx` (the Project tab's no-profile empty state from Task 4)

**Interfaces:**
- Consumes: Task 6 `parseUsedLibraries`, Task 7 `pinCurrentSetup`; existing
  `compileSketch` (`src/api.ts:446`), `profileNameForFqbn`, `listen` from
  `@tauri-apps/api/event`, `OutputLine` type from `../api`; `fqbn` prop.
- Produces: nothing for later tasks.

- [ ] **Step 1: Replace the no-profile empty state** (from Task 4 Step 3) with:

```tsx
          {sketchDir && !profile && (
            <div className="empty-hint">
              <p>
                This sketch has no build profile, so nothing is pinned and
                builds use whatever the sketchbook holds.
              </p>
              <button
                className="btn small primary"
                disabled={working || !fqbn}
                onClick={pinCurrent}
                title={
                  fqbn
                    ? "Build once, then pin every library the build used"
                    : "Select a board first"
                }
              >
                Pin current setup
              </button>
            </div>
          )}
```

- [ ] **Step 2: Add the handler.** The compile runs *without* a profile (the
  fresh profile pins nothing yet, and profile builds are hermetic — it would
  fail on every missing library), so it sees the sketchbook exactly like
  today's builds; its report is what gets pinned:

```ts
  const pinCurrent = async () => {
    if (!sketchDir || !fqbn) return;
    setWorking(true);
    const lines: string[] = [];
    const unlisten = await listen<OutputLine>("build://line", (e) =>
      lines.push(e.payload.line),
    );
    try {
      notify("Building to discover used libraries…");
      const r = await compileSketch(sketchDir, undefined, fqbn);
      if (!r.success) {
        notify("Build failed — fix the sketch, then pin", true);
        return;
      }
      const used = parseUsedLibraries(lines);
      if (used.length === 0) {
        notify("Build used no libraries — created the profile only");
      }
      const rep = await pinCurrentSetup(sketchDir, profileNameForFqbn(fqbn), fqbn, used);
      onYamlChanged(rep.yaml);
      const pinnedCount = rep.registry.length + rep.local.length;
      notify(
        rep.errors.length
          ? `Pinned ${pinnedCount}, ${rep.errors.length} failed: ${rep.errors.join("; ")}`
          : `✓ Pinned ${pinnedCount} librar${pinnedCount === 1 ? "y" : "ies"}` +
            (rep.local.length ? ` (${rep.local.length} as local dir — consider vendoring)` : "") +
            (rep.skipped.length ? `, skipped ${rep.skipped.length} platform-bundled` : ""),
        rep.errors.length > 0,
      );
    } catch (e) {
      notify(String(e), true);
    } finally {
      unlisten();
      setWorking(false);
    }
  };
```

- [ ] **Step 3: Verify**

Run: `npm test && npm run build` — green. In the dev app with a profile-less
sketch: button builds, then the Project tab fills with pins; a second look at
`sketch.yaml` shows the profile with `libraries:` entries; platform-bundled
libraries (WiFi etc.) are absent from the pins.

- [ ] **Step 4: Commit**

```bash
git add src/components/LibraryManager.tsx
git commit -m "feat(ui): one-click adoption pins a profile-less sketch's setup"
```

---

### Task 9: Gated integration test — pin round-trip against real arduino-cli

**Files:**
- Create: `core/tests/profile_pin_roundtrip.rs`

**Interfaces:**
- Consumes: `ArduinoCli::{sketch_new, profile_create, profile_lib_add}`,
  `SketchProject::{open, load_yaml, remove_library}`, `LibraryDep`,
  `registry_pin_name` (Task 1).
- Produces: nothing — proof.

- [ ] **Step 1: Write the test** (same `#[ignore]` gating as `core/tests/new_project_builds.rs`):

```rust
//! Opt-in proof that pin management round-trips through a real arduino-cli.
//!
//! ```text
//! cargo test -p bancada-core --test profile_pin_roundtrip -- --ignored --nocapture
//! ```
//!
//! Needs arduino-cli, an installed core for `BANCADA_TEST_FQBN`
//! (default `arduino:avr:uno`) and a populated library index.

use bancada_core::cli::ArduinoCli;
use bancada_core::sketch::{registry_pin_name, LibraryDep, SketchProject};

#[test]
#[ignore = "needs arduino-cli, an installed core and the library index"]
fn registry_pin_roundtrip() {
    let fqbn = std::env::var("BANCADA_TEST_FQBN").unwrap_or_else(|_| "arduino:avr:uno".to_string());
    let cli = ArduinoCli::default();

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("PinDemo");
    cli.sketch_new(&dir).expect("sketch new");
    cli.profile_create(&dir, "test", &fqbn, true).expect("profile create");

    // Pin, at an explicit version so the assertion is exact.
    cli.profile_lib_add(&dir, "test", "ArduinoJson@7.4.2")
        .expect("profile lib add");
    let project = SketchProject::open(&dir).unwrap();
    let y = project.load_yaml().unwrap();
    let pins = &y.profiles["test"].libraries;
    assert!(
        pins.iter().any(|l| matches!(l, LibraryDep::Registry(s)
            if registry_pin_name(s) == "ArduinoJson" && s.contains("7.4.2"))),
        "{pins:?}"
    );

    // Change version = remove + re-add, the same sequence the command runs.
    project
        .remove_library("test", &LibraryDep::Registry("ArduinoJson".into()))
        .expect("remove");
    cli.profile_lib_add(&dir, "test", "ArduinoJson@7.0.4").expect("re-add");
    let y = project.load_yaml().unwrap();
    assert!(
        y.profiles["test"].libraries.iter().any(|l| matches!(l, LibraryDep::Registry(s)
            if s.contains("7.0.4") && !s.contains("7.4.2"))),
        "{:?}",
        y.profiles["test"].libraries
    );

    // Unpin leaves the profile with no libraries.
    project
        .remove_library("test", &LibraryDep::Registry("ArduinoJson".into()))
        .expect("unpin");
    assert!(project.load_yaml().unwrap().profiles["test"].libraries.is_empty());
}
```

- [ ] **Step 2: Run it** (arduino-cli is on this machine):

Run: `cargo test -p bancada-core --test profile_pin_roundtrip -- --ignored --nocapture`
Expected: PASS. If `profile lib add` output shape differs from expectations,
this is where it surfaces — fix Task 2's command accordingly, not the test.

- [ ] **Step 3: Run everything once more**

Run: `cargo test -p bancada-core && npm test && npm run build`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add core/tests/profile_pin_roundtrip.rs
git commit -m "test: gated pin round-trip against real arduino-cli"
```
