# Frontend

React 18 + zustand + CodeMirror 6. No router, no UI framework, no charting
library. `src/main.tsx` is ten lines and mounts `<App/>` — there are no
providers, no context, and no global store.

```
┌─ Toolbar ──────────────────────────────────────────────────────────┐
│ brand │ 📁 project ▾ │ profile ＋ ✎ │ port ⟳ │ git ⟷ 📊 Verify Flash │
├─ ProfileInit (conditional row) ────────────────────────────────────┤
├─ main ─────────────────────────────────────────────────────────────┤
│ ┌ sidebar ─────────┐ ┌ editor-area ────────────────────────────┐   │
│ │ Software│Hardware│ │ New | Duplicate | Rename | Usage | edit │   │
│ │ ──────────────── │ │   …or…                                  │   │
│ │ Files│Libraries  │ │ EditorTabs + CodeMirror                 │   │
│ │ Boards│Fleet     │ │                                         │   │
│ └──────────────────┘ └─────────────────────────────────────────┘   │
├─ bottom ───────────────────────────────────────────────────────────┤
│ Console │ Debugging │ Observability │ Assistant     ← groups       │
│ Build│Serial   Scope    MQTT│WS│Web    Chat          ← sub-tabs    │
├─ statusbar ────────────────────────────────────────────────────────┤
└────────────────────────────────────────────────────────────────────┘
```

---

## 1. The shell

`src/App.tsx` is 2,217 lines and is the de-facto orchestrator: it owns nearly
all cross-panel state and registers every backend subscription. Its render tree
begins around line 1783.

Five composition rules are encoded there and are easy to break by accident:

**One project affordance.** `📁 <name> ▾` (`ProjectMenu.tsx`) names the open
project and holds every action on it — Open, Recent, New, Duplicate, Rename.
It exists because the bar had drifted into six project controls and three
glyphs each meaning two things. What it offers, and what is disabled and why,
lives in `src/toolbarModel.ts`: `vitest`'s include glob does not match `.tsx`,
so a decision left in the component is verified by eye alone.

`ProjectMenu` is also the codebase's only **nested** `Menu`. `Menu` needed no
change — a child rendered inside the parent's `children` is inside the parent's
`ref`, so clicking it does not dismiss the parent. Escape was the one gap: both
listen on `window` in the bubble phase and would close together, so
`ProjectMenu` takes Escape in the **capture** phase and stops it there.

**One editor-area form at a time.** `NewProject`, `DuplicateProject`,
`RenameProject` and `UsageDashboard` are mutually exclusive, and the profile
form is exclusive with all of them. That is expressed once, by `showPane(pane,
profileMode?)` — call it rather than setting the booleans. The reset list used
to be repeated at all seven call sites; adding a pane meant editing every one
of them, and missing one would have shown two forms stacked.

**Two-level tab hierarchy.** Both the sidebar and the bottom panel have a group
switcher over per-group sub-tabs, and **each group remembers its last-used
tab**. The bottom panel's mapping is data-driven from `src/bottomTabs.ts`
(`GROUP_OF`, `GROUP_TABS`, `GROUP_LABEL`, `TAB_LABEL`, `groupHasUnseen`) — add a
tab there, not in JSX.

**Hide, don't unmount.** Panels holding a live connection — scope, MQTT, WS,
device browser, agent — latch mounted on first open (`scopeMounted`,
`mqttMounted`, …) and are thereafter hidden with `display:none`. Unmounting them
would drop a socket, a channel or an agent session on a tab switch. The sidebar
and `.main` use the same trick.

The one deliberate exception: `AgentPanel` is **keyed on `sketchDir`**, so
switching project does hard-reset its panel-local state.

**Layout prefs go to `localStorage`, not settings.** `bancada.bottomHeight`,
`bancada.sidebarWidth`, `bancada.sidebarCollapsed`. They are per-machine window
furniture, not project state.

---

## 2. State — three tiers, no global store

The absence of a global store is deliberate. State lives at the tier that
matches its update frequency.

