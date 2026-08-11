# bancada 0.14.0

Projects get version control. A **git pill** joins the toolbar: one glance
tells you whether the open sketch is clean, dirty, ahead or behind — and one
click checkpoints it or syncs it with a remote. Bancada drives the same `git`
you use in a terminal (and `gh` for one-button GitHub repos); it reimplements
nothing.

## The git pill

- **One glanceable state** while a sketch is open: `✓ clean`, `3 changed`,
  `↑2 ↓1` against upstream, `detached`, `no git`, or `tracked by <parent>`
  when the sketch lives inside a bigger repository.
- **Commit** — a one-line input prefilled with a generated summary
  (`checkpoint: sketch.ino, sketch.yaml (+2)`); Enter stages everything in
  the sketch and commits. In a nested sketch the checkpoint stays scoped to
  the sketch's subtree — the parent repo's other changes are untouched.
- **Sync** — fetch → rebase → push, output streaming to the Build console.
  A dirty tree is refused ("commit first"), and a conflicted rebase is
  **aborted on the spot**: local commits intact, tree clean, a clear message
  to resolve in a terminal. The pill never leaves a repo mid-operation.
- **Initialize repository** for a project without one: `git init` plus a
  credential-covering `.gitignore` (`secrets.h`, `.env`, …) written *before*
  the baseline commit, so credentials can never be part of history.
- **Remote setup** — create a private GitHub repo with one button (`gh repo
  create --private --push`, name prefilled) or paste any git URL. A failed
  first push rolls the origin back rather than leaving it half-configured.
- **Tracked-secrets warning**: files `.gitignore` can no longer protect
  (already tracked) are flagged in the popover before you commit or publish.

## Under the hood

- New-project creation now puts standalone projects under git with a
  baseline commit (skipped when the parent is already a work tree), and the
  Assistant panel's "not under git" warning finally answers by **ancestry**,
  the way git itself does — a sketch inside `~/Projects` counts.
- `core::git` drives everything through the `git` binary with pure,
  unit-tested arg builders and porcelain-v2 parsing; sync scenarios are
  tested against real two-clone repositories, including the conflict-abort
  path. `gh` is optional — its absence just hides the create-repo button.
- Retired `sketch_has_git`; the pill's single `git_state` read powers both
  the toolbar and the Assistant warning.

## Notes

- Sync is deliberately disabled for sketches nested inside a bigger
  repository — pushing would publish the whole parent repo, which is more
  than the button promises. Commit still works there, subtree-scoped.
- `gh` joins the optional prerequisites in the README (`zypper install gh`,
  then `gh auth login`).
