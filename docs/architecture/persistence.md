# Persistence

Every file Bancada writes, where it lives, and the rules that govern it.

There is **no database**. Everything is JSON, NDJSON or YAML, chosen so a user
can read and repair it with a text editor — a bench tool should not hide its
state in a binary blob.

Three locations, with different lifetimes and different trust:

| Location | Lifetime | Contents |
|---|---|---|
| App config dir | forever | app state that is not part of any project |
| Inside the sketch | with the project | anything that belongs in version control |
| Temp dir | one session | the agent's 0600 credential files, staged firmware |

---

## 1. App config directory

Resolved via `AppHandle::path().app_config_dir()` — on Linux
`~/.config/dev.magj.bancada/`. `src-tauri` resolves it and passes it in; `core`
never guesses a path.

```
~/.config/dev.magj.bancada/
├── settings.json
├── usage.json
├── fleet.json
├── mqtt.json
└── chats/
    └── <sketch_key>/
        ├── 2026-08-11T09-04-20.ndjson
        └── …
```

### `settings.json`

`core::settings::AppSettings` — last sketch and open file, last project parent,
and the recent-projects list capped at `MAX_RECENT = 10`.

All paths here are absolute, so a project rename invalidates them.
`replace_recent(old, new)` swaps the entry **in place**, keeping its position:
a rename is not a visit, and should not reorder the list.

### `chats/<sketch_key>/*.ndjson`

Assistant transcripts, **one file per chat**.

`sketch_key` is `fnv1a-64` over the *full* path, rendered as 16 hex digits, then
`-`, then a sanitised basename — e.g. `a3f1…9c2e-my-sketch`. The hash is what
keeps two projects with the same basename in different directories apart; the
basename is there so a human browsing `chats/` can tell which is which.

**The format is an operation log, not a message schema.** Each line records one
mutating call on the frontend's `AgentStore` — `meta`, `sessionStarted`,
`userSent`, `push`, `closed`. Replaying the lines through a fresh store
reproduces the live rendering *exactly*, which is why there is no second schema
to keep in sync and why `UsageDashboard` can replay a saved chat inline.

Rules:

- Empty sessions never write a file.
- Corrupt lines are skipped silently on replay — one bad line must not lose a
  whole conversation.
- **Pruned to 50 chats per project**, on new-chat creation.
- Filenames arriving from the webview are validated as plain `*.ndjson`
  basenames (`valid_chat_file`) before any file operation.

**Renaming a project changes both halves of the key** — the hash, because the
path changed, and the basename. `chatlog::rename_key(chats_root, old, new)`
moves the directory so the transcripts stay reachable; without it they would
remain on disk and be unreachable from the UI, which reads as data loss.
It refuses an occupied destination rather than merging.

The `meta` line inside each transcript keeps the **old** `sketchDir` after a
rename, deliberately: it records where that conversation actually happened.
See the caveat under `usage.json`.

### `usage.json`

Cumulative per-project cost, tokens, turns and session counts. Versioned
(`USAGE_VERSION = 1`), keyed by the same `sketch_key`.

It is **separate from the chat logs on purpose: it must survive pruning.**
Deleting old chats must not make spend appear to drop.

Two ordering rules protect the totals from double-counting:

- The store is **seeded once** by `usage::backfill(chats_root)` if the file does
  not exist, and saved immediately — so backfill can never run twice.
- `chat_append` loads the store **before** appending the line, for the same
  reason.

`rename_project` calls `UsageStore::rename_project_key`, which moves the entry
and repoints its `sketch_dir`, merging rather than clobbering if the target
somehow exists. Without it a renamed project shows up twice in the dashboard —
its whole history under the old name, and $0 under the new one.

**A row's identity is its key, never its path.** `ProjectUsage` carries the
`key` it is stored under — filled by `overview()`, never persisted, because it
would duplicate the map key and could drift from it.

This matters because `sketch_dir` cannot do that job. `usage::backfill`
recovers it from the transcripts' `meta` lines, which record where a
conversation *happened*, not where the project is now. Delete `usage.json`
after a rename and backfill re-seeds the pre-rename path — so hashing it yields
a key nothing is stored under. That is why the dashboard's two commands
(`chat_list_usage`, `chat_load_by_key`) take a key, while the live chat
commands keep taking the open sketch's path, which is current by definition.

A stale display *name* after such a backfill is left alone, and is arguably
right: it names where those conversations actually happened.

### `fleet.json`

The board registry. Versioned (`FLEET_VERSION = 1`). Each entry is a board
identified by MAC address (preferred) or USB descriptors, with a nickname, a
last-seen timestamp, an FQBN and a chip type.

