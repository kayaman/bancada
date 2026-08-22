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
│ Build · Serial │ Scope │ MQTT · WS · Web │ Assistant               │
├─ statusbar ─ toasts overlay · activity · elapsed · progress ───────┤
└────────────────────────────────────────────────────────────────────┘
```

---

## 1. The shell

`src/App.tsx` is 3,128 lines and is the de-facto orchestrator: it owns nearly
all cross-panel state and registers every backend subscription. Its render tree
begins around line 2708.

Six composition rules are encoded there and are easy to break by accident:

**One project affordance.** `📁 <name> ▾` (`ProjectMenu.tsx`) names the open
project and holds every action on it — Open, Recent, New, Duplicate, Rename.
It exists because the bar had drifted into six project controls and three
glyphs each meaning two things. What it offers, and what is disabled and why,
lives in `src/toolbarModel.ts`. `ProjectMenu` is a leaf component now within
reach of the `.tsx` harness (conventions.md §2), so what it renders disabled,
and why, can be asserted directly rather than verified by eye.

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

**Tabs: two levels in the sidebar, one flat row at the bottom.** The sidebar
has a Software/Hardware group switcher over per-group sub-tabs, and **each
group remembers its last-used tab**. The bottom panel does not: it is a single
row of seven tabs, `Build · Serial │ Scope │ MQTT · WS · Web │ Assistant`,
with thin separators where the old group boundaries were. Order, labels,
separators and the per-tab dot/badge view model are data-driven from
`src/bottomTabs.ts` (`BOTTOM_TABS`, `TAB_LABEL`, `SEPARATOR_AFTER`, `tabRow`)
and rendered by `BottomTabBar.tsx` — add a tab there, not in JSX.

**Hide, don't unmount.** Panels holding a live connection — scope, MQTT, WS,
device browser, agent — latch mounted on first open (`scopeMounted`,
`mqttMounted`, …) and are thereafter hidden with `display:none`. Unmounting them
would drop a socket, a channel or an agent session on a tab switch. The sidebar
and `.main` use the same trick.

The one deliberate exception: `AgentPanel` is **keyed on `sketchDir`**, so
switching project does hard-reset its panel-local state.

**Two announcement channels, and they do not mix.** An *outcome* is a toast —
`notify(msg, isError?)`, whose signature has not changed; it reduces into
`toasts` (`notifications.ts`) and renders in `ToastStack`. What is *running* is
the status bar: one activity at a time (label + elapsed + progress), begun with
`beginActivity` and closed with `endActivity`. The bar had a single text slot
before, so App.tsx used to concatenate the outcome onto the in-progress
message; that workaround is gone, and putting two messages in one string is now
a bug, not a style. `endActivity` takes the key it expects to close, so a
straggling agent event cannot end the user's build.

**Layout prefs go to `localStorage`, not settings.** `bancada.bottomHeight`,
`bancada.sidebarWidth`, `bancada.sidebarCollapsed`, `bancada.buildDurations`
(per-project compile/upload times, which only feed the "usually ~" hint and the
estimate bar), `bancada.serial.baud` and `bancada.serial.ui`. They are
per-machine window furniture, not project state.

`bancada.serial.baud` is the one that looks like project state and is not. The
sketch's own `Serial.begin` is the project's truth about baud — it is in git,
it travels with the code — so the picker follows it by default and the stored
per-sketch override is bench furniture: what *this* machine was told to listen
at, until told otherwise.

That makes the displayed baud **derived**, and derived state can drift away
from a running child: switching project, or saving a sketch whose
`Serial.begin` changed, moves it under a monitor still reading at the old rate.
`openBaudRef` holds what the live child was actually opened with — it drives
the toolbar while a monitor is up, and an effect re-opens the port when the two
disagree. Do not display `baudrate` there; a monitor decoding at a rate the UI
denies looks like a broken board.

---

## 2. State — three tiers, no global store

The absence of a global store is deliberate. State lives at the tier that
matches its update frequency.

### Tier 1 — `App.tsx` local state, passed by props

The majority: `sketchDir`, `profile`, open file and buffers, dirty set, open
tabs, ports and selection, build lines, monitor flags, layout, `busy`,
conflicts.

Announcements are four fields, not one status string: `toasts`, `activity`,
`lastResult`, `progress`. Baud is three: `baudOverrides` (persisted per sketch
dir), `sketchBaud` (sniffed out of the sketch), and the effective `baudrate` +
`baudSource` that `effectiveBaud` derives from them each render.

**There is no `serialLines`.** The serial feed is a Tier 3 store (below); only
the build feed is state.

Shared downward **by props only** — which is why `<Toolbar>` takes ~30 of them.

Heavy use of **ref mirrors** (`sketchDirRef`, `openFileRef`, `monitorOnRef`,
`busyRef`, `bottomTabRef`, `uploadsArmedRef`, `activityRef`, `progressRef`,
`monitorSessionRef`, `openBaudRef`, …) because the subscription effect is
registered once with `[]` deps and must read current values without
re-subscribing. Two refs point
at the editor rather than at data: `editorRef` (the `ReactCodeMirrorRef`) and
`pendingGotoRef`, the diagnostic jump parked while the file opens.

**The build feed is state, and stays state.** `onBuildLine` pushes into a ref
and schedules one rAF flush, so a chatty compile commits once a frame instead
of once a line. It is not Tier 3 because it does not need to be: build output
is human-paced, capped at 5,000 lines, and `parseBuildOutput` has to see the
whole buffer anyway. Serial is the opposite case, and gets the opposite
answer.

### Tier 2 — one zustand store

`src/explorerStore.ts` (`useExplorerStore`), scoped strictly to *file-tree UI
state*: files, expanded, selected, renaming, creating, dragging, drop target,
context menu.

It **never calls `api`**. App handlers perform the filesystem operation and feed
the new listing back via `setFiles` / `setFilesAfterRename`. Keep it that way —
the store is a view model, not a controller.

### Tier 3 — plain classes, polled

`AgentStore` and `SerialStore` (both App-owned singletons) and `ObsStore` (one
per MQTT/WS/Web panel), plus `ScopeEngine`. All expose a monotonic `version`;
components **poll** it rather than subscribe — 100 ms for the agent panel,
10 Hz for the Serial Monitor, ~4 Hz for observability, 250 ms for scope
readouts.

This is the important one. These ingest at rates React should never see: a
50 kSa/s ADC stream, a chatty MQTT topic, token-level streaming deltas, a board
printing at 921600 baud. Polling a version counter decouples ingest rate from
render rate. **Do not "fix" this into `useState`.**

`SerialStore` is the newest of them and shows the shape end to end: the
`serial://line` listener only calls `push`; the store caps at 5,000 rows and
trims to 4,000 in one `splice` (amortised, not a `shift` per line); pausing
takes a copy rather than a sequence watermark, because the ring keeps evicting
behind a pause and a watermark would empty the very screen the user paused to
read; and `SerialMonitor` renders only the rows in view. Being module-level
rather than panel-local is deliberate — lines arrive while the tab is hidden
(the recapture ladder, the agent's `serial_read`), so neither the scrollback
nor the unseen dot may depend on a panel having been mounted.

There are **no custom hooks** in the codebase. `useExplorerStore` is the only
`use*` export.

---

## 3. The pure-logic tier

`vitest` runs in the **node** environment by default; a `.tsx` test can opt
into jsdom per file to render a **leaf component**, but `.ts` stays where the
logic lives ([conventions §2](conventions.md#typescript-node-by-default-jsdom-per-file)).
So anything worth testing has been extracted into a plain `.ts` module — a
`.tsx` test proves a component's wiring and accessibility, not its logic:

| Module | Owns |
|---|---|
| `api.ts` | the entire IPC surface (§4) |
| `bottomTabs.ts` | bottom panel tab order, labels, separators, and the per-tab dot/badge view model |
| `notifications.ts` | the toast reducer: per-kind TTL, dedupe against the newest, the cap that never drops an error or the toast just pushed |
| `statusLine.ts` | the status bar's whole vocabulary, and which of the four progress modes applies |
| `buildProgress.ts` | esptool/avrdude output → a fraction, or honestly `null` |
| `buildHistory.ts` | remembered per-project compile/upload durations, and the estimate derived from them |
| `diagnostics.ts` | build output → diagnostics, memory summary, summary label, jump targets |
| `editorGoto.ts` | line/column → a CodeMirror document position, clamped both ways |
| `serialPrefs.ts` | the baud list, the line-ending vocabulary, the sketch-baud sniffer, and which baud wins |
| `timeFormat.ts` | `hms` for a log stamp, `fileStamp` for an export filename |
| `editorTabs.ts` | open/close/rename/delete tab transitions; dirty tabs are never bulk-closed |
| `explorerOps.ts` | rename/delete path math, protected paths (mirrors core's `is_protected`) |
| `fileTreeModel.ts` | tree building, visible nodes, expansion pruning |
| `newFile.ts` | friendly filename pre-validation |
| `conflicts.ts` | the agent/user edit-conflict guard used before send, verify and upload |
| `ports.ts` | **how a port is named**, visible boards, flash-target mismatch, port options |
| `portWatch.ts` | hotplug arrival diffing |
| `gitStatus.ts` | the git pill's entire vocabulary, incl. why flashes are untagged |
| `boardOffer.ts` | whether to offer a plugged-in board's project back, and which |
| `boardOptions.ts` | composing and parsing an FQBN's board options; the silent-serial warning |
| `monitorRecovery.ts` | when to re-take the serial port after capture is lost |
| `check.ts` | the one `{ok} \| {ok:false, reason}` shape, and `reasonOf` |
| `publishRepo.ts` | repo-name rules and why publishing is blocked |
| `projectRename.ts` | project-name rules (mirrors `core::project::validate_project_name`) and the rename plan |
| `toolbarModel.ts` | the project button's label; what the project menu offers, and what is disabled and why |
| `boardSearch.ts` | board filtering and grouping |
| `profileInit.ts` | profile form modes and submit plans |
| `usageDashboard.ts` | totals and display names |
| `keys.ts` | accelerator parsing and matching; Ctrl and Cmd are one modifier |

Plus `src/scope/`, `src/agent/`, `src/obs/` and `src/serial/`, which are
subsystems in their own right (§5).

**The rule for new work:** if you are about to put logic in a `.tsx` file, put
it in a module and call it from the component. Two components already export a
pure helper for exactly this reason — `BoardPicker.fallbackFqbnLabel` and
`DeviceBrowserPanel.exchangeRow`.

Injected clocks are used throughout for the same testability reason:
`ObsStore.push(ts)`, `chatFileName(now)`, the resume-watch timeouts. **`App.tsx`
owns "now".**

---

## 4. The IPC layer

`src/api.ts` (999 lines) is the only file importing `@tauri-apps/api/core` or
`/event`. Elsewhere only `plugin-dialog` and `getVersion` appear.

It holds the TypeScript mirrors of the Rust types, 96 `invoke` wrappers, the
three `Channel` openers, and the seven `listen` subscriptions. Full surface:
[ipc-contract](ipc-contract.md).

**Where subscriptions live:** almost all in a single empty-dep effect in
`App.tsx` — build, serial ×3, ports hotplug (with 500 ms coalescing and
deferral during a flash), agent event and agent closed. The only subscription
outside `App.tsx` is `onSerialLine` inside `ScopeView` for the plotter source.

The serial trio is session-guarded: `startMonitor` returns the monitor's
session id, `serial://started` and `serial://closed` carry one, and App keeps
the current id in `monitorSessionRef` so a reader thread that outlived its own
child cannot report a live monitor as closed. A `null` ref still accepts any
close — that is the case where the backend started the monitor before this
window knew about it.

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

### `src/serial/` — the Serial Monitor's plumbing

`serialStore.ts` (the Tier 3 store of §2: capped ring, amortised trim, frozen
pause view, `rows` memoised per (version, filter), plus `filterRows` and
`exportText`), `txHistory.ts` (shell-style ↑/↓ recall, immutable, and it saves
the half-typed draft before recalling over it), `virtualize.ts` (`visibleRange`
/ `isNearBottom`; `ROW_HEIGHT` is pinned to the `--serial-row-h` custom
property, so the two must move together).

No React and no wall clock in any of them: timestamps arrive with `push`, the
same contract `ObsStore` keeps. `SerialMonitor.tsx` is the only consumer.

---

## 6. Styling

One stylesheet: `src/styles.css`, 3,152 lines, imported once. No CSS modules, no
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

The announcement surfaces added with the toasts and the status bar have rules
of their own:

- **A toast is `role="alert"` when it is an error, `role="status"` otherwise.**
  An error stays until dismissed, so it may interrupt; a success that vanishes
  in four seconds may not.
- **A live region is mounted before it has anything to say.** The status bar's
  progress element and the Build console's summary strip are always in the
  DOM — a region that appears *with* its first message is not reliably
  announced. Only the text comes and goes.
- **`aria-valuenow` only when the number is real.** The status bar's
  `role="progressbar"` always carries `aria-valuemin`/`max` and a label, but
  `aria-valuenow` appears only in *measured* mode; an estimate says
  `aria-valuetext="estimated"` instead, and an indeterminate bar says neither.
  A guessed percentage announced as a fact is worse than no number.
- **A clickable log row is a `<button>`.** The Build console's jumpable
  diagnostics are real buttons with the absolute path in `title`, not `div`s
  with an `onClick`. The errors-only toggle carries `aria-pressed`, bound to
  what is actually filtered rather than to the raw state — so it cannot claim
  a filter that the absence of errors has already released.
- **Every Serial Monitor control has a name.** The two `<select>`s (baud, line
  ending) and the filter and send inputs carry `aria-label`; the toggles say
  what they do in `title`; the connection chip is a `role="status"`.

---

## 7. Known limitations

Not bugs to be fixed in passing — each is a deliberate stop, and knowing where
it is saves re-deriving it.

**Progress is honest before it is informative.** A *measured* bar needs
esptool's `Flash will be erased from … to …` announcements to weight the
regions; a recipe that suppresses them leaves `fraction` null and the bar
indeterminate, with only the per-segment `Writing … (62 %)` available as text.
avrdude through a pipe prints no percentage at all — its progress is phase
only (`Writing flash` → `Verifying` → `Done`). A plain compile has nothing to
measure either. So the only measured bar in the app is an esptool flash that
announces its regions; everything else with a remembered duration gets the
dashed *estimate* bar, which is a guess and says so.

**Not every diagnostic is clickable, and that is the design.** A jump target
must relativise to the sketch directory *and* name a file the explorer lists.
Two consequences: a diagnostic in a core header or a library keeps its
shortened path and stays inert, and so does one gcc reports against the
generated prologue (`/tmp/arduino/sketches/<hash>/sketch/X.ino.cpp`), which is
outside the sketch. Linker errors carry no location by design, even though
avr-gcc's `undefined reference` line does have a usable one.

**`detectBaud` is best-effort.** It strips comments, resolves one level of
`#define`/`const`/`constexpr`, and takes a single agreed `Serial.begin` as the
answer — but it is not string-literal aware, so a `//` inside a string reads as
a comment start. A wrong guess costs one click on the baud picker, which is why
it is not worth a C tokenizer. `Serial1`/`Serial2` deliberately do not vote.

**A baud override is keyed by absolute sketch directory.** Moving or renaming a
project orphans its override — the sketch's own rate takes over, which is the
benign direction — and leaves a dead key behind. Nothing collects them.

---

## See also

- [ipc-contract](ipc-contract.md) — the full command, event and channel surface
- [conventions](conventions.md) — the testing rules that produced this structure
- [data-flows](data-flows.md) — how a click reaches the backend and returns
