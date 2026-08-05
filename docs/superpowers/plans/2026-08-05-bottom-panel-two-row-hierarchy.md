# Bottom Panel Two-Row Hierarchy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the bottom panel's single header row into two rows — group buttons (Console / Debugging / Observability / Assistant) on top, the active group's sub-tabs always visible below — per `docs/superpowers/specs/2026-08-05-bottom-panel-two-row-hierarchy-design.md`.

**Architecture:** Render/CSS change only. The grouping data and predicates in `src/bottomTabs.ts` and all routing/mounting logic in `App.tsx` stay untouched; only the header JSX inside the `<section className="bottom">` block and one CSS rule change.

**Tech Stack:** React 18 + TypeScript (Vite), plain CSS in `src/styles.css`, vitest for the (unchanged) unit tests.

## Global Constraints

- Serial Monitor and Oscilloscope remain under the Debugging group; MQTT and WebSocket under Observability (no changes to `GROUP_TABS`).
- Row 2 is **always** rendered, even for single-tab groups — header height must not change when switching groups.
- No changes to `src/bottomTabs.ts`, `openBottomTab`, per-group tab memory, lazy mounting, or unseen-dot logic.
- TDD does not apply: no logic changes anywhere. The gate is `npm run build` (tsc + vite) and `npm test` staying green, plus the visual checklist in Task 1 Step 4.

---

### Task 1: Two-row header markup + CSS

**Files:**
- Modify: `src/App.tsx:1187-1256` (the `<div className="panel-tabs">` block inside `<section className="bottom">`)
- Modify: `src/styles.css` (add `.bottom-groups` after the `.bottom-group-btn:focus-visible` rule at ~line 499; delete the `.tab-sep` rule at ~line 501)

**Interfaces:**
- Consumes: `GROUP_TABS`, `GROUP_LABEL`, `TAB_LABEL`, `groupHasUnseen`, `BottomGroup` from `src/bottomTabs.ts`; existing `openBottomTab`, `bottomGroup`, `bottomTab`, `debugTab`, `obsTab`, `unseen`, `bottomMax`, `setBottomMax`, `baudrate`, `setBaudrate`, `monitorOn`, `toggleMonitor` — all already in scope in `App.tsx`.
- Produces: nothing new — no exported surface changes.

- [ ] **Step 1: Replace the header JSX in `src/App.tsx`**

Replace the entire `<div className="panel-tabs">…</div>` block (currently lines 1187–1256, between the resize-handle conditional and `{bottomTab === "build" && …}`) with:

```tsx
        <div className="bottom-groups">
          {(Object.keys(GROUP_TABS) as BottomGroup[]).map((g) => (
            <button
              key={g}
              className={
                bottomGroup === g
                  ? "bottom-group-btn active"
                  : "bottom-group-btn"
              }
              onClick={() =>
                openBottomTab(
                  g === "console"
                    ? "build"
                    : g === "debug"
                      ? debugTab
                      : g === "obs"
                        ? obsTab
                        : "agent",
                )
              }
            >
              {GROUP_LABEL[g]}
              {groupHasUnseen(g, bottomGroup, unseen) && (
                <span className="tab-dot">●</span>
              )}
            </button>
          ))}
          <div className="spacer" />
          <button
            className="btn small icon"
            onClick={() => setBottomMax((m) => !m)}
            title={bottomMax ? "Restore panel" : "Maximize panel"}
          >
            {bottomMax ? "❐" : "⛶"}
          </button>
        </div>
        <div className="panel-tabs">
          {GROUP_TABS[bottomGroup].map((t) => (
            <button
              key={t}
              className={bottomTab === t ? "tab active" : "tab"}
              onClick={() => openBottomTab(t)}
            >
              {TAB_LABEL[t]}
              {unseen[t] && <span className="tab-dot">●</span>}
            </button>
          ))}
          <div className="spacer" />
          {bottomTab === "serial" && (
            <>
              <select
                className="select small"
                value={baudrate}
                onChange={(e) => setBaudrate(Number(e.target.value))}
                disabled={monitorOn}
              >
                {[9600, 19200, 57600, 115200, 230400, 921600].map((b) => (
                  <option key={b} value={b}>
                    {b} baud
                  </option>
                ))}
              </select>
              <button className="btn small" onClick={toggleMonitor}>
                {monitorOn ? "Stop" : "Start"}
              </button>
            </>
          )}
        </div>
```

Deliberate differences from the old block: the sub-tab list loses its `GROUP_TABS[bottomGroup].length > 1 &&` guard (row 2 is always rendered), the `<div className="tab-sep" />` is gone, and the maximize button moves from the end of the shared row to the end of the group row.

- [ ] **Step 2: Update `src/styles.css`**

Insert after the `.bottom-group-btn:focus-visible` rule (~line 499), replacing the comment above `.bottom-group-btn` is NOT needed — but update that comment since the "single shared row" claim becomes false. Change the comment at ~line 467-468 from:

```css
/* bottom-panel group buttons: same state language as .side-group-btn but
   sized for a single shared row (group buttons | sub-tabs | controls) */
```

to:

```css
/* bottom-panel group row: panel-level tier (groups + maximize) over the
   per-group sub-tab row — the sidebar's .side-groups/.panel-tabs pattern */
```

Then add the new container rule right after `.bottom-group-btn:focus-visible`:

```css
.bottom-groups {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 6px 2px;
  background: var(--bg-panel);
}
```

And delete the now-unused `.tab-sep` rule (~lines 501–506):

```css
.tab-sep {
  width: 1px;
  align-self: stretch;
  margin: 2px 4px;
  background: var(--border);
}
```

- [ ] **Step 3: Type-check, build, and run the unit tests**

Run: `npm run build && npm test` (in `/home/kayaman/Projects/bancada`)
Expected: tsc clean, vite build succeeds, all vitest suites PASS (nothing in `bottomTabs.ts` or its tests changed).

- [ ] **Step 4: Visual verification**

Run the app (`npm run tauri dev`, or `npm run dev` for the browser shell) and check:
- Header shows two rows; switching across all four groups keeps the header height constant and group buttons never shift horizontally.
- Debugging shows ❯ Serial Monitor and ∿ Oscilloscope in row 2; Observability shows MQTT and WebSocket; Console and Assistant each show their single tab as active.
- Baud select + Start/Stop appear in row 2 only while Serial Monitor is the active tab.
- Maximize/restore button works from row 1 in both states.
- Unseen dots: trigger a build with the Debugging group open — the Console group button gets a dot; open a tab with a dot and it clears.

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx src/styles.css
git commit -m "feat: two-row bottom-panel hierarchy (groups over sub-tabs)"
```
