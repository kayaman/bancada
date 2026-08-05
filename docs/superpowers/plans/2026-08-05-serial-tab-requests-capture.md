# Serial Tab Requests Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Showing the Serial Monitor tab (re)requests capture, per `docs/superpowers/specs/2026-08-05-serial-tab-requests-capture-design.md`.

**Architecture:** One `useEffect` in `src/App.tsx`; no new modules, no backend.

**Tech Stack:** React/TS; existing suites as the gate.

## Global Constraints

- Must not restart the monitor after an explicit Stop while the tab stays open (monitor state via ref, not deps).
- Must not open the port while `busy` (build/flash in flight).

---

### Task 1: The effect

**Files:** Modify `src/App.tsx` (after the existing capture-by-default effect at ~line 706).

- [ ] **Step 1:** Add below the `[selectedPort]` auto-start effect:

```tsx
  // Capture by default, part 2: the Serial Monitor tab being visible is a
  // standing request for capture. The port-selection auto-start above is a
  // one-shot whose failure is deliberately silent, so a lost attempt (port
  // briefly held by a dying previous monitor child, say) left the monitor
  // dead until a manual Start. Edge-triggered on tab/port — monitor state
  // is read via refs inside startMonitorQuiet, deliberately NOT a dep, so
  // Stop while staying on the tab stays stopped; leaving and returning
  // re-requests capture. busyRef guards flash-time port contention.
  useEffect(() => {
    if (bottomTab !== "serial" || busyRef.current) return;
    startMonitorQuiet();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bottomTab, selectedPort, startMonitorQuiet]);
```

- [ ] **Step 2:** `npm run build && npm test` — tsc clean, all suites green.
- [ ] **Step 3:** Live check: with monitor off, open Debugging → Serial Monitor; `fuser /dev/ttyACM0` must show the monitor holding the port; press Stop and stay on the tab — it must stay stopped.
- [ ] **Step 4:** Commit `src/App.tsx` + both docs: `feat: showing the Serial Monitor re-requests capture`.
