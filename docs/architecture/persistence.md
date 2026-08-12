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
- **Pruned to 50 chats per sketch**, on new-chat creation.
- Filenames arriving from the webview are validated as plain `*.ndjson`
  basenames (`valid_chat_file`) before any file operation.

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

### `fleet.json`

The board registry. Versioned (`FLEET_VERSION = 1`). Each entry is a board
identified by MAC address (preferred) or USB descriptors, with a nickname, a
last-seen timestamp, an FQBN and a chip type.

Writes are **skipped unless a board is new or `last_seen` is stale** beyond a
resolution threshold — otherwise a 2 s hotplug poll would rewrite the file
forever.

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

**Clone writes `.gitignore` into the staging directory** — before the atomic
rename — so credential ignore rules exist before any possible commit. A clone
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
