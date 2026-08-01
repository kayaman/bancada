# Board Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make both board pickers searchable via a shared filter-input + native-select component.

**Architecture:** Pure matching/grouping logic in `src/boardSearch.ts` (vitest), one thin `BoardPicker` component consumed by NewProject and ProfileInit.

**Tech Stack:** TypeScript/React 18, vitest. No backend changes.

## Global Constraints

- Native `<select>` stays — no custom dropdown.
- Commit trailer: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: boardSearch logic

**Files:**
- Create: `src/boardSearch.ts`, `src/__tests__/boardSearch.test.ts`

**Interfaces:**
- Produces: `filterBoards(boards: BoardOption[], query: string): BoardOption[]`;
  `visibleBoards(boards: BoardOption[], query: string, selectedFqbn: string): BoardOption[]`;
  `groupByPlatform(boards: BoardOption[]): [string, BoardOption[]][]`

- [ ] **Step 1: Write the failing tests** (fixture: esp32s3/uno/nano boards across two platforms; cases: empty query → all; "uno" case-insensitive by name; fqbn segment "esp32s3"; platform "AVR"; "esp32 s3" AND-match; selected pinned when filtered out; not duplicated when it matches; grouping keeps first-seen platform order)
- [ ] **Step 2: `npx vitest run src/__tests__/boardSearch.test.ts`** → FAIL (module not found)
- [ ] **Step 3: Implement** — tokenized `every(token => haystack.includes(token))` over lowercased `name + fqbn + platform_name`; `visibleBoards` prepends the selected board when missing; `groupByPlatform` is the Map loop lifted from NewProject.tsx:65-73
- [ ] **Step 4: Re-run** → all PASS
- [ ] **Step 5: Commit** — `feat: add boardSearch — filter, pin-selected, and grouping logic`

### Task 2: BoardPicker component + wiring

**Files:**
- Create: `src/components/BoardPicker.tsx`
- Modify: `src/components/NewProject.tsx` (drop `grouped` memo, select → BoardPicker), `src/components/ProfileInit.tsx` (drop `groups` memo, select → BoardPicker), `src/styles.css` (`.board-picker`, `.board-search`)

**Interfaces:**
- Consumes: Task 1 exports
- Produces: `BoardPicker({ boards, value, onChange, title }: { boards: BoardOption[]; value: string; onChange: (fqbn: string) => void; title: string })`

- [ ] **Step 1: Write BoardPicker** — local `query` state; `visibleBoards(boards, query, value)` → `groupByPlatform` → optgroups; search `<input className="input board-search" placeholder="filter boards…">`
- [ ] **Step 2: Swap into NewProject and ProfileInit**, removing their grouping memos
- [ ] **Step 3: Add styles** — `.board-picker { display: flex; gap: 6px; flex: 1; }`, `.board-search { width: 130px; flex: none; }`, select flexes
- [ ] **Step 4: Verify** — `npx vitest run` green, `npm run build` clean
- [ ] **Step 5: Commit** — `feat: searchable board picker in New Project and Create profile`
