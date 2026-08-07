# Clone a project (from local), rename, and modify

## Context

Bancada can create and open projects but not duplicate one. The user wants to
clone any local sketch folder under a new name and start editing it — the
"rename" is choosing the clone's name; "modify" is normal editing afterwards.
Decisions (AskUserQuestion): **clone only** (no rename-in-place), **any local
folder** as source (picker prefilled with the open sketch), **fresh git repo**
in the clone (exclude source `.git`, `git init`, write a `.gitignore`).

Chat history/usage is NOT copied — per-project scoping (chats keyed by fnv1a
of the full path) already gives the clone a fresh zero history by
construction. Design was adversarially verified by three parallel agents;
their corrections are folded in below.

## Backend

### `core/src/clone.rs` (new; all pure logic, TDD) + `pub mod clone;` in `core/src/lib.rs`

```rust
pub struct ClonedProject { pub dir: PathBuf, pub name: String, pub warnings: Vec<String> }
pub fn clone_project(src_dir: &Path, dest_parent: &Path, new_name: &str) -> Result<ClonedProject>
```

Flow (order matters):
1. **Validate**: `project::validate_project_name(new_name)` (project.rs:28);
   `src_dir` is a dir containing `<srcname>.ino` (the `main_ino` invariant,
   sketch.rs:92) else "not a sketch folder" error; `create_dir_all(dest_parent)`;
   containment via canonicalized paths — `dest != src` and dest_parent not
   inside src (canonicalize *after* create_dir_all so it exists; the
   longest-existing-prefix dance in agent.rs:366-390 is the precedent if
   needed); collision via `dest.symlink_metadata().is_ok()` (never `exists()`
   — dangling-symlink rationale at library.rs:297-300); case-clash via
   `library::case_insensitive_clash` (library.rs:390 — make `pub(crate)`;
   run the exact-existence check first, its message assumes case-only).
   Refuse when the source contains a top-level `<new_name>.ino` (the main-ino
   rename would clobber it — silent data loss otherwise).
2. **Staging**: `.bancada-clone-<new_name>` inside `dest_parent` (same
   filesystem → atomic rename; EXDEV is why staging never goes to /tmp).
   Reuse the `Staging(Option<PathBuf>)` Drop-guard + stale-staging reclaim
   verbatim from library.rs:270-320. Known caveat (record in doc comment):
   two concurrent clones to the same name sabotage each other's staging —
   pre-existing pattern behavior, acceptable.
