# Clone a project — design

**Date:** 2026-08-07
**Request:** "be able to clone a project (from local), rename and modify"

## Scope (user-decided)

Clone any local sketch folder under a new name and open it for editing — the
"rename" is choosing the clone's name, "modify" is normal editing. No
rename-in-place. Source comes from a folder picker prefilled with the open
sketch. The clone gets a **fresh git repo**: the source's `.git` is excluded,
`git init` runs, and a `.gitignore` is written (`build/`, `.bancada/`,
`.env`, `secrets.h`, `arduino_secrets.h`).

## Design

One pure core module, `core/src/clone.rs`, does everything test-first;
the Tauri command and dialog are thin.

- **Atomicity**: build in a `.bancada-clone-<name>` staging dir *inside* the
  destination parent (same filesystem → single atomic `fs::rename`), with
  the `Staging` Drop-guard pattern from `library::create_library`. Nothing
  half-copied ever lands.
- **Ordering**: `git init` in staging first, then the file copy, then the
  merged `.gitignore` — the repo has no commits until the user makes one,
  so ignore rules always precede any possible commit of credentials.
- **Copy semantics**: directories named in `sketch::SKIP_DIRS` plus
  `.bancada` (re-fetchable via `gh restore`) and `.claude` are skipped;
  files named like them are copied. Symlinks are *recreated*, not
  dereferenced (symlinked local libraries are routine), with a warning.
  The top-level `<src>.ino` becomes `<new>.ino` (Arduino's folder-name
  invariant); secondary `.ino`s keep their names. If the first line is the
  template's `// <src> — …` comment, the name on line 1 is retitled.
- **sketch.yaml is byte-copied, never round-tripped** — the serde model is
  lossy (arduino-cli keys like `default_fqbn`/`programmer` would be
  silently dropped, comments destroyed, profiles reordered). Only absolute
  local-library `dir:` entries pointing inside the source are rewritten,
  textually, to the clone. The port pin is kept (same bench, probably the
  same port; a wrong pin fails loudly, a dropped pin regresses silently).
- **Warnings, not failures**: missing git, skipped `.bancada`, recreated
  symlinks, unparseable yaml, and escaping relative library paths all
  surface as warnings on the result; the clone itself still succeeds.
- **Chat history/usage is not copied** — per-project scoping keys chats by
  the full path, so the clone starts at zero by construction.
- **Frontend**: `CloneProject.tsx` dialog (source/name/location) modeled on
  NewProject with its classes, a `⧉ Clone…` toolbar button in the project
  group, success path through the existing `loadSketch`. Opening any of the
  three editor-area forms closes the others, and `loadSketch` now clears
  all of them (fixes a pre-existing quirk where opening a sketch left a
  form covering the editor).

## Verification notes

Design claims were adversarially verified by three parallel agents before
implementation; the lossy-yaml finding, git-before-files ordering, symlink
recreation, and the `#[ignore = "runs real git"]` gating of the sole
subprocess test all came out of that pass.
