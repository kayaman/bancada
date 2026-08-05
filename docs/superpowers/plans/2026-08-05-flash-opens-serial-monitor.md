# Flash Opens Serial Monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After a successful flash, open the Serial Monitor tab immediately instead of waiting for the first serial line, per `docs/superpowers/specs/2026-08-05-flash-opens-serial-monitor-design.md`.

**Architecture:** App wiring only — one ref and one handler branch deleted, one `openBottomTab("serial")` added in `upload()`'s success path.

**Tech Stack:** React/TypeScript, vitest (unchanged suites as the gate).

## Global Constraints

- Failed flashes must keep the Build console in front; Verify and the scope-firmware flash paths are untouched.
- No new state; `startMonitorQuiet`'s 1200 ms settle stays as is.

---

### Task 1: Open serial tab on flash success, drop the first-line trigger

**Files:**
- Modify: `src/App.tsx` (three spots: the `autoOpenSerialUntilRef` declaration ~line 140, the `onSerialLine` handler ~line 303, the `upload()` success block ~line 666)

**Interfaces:**
- Consumes: existing `openBottomTab`, `startMonitorQuiet` — no signature changes.
- Produces: nothing new.

- [ ] **Step 1: Delete `autoOpenSerialUntilRef`** (declaration next to `bottomTabRef`).

- [ ] **Step 2: Simplify the `onSerialLine` handler** to append + unseen-dot only:

```tsx
      api.onSerialLine((l) => {
        setSerialLines((prev) => appendCapped(prev, l));
        if (bottomTabRef.current !== "serial")
          setUnseen((u) => ({ ...u, serial: true }));
      }),
```

- [ ] **Step 3: In `upload()`'s `if (r.success)` block**, replace `autoOpenSerialUntilRef.current = Date.now() + 15000;` with `openBottomTab("serial");` and update the comment to say the tab opens now and capture resumes after the re-enumeration settle.

- [ ] **Step 4: Verify** — `npm run build && npm test`: tsc clean (the deleted ref has no remaining references), all suites green.

- [ ] **Step 5: Commit** `src/App.tsx` + both docs: `feat: open the Serial Monitor as soon as a flash succeeds`.
