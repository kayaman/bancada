# Project usage totals in the History view

**Date:** 2026-08-05
**Status:** Approved design

## Problem

Per-turn and per-session usage are visible, but nothing answers "what
has the Assistant cost this project in total?" The data already exists:
every saved chat records its `result` events verbatim.

## Semantics (verified)

Each stream-json `result` event's `total_cost_usd`, `usage`, `num_turns`
and `duration_ms` are **per query call**, not cumulative per session —
the Agent SDK cost-tracking docs explicitly instruct accumulating
totals yourself. Summing every result event across files is therefore
the correct aggregation, and the existing Σ session chip is correct.

## Design

### Backend

`core/src/chatlog.rs` gains one pure function:

    pub struct ProjectTotals {
        pub cost_usd: f64,
        pub input_tokens: u64,
        pub output_tokens: u64,
        pub chats: usize,
        pub turns: u64,
        pub last_chat: Option<String>,   // newest chat's file stem
    }
    pub fn project_totals(chats_root: &Path, key: &str) -> ProjectTotals

One `BufReader` pass per chat file, summing from lines whose op is
`push` with `ev.type == "result"`: `ev.usage.input_tokens`,
`ev.usage.output_tokens`, `ev.total_cost_usd`, `ev.num_turns`.
Malformed lines are skipped. Full-file reads are acceptable: this runs
when History opens, never on a poll. A thin `chat_totals(sketch_dir)`
Tauri command wraps it.

### Frontend

`openHistory` fetches totals alongside the chat list. A summary card
above the rows shows: sketch name, chat count, Σ cost, tokens in/out
(via `formatTokens`), total turns, and the last chat's date. It clears
with the rest of the history state; deleting a chat refreshes list and
totals together.

### Out of scope

Per-day breakdowns, cross-project roll-ups, live updating while a
session streams (reopen History to refresh).

## Testing

Rust: totals summed across multiple files and multiple results per
file; malformed lines skipped; empty dir → zeros; `last_chat` is the
newest stem. Panel wiring verified visually per repo convention.
