# Assistant Chat History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `docs/superpowers/specs/2026-08-05-assistant-chat-history-design.md`: persist every Assistant chat as a store-op NDJSON per sketch, browsable/replayable read-only from the panel.

**Architecture:** Pure Rust module (`core/src/chatlog.rs`) + thin Tauri commands; pure TS recorder/replayer (`src/agent/chatLog.ts`); App/Panel wiring. Tasks 1 and 2 are independent and can run in parallel; Task 3 wires them together.

**Tech Stack:** Rust (std only), React/TS, vitest, cargo test.

## Global Constraints

- TDD: failing test first for every pure function (cargo test / vitest).
- `record`/append is fire-and-forget in the frontend: failures must never surface into the chat flow.
- `file` parameters must be validated basenames (`*.ndjson`, no `/`, no `\`, no `..`) — these commands take paths from the webview.
- No clock reads inside `core/src/chatlog.rs` or `chatLog.ts` logic — timestamps/filenames are parameters.

---

### Task 1: `core/src/chatlog.rs` (Rust, TDD) — parallelizable

**Files:** Create `core/src/chatlog.rs`; register `pub mod chatlog;` in `core/src/lib.rs` (alphabetical position, after `cli`, before `esptool` is wrong — after `boards`/`cli`? insert as `pub mod chatlog;` between `cli` and `esptool`).

**Interfaces (produces — exact signatures Task 3 relies on):**
```rust
pub struct ChatEntry { pub file: String, pub title: String }
pub fn sketch_key(sketch_dir: &str) -> String            // "<fnv1a-64-hex>-<sanitized-basename>"
pub fn valid_chat_file(file: &str) -> bool               // basename.ndjson only
pub fn append_line(chats_root: &Path, key: &str, file: &str, line: &str) -> crate::Result<bool>
    // creates parent dirs; appends line + '\n'; returns true when the file was newly created
pub fn list_chats(chats_root: &Path, key: &str) -> Vec<ChatEntry>   // newest first by name (desc)
pub fn load_chat(chats_root: &Path, key: &str, file: &str) -> crate::Result<Vec<String>>
pub fn delete_chat(chats_root: &Path, key: &str, file: &str) -> crate::Result<()>
pub fn prune(chats_root: &Path, key: &str, keep: usize)  // best-effort, silent
```
Title derivation: first line parsing as JSON with `"op":"userSent"` → its `text`, chars truncated to 80; else the file stem. Use `serde_json::Value` (already a core dep). fnv1a-64 implemented inline (no new deps). Errors use the existing `crate::Error`/`Result` from `core/src/lib.rs`.

- [ ] Write `#[cfg(test)] mod tests` first (tempdir via `std::env::temp_dir()` + unique subdir, or the pattern `core/src/settings.rs` tests use — read them): append creates dirs + returns created flag; append validates file name (reject `../x.ndjson`, `a/b.ndjson`, `x.txt`); list newest-first + title from first userSent line + corrupt file skipped (title falls back, no panic); load returns lines; delete removes one file; prune keeps newest N; sketch_key stable + sanitizes basename (spaces → `-`).
- [ ] Run: `cargo test -p bancada-core chatlog` — expect FAIL (todo!()).
- [ ] Implement; run full `cargo test -p bancada-core` — all green.
- [ ] Do NOT commit (Task 3 commits the feature whole).

### Task 2: `src/agent/chatLog.ts` (TS, TDD) — parallelizable

**Files:** Create `src/agent/chatLog.ts`, `src/agent/__tests__/chatLog.test.ts`.

**Interfaces (produces):**
```ts
export type ChatOp =
  | { op: "meta"; sketchDir: string; startedAt: string }
  | { op: "sessionStarted"; pid: number }
  | { op: "userSent"; text: string }
  | { op: "push"; ev: unknown }
  | { op: "closed"; reason: string; pid?: number };
export function chatFileName(now: Date): string;         // "2026-08-05T14-02-31.ndjson" (local time)
export function replayChat(lines: string[]): AgentStore; // corrupt/unknown lines skipped
export class ChatRecorder {
  /** send: injected api call, e.g. (file, line) => api.chatAppend(sketchDir, file, line) */
  start(fileName: string, meta: { sketchDir: string; startedAt: string }, send: (file: string, line: string) => Promise<void>): void;
  record(op: ChatOp): void;   // no-op unless started; serializes; send().catch(() => {}) — never throws
  stop(): void;               // forget current file
  readonly active: boolean;
}
```
`replayChat` applies ops to a fresh `AgentStore` (`sessionStarted` → `.sessionStarted(pid)`, `userSent` → `.userSent(text)`, `push` → `.push(ev as AgentEvent)`, `closed` → `.closed(reason, pid)`; `meta` ignored). The store import must be type-safe against `src/agent/agentStore.ts` as of commit 41a8824.

- [ ] Failing tests first: `chatFileName(new Date(2026, 7, 5, 14, 2, 31))` → `"2026-08-05T14-02-31.ndjson"`; recorder emits meta line first then ops as JSON lines to the injected `send` (capture with a spy array; assert exact serialized lines); recorder inactive → record is a no-op; a rejecting `send` does not throw (await a tick); `replayChat` of a recorded round trip (user msg, tool_use push, tool_result push, result push) produces a snapshot deep-equal to driving a fresh store directly with the same calls; corrupt line (`"{oops"`) and unknown op (`{"op":"wat"}`) are skipped without throwing.
- [ ] Run `npx vitest run src/agent/__tests__/chatLog.test.ts` — FAIL, then implement, then green + full `npm test`.
- [ ] Do NOT commit (Task 3 commits the feature whole).

### Task 3: Tauri commands + App/Panel wiring (sequential, after 1 & 2)

**Files:** Modify `src-tauri/src/lib.rs`, `src/api.ts`, `src/App.tsx`, `src/components/AgentPanel.tsx`, `src/styles.css`.

- [ ] Commands (mirror the `settings` plumbing for `app_config_dir`): `chat_append(sketch_dir, file, line)` → `append_line`; when it returns `created == true`, call `prune(root, key, 50)`. `chat_list(sketch_dir) -> Vec<ChatEntry>`, `chat_load(sketch_dir, file) -> Vec<String>`, `chat_delete(sketch_dir, file)`. Register all four in `generate_handler!`.
- [ ] `src/api.ts`: `chatAppend`, `chatList` (returns `{ file: string; title: string }[]`), `chatLoad`, `chatDelete`.
- [ ] `App.tsx`: one module-level `ChatRecorder`; in `sendToAgent` after the `agentStart` branch — if recorder inactive: `recorder.start(chatFileName(new Date()), { sketchDir, startedAt: new Date().toISOString() }, (f, l) => api.chatAppend(sketchDir, f, l))`; `record({op:"userSent",…})` next to `agentStore.userSent`; `record({op:"sessionStarted",…})` next to `agentStore.sessionStarted`; `record({op:"push", ev})` in the `onAgentEvent` listener next to `agentStore.push(ev)` (use a ref for sketchDir as the listeners do); `record({op:"closed",…})` + `recorder.stop()` in `onAgentClosed`; `recorder.stop()` in `newAgentSession`.
- [ ] `AgentPanel.tsx`: `🕘 History` button in the footer (left of "New session"); `histList` state loaded via `api.chatList(sketchDir)`; list view rendered in the scroll area (rows: title, file-stem date, 🗑 calling `api.chatDelete` then reloading the list); clicking a row → `api.chatLoad` → `replayChat(lines)` → `histStore` state; while `histStore` set, render its snapshot messages with the existing `MessageView` (read-only: `onOpenTurn` may still open turn summaries) and replace the input row with a `← Back to current chat` bar. History views win over `viewTurn` state conflicts by clearing `viewTurn` when entering/leaving history.
- [ ] CSS: `.agent-hist-list`, `.agent-hist-row` (hover raised, title ellipsis), `.agent-hist-date`, `.agent-back-bar`.
- [ ] Verify: `npm run build && npm test && cargo test -p bancada-core && cargo check -p bancada` all green.
- [ ] Commit everything: `feat: browsable per-sketch assistant chat history`.