Writes are **skipped unless a board is new or `last_seen` is stale** beyond a
resolution threshold — otherwise a 2 s hotplug poll would rewrite the file
forever.

`last_flash` (a `FlashRecord`: project dir, tag, branch, commit sha, when) is
the exception to that rule and must stay one: it is written **only on the
flash path**, once per upload, never from `sight`/`sight_all`, and it must
never enter the staleness test — that is what keeps the poll from churning.
Only the last flash is kept; the repository's `flash/*` tags are the history,
and they survive this file being deleted.

Two traps it carries:

- It is `#[serde(default)]`, like every field past the id. A required field
  would make every existing `fleet.json` fail to parse, and `load` **refuses a
  corrupt file rather than emptying it** — the panel would simply error.
- `merge_identified` has two arms, and both name it explicitly. When a board
  known only by a vendor serial gains a MAC through esptool Identify its
  record is *migrated*, and anything not named in that merge is silently
  dropped.

A project rename repoints it (`Fleet::repoint_project`), for the same reason
the chat and usage keys move: otherwise every board points at a path that is
no longer there.

### `mqtt.json`

Saved broker configurations. Passwords in URLs are redacted for display by
`core::mqtt::redact_password`, which has a TypeScript twin in
`src/obs/redact.ts`.

---

## 2. Inside the sketch directory

These belong to the project and are meant to be committed.

| File | Owner | Notes |
|---|---|---|
| `sketch.yaml` | **arduino-cli** | Profiles, platform and library pins. Bancada edits it but does not own the schema — this is what keeps a project buildable without Bancada. |
| `bancada.yaml` | **Bancada** | The git-hosted library manifest: `{alias, ref, commit, vendor}` per entry. The only schema Bancada owns, and it is additive. |
| `.bancada/libs/<Name>/` | Bancada | Vendored library bytes, **auto-added to `.gitignore`** — they are re-fetchable from the manifest at their pinned commit. |
| `.gitignore` | shared | Merged against `GITIGNORE_REQUIRED` rather than overwritten, so user entries survive. |

Two details that matter:

**The commit pin is verified, not trusted.** `gh_restore` re-fetches each entry
at its recorded commit; if the tag has moved, that is a **refusal**, not a
silent rebuild.

**Duplicate writes `.gitignore` into the staging directory** — before the atomic
rename — so credential ignore rules exist before any possible commit. A copy
that failed halfway must never leave a directory that would commit secrets.

---

## 3. Temp directory

### The agent's two credential files

```
$TMPDIR/bancada-agent-mcp-<nonce>.json       --mcp-config: loopback URL + bearer
$TMPDIR/bancada-agent-settings-<nonce>.json  --settings:  deny rules + PreToolUse hook
```

Both are created **0600 with `create_new` (`O_EXCL`)** on Unix, and both are
removed on every normal exit path.

Three deliberate choices here, each worth understanding before changing
anything:

- **A file, not argv.** Argv is world-readable through `/proc/<pid>/cmdline`, so
  a bearer token on the command line would be readable by any local process.
- **The filename carries a fresh nonce, not the token.** Temp-directory listings
  are typically world-readable.
- **The settings file lives outside the project tree** — deliberately, so the
  thing it constrains cannot edit it. It is also covered by the deny rules.

See [agent-safety](agent-safety.md).

### Staged firmware

`scope_install_firmware` materialises `bancada_scope.ino` and its `sketch.yaml`
from `include_str!` into `$TMPDIR/bancada_scope_fw/bancada_scope/`, unless the
caller supplies a destination. The firmware source lives in
`firmware/bancada_scope/` and is compiled into the binary, so a user never has
to find it.

---

## 4. Browser-local state

Window furniture goes to `localStorage`, not to `settings.json` — it is
per-machine, not per-project:

```
bancada.bottomHeight
bancada.sidebarWidth
bancada.sidebarCollapsed
```

---

## 5. Rules for new persistent state

1. **`core` never resolves a path or reads a clock.** Both are injected by
   `src-tauri`. This is what makes `settings`, `fleet`, `chatlog` and `usage`
   deterministic under test.
2. **Version anything with a schema** — a `version` field from day one, as
   `usage.json` and `fleet.json` have.
3. **A corrupt record is refused, never silently emptied.** Returning an empty
   default on a parse error looks like data loss to the user and hides the bug.
   The chat log's line-level skip is the one deliberate exception, and it is
   scoped to a single line.
4. **Avoid write churn.** Anything on a poll loop must check whether the write
   is needed at all, as `fleet_sync` does.

---

## See also

- [backend-modules](backend-modules.md) — the modules that own these formats
- [runtime-model](runtime-model.md) — shutdown, which removes the temp files
- [agent-safety](agent-safety.md) — why the agent's temp files are 0600