### Tier 1 — `App.tsx` local state, passed by props

The majority: `sketchDir`, `profile`, open file and buffers, dirty set, open
tabs, ports and selection, build and serial lines, monitor flags, layout,
`busy`, conflicts, status.

Shared downward **by props only** — which is why `<Toolbar>` takes ~30 of them.

Heavy use of **ref mirrors** (`sketchDirRef`, `openFileRef`, `monitorOnRef`,
`busyRef`, `bottomTabRef`, `uploadsArmedRef`, …) because the subscription effect
is registered once with `[]` deps and must read current values without
re-subscribing.

### Tier 2 — one zustand store

`src/explorerStore.ts` (`useExplorerStore`), scoped strictly to *file-tree UI
state*: files, expanded, selected, renaming, creating, dragging, drop target,
context menu.

It **never calls `api`**. App handlers perform the filesystem operation and feed
the new listing back via `setFiles` / `setFilesAfterRename`. Keep it that way —
the store is a view model, not a controller.

### Tier 3 — plain classes, polled

`AgentStore` (App-owned singleton) and `ObsStore` (one per MQTT/WS/Web panel),
plus `ScopeEngine`. All expose a monotonic `version`; components **poll** it
rather than subscribe — 100 ms for the agent panel, ~4 Hz for observability,
250 ms for scope readouts.

This is the important one. These ingest at rates React should never see: a
50 kSa/s ADC stream, a chatty MQTT topic, token-level streaming deltas. Polling
a version counter decouples ingest rate from render rate. **Do not "fix" this
into `useState`.**

There are **no custom hooks** in the codebase. `useExplorerStore` is the only
`use*` export.

---

## 3. The pure-logic tier

