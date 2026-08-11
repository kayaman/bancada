# Usage dashboard: persistent per-project token/cost totals

**Date:** 2026-08-09
**Status:** Approved (design review with Marco, 2026-08-09)

## Problem

Bancada already shows token/cost usage in two places: the live session Σ chip
(`src/agent/usage.ts`) and the per-project totals card in the assistant
History view (`chatlog::project_totals`). Both are scoped to the currently
open project, and both are computed by scanning chat NDJSON files — which are
pruned at 50 per project and user-deletable, so historical usage silently
shrinks.

Wanted: token usage (in/out) and cost **per project across all projects**,
**persisted durably** (surviving prune and chat deletion), on a **dashboard
screen** with **links to individual sessions**.

The cross-project roll-up was explicitly out of scope in
`2026-08-05-project-usage-totals-design.md`; this spec brings it in scope
without disturbing that design's on-demand philosophy for what remains
scan-based.

## Decisions (made with Marco)

1. **Persistence:** cumulative `usage.json` store, not scan-on-demand —
   totals must survive chat pruning/deletion.
2. **Entry point:** toolbar button → full-area screen in the editor area
   (New Project / Clone Project pattern), because the dashboard is app-level
   while the bottom panel is project-scoped.
3. **Session links:** inline read-only replay inside the dashboard (existing
   `ReplayView`), regardless of which project is open. Pruned sessions keep
   their usage in the totals but have no link.

## Architecture

### 1. Store — new `core/src/usage.rs`, persisted at `<app_config_dir>/usage.json`

```rust
pub const USAGE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct ProjectUsage {
    #[serde(default)] pub sketch_dir: String, // original path: display + re-hash for chat commands
    #[serde(default)] pub cost_usd: f64,
    #[serde(default)] pub input_tokens: u64,
    #[serde(default)] pub output_tokens: u64,
    #[serde(default)] pub turns: u64,
    #[serde(default)] pub sessions: u64,      // chat files ever created
    #[serde(default)] pub last_chat: Option<String>, // newest contributing chat's file stem
}

#[derive(Serialize, Deserialize)]
pub struct UsageStore {
    #[serde(default = "usage_version")] pub version: u32,
    #[serde(default)] pub projects: BTreeMap<String, ProjectUsage>, // key = chatlog::sketch_key
}
```

Conventions copied from `fleet.rs`, deliberately:

- **Corruption policy:** missing file = empty store; corrupt file = **error**
  (`Error::Json`). This file is the user's accumulated record — silently
  defaulting would destroy it on the next save. (Unlike `settings::load`,
  which swallows corruption to keep startup unblockable.)
- **Atomic save:** `usage.json.tmp` + rename, `to_string_pretty`.
- **Forward compat:** additive fields ride `#[serde(default)]`; `version`
  bumps only for breaking shape changes.
- **Path-agnostic and clock-free:** core takes `&Path`; no clock is needed at
  all (recency comes from chat file stems, which are timestamps).

Pure API:

```rust
impl UsageStore {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn save(&self, path: &Path) -> Result<()>;
    /// Parse one chat op line; accumulate iff op=="push" && ev.type=="result".
    /// Returns true when the store changed (caller saves only then).
    pub fn record_line(&mut self, key: &str, sketch_dir: &str, file: &str, line: &str) -> bool;
    /// A chat file was newly created: bump `sessions`. Returns true (changed).
    pub fn note_new_chat(&mut self, key: &str, sketch_dir: &str) -> bool;
}
/// Seed a store from existing chat logs (each dir's `meta` op supplies
/// sketchDir). Used once, when usage.json does not exist yet.
pub fn backfill(chats_root: &Path) -> UsageStore;
```

Field extraction in `record_line` matches `project_totals` exactly:
`ev.total_cost_usd`, `ev.num_turns`, `ev.usage.input_tokens`,
`ev.usage.output_tokens`; malformed lines are ignored (return false).

### 2. Write path — hook in the existing `chat_append` command

`chat_append` (sync, main-thread — read-modify-write already serialized) gains
a bookkeeping step after the append:

1. Lazy backfill: if `usage.json` does not exist, build it with
   `usage::backfill(chats_root)` first. Never re-run once the file exists —
   no double counting.
2. If the append created a new chat file (`chat_append`'s existing `created`
   flag): `note_new_chat`.
3. `record_line` with the appended line.
4. Save only if either returned true.

**Bookkeeping never fails the user action** (`note_board_fqbn` precedent): any
usage-store error is swallowed (logged to stderr); the chat append still
succeeds. Result events arrive once per turn, so there is no write-churn
concern and no `LAST_SEEN_RESOLUTION`-style throttle is needed.

### 3. Read path — two commands

