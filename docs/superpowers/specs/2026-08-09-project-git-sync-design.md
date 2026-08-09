# Project git support — checkpoint & sync — design

**Date:** 2026-08-09
**Request:** "Add git support for projects, sync to a repo, etc"

## Scope (user-decided)

Checkpoint & Sync, nothing more: a toolbar status pill, a Commit action
(always `add -A` scoped to the sketch, one-line editable message with a
generated suggestion), and a Sync action (`fetch → rebase → push`). No
branches, no history browsing, no diff viewer. Remotes are created with
`gh repo create --private` (paste-a-URL fallback for gh-less hosts). Git is
driven as an external engine, like `arduino-cli`, `esptool` and `claude` —
no libgit2.

Explicitly out of scope: auto-commit on flash, branch operations, conflict
resolution UI, history/log UI, multi-remote.

## UI & flows

One **git pill** in the toolbar, rendered only while a sketch is open — no
placeholder otherwise. It shows exactly one state:

| Pill | Meaning |
|---|---|
| `✓ clean` | repo at sketch root, nothing to commit |
| `N changed` | repo at sketch root, N dirty/untracked paths |
| `↑A ↓B` suffix | commits ahead/behind upstream (shown when nonzero) |
| `no git` | no repository in the sketch dir or any ancestor |
| `tracked by <parent>` | sketch is inside a bigger repo (e.g. `~/Projects` is a checkout) |

Clicking the pill opens a small popover (the listbox-overlay pattern):

- **Commit** — one-line input prefilled with a generated summary, e.g.
  `checkpoint: teste-uno-veia.ino, sketch.yaml (+2)`. Enter commits.
- **Sync** — enabled when a remote exists and the tree is clean;
  runs fetch → rebase → push with output streamed to the Build console.
- **No remote yet** — popover swaps to setup: "Create private GitHub repo"
  (name prefilled from the folder) plus a URL input as fallback.
- **`no git`** — popover offers "Initialize repository": `git init` +
  credential `.gitignore` + initial commit.
- **Nested repo** — Commit works (subtree-scoped); **Sync is disabled**
  with an explanation: pushing would publish the whole parent repo, which
  is more than this button promises.

Status refreshes on project open, after saves, after agent side-effects,
and after each git action. Event-driven; no polling.

## Core module: `core/src/git.rs`

Extends the module the `fix-git-detection` branch introduces (git runner,
`is_under_git`, `repo_root`, `init_repo`) — the `ghlib.rs` pattern: pure,
unit-tested decision logic; thin runners that shell out to `git` (resolved
from PATH per call, like every other engine; `gh` likewise, its absence
merely hiding the create-repo button).

One read call powers the pill: `git status --porcelain=v2 --branch -- .`
(branch, upstream, ahead/behind and the change list in a single
invocation), parsed into:

```rust
enum RepoState {
    NoGit,
    Root   { branch: String, dirty: Vec<ChangedPath>,
             remote: Option<String>, ahead: u32, behind: u32 },
    Nested { root: PathBuf, dirty: Vec<ChangedPath> },
}
```

Root-vs-nested is decided by `git rev-parse --show-toplevel` ancestry —
the same semantics the unmerged `fix-git-detection` branch (`12e38cc`)
introduced; merging that branch is a prerequisite of this work, and this
feature retires the bare `.git`-dir check (`sketch_has_git`) it fixed.

Operations (each a small function, args built by a pure tested builder):

- `init_repo` — `git init`, write/merge the credential `.gitignore`
  (list extracted to a shared constant that `clone.rs` also uses),
  initial commit.
- `suggested_message` — from the dirty list; truncates to
  `checkpoint: a, b (+N)`.
- `commit` — `git add -A -- .` then `git commit -m <msg>`.
- `create_remote` — `gh repo create <name> --private --source . --push`.
- `set_remote` — `git remote add origin <url>` + `git push -u origin HEAD`.
- `sync` — `git fetch origin` → `git rebase origin/<branch>` →
  `git push` (`-u origin HEAD` when upstream is missing). **Refuses a
  dirty tree** with "commit first" — no autostash; in a checkpoint model
  that is one honest click, and it keeps rebase away from uncommitted work.

## Tauri commands & data flow

Six thin commands in the `spawn_blocking` + `err_str` shape of
`upload_sketch`: `git_state`, `git_commit`, `git_init`,
`git_create_remote`, `git_set_remote`, `git_sync`. Sync (and remote
creation) stream through the existing `build://line` event into the Build
console. `api.ts` mirrors them; `App.tsx` holds one `gitState` value with
the refresh triggers above. The Assistant panel's "under git" warning
switches from `sketch_has_git` to `git_state`.

## Error handling

Rule: **never leave the repo mid-operation.**

- **Rebase conflict** — automatically `git rebase --abort`, then report
  "local and remote diverged with conflicting changes — resolve in a
  terminal, then Sync again." Local commits intact, tree clean. A
  half-finished rebase in a GUI with no conflict UI is the one trap this
  feature must never set.
- **Tracked secrets** — the status parser flags credential-named files
  (`secrets.h`, `arduino_secrets.h`, `.env`, …) that are already tracked —
  `.gitignore` cannot protect those — and the popover shows a warning
  before Commit and before remote creation. Pure string logic.
- **gh absent / unauthenticated / name taken** — stderr surfaced verbatim
  plus the obvious hint (`gh auth login`); `--push` being the final step
  means nothing partial happens.
- **Quiet cases** — "nothing to commit" is info, not error; missing
  upstream handled by `push -u`; detached HEAD shows in the pill and
  disables Sync; a dead network fails the push loudly and changes nothing
  local; `git` missing from PATH surfaces as the existing ToolMissing
  error shape.

## Testing

1. **core unit** — porcelain v2 parsing (renames, untracked, ahead/behind,
   detached HEAD), arg builders, suggested-message truncation,
   tracked-secrets detection. No git binary required.
2. **core integration** (`core/tests/`) — real `git` in a tempdir:
   init → commit → state → diverge → sync-refusal round-trips, using the
   existing skip-when-tool-missing convention.
3. **vitest** — pill-state mapping and popover mode selection as pure
   helpers in `src/gitStatus.ts`, tested like `ports.ts`.

Manual smoke matrix: fresh project (init path), `teste-uno-veia`
(existing repo, no remote → gh create → sync), a nested sketch under the
`~/Projects` checkout (commit allowed, sync disabled), and a two-clone
divergence exercising rebase and the conflict-abort path.
