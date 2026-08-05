# Project Usage Totals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `docs/superpowers/specs/2026-08-05-project-usage-totals-design.md`.

**Architecture:** One pure Rust aggregation in `chatlog.rs` + one command + a History summary card. No store changes.

## Global Constraints

- `project_totals` never errors: unreadable files/lines are skipped.
- Summation is per result event (verified per-call semantics).

---

### Task 1: `project_totals` (Rust, TDD)

**Files:** Modify `core/src/chatlog.rs`.

- [ ] RED: tests — two files × two results each sum correctly (cost, in/out tokens, turns), `chats` counts files, `last_chat` is newest stem, malformed/irrelevant lines skipped, missing dir → all zeros/None.
- [ ] GREEN: `ProjectTotals` (Serialize + Debug/Clone/PartialEq) + `project_totals` using a per-file `BufReader` line pass.
- [ ] `cargo test -p bancada-core` fully green.

### Task 2: command + API + card

**Files:** `src-tauri/src/lib.rs`, `src/api.ts`, `src/components/AgentPanel.tsx`, `src/styles.css`.

- [ ] `chat_totals(sketch_dir) -> ProjectTotals` command (register in `generate_handler!`); `api.chatTotals` with a `ProjectTotals` TS interface (snake_case fields as serialized).
- [ ] AgentPanel: `histTotals` state; `openHistory` fetches list + totals; `deleteChat` refreshes both; summary card above `HistListView` (sketch basename from `sketchDir`, chats, Σ $cost, tokens via `formatTokens`, turns, `last_chat` with `T` → space); cleared wherever `histList` is cleared.
- [ ] CSS: `.agent-proj-card` (raised card, small grid/inline stats).
- [ ] `npm run build && npm test && cargo test -p bancada-core && cargo check -p bancada` green; visual check.
- [ ] Commit: `feat: project usage totals in the assistant history view`.