`vitest` runs in the **node** environment with no jsdom, and no component is
rendered in any test ([conventions §2](conventions.md#typescript-node-environment-no-dom)).
So anything worth testing has been extracted into a plain `.ts` module:

| Module | Owns |
|---|---|
| `api.ts` | the entire IPC surface (§4) |
| `bottomTabs.ts` | bottom panel group/tab taxonomy |
| `editorTabs.ts` | open/close/rename/delete tab transitions; dirty tabs are never bulk-closed |
| `explorerOps.ts` | rename/delete path math, protected paths (mirrors core's `is_protected`) |
| `fileTreeModel.ts` | tree building, visible nodes, expansion pruning |
| `newFile.ts` | friendly filename pre-validation |
| `conflicts.ts` | the agent/user edit-conflict guard used before send, verify and upload |
| `ports.ts` | **how a port is named**, visible boards, flash-target mismatch, port options |
| `portWatch.ts` | hotplug arrival diffing |
| `gitStatus.ts` | the git pill's entire vocabulary, incl. why flashes are untagged |
| `boardOffer.ts` | whether to offer a plugged-in board's project back, and which |
| `monitorRecovery.ts` | when to re-take the serial port after capture is lost |
| `check.ts` | the one `{ok} \| {ok:false, reason}` shape, and `reasonOf` |
| `publishRepo.ts` | repo-name rules and why publishing is blocked |
| `projectRename.ts` | project-name rules (mirrors `core::project::validate_project_name`) and the rename plan |
| `toolbarModel.ts` | the project button's label; what the project menu offers, and what is disabled and why |
| `boardSearch.ts` | board filtering and grouping |
| `profileInit.ts` | profile form modes and submit plans |
| `usageDashboard.ts` | totals and display names |
| `keys.ts` | accelerator parsing and matching; Ctrl and Cmd are one modifier |

Plus `src/scope/`, `src/agent/` and `src/obs/`, which are subsystems in their
own right (§5).

**The rule for new work:** if you are about to put logic in a `.tsx` file, put
it in a module and call it from the component. Two components already export a
pure helper for exactly this reason — `BoardPicker.fallbackFqbnLabel` and
`DeviceBrowserPanel.exchangeRow`.

Injected clocks are used throughout for the same testability reason:
`ObsStore.push(ts)`, `chatFileName(now)`, the resume-watch timeouts. **`App.tsx`
owns "now".**

---

## 4. The IPC layer

`src/api.ts` (920 lines) is the only file importing `@tauri-apps/api/core` or
`/event`. Elsewhere only `plugin-dialog` and `getVersion` appear.

It holds the TypeScript mirrors of the Rust types, 94 `invoke` wrappers, the
three `Channel` openers, and the seven `listen` subscriptions. Full surface:
[ipc-contract](ipc-contract.md).

**Where subscriptions live:** almost all in a single empty-dep effect in
`App.tsx` — build, serial ×3, ports hotplug (with 500 ms coalescing and
deferral during a flash), agent event and agent closed. The only subscription
outside `App.tsx` is `onSerialLine` inside `ScopeView` for the plotter source.

Channel streams are opened by the panel that owns them: `ScopeView`
(`scopeStart`), `MqttPanel` (`mqttConnect`), `DeviceBrowserPanel`
(`deviceBrowseStart`).

---

## 5. Subsystems

### `src/scope/` — the oscilloscope engine

The UI imports only `types.ts` and `engine.ts`. Everything else is internal:
`ring.ts` (power-of-two `Float32Array` addressed by *absolute* sample index),
`binary.ts` (envelope decoder), `textParser.ts`, `trigger.ts` (hysteresis
arm/fire scanned incrementally), `decimate.ts` (min/max buckets into
caller-supplied arrays), `measure.ts`, `fft.ts`, `fftWorker.ts`.

`ScopeEngine` (`engine.ts`, 771 lines) orchestrates. The performance-critical
invariant: **`renderFrame(columns)` allocates nothing.** It decimates into
reused scratch arrays and returns a single reused `RenderFrame` object, because
it runs in a 60 fps rAF loop. `engineRender.test.ts` pins this — if you make it
allocate, that test fails, and it is right and you are wrong.

FFT runs off-thread in a module worker, with a synchronous fallback for node
(tests) and for worker failure.

Contract: [`docs/scope-architecture.md`](../scope-architecture.md) §4.

### `src/agent/` — the Assistant

`agentStore.ts` (623 lines) is the reducer and view model. `types.ts` mirrors
the `claude` CLI's wire objects. `chatLog.ts` persists **operations, not
messages** — one NDJSON line per mutating store call, so replaying the log
through a fresh store reproduces the live rendering exactly. There is no second
message schema, which is why `UsageDashboard` can replay a saved chat inline by
building a throwaway store.

Also: `resumeWatch.ts` (the `--resume` confirm/fail state machine),
`continueChat.ts` (bounded plain-text fallback summary), `diff.ts` (hand-rolled
LCS unified diff), `verifyResult.ts` (parses the MCP `verify` reply — the other
half of a cross-language contract), `activity.ts`, `alarmCopy.ts`.

`AgentPanel` never subscribes to events; App delivers them to the store and the
panel polls.

### `src/obs/` — observability plumbing

`obsStore.ts` (a capped ring with per-topic stats, pause and buffered count),
`backoff.ts` (deterministic 1s→2s→4s…30s, **no jitter**, because the values are
shown verbatim in a countdown), `redact.ts` (URL password redaction, a TS twin
of the Rust one).

Shared by `MqttPanel`, `WsPanel` and `DeviceBrowserPanel`, all rendering through
the dumb `ObsLog` component.

---

## 6. Styling

One stylesheet: `src/styles.css`, 2,807 lines, imported once. No CSS modules, no
Tailwind, no CSS-in-JS.

Design tokens in `:root` — `--bg`, `--bg-panel`, `--bg-raised`, `--bg-hover`,
`--border`, `--text`, `--text-dim`, `--accent` (teal `#2dd4bf`), `--warn`,
`--error`, `--success`, `--radius`, `--transition`. `color-scheme: dark` is set
so native selects and scrollbars render dark. **Dark only; there is no light
theme.**

Flat, feature-prefixed class names (`.agent-*`, `.obs-*`, `.np-*`) over shared
primitives (`.btn`, `.input`, `.select`, `.tab`, `.panel-tabs`). Inline `style`
is used **only** for computed geometry — sidebar width, bottom height, tree
indentation, `Menu`'s position — and for data-driven colour, which is the one
thing a class cannot carry (`ScopeView`'s per-channel traces).

### Unavailable controls: disable and say why

Two idioms were in use for the same condition — some controls vanished when no
project was open, others greyed out — and a greyed-out Flash button did not say
whether it wanted a project, a port, or patience.

**The rule: disable, and put the reason in `title`.** Hiding teaches nothing;
a disabled control with a static description of the action teaches less than
nothing, because it looks like an answer. `gitStatus.syncDisabledReason` exists
because that cost us an afternoon in August 2026.

The reason is computed in the pure-logic tier — `syncDisabledReason`,
`publishBlockedReason`, `buildBlockedReason`, `retargetBlockedReason`,
`projectMenuItems`' `disabledReason` — so the rule is testable rather than a
habit. Where a pane shows the reason inline *and* on a button, suppress the
inline copy until the user has typed, but never the button's.

Hiding is still right for a control that would be *meaningless*, not merely
unavailable: the profile `＋`/`✎` pair and the git pill have nothing to refer
to without a project.

### Naming a port

`/dev/ttyACM0` is whichever board enumerated first, so it is the *least* stable
thing about a board and was the only thing most of the UI showed. One rule now,
in `ports.ts`, used by every surface that names a port:

**`portName` = `portTitle` · device path.** The title is the nickname the user
chose, then a board arduino-cli is *sure* about, then what the fleet remembers,
then an honest `USB serial bridge`.

Two rules inside that are easy to undo:

- **`confidentBoardName` is not `visibleBoard`.** A hidden sibling in
  `matching_boards` means the match was family-wide on USB vid/pid, so the
  non-hidden entry is an arbitrary member — which is how a plain ESP32-S3 was
  labelled "Ozobot DRVKit". `visibleBoard` still returns it, because an FQBN to
  *compile with* is a different question from a name to *read*. This mirrors
  `core::fleet::board_name`, which has always refused it.
- **A nickname is only used for a board confirmed online.** `last_port` is a
  memory, and the kernel hands `ttyACM0` to whatever is plugged in next.

`fleetDisplayName` mirrors `core::fleet::FleetEntry::display_name`. There were
three copies of that chain and they had drifted — one dropped `chip_type`, one
also dropped `board_name`, so "Forgot …" reported a bare MAC.

Machine-facing strings keep the bare address: `arduino-cli` argv, the MCP
tools, `esptool --port`. The Assistant is never given a port at all — it flashes
whatever the UI has selected.

### Accessibility

Maintained, and worth keeping: `role="separator"` with `aria-orientation` on
resize handles and inside menus, `role="alert"` on the conflict banner and
agent alarm, `:focus-visible` rules, and a `prefers-reduced-motion` block.

Two conventions that were practised but unwritten:

- **An icon-only button carries both `title` and `aria-label`.** `title` alone
  is not an accessible name in every AT/browser pairing. A button whose visible
  content includes real text does not need the label.
- **A button that opens a `Menu` also carries `aria-haspopup="menu"` and
  `aria-expanded`.** Right-click targets are exempt — there is no trigger
  element to annotate.

`Menu` defaults to `role="menu"`, which is only valid when **every** child is a
`menuitem`. A popover holding inputs must pass `role="group"` and an
`ariaLabel` — `GitPill` does, because its popover is a small form. Getting this
wrong is invisible on screen and makes the fields unreachable in menu-mode
navigation.

---

## See also

- [ipc-contract](ipc-contract.md) — the full command, event and channel surface
- [conventions](conventions.md) — the testing rules that produced this structure
- [data-flows](data-flows.md) — how a click reaches the backend and returns
