# Assistant UX (activity, usage, summaries, markdown) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `docs/superpowers/specs/2026-08-05-assistant-ux-activity-usage-design.md`.

**Architecture:** Two new pure modules (`usage.ts`, `activity.ts`) with tests; store grows `turn_end` messages, a streaming flag, and session totals; panel grows the activity footer, dividers, a turn-summary view, and react-markdown rendering. No backend changes.

**Tech Stack:** React/TS, vitest, react-markdown + remark-gfm (new deps).

## Global Constraints

- `parseTurnUsage` must never throw on any input shape.
- `activity.ts` returns null unless status is "running"/"starting".
- Raw HTML in markdown stays escaped (react-markdown default — do not add rehype-raw).

---

### Task 1: usage.ts (TDD)

**Files:** Create `src/agent/usage.ts`, `src/agent/__tests__/usage.test.ts`.

**Interfaces (produces):**
- `interface TurnUsage { inputTokens: number; outputTokens: number; cacheReadTokens: number; costUsd?: number; numTurns?: number; durationMs?: number }`
- `interface SessionUsage { costUsd: number; inputTokens: number; outputTokens: number }`
- `parseTurnUsage(ev: unknown): TurnUsage | null` — reads `usage.input_tokens` etc., `total_cost_usd`, `num_turns`, `duration_ms`; null when `usage` is not an object with numeric input+output.
- `addUsage(s: SessionUsage, t: TurnUsage): SessionUsage` (pure, new object)
- `emptySessionUsage(): SessionUsage`
- `formatTokens(n: number): string` — `999` → "999", `12401` → "12.4k", `1200000` → "1.2M".

- [ ] Steps: failing tests (parse full result event, missing usage → null, non-object → null, accumulate, format thresholds) → verify RED → implement → verify GREEN.

### Task 2: activity.ts (TDD)

**Files:** Create `src/agent/activity.ts`, `src/agent/__tests__/activity.test.ts`.

**Interfaces:**
- Consumes `AgentMessage`, `AgentStatus` from `agentStore.ts`.
- `activityLabel(a: { status: AgentStatus; verifyRunning: boolean; streaming: boolean; messages: AgentMessage[] }): string | null` — priority: verify → newest running tool (`⚙ ${name} ${hint}…`) → streaming (`✍ writing…`) → `thinking…`; null unless status is "running" or "starting".
- Hint: basename of `input.file_path` for Edit/Write/Read; `input.pattern` for Grep/Glob; "" otherwise.

- [ ] Steps: failing tests (each priority tier, hint extraction, null when ended/idle) → RED → implement → GREEN.

### Task 3: store turn_end + streaming + session totals (TDD)

**Files:** Modify `src/agent/agentStore.ts`; create `src/agent/__tests__/agentStore.test.ts`.

**Interfaces:**
- New message kind: `{ kind: "turn_end"; usage?: TurnUsage; summary?: string; tools: { name: string; status: "running" | "ok" | "error" }[] }` added to `AgentMessage`.
- Snapshot gains `streaming: boolean` and `sessionUsage: SessionUsage`.
- `handleResult` additionally: collect tool messages backward until the previous `user` message; push `turn_end` (summary from `ev.result`); accumulate `sessionUsage` when usage parsed; clear streaming.
- Streaming flag: true in `appendAssistantDelta`, false in `handleResult`, tool_use push, `userSent`, `closed`.

- [ ] Steps: failing tests (result event pushes turn_end with usage+tools+summary; usage-less result still pushes turn_end; session totals accumulate across two results; streaming flips on delta and off on result) → RED → implement → GREEN → full `npm test`.

### Task 4: panel + markdown + CSS (visual)

**Files:** Modify `src/components/AgentPanel.tsx`, `src/styles.css`, `package.json`.

- [ ] `npm i react-markdown remark-gfm`.
- [ ] Assistant bubble: replace `splitFences` rendering with `<ReactMarkdown remarkPlugins={[remarkGfm]}>` inside `div.agent-markdown`.
- [ ] `MessageView` case "turn_end": divider `${numTurns} turns · ${formatTokens(in)} in / ${formatTokens(out)} out · $cost` + `details ▸` button calling `onOpenTurn(msg)`.
- [ ] Turn summary view: `viewTurn` state in AgentPanel; when set, replace `.agent-scroll` content with `← back to chat` bar + markdown summary + usage grid + tools list (✓/✗/… per status).
- [ ] Footer: `activityLabel(...)` result when non-null, else `statusLabel(...)`; add `Σ ${cost} · ${formatTokens(in+out)}` chip when sessionUsage is non-zero.
- [ ] Delete `splitFences` import; if `src/agent/fences.ts` has no remaining importers, delete it and its test file.
- [ ] CSS: `.agent-turn-end`, `.agent-markdown` (headings, lists, tables, inline code, blockquote on dark theme), `.agent-usage-view`, `.agent-activity`.

- [ ] Verify: `npm run build && npm test`; visual check in the running app.

### Task 5: Commit

- [ ] `git add -A src docs package.json package-lock.json && git commit -m "feat: assistant activity strip, per-turn usage, turn summaries, markdown"`.
