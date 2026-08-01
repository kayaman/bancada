# Default Project Location Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** New Project defaults to `~/Projects` (else `$HOME`) instead of the sketchbook, and creates a missing parent directory instead of erroring.

**Architecture:** Pure decision in `bancada-core` (tempfile-tested), thin Tauri command resolving `$HOME` via the path plugin, one-line fallback swap in the dialog.

**Tech Stack:** Rust (bancada-core, Tauri 2), TypeScript/React, vitest.

## Global Constraints

- Remembered `last_new_project_parent` keeps precedence over the new default.
- `sketchbook_dir` command and `sketchbookDir()` wrapper stay; only NewProject.tsx stops using them.
- Commit trailer: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Core `default_project_parent`

**Files:**
- Modify: `core/src/project.rs` (function after `profile_name_for_fqbn`, tests in existing `mod tests`)

**Interfaces:**
- Produces: `pub fn default_project_parent(home: &Path) -> PathBuf`

- [ ] **Step 1: Write the failing tests** (in `core/src/project.rs` `mod tests`)

```rust
#[test]
fn default_parent_prefers_projects_dir() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("Projects")).unwrap();
    assert_eq!(
        default_project_parent(home.path()),
        home.path().join("Projects")
    );
}

#[test]
fn default_parent_falls_back_to_home() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(default_project_parent(home.path()), home.path());
}

#[test]
fn default_parent_ignores_a_projects_file() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("Projects"), "not a dir").unwrap();
    assert_eq!(default_project_parent(home.path()), home.path());
}
```

- [ ] **Step 2: Run to verify they fail** — `cargo test -p bancada-core default_parent` → FAIL (not found)
- [ ] **Step 3: Implement**

```rust
/// Where a new project goes by default: `~/Projects` when the user has
/// one, otherwise the home directory itself.
pub fn default_project_parent(home: &Path) -> PathBuf {
    let projects = home.join("Projects");
    if projects.is_dir() { projects } else { home.to_path_buf() }
}
```

(`use std::path::PathBuf;` joins the existing `Path` import.)

- [ ] **Step 4: Run to verify pass** — 3 PASS, whole core suite green
- [ ] **Step 5: Commit** — `feat: add default_project_parent to core`

### Task 2: Tauri command + parent creation

**Files:**
- Modify: `src-tauri/src/lib.rs` (command next to `sketchbook_dir` ~line 460; guard in `create_project` ~line 513; register in `generate_handler!` after `sketchbook_dir`)

**Interfaces:**
- Consumes: `bancada_core::project::default_project_parent`
- Produces: command `default_project_parent() -> String`; `create_project` now accepts a missing parent

- [ ] **Step 1: Add the command**

```rust
/// Default location for a new project: `~/Projects` when present, else home.
#[tauri::command]
fn default_project_parent(app: AppHandle) -> Result<String, String> {
    let home = app.path().home_dir().map_err(err_str)?;
    Ok(bancada_core::project::default_project_parent(&home)
        .to_string_lossy()
        .into_owned())
}
```

- [ ] **Step 2: Replace the parent guard in `create_project`**

```rust
let parent_path = Path::new(&parent);
if parent_path.exists() && !parent_path.is_dir() {
    return Err(format!("{parent} is not a directory"));
}
std::fs::create_dir_all(parent_path)
    .map_err(|e| format!("could not create {parent}: {e}"))?;
```

- [ ] **Step 3: Register** `default_project_parent,` after `sketchbook_dir,` in `generate_handler![]`
- [ ] **Step 4: Verify** — `cargo check` in src-tauri clean; core suite still green
- [ ] **Step 5: Commit** — `feat: default_project_parent command; create_project makes missing parents`

### Task 3: Frontend wiring

**Files:**
- Modify: `src/api.ts` (after `sketchbookDir`), `src/components/NewProject.tsx:41-52`

**Interfaces:**
- Consumes: command `default_project_parent`
- Produces: `export const defaultProjectParent = () => invoke<string>("default_project_parent");`

- [ ] **Step 1: Add the api.ts wrapper** (code above, with doc comment `/** ~/Projects when it exists, else the home directory. */`)
- [ ] **Step 2: Swap the fallback in NewProject.tsx** — import `defaultProjectParent` instead of `sketchbookDir`; in the mount effect replace `sketchbookDir().catch(() => "")` with `defaultProjectParent().catch(() => "")` and update the comment to say the fallback is `~/Projects` (or home).
- [ ] **Step 3: Verify** — `npx vitest run` all green; `npm run build` clean
- [ ] **Step 4: Commit** — `feat: default new projects to ~/Projects`