- `usage_overview() -> Vec<ProjectUsageRow>` — the store flattened
  (`key` + `ProjectUsage` fields), sorted by `cost_usd` descending in core.
  Runs the same lazy backfill check first, so the dashboard is populated on
  first open even before any new chat activity. Load errors (corrupt file)
  surface to the frontend as strings, per the universal `Result<T, String>`
  convention.
- `chat_list_usage(sketch_dir) -> Vec<SessionEntry>` — new core fn
  `chatlog::list_chats_with_usage(root, key)`: one streaming pass per chat
  file merging today's title extraction (`list_chats`) with per-file result
  summation (`project_totals`), returning
  `{ file, title, cost_usd, input_tokens, output_tokens, turns }`,
  newest first (filename order, as today). Serialize-only struct.

Session replay is unchanged plumbing: `chat_load(sketch_dir, file)` →
`replayChat(lines)` → `ReplayView`. The stored `sketch_dir` re-hashes to the
same `sketch_key` server-side, so links work regardless of the currently open
project. `ReplayView` is exported from `AgentPanel.tsx` (it stays there; only
visibility changes).

### 4. UI — `src/components/UsageDashboard.tsx`

- **Entry:** `📊 Usage` toolbar button (`Toolbar.tsx` gains `onUsage`);
  `showingUsage` boolean joins the `.editor-area` ternary in `App.tsx` with
  the same hand-rolled mutual exclusion as `creatingProject` /
  `cloningProject` / `profileForm`.
- **Layout:** `.panel-tabs`-style header with grand totals (summed client-side
  from the rows: Σ cost, Σ in/out tokens, project count) plus ⟳ refresh and ✕
  close; scrollable list of project rows styled after
  `.fleet-card`/`.agent-proj-card`, each showing project basename (full path
  dimmed), cost (`$x.toFixed(4)`, existing convention), tokens in/out
  (`formatTokens`), turns, sessions, last activity (parsed from the
  `last_chat` stem).
- **Expand a project** → fetch `chat_list_usage` for it; session rows show
  title, timestamp, in/out tokens, cost. Clicking a session loads the inline
  read-only replay (with a back affordance to the list). If the store's
  `sessions` exceeds the files on disk, the list ends with one line —
  "N older sessions pruned" — honest, no placeholder rows.
- **Freshness:** fetch on open + manual ⟳. No live updates while a session
  streams (consistent with the totals spec).
- **Styling:** new `/* ---------- usage dashboard ---------- */` section in
  `styles.css`, existing variables and control primitives only.

### 5. Data flow summary

```
claude CLI result event
  → frontend chatRecorder (push op, verbatim)      [existing]
  → chat_append: append NDJSON                     [existing]
      + usage.json accumulate (lazy backfill)      [new]
dashboard open
  → usage_overview → project rows                  [new]
  → expand: chat_list_usage → session rows         [new]
  → click: chat_load → replayChat → ReplayView     [existing]
```

## Error handling

| Failure | Behavior |
|---|---|
| `usage.json` corrupt | `usage_overview` errors (shown in dashboard); `chat_append` bookkeeping swallows it and appends anyway |
| Chat file pruned/deleted | Totals unaffected (that's the point); session row absent; "N older sessions pruned" hint |
| Malformed op line | `record_line` returns false; ignored, matching `project_totals` tolerance |
| Backfill encounters unreadable files | Skipped silently, like `project_totals` |

## Testing

- **`core/src/usage.rs`** (tempdir suite, `settings.rs`/`fleet.rs` precedent):
  roundtrip; missing → default; corrupt → error; `record_line` accumulates a
  result line and ignores user/assistant/meta lines and garbage; repeated
  results accumulate; `note_new_chat` bumps sessions; save-only-when-changed
  (record_line returning false on a non-result line); backfill over a fixture
  `chats/` tree (reusing `write_chat` + `result_line` helpers) matches
  `project_totals` per key and captures `sketch_dir` from meta ops.
- **`core/src/chatlog.rs`**: `list_chats_with_usage` sums per file, keeps
  title extraction, orders newest-first, tolerates malformed lines.
- **Tauri layer:** command wrappers stay thin (delegation only), consistent
  with existing chat commands; no new lib.rs test surface beyond registration.
- **Frontend:** api wrapper shape checks in `src/__tests__/api.test.ts`;
  grand-total math and last-chat-stem parsing live in a pure
  `src/usageDashboard.ts` module with unit tests (ordering is core's job);
  the component itself stays thin.

## Out of scope (deliberate)

- Per-model breakdown (the CLI's `modelUsage` block is preserved verbatim in
  the NDJSON if ever wanted).
- Per-day/time-series charts.
- Live updating while a session streams.
- Cost budgets or alerts.
- Retroactive capture of usage from chats already pruned before this ships —
  the backfill can only see files that still exist.
