# Board Picker Open Results Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While the board filter has a query, the select shows the matches as an open listbox overlay; picking (click or Enter) collapses it.

**Architecture:** Pure logic stays in `src/boardSearch.ts` (vitest-tested); `BoardPicker.tsx` swaps between closed select and open `size=N` listbox; CSS anchors the open listbox as an absolute overlay so neither host layout (New Project dialog, profile-init row) jumps.

**Tech Stack:** TypeScript, React 18, vitest, native `<select size>`.

**Spec:** `docs/superpowers/specs/2026-08-07-board-picker-open-results-design.md`

## Global Constraints

- Open ⇔ trimmed query non-empty. Picking clears the query.
- Rows: `min(10, max(3, matches + group headers))`; zero matches → one disabled `— no boards match —` option.
- Enter in the input picks the first **match** (`filterBoards`, never a pinned stale selection); Escape clears the query.
- `visibleBoards` is deleted along with its tests.

---

### Task 1: boardSearch.ts — add `listboxRows`, delete `visibleBoards`

**Files:**
- Modify: `src/boardSearch.ts`
- Test: `src/__tests__/boardSearch.test.ts`

**Interfaces:**
- Produces: `listboxRows(matchCount: number, groupCount: number): number`. Task 2 consumes it plus the existing `filterBoards`/`groupByPlatform`.

- [ ] **Step 1: Update the tests** — delete the `describe("visibleBoards", ...)` block and its import; append:

```ts
describe("listboxRows", () => {
  it("counts matches plus group headers", () => {
    expect(listboxRows(4, 2)).toBe(6);
  });
  it("never below 3 (room for the no-match row)", () => {
    expect(listboxRows(0, 0)).toBe(3);
    expect(listboxRows(1, 1)).toBe(3);
  });
  it("caps at 10", () => {
    expect(listboxRows(200, 30)).toBe(10);
  });
});
```

- [ ] **Step 2: Run to verify failure** — `npx vitest run src/__tests__/boardSearch.test.ts` → FAIL (`listboxRows` not exported).

- [ ] **Step 3: Implement** — in `src/boardSearch.ts`, replace `visibleBoards` with:

```ts
/** Rows for the open results listbox: matches + their optgroup headers,
 *  clamped to [3, 10] — 3 keeps the "no boards match" state from looking
 *  like a sliver, 10 keeps the overlay from swallowing the dialog. */
export function listboxRows(matchCount: number, groupCount: number): number {
  return Math.min(10, Math.max(3, matchCount + groupCount));
}
```

- [ ] **Step 4: Run to verify pass** — same command → PASS.
- [ ] **Step 5: Commit** — `git add src/boardSearch.ts src/__tests__/boardSearch.test.ts && git commit -m "feat: listboxRows sizing helper, drop visibleBoards pinning"`

---

### Task 2: BoardPicker.tsx + CSS — open listbox overlay

**Files:**
- Modify: `src/components/BoardPicker.tsx`
- Modify: `src/styles.css` (after `.board-picker .select` block, line ~412)

**Interfaces:**
- Consumes: `filterBoards`, `groupByPlatform`, `listboxRows`.
- Produces: user-visible behavior only; props unchanged, so NewProject/ProfileInit need no edits.

- [ ] **Step 1: Rewrite the component**

```tsx
import { useState } from "react";
import type { BoardOption } from "../api";
import { filterBoards, groupByPlatform, listboxRows } from "../boardSearch";

interface Props {
  boards: BoardOption[];
  /** Selected FQBN, or "" for none. */
  value: string;
  onChange: (fqbn: string) => void;
  title: string;
}

/** A grouped board <select> with a filter input in front. While the query
 *  is non-empty the select opens into a results listbox overlaying the
 *  content below (see .board-select-anchor); picking collapses it. */
export default function BoardPicker({ boards, value, onChange, title }: Props) {
  const [query, setQuery] = useState("");
  const open = query.trim().length > 0;
  const matches = open ? filterBoards(boards, query) : boards;
  const groups = groupByPlatform(matches);

  const pick = (fqbn: string) => {
    onChange(fqbn);
    setQuery(""); // collapse back to the closed select showing the pick
  };

  return (
    <span className="board-picker">
      <input
        className="input board-search"
        placeholder="filter boards…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && open && matches.length > 0) {
            e.preventDefault();
            pick(matches[0].fqbn);
          } else if (e.key === "Escape") {
            setQuery("");
          }
        }}
        title="Filter by name, FQBN or platform — results open below"
      />
      <span className="board-select-anchor">
        <select
          className={open ? "select board-select-open" : "select"}
          size={open ? listboxRows(matches.length, groups.length) : undefined}
          value={value}
          onChange={(e) => pick(e.target.value)}
          title={title}
        >
          {open ? (
            <>
              {/* The control's value must always have an option, even when
                  the current selection doesn't match the filter. */}
              {value && !matches.some((b) => b.fqbn === value) && (
                <option value={value} hidden />
              )}
              {matches.length === 0 && (
                <option value="" disabled>
                  — no boards match —
                </option>
              )}
            </>
          ) : (
            <option value="">— choose a board —</option>
          )}
          {groups.map(([platform, list]) => (
            <optgroup key={platform} label={platform}>
              {list.map((b) => (
                <option key={b.fqbn} value={b.fqbn}>
                  {b.name}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </span>
    </span>
  );
}
```

- [ ] **Step 2: CSS** — replace the `.board-picker .select` rule with:

```css
/* The select lives in a fixed-height anchor so the open listbox can
   overlay the content below instead of shoving it down. */
.board-picker .board-select-anchor {
  position: relative;
  flex: 1;
  min-width: 0;
  height: 28px; /* the shared .select height — keeps the row from jumping */
}

.board-select-anchor .select {
  width: 100%;
}

.board-select-anchor .select.board-select-open {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  z-index: 30;
  height: auto;
  padding: 4px 0;
  background: var(--bg-raised);
  border: 1px solid var(--accent);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
}
```

- [ ] **Step 3: Verify** — `npm test` (all suites) and `npm run build` (tsc) pass. Visual: `npm run tauri dev` → New Project: type "esp32" → listbox opens over the fields below, Enter picks the first match, select collapses showing it; profile-init row same; Escape closes.

- [ ] **Step 4: Commit** — `git add src/components/BoardPicker.tsx src/styles.css && git commit -m "feat: board search results open as a listbox overlay"`
