# Profile Board Switching (add / retarget) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the toolbar, add a sketch.yaml profile for another board (libraries copied) or change the selected profile's board in place.

**Architecture:** Two pure yaml edits in `bancada-core` (`add_profile` with library copy, `retarget_profile`), wrapped by Tauri commands that resolve the installed platform pin and inject board-required libraries (`required_profile_libs`), surfaced through the existing one-row `ProfileInit` form generalized with a `mode`, plus two toolbar buttons beside the profile selector.

**Tech Stack:** Rust (serde_yaml core, Tauri 2 commands), TypeScript/React (vitest), arduino-cli via existing `ArduinoCli` wrapper.

**Spec:** `docs/superpowers/specs/2026-08-09-profile-board-switching-design.md`

## Global Constraints

- The working tree already carries two approved-but-uncommitted bug fixes (RouterBridge profile-lib injection; flash-target mismatch warning). Task 0 commits them first so feature commits stay clean. Do not revert or fold them into feature commits.
- Every core function stays pure: no subprocess, no CLI calls inside `core/` (platform pins arrive as pre-built strings).
- `default_profile` is never modified by add or retarget (bootstrap's "first profile becomes default" behavior stays).
- Commit messages follow repo style: `feat:`/`fix:`/`docs:` prefix, imperative, no scope parens. End with the Co-Authored-By trailer used in Task 0.
- After each task: the full suite you touched stays green (`cargo test -p bancada-core --lib`, `npx vitest run`, `cargo check -p bancada` for command changes).
- This is a shared checkout — before every commit, run `git status --short` and stage only the files the task names.

---

### Task 0: Commit the two pending bug fixes

**Files:**
- Commit (no edits): `core/src/project.rs`, `src-tauri/src/lib.rs`, `src/App.tsx`, `src/ports.ts`, `src/__tests__/ports.test.ts`

**Interfaces:**
- Produces: a clean working tree; `required_profile_libs()` (core/src/project.rs) and `flashTargetMismatch()` (src/ports.ts) exist on HEAD for later tasks.

- [ ] **Step 1: Verify the pending changes are exactly the two known fixes**

Run: `git status --short && git diff --stat`
Expected: exactly `core/src/project.rs`, `src-tauri/src/lib.rs`, `src/App.tsx`, `src/__tests__/ports.test.ts`, `src/ports.ts` modified. If anything else appears, STOP and ask.

- [ ] **Step 2: Run both suites**

Run: `cargo test -p bancada-core --lib && cargo check -p bancada && npx vitest run`
Expected: all green (424+ core tests, 429 vitest tests).

- [ ] **Step 3: Commit fix 1 (RouterBridge injection)**

```bash
git add core/src/project.rs src-tauri/src/lib.rs
git commit -m "fix: pin board-required libs into new profiles

A hermetic profile build only sees sketch.yaml-pinned libraries, and the
UNO Q core #errors out of Arduino.h without Arduino_RouterBridge — so
every fresh arduino:zephyr profile failed to compile. create_project and
init_profile now inject required_profile_libs() via profile lib add.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

- [ ] **Step 4: Commit fix 2 (mismatch warning)**

```bash
git add src/App.tsx src/ports.ts src/__tests__/ports.test.ts
git commit -m "feat: warn when the profile targets a different board than the port

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Verify clean tree**

Run: `git status --short`
Expected: empty output.

---

### Task 1: core parses `dependency:` library entries (bug found during planning)

`arduino-cli profile lib add` writes resolved dependencies as
`- dependency: MsgPack (0.4.2)` — a YAML mapping. `LibraryDep` has only
`Registry(String)` and `Local { dir }`, so `load_yaml` **errors** on any
profile that ever went through `profile lib add` (verified 2026-08-09 with a
failing probe test). Since the RouterBridge fix, that is every fresh UNO Q
profile: without this task the app cannot even open such projects, and the
library copy in Task 2 could not preserve these entries.

**Files:**
- Modify: `core/src/sketch.rs` (the `LibraryDep` enum, ~line 48; tests in its test module)

**Interfaces:**
- Produces: `LibraryDep::Dependency { dependency: String }` variant; Task 2's copy test uses it.

- [ ] **Step 1: Write the failing test**

Add to `core/src/sketch.rs` tests:

```rust
#[test]
fn parses_and_round_trips_dependency_library_entries() {
    // `profile lib add` writes resolved deps as `- dependency: Name (v)`.
    let (_t, proj) = proj();
    std::fs::write(
        proj.dir.join("sketch.yaml"),
        r#"profiles:
  unoq:
    fqbn: arduino:zephyr:unoq
    libraries:
      - dependency: Arduino_RPClite (0.3.0)
      - Arduino_RouterBridge (0.4.3)
      - dir: ../libs/Foo
default_profile: unoq
"#,
    )
    .unwrap();
    let y = proj.load_yaml().expect("dependency: entries must parse");
    let libs = &y.profiles["unoq"].libraries;
    assert_eq!(libs.len(), 3);
    assert_eq!(
        libs[0],
        LibraryDep::Dependency { dependency: "Arduino_RPClite (0.3.0)".into() }
    );
    // Round-trip: saving must keep the mapping form arduino-cli understands.
    proj.save_yaml(&y).unwrap();
    let text = std::fs::read_to_string(proj.dir.join("sketch.yaml")).unwrap();
    assert!(text.contains("dependency: Arduino_RPClite (0.3.0)"), "{text}");
}
```

(Adapt to the test module's actual project helper, as in later tasks.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p bancada-core --lib dependency_library_entries`
Expected: FAIL — `Dependency` variant not found (compile error).

- [ ] **Step 3: Add the variant**

In `core/src/sketch.rs`, extend the enum (order matters for untagged serde:
keep `Registry` first so plain strings still match it; mappings fall through
to the struct-like variants):

```rust
/// A profile library dependency: a registry entry like
/// `"ArduinoJson (7.4.2)"`, a resolved dependency arduino-cli records as
/// `dependency: Name (v)`, or a local path entry `{ dir: ../libs/Foo }`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum LibraryDep {
    Registry(String),
    Dependency { dependency: String },
    Local { dir: String },
}
```

- [ ] **Step 4: Run the core suite and type-check dependents**

Run: `cargo test -p bancada-core --lib && cargo check -p bancada && npx tsc --noEmit`
Expected: all green. If any Rust `match` on `LibraryDep` fails exhaustiveness, extend it treating `Dependency` like `Registry`. Check the TS side too: `LibraryDep` in `src/api.ts` — if it is a union type, add `{ dependency: string }` to it and follow the compiler to any switch sites (the Library manager renders entries).

- [ ] **Step 5: Commit**

```bash
git add core/src/sketch.rs
git commit -m "fix: parse arduino-cli dependency: library entries in sketch.yaml

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

(Include any `src/api.ts` / component files Step 4 forced you to touch.)

---

### Task 2: core `add_profile` — create a profile, optionally copying libraries

**Files:**
- Modify: `core/src/sketch.rs` (function beside `init_profile`, ~line 129; tests in the `init_profile` test section, ~line 655)

**Interfaces:**
- Consumes: `SketchYaml`, `Profile`, `PlatformDep`, `LibraryDep` (core/src/sketch.rs:17-53), existing `load_yaml`/`save_yaml`.
- Produces: `pub fn add_profile(&self, profile_name: &str, fqbn: &str, platform_entry: Option<&str>, copy_libs_from: Option<&str>) -> Result<SketchYaml>` on `SketchProject`. `init_profile` becomes a thin wrapper (`self.add_profile(name, fqbn, None, None)`) with unchanged behavior.

- [ ] **Step 1: Write the failing tests**

Add to the `init_profile` test section of `core/src/sketch.rs` (uses the file's existing `tempfile`-based `proj()` helper — mirror how `init_profile_creates_yaml_and_sets_default` builds a project):

```rust
#[test]
fn add_profile_copies_libraries_from_the_source_profile() {
    let (_t, proj) = proj();
    proj.init_profile("uno", "arduino:avr:uno").unwrap();
    // One of each LibraryDep shape the copy must preserve.
    let mut y = proj.load_yaml().unwrap();
    y.profiles.get_mut("uno").unwrap().libraries = vec![
        LibraryDep::Registry("ArduinoJson (7.4.2)".into()),
        LibraryDep::Dependency { dependency: "MsgPack (0.4.2)".into() },
        LibraryDep::Local { dir: "../libs/Foo".into() },
    ];
    proj.save_yaml(&y).unwrap();

    let out = proj
        .add_profile("nano", "arduino:avr:nano", Some("arduino:avr (1.8.8)"), Some("uno"))
        .unwrap();
    let nano = &out.profiles["nano"];
    assert_eq!(nano.fqbn, "arduino:avr:nano");
    assert_eq!(nano.libraries, out.profiles["uno"].libraries);
    assert_eq!(nano.platforms.len(), 1);
    assert_eq!(nano.platforms[0].platform, "arduino:avr (1.8.8)");
    // The addition is persisted, not just returned.
    assert_eq!(proj.load_yaml().unwrap().profiles["nano"].fqbn, "arduino:avr:nano");
}

#[test]
fn add_profile_does_not_touch_the_default_profile() {
    let (_t, proj) = proj();
    proj.init_profile("uno", "arduino:avr:uno").unwrap();
    let out = proj
        .add_profile("nano", "arduino:avr:nano", None, Some("uno"))
        .unwrap();
    assert_eq!(out.default_profile.as_deref(), Some("uno"));
}

#[test]
fn add_profile_rejects_an_unknown_copy_source() {
    let (_t, proj) = proj();
    proj.init_profile("uno", "arduino:avr:uno").unwrap();
    let err = proj
        .add_profile("nano", "arduino:avr:nano", None, Some("mega"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("mega"), "got: {err}");
}

#[test]
fn add_profile_without_a_source_starts_with_no_libraries() {
    let (_t, proj) = proj();
    let out = proj
        .add_profile("uno", "arduino:avr:uno", Some("arduino:avr (1.8.8)"), None)
        .unwrap();
    assert!(out.profiles["uno"].libraries.is_empty());
    // First profile still becomes the default (bootstrap behavior).
    assert_eq!(out.default_profile.as_deref(), Some("uno"));
}
```

If the test section's project helper has a different name/shape than `proj()`, adapt the calls to the file's actual helper — behavior asserted stays identical.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bancada-core --lib add_profile`
Expected: compile error — `add_profile` not found.

- [ ] **Step 3: Implement `add_profile`, re-route `init_profile`**

Replace the body of `init_profile` (core/src/sketch.rs:129) and add below it:

```rust
    /// Create `profile_name` pointing at `fqbn`, creating sketch.yaml when
    /// absent. Never overwrites: a profile that already exists is an error.
    /// The sketch's first profile also becomes `default_profile`, so gaining
    /// a profile gives Verify/Flash a working default immediately.
    pub fn init_profile(&self, profile_name: &str, fqbn: &str) -> Result<SketchYaml> {
        self.add_profile(profile_name, fqbn, None, None)
    }

    /// Create `profile_name` for `fqbn`, optionally pinning a platform and
    /// copying another profile's `libraries:` list verbatim — registry pins,
    /// `dependency:` entries and `dir:` locals alike. Copying is what makes
    /// "flash the same project on a second board" build immediately: a
    /// profile build only sees its own pinned libraries.
    pub fn add_profile(
        &self,
        profile_name: &str,
        fqbn: &str,
        platform_entry: Option<&str>,
        copy_libs_from: Option<&str>,
    ) -> Result<SketchYaml> {
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
        let libraries = match copy_libs_from {
            Some(src) => y
                .profiles
                .get(src.trim())
                .ok_or_else(|| {
                    Error::Other(format!("profile `{}` not found to copy libraries from", src.trim()))
                })?
                .libraries
                .clone(),
            None => Vec::new(),
        };
        let platforms = platform_entry
            .map(|p| {
                vec![PlatformDep {
                    platform: p.to_string(),
                    platform_index_url: None,
                }]
            })
            .unwrap_or_default();
        y.profiles.insert(
            name.to_string(),
            Profile {
                fqbn: fqbn.to_string(),
                platforms,
                libraries,
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

- [ ] **Step 4: Run the core suite**

Run: `cargo test -p bancada-core --lib`
Expected: all pass — the four new tests plus every existing `init_profile` test (wrapper behavior unchanged).

- [ ] **Step 5: Commit**

```bash
git add core/src/sketch.rs
git commit -m "feat: add_profile with platform pin and library copy in core

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: core `retarget_profile` — change a profile's board in place

**Files:**
- Modify: `core/src/sketch.rs` (below `add_profile`; tests beside Task 2's)

**Interfaces:**
- Consumes: `add_profile` neighborhood from Task 2 (same file, same model types).
- Produces: `pub fn retarget_profile(&self, profile_name: &str, fqbn: &str, platform_entry: &str) -> Result<SketchYaml>` on `SketchProject`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn retarget_swaps_fqbn_and_platform_pin_keeping_everything_else() {
    let (_t, proj) = proj();
    proj.add_profile("uno", "arduino:avr:uno", Some("arduino:avr (1.8.8)"), None)
        .unwrap();
    let mut y = proj.load_yaml().unwrap();
    {
        let p = y.profiles.get_mut("uno").unwrap();
        p.libraries = vec![LibraryDep::Registry("ArduinoJson (7.4.2)".into())];
        p.port = Some("/dev/ttyACM0".into());
        p.notes = Some("bench uno".into());
    }
    proj.save_yaml(&y).unwrap();

    let out = proj
        .retarget_profile("uno", "arduino:zephyr:unoq", "arduino:zephyr (0.90.0)")
        .unwrap();
    let p = &out.profiles["uno"];
    assert_eq!(p.fqbn, "arduino:zephyr:unoq");
    assert_eq!(p.platforms.len(), 1);
    assert_eq!(p.platforms[0].platform, "arduino:zephyr (0.90.0)");
    // The point of "in place": name, libraries, port and notes survive.
    assert_eq!(p.libraries, vec![LibraryDep::Registry("ArduinoJson (7.4.2)".into())]);
    assert_eq!(p.port.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(p.notes.as_deref(), Some("bench uno"));
    // Persisted, and default_profile untouched.
    assert_eq!(proj.load_yaml().unwrap().profiles["uno"].fqbn, "arduino:zephyr:unoq");
    assert_eq!(out.default_profile.as_deref(), Some("uno"));
}

#[test]
fn retarget_to_the_same_board_is_a_no_op_success() {
    let (_t, proj) = proj();
    proj.add_profile("uno", "arduino:avr:uno", Some("arduino:avr (1.8.8)"), None)
        .unwrap();
    let out = proj
        .retarget_profile("uno", "arduino:avr:uno", "arduino:avr (1.8.8)")
        .unwrap();
    assert_eq!(out.profiles["uno"].fqbn, "arduino:avr:uno");
}

#[test]
fn retarget_rejects_an_unknown_profile_and_blank_inputs() {
    let (_t, proj) = proj();
    proj.add_profile("uno", "arduino:avr:uno", None, None).unwrap();
    assert!(proj.retarget_profile("mega", "arduino:avr:nano", "arduino:avr (1.8.8)").is_err());
    assert!(proj.retarget_profile("uno", "  ", "arduino:avr (1.8.8)").is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bancada-core --lib retarget`
Expected: compile error — `retarget_profile` not found.

- [ ] **Step 3: Implement**

```rust
    /// Point an existing profile at a different board, replacing its platform
    /// pin and touching nothing else — name, libraries, port and notes stay.
    /// Replacing the pin is not optional: a leftover `arduino:avr` pin under
    /// an `arduino:zephyr` fqbn is exactly the broken state this prevents.
    pub fn retarget_profile(
        &self,
        profile_name: &str,
        fqbn: &str,
        platform_entry: &str,
    ) -> Result<SketchYaml> {
        let name = profile_name.trim();
        let fqbn = fqbn.trim();
        if fqbn.is_empty() {
            return Err(Error::Other("a profile needs a board (FQBN)".into()));
        }
        let mut y = self.load_yaml()?;
        let p = y
            .profiles
            .get_mut(name)
            .ok_or_else(|| Error::Other(format!("profile `{name}` not found")))?;
        p.fqbn = fqbn.to_string();
        p.platforms = vec![PlatformDep {
            platform: platform_entry.to_string(),
            platform_index_url: None,
        }];
        self.save_yaml(&y)?;
        Ok(y)
    }
```

- [ ] **Step 4: Run the core suite**

Run: `cargo test -p bancada-core --lib`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add core/src/sketch.rs
git commit -m "feat: retarget_profile swaps a profile's board in place

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: platform id from FQBN (core) + Tauri commands

**Files:**
- Modify: `core/src/boards.rs` (beside `parse_core_id`, ~line 26; tests in its test module)
- Modify: `src-tauri/src/lib.rs` (replace the `init_profile` command, ~line 357; register `retarget_profile` in the `invoke_handler` list near `init_profile`, ~line 3088)

**Interfaces:**
- Consumes: `add_profile`/`retarget_profile` (Tasks 2-3); `boards::platform_dep_entry(id, version)` (core/src/boards.rs:171); `cli.core_list() -> Result<Vec<Platform>>` (core/src/cli.rs:169, `Platform.id`/`Platform.installed_version`); `bancada_core::project::required_profile_libs(&fqbn)`; `cli.profile_lib_add(&Path, &str, &str)` (core/src/cli.rs:251); existing `err_str`, `AppState`, `SketchProject`.
- Produces: core `pub fn fqbn_platform_id(fqbn: &str) -> Result<String>`; commands `init_profile(state, sketch_dir, profile, fqbn, copy_libs_from: Option<String>)` and `retarget_profile(state, sketch_dir, profile, fqbn)`, both returning `Result<SketchYaml, String>`. JS argument keys: `sketchDir`, `profile`, `fqbn`, `copyLibsFrom`.

- [ ] **Step 1: Write the failing core test**

Add to `core/src/boards.rs` tests:

```rust
#[test]
fn fqbn_platform_id_takes_the_first_two_segments() {
    assert_eq!(fqbn_platform_id("arduino:avr:uno").unwrap(), "arduino:avr");
    assert_eq!(
        fqbn_platform_id("esp32:esp32:esp32s3:CDCOnBoot=cdc").unwrap(),
        "esp32:esp32"
    );
}

#[test]
fn fqbn_platform_id_rejects_malformed_fqbns() {
    for bad in ["arduino", "arduino:avr", "", ":a:b", "a::b"] {
        assert!(fqbn_platform_id(bad).is_err(), "{bad:?} should be rejected");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p bancada-core --lib fqbn_platform_id`
Expected: compile error — not found.

- [ ] **Step 3: Implement in `core/src/boards.rs`**

```rust
/// The `packager:architecture` platform id of a board FQBN
/// (`arduino:avr:uno` → `arduino:avr`). Board options after the third
/// segment are ignored; fewer than three segments is not an FQBN.
pub fn fqbn_platform_id(fqbn: &str) -> Result<String> {
    let trimmed = fqbn.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() < 3 || parts[..3].iter().any(|p| p.is_empty()) {
        return Err(Error::Other(format!(
            "`{trimmed}` is not a board FQBN (expected `packager:architecture:board`)"
        )));
    }
    Ok(format!("{}:{}", parts[0], parts[1]))
}
```

- [ ] **Step 4: Run core suite**

Run: `cargo test -p bancada-core --lib`
Expected: all pass.

- [ ] **Step 5: Rework the commands in `src-tauri/src/lib.rs`**

Replace the whole existing `init_profile` command (the `async fn init_profile` added on 2026-08-09, which already does required-lib injection) with:

```rust
/// The pinned `platform:` entry for `fqbn`, from the installed platform.
/// Not installed → an error naming the core to install; nothing is written
/// by callers before this succeeds.
fn installed_platform_entry(
    cli: &bancada_core::cli::ArduinoCli,
    fqbn: &str,
) -> Result<String, String> {
    let id = bancada_core::boards::fqbn_platform_id(fqbn).map_err(err_str)?;
    let platforms = cli.core_list().map_err(err_str)?;
    let installed = platforms
        .iter()
        .find(|p| p.id == id && !p.installed_version.is_empty())
        .ok_or_else(|| {
            format!("the {id} core is not installed — install it in the Boards manager first")
        })?;
    Ok(bancada_core::boards::platform_dep_entry(
        &id,
        &installed.installed_version,
    ))
}

/// Pin `required_profile_libs` into a fresh or retargeted profile, loud on
/// failure — a profile that silently cannot build is the bug this replaces.
fn pin_required_libs(
    cli: &bancada_core::cli::ArduinoCli,
    sketch_dir: &str,
    profile: &str,
    fqbn: &str,
) -> Result<(), String> {
    for lib in bancada_core::project::required_profile_libs(fqbn) {
        cli.profile_lib_add(Path::new(sketch_dir), profile, lib)
            .map_err(|e| {
                format!(
                    "profile \"{profile}\" was written, but this board needs the \
                     {lib} library and pinning it failed: {e}. Add it in the \
                     Library manager before building."
                )
            })?;
    }
    Ok(())
}

#[tauri::command]
async fn init_profile(
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: String,
    fqbn: String,
    copy_libs_from: Option<String>,
) -> Result<SketchYaml, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let entry = installed_platform_entry(&cli, &fqbn)?;
        let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
        proj.add_profile(&profile, &fqbn, Some(&entry), copy_libs_from.as_deref())
            .map_err(err_str)?;
        pin_required_libs(&cli, &sketch_dir, &profile, &fqbn)?;
        // Reload: profile lib add rewrites sketch.yaml behind the first write.
        proj.load_yaml().map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn retarget_profile(
    state: State<'_, AppState>,
    sketch_dir: String,
    profile: String,
    fqbn: String,
) -> Result<SketchYaml, String> {
    let cli = state.cli.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let entry = installed_platform_entry(&cli, &fqbn)?;
        let proj = SketchProject::open(&sketch_dir).map_err(err_str)?;
        proj.retarget_profile(&profile, &fqbn, &entry)
            .map_err(err_str)?;
        pin_required_libs(&cli, &sketch_dir, &profile, &fqbn)?;
        proj.load_yaml().map_err(err_str)
    })
    .await
    .map_err(err_str)?
}
```

Then add `retarget_profile,` next to `init_profile,` in the `generate_handler![...]` list (~line 3088). Check the `ArduinoCli` import path used elsewhere in lib.rs (`state.cli` type) and match it in the helper signatures.

- [ ] **Step 6: Compile and run suites**

Run: `cargo check -p bancada && cargo test -p bancada-core --lib`
Expected: clean check, all core tests pass.

- [ ] **Step 7: Commit**

```bash
git add core/src/boards.rs src-tauri/src/lib.rs
git commit -m "feat: init_profile copies libraries and pins platforms; retarget_profile command

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: typed API wrappers

**Files:**
- Modify: `src/api.ts` (the `initProfile` export, ~line 306)
- Test: `src/__tests__/api.test.ts` (beside the existing `initProfile` contract test, ~line 383)

**Interfaces:**
- Consumes: command names/keys from Task 4 (`init_profile`, `retarget_profile`; keys `sketchDir`, `profile`, `fqbn`, `copyLibsFrom`).
- Produces:
  `initProfile(sketchDir: string, profile: string, fqbn: string, copyLibsFrom?: string): Promise<SketchYaml>`
  `retargetProfile(sketchDir: string, profile: string, fqbn: string): Promise<SketchYaml>`

- [ ] **Step 1: Write the failing contract tests**

In `src/__tests__/api.test.ts`, replace the existing `initProfile` test and add one:

```ts
  it("initProfile passes sketchDir, profile, fqbn and copyLibsFrom", async () => {
    await api.initProfile("/s", "nano", "arduino:avr:nano", "uno");
    expect(called()).toEqual([
      "init_profile",
      { sketchDir: "/s", profile: "nano", fqbn: "arduino:avr:nano", copyLibsFrom: "uno" },
    ]);
    await api.initProfile("/s", "uno", "arduino:avr:uno");
    expect(called()).toEqual([
      "init_profile",
      { sketchDir: "/s", profile: "uno", fqbn: "arduino:avr:uno", copyLibsFrom: null },
    ]);
  });

  it("retargetProfile passes sketchDir, profile and fqbn", async () => {
    await api.retargetProfile("/s", "uno", "arduino:zephyr:unoq");
    expect(called()).toEqual([
      "retarget_profile",
      { sketchDir: "/s", profile: "uno", fqbn: "arduino:zephyr:unoq" },
    ]);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/__tests__/api.test.ts`
Expected: FAIL — `retargetProfile` not a function, `initProfile` missing `copyLibsFrom`.

- [ ] **Step 3: Implement in `src/api.ts`**

```ts
/** Create sketch.yaml (when absent) with a profile for `fqbn`; optionally
 *  copy another profile's libraries so the project builds on the new board. */
export const initProfile = (
  sketchDir: string,
  profile: string,
  fqbn: string,
  copyLibsFrom?: string,
) =>
  invoke<SketchYaml>("init_profile", {
    sketchDir,
    profile,
    fqbn,
    copyLibsFrom: copyLibsFrom ?? null,
  });
/** Point an existing profile at a different board, keeping its libraries. */
export const retargetProfile = (sketchDir: string, profile: string, fqbn: string) =>
  invoke<SketchYaml>("retarget_profile", { sketchDir, profile, fqbn });
```

- [ ] **Step 4: Run the frontend suite**

Run: `npx tsc --noEmit && npx vitest run`
Expected: clean, all pass.

- [ ] **Step 5: Commit**

```bash
git add src/api.ts src/__tests__/api.test.ts
git commit -m "feat: initProfile copyLibsFrom arg and retargetProfile wrapper

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: `ProfileInit` grows modes (bootstrap / add / retarget)

**Files:**
- Modify: `src/components/ProfileInit.tsx`

**Interfaces:**
- Consumes: `initProfile`/`retargetProfile` (Task 5), `profileNameForFqbn` (src/profileInit.ts), `BoardPicker`.
- Produces: the component's new props, consumed by Task 7:

```ts
export type ProfileFormMode = "bootstrap" | "add" | "retarget";
interface Props {
  mode: ProfileFormMode;
  sketchDir: string;
  detectedFqbn: string | null;
  /** Selected profile: retarget's target, add's library-copy source. */
  currentProfile: string | null;
  /** That profile's FQBN (retarget's picker preselect). */
  currentFqbn: string | null;
  onDone: (yaml: SketchYaml, profile: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}
```

- [ ] **Step 1: Rewrite the component**

Replace `src/components/ProfileInit.tsx` with:

```tsx
import { useEffect, useState } from "react";
import BoardPicker from "./BoardPicker";
import {
  initProfile,
  listAllBoards,
  retargetProfile,
  type BoardOption,
  type SketchYaml,
} from "../api";
import { profileNameForFqbn } from "../profileInit";

export type ProfileFormMode = "bootstrap" | "add" | "retarget";

interface Props {
  mode: ProfileFormMode;
  sketchDir: string;
  /** FQBN detected on the selected port, preselected for new profiles. */
  detectedFqbn: string | null;
  /** Selected profile: retarget's target, add's library-copy source. */
  currentProfile: string | null;
  /** That profile's FQBN (retarget's picker preselect). */
  currentFqbn: string | null;
  onDone: (yaml: SketchYaml, profile: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

const LABEL: Record<ProfileFormMode, string> = {
  bootstrap: "New sketch.yaml profile:",
  add: "Add profile for another board:",
  retarget: "Change this profile's board:",
};

/** One-row form under the toolbar: bootstrap the first profile, add one for
 *  another board (libraries copied from the current profile), or point the
 *  current profile at a different board in place. */
export default function ProfileInit({
  mode,
  sketchDir,
  detectedFqbn,
  currentProfile,
  currentFqbn,
  onDone,
  onCancel,
  notify,
}: Props) {
  const initialFqbn = (mode === "retarget" ? currentFqbn : detectedFqbn) ?? "";
  const [boards, setBoards] = useState<BoardOption[]>([]);
  const [fqbn, setFqbn] = useState(initialFqbn);
  const [name, setName] = useState(
    mode !== "retarget" && initialFqbn ? profileNameForFqbn(initialFqbn) : "",
  );
  const [nameTouched, setNameTouched] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    listAllBoards()
      .then(setBoards)
      .catch((e) => notify(String(e), true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pick = (f: string) => {
    setFqbn(f);
    if (mode !== "retarget" && !nameTouched)
      setName(f ? profileNameForFqbn(f) : "");
  };

  const submit = async () => {
    setBusy(true);
    try {
      if (mode === "retarget") {
        if (!currentProfile) return; // button is disabled without a profile
        const yaml = await retargetProfile(sketchDir, currentProfile, fqbn);
        notify(`✓ Profile “${currentProfile}” now builds for ${fqbn}`);
        onDone(yaml, currentProfile);
      } else {
        const yaml = await initProfile(
          sketchDir,
          name.trim(),
          fqbn,
          mode === "add" ? (currentProfile ?? undefined) : undefined,
        );
        notify(`✓ Profile “${name.trim()}” written to sketch.yaml`);
        onDone(yaml, name.trim());
      }
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const ready = mode === "retarget" ? !!fqbn : !!fqbn && !!name.trim();
  return (
    <div className="profile-init">
      <span className="profile-init-label">{LABEL[mode]}</span>
      <BoardPicker
        boards={boards}
        value={fqbn}
        onChange={pick}
        title="Board for this profile"
      />
      {mode === "retarget" ? (
        <span className="profile-init-label" title="Profile being retargeted">
          {currentProfile}
        </span>
      ) : (
        <input
          className="input"
          value={name}
          placeholder="profile name"
          onChange={(e) => {
            setName(e.target.value);
            setNameTouched(true);
          }}
        />
      )}
      <button className="btn primary" disabled={busy || !ready} onClick={submit}>
        {mode === "retarget" ? "Change board" : "Create"}
      </button>
      <button className="btn" disabled={busy} onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}
```

- [ ] **Step 2: Type-check (App.tsx will fail — expected)**

Run: `npx tsc --noEmit`
Expected: errors ONLY in `src/App.tsx` (old props `onCreated`, missing `mode` etc.). Any error inside `ProfileInit.tsx` itself must be fixed now. Task 7 fixes the App call site — do NOT commit yet; Tasks 6 and 7 land as one commit there.

---

### Task 7: Toolbar buttons + App wiring

**Files:**
- Modify: `src/components/Toolbar.tsx` (profile group, ~lines 78-105)
- Modify: `src/App.tsx` (state ~line 110, Toolbar props ~line 1165, ProfileInit render ~line 1198)

**Interfaces:**
- Consumes: `ProfileFormMode`, new `ProfileInit` props (Task 6); `selectProfile` (App.tsx:451).
- Produces: Toolbar props `onCreateProfile: () => void` (unchanged name, now bootstrap-only), `onAddProfile: () => void`, `onRetargetProfile: () => void`.

- [ ] **Step 1: Toolbar — two buttons beside the selector**

In `src/components/Toolbar.tsx`, add to `Props`:

```ts
  onAddProfile: () => void;
  onRetargetProfile: () => void;
```

Replace the profile group block (the `sketchDir && profiles.length === 0` ternary) with:

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
          <div className="toolbar-pair">
            <select
              className="select"
              value={props.profile ?? ""}
              onChange={(e) => props.onSelectProfile(e.target.value)}
              disabled={profiles.length === 0}
              title="Build profile (sketch.yaml)"
            >
              {profiles.length === 0 && (
                <option value="">no sketch.yaml profile</option>
              )}
              {profiles.map((p) => {
                const board = props.sketchYaml?.profiles?.[p]?.fqbn.split(":")[2];
                return (
                  <option key={p} value={p}>
                    {board ? `${p} — ${board}` : p}
                  </option>
                );
              })}
            </select>
            {props.sketchDir && (
              <>
                <button
                  className="btn icon"
                  onClick={props.onAddProfile}
                  title="Add a profile for another board (libraries copied)"
                  aria-label="Add profile"
                >
                  ＋
                </button>
                <button
                  className="btn icon"
                  onClick={props.onRetargetProfile}
                  disabled={profiles.length === 0 || !props.profile}
                  title="Change this profile's board"
                  aria-label="Change profile board"
                >
                  ✎
                </button>
              </>
            )}
          </div>
        )}
```

- [ ] **Step 2: App — mode state and wiring**

In `src/App.tsx`:

1. Import the type: extend the ProfileInit import to
   `import ProfileInit, { type ProfileFormMode } from "./components/ProfileInit";`
   (check the current import name/path at the top of App.tsx and keep its style).
2. Replace the `creatingProfile` boolean state (~line 110) with:

```ts
  /** When set, the one-row profile form shows under the toolbar. */
  const [profileForm, setProfileForm] = useState<ProfileFormMode | null>(null);
```

3. Update every `creatingProfile` / `setCreatingProfile` reference (grep for them; they appear in the state declaration, the three toolbar callbacks and the render):
   - `setCreatingProfile(false)` → `setProfileForm(null)`
   - `setCreatingProfile(true)` in `onCreateProfile` → `setProfileForm("bootstrap")`
4. Toolbar gets the two new props (mutual exclusivity mirrors `onCreateProfile`):

```tsx
        onAddProfile={() => {
          setProfileForm("add");
          setCreatingProject(false);
          setCloningProject(false);
        }}
        onRetargetProfile={() => {
          setProfileForm("retarget");
          setCreatingProject(false);
          setCloningProject(false);
        }}
```

5. Replace the ProfileInit render block:

```tsx
      {profileForm && sketchDir && (
        <ProfileInit
          mode={profileForm}
          sketchDir={sketchDir}
          detectedFqbn={detectedFqbn() ?? null}
          currentProfile={profile}
          currentFqbn={profile ? (sketchYaml?.profiles?.[profile]?.fqbn ?? null) : null}
          onDone={(yaml, prof) => {
            setSketchYaml(yaml);
            setProfileForm(null);
            // Select the touched profile; applies its pinned port if any.
            selectProfile(prof);
          }}
          onCancel={() => setProfileForm(null)}
          notify={notify}
        />
      )}
```

Note: `selectProfile` reads `sketchYaml` state for the pinned port; it was just replaced by `setSketchYaml(yaml)` in the same handler, and React state reads inside `selectProfile` still see the *old* yaml this tick. That is acceptable — the pinned port of the touched profile can only come from a pre-existing yaml (add/bootstrap create profiles without ports; retarget keeps the port that the old yaml already had).

- [ ] **Step 3: Type-check and full suites**

Run: `npx tsc --noEmit && npx vitest run && cargo check -p bancada`
Expected: all clean/green (429+ vitest).

- [ ] **Step 4: Commit (Tasks 6+7 together — the component rewrite and its only call site)**

```bash
git add src/components/ProfileInit.tsx src/components/Toolbar.tsx src/App.tsx
git commit -m "feat: add and retarget sketch.yaml profiles from the toolbar

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: End-to-end verification against real arduino-cli

**Files:**
- None created (scratch project only; use the session scratchpad dir)

**Interfaces:**
- Consumes: everything above, plus installed cores `arduino:avr` and `arduino:zephyr`.

- [ ] **Step 1: Simulate the add flow at the CLI level**

The commands are thin over core + arduino-cli, so verify the *product*: build a scratch sketch, drive core functions through a scratch cargo test or replicate the yaml by hand, then confirm arduino-cli accepts it. Concretely:

```bash
S=$(mktemp -d --tmpdir="${TMPDIR:-/tmp}" profswitch.XXXX)/Probe
mkdir -p "$S"
printf 'void setup() {}\nvoid loop() {}\n' > "$S/Probe.ino"
cat > "$S/sketch.yaml" <<'EOF'
profiles:
  uno:
    fqbn: arduino:avr:uno
    platforms:
      - platform: arduino:avr (1.8.8)
    libraries:
      - ArduinoJson (7.4.2)
  nano:
    fqbn: arduino:avr:nano
    platforms:
      - platform: arduino:avr (1.8.8)
    libraries:
      - ArduinoJson (7.4.2)
default_profile: uno
EOF
arduino-cli compile --profile nano "$S"
```

Expected: compiles (this is the exact yaml shape `add_profile` + the command layer emit for an add-with-copied-libs; adjust the pinned versions to what `arduino-cli core list` reports installed).

- [ ] **Step 2: Simulate retarget to UNO Q (the RouterBridge case)**

```bash
cat > "$S/sketch.yaml" <<'EOF'
profiles:
  uno:
    fqbn: arduino:zephyr:unoq
    platforms:
      - platform: arduino:zephyr (0.90.0)
default_profile: uno
EOF
arduino-cli profile lib add Arduino_RouterBridge -m uno --sketch-path "$S"
arduino-cli compile --profile uno "$S"
```

Expected: compiles — proving the retarget output plus the `pin_required_libs` step yields a building UNO Q profile.

- [ ] **Step 3: Full suites one last time**

Run: `cargo test -p bancada-core --lib && cargo check -p bancada && npx tsc --noEmit && npx vitest run`
Expected: all green.

- [ ] **Step 4: Report for manual bench check**

No commit. Tell Marco the feature is ready to try in the app: open a project with a `uno` profile, ＋ → add `nano` (libraries should appear copied in sketch.yaml), ✎ → retarget to the UNO Q and confirm RouterBridge lands in the profile.