3. **Git first, then files**: `git -C <staging> init` via `ghlib::git`
   (ghlib.rs:265-281 — make `pub(crate)`; `-C` form has precedent at
   ghlib.rs:350). `ToolMissing`/`ToolFailed` → warning ("git not found —
   clone created without a repository"), never an error — this also keeps
   core tests hermetic. Then write `.gitignore` (see merge below). Only
   then copy files — credentials can never exist in the repo before their
   ignore rule (the user's global rule, honored in ordering).
4. **Copy tree** recursively (plain `read_dir`, ~30 lines; sort entries by
   name so copy order and warning order are deterministic):
   - Exclusions, directory-names-only (a *file* named `build` is copied,
     matching sketch.rs:281 semantics): make `sketch::SKIP_DIRS`
     (sketch.rs:273) `pub(crate)` and use `SKIP_DIRS + [".bancada", ".claude"]`
     — one source of truth, no drift. `.bancada` is re-fetchable via the
     existing `gh_restore` (lib.rs:858); push a "run gh restore" warning if
     it was present.
   - Top-level `<srcname>.ino` is written as `<new_name>.ino`; every other
     file (including secondary `.ino`s) keeps its name and bytes.
   - Symlinks are **recreated** (`read_link` + `std::os::unix::fs::symlink`),
     not dereferenced — symlinked local libraries are routine
     (sketch.rs:310-311). Warn per link ("shared with the source project").
     Non-unix: skip + warn.
   - `fs::copy` preserves unix permission bits — sufficient.
5. **Retitle**: if `<new_name>.ino`'s first line starts with `// <srcname>`
   followed by a non-alphanumeric char or EOL (the blink template shape,
   project.rs:125), replace that one occurrence of the old name on line 1.
   Everything after line 1 byte-identical; no other file touched.
6. **`.gitignore` merge** (semantics of `ensure_gitignored`, ghlib.rs:399:
   trimmed-line comparison, once-only, trailing newline preserved): start
   from the copied source `.gitignore` if any, append missing required
   entries: `build/`, `.bancada/`, `.env`, `secrets.h`, `arduino_secrets.h`.
7. **sketch.yaml — byte-copy, targeted rewrite only** (serde round-trip is
   LOSSY: the model drops arduino-cli keys `default_fqbn`, `default_port`,
   `default_programmer`, per-profile `programmer`/`port_config`, destroys
   comments, reorders profiles — verified). Copy verbatim; use `load_yaml`
   read-only to find `LibraryDep::Local { dir }` entries that are absolute
   AND under `src_dir`; rewrite only those `- dir:` lines textually to the
   final dest path. Relative entries that escape the sketch (`../libs/X`)
   are left untouched but produce a warning when dest's parent differs from
   src's parent (they now dangle). Unparseable yaml → copy verbatim +
   warning. The **port pin is kept** (same bench, probably same port; a
   wrong pin fails loudly at flash time, a dropped pin regresses silently).
8. **Atomic finish**: single `fs::rename(staging, dest)`, `guard.disarm()`.

### `src-tauri/src/lib.rs` — command

`#[derive(serde::Serialize)] struct ClonedProject { dir: String, name: String, warnings: Vec<String> }`
— plain snake_case, NO rename attrs (matches `CreatedProject`, lib.rs:686).
`#[tauri::command] async fn clone_project(src_dir, dest_parent, new_name)` →
`spawn_blocking` → core, mirroring `create_project` (lib.rs:698). Register in
`invoke_handler` (~lib.rs:3037).

## Frontend

### `src/api.ts`
`ClonedProject` interface (snake_case fields) + `cloneProject(srcDir, destParent, newName)`
invoke wrapper next to `createProject` (~api.ts:407). Args go camelCase→snake
via Tauri's mapping (pinned by the test).

### `src/components/CloneProject.tsx` (new)
Separate dialog modeled on NewProject.tsx — reuse its `np-*`/`field`/`lib-dest`
classes exactly (zero new CSS; don't invent `cp-*`). Props
`{ sourceDir: string | null; onCreated; onCancel; notify }`. Fields:
- **Source**: text input + Choose… (`open({ directory: true, title: … })`,
  NewProject.tsx:68-72 pattern), prefilled from `sourceDir`.
- **New name**: prefilled `<srcBasename>-copy`, `autoFocus`, Enter submits
  (NewProject.tsx:150 parity).
- **Location**: `Promise.all([loadSettings().catch(…), defaultProjectParent().catch(() => "")])`
  then `last_new_project_parent || fallback` — copy the exact NewProject.tsx:44-53
  shape (`defaultProjectParent` is `api.ts:400`).
- `working` flag disables buttons during the clone (NewProject parity; no
  spinner — sketches are small).
- On success: fire-and-forget `saveSettings({ last_new_project_parent: parent })`
  (same key as NewProject — shared "where projects go" memory); notify,
  including `warnings.join("; ")` as a non-error note when non-empty;
  `onCreated(res.dir)`.

### `src/components/Toolbar.tsx`
`onCloneProject` prop; `⧉ Clone…` button in the project group after New
Project, `title="Copy a sketch into a new project with a fresh git repo"`.
Always enabled (source is pickable without an open sketch). No overflow-safety
claims — the toolbar has none today; the label stays short.

### `src/App.tsx`
- `cloningProject` state; Toolbar gets `onCloneProject`; render `CloneProject`
  in the editor-area ternary next to NewProject (App.tsx:1331-1340 pattern),
  `sourceDir={sketchDir}`, `onCreated: async (dir) => { setCloningProject(false); await loadSketch(dir); }`.
- **Dialog hygiene** (fixes an existing quirk that a third dialog would
  double): `loadSketch` clears `creatingProject` AND `cloningProject` (today
  it clears only `creatingProfile`, App.tsx:484 — opening a sketch under an
  open form leaves the form covering the editor). Opening any of the three
  forms closes the other two.
- `loadSketch` needs nothing else — it already handles agent teardown,
  git-warning, profile/port pick, opening `<name>.ino`; LibraryManager
  refetches off `sketchDir` on its own.

## Tests (TDD — write first; conventions: sentence-style snake_case names,
`tempfile::tempdir`, error substring asserts with `"{err}"`, warnings via
`any(contains)`, `#[cfg(unix)]` for symlinks, `#[ignore = "…"]` for
environment-dependent tests)

`core/src/clone.rs` `#[cfg(test)]`, with a `sample_sketch(name)` fixture:
1. Happy path: 3-level tree copied with **contents** asserted; `Src.ino` →
   `New.ino`; a dotfile (`.clang-format`) copied; no staging residue in
   dest_parent; returned struct pinned; **source tree byte-identical after**
   (ino first line, sketch.yaml, .gitignore).
2. Exclusions: `build/`, `.git/`, `.bancada/`, `.claude/`, `node_modules/`
   absent (top-level AND nested; nested case asserts siblings still arrive);
   a **file** named `build` IS copied; exclusion set asserted ⊇ `SKIP_DIRS`.
3. Retitle: `// Src — blink` → `// New — blink` with rest-of-file
   byte-identical; `// Srcocity` (not word-bounded) untouched; first line not
   a comment → file byte-identical; old name in body (MQTT topic string)
   untouched.
4. Gitignore: created-from-nothing content pinned exactly; source lacking
   trailing newline → exact-equality merge; variant spelling (`build`) not
   duplicated (`matches().count() == 1`); source `.gitignore` unchanged.
5. sketch.yaml: **unknown keys (`default_fqbn`, `programmer`) preserved**;
   port pin preserved; abs-under-src `dir:` rewritten to a path that exists
   in the clone; relative + external-absolute untouched;
   `../libs/X` with a different dest parent → warning; garbage yaml copied
   verbatim + warning.
6. Errors: bad name (delegation, one case); dest collision (concrete case:
   new_name == source name, same parent); case clash; missing src; src
   without `<src>.ino`; dest_parent inside src; source containing a
   top-level `<new_name>.ino`.
7. Atomicity: plant a regular file at the staging path (library.rs:744
   trick — deterministic, no chmod) → error, dest absent, planted file's
   content intact; stale staging dir does not leak into a later clone.
8. Symlinks (`#[cfg(unix)]`): recreated pointing at the same target (incl. a
   dangling one — no error); warning names the rel path; rest of copy
   completes. Clean clone (no symlinks/yaml issues) → `warnings.is_empty()`.
9. Git (`#[ignore = "runs real git"]`): clone has a **fresh** repo — `.git`
   exists, no commits (unborn HEAD), and no `.git` appeared in the source.
   The warning-only contract is carried by the doc comment (core tests stay
   hermetic by default; this is the first subprocess test, hence the gate).

Frontend: `src/__tests__/api.test.ts` — one `cloneProject` test mirroring
`createProject passes every field…` (:181) via the file-local `invokeMock`/
`called()` pattern: command name `clone_project` + snake_case arg mapping.
JSX (CloneProject/Toolbar/App wiring) untested by convention.

## Visibility changes (one-word diffs)
- `core/src/ghlib.rs` `fn git` → `pub(crate)`
- `core/src/library.rs` `fn case_insensitive_clash` → `pub(crate)`
- `core/src/sketch.rs` `SKIP_DIRS` → `pub(crate)`

## Files
Create: `core/src/clone.rs`, `src/components/CloneProject.tsx`.
Modify: `core/src/lib.rs`, `core/src/ghlib.rs`, `core/src/library.rs`,
`core/src/sketch.rs`, `src-tauri/src/lib.rs`, `src/api.ts`,
`src/__tests__/api.test.ts`, `src/components/Toolbar.tsx`, `src/App.tsx`.
No new dependencies.

## Verification
- `cargo test -p bancada-core` (incl. once with `-- --include-ignored` on
  this machine for the git test), full `cargo test`, `npx tsc --noEmit`,
  `npm test`.
- Bench: clone the open sketch → new project opens with `<new>.ino` in the
  editor, fresh empty Assistant (Σ absent, History empty — per-project
  scoping), git warning absent (repo inited), `git -C <clone> status` clean
  with untracked files, source untouched; clone a non-sketch folder → clear
  error; clone to an existing name → collision error.
- Docs: spec + plan copies under `docs/superpowers/{specs,plans}/2026-08-07-clone-project-*`,
  committed with the change.
