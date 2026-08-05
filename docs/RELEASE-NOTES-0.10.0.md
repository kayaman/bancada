# bancada 0.10.0

Merges the 0.9.0 file explorer into main and ships a day of workbench and
Assistant improvements on top.

## Bottom panel

- **Two-row hierarchy**: group buttons (⚙ Console · 🐞 Debugging ·
  📡 Observability · 🤖 Assistant) over the active group's sub-tabs, the
  same pattern as the sidebar. Header height is constant across groups;
  single-tab groups show a clean empty row instead of a duplicate button.
- **Flashing opens the Serial Monitor**: a successful flash switches to
  the monitor immediately and resumes capture after re-enumeration — a
  silent sketch no longer hides behind the Build console.
- **The Serial Monitor tab is a standing request for capture**: showing
  it (re)starts reading if the monitor is off; an explicit Stop while on
  the tab is respected.

## Port recognition

- The hotplug watcher keys ports by **identity** (name + vid:pid:serial),
  not name alone — swapping a DevKit's USB-C connectors, which reuses
  `/dev/ttyACM0`, is now detected instead of silently keeping the old
  board's identity.
- Hidden umbrella FQBNs (`esp32_family`) are no longer handed to the
  compiler or the toolbar; the port picker never renders blank for a
  pinned-but-absent port; bridge boards (CH343/CP210x) announce their
  arrival; a deferred rescan can no longer be lost for good.

## Assistant

- **Live activity strip**: the footer shows what the agent is doing right
  now — `🔨 verify (compiling)…`, `⚙ Edit soil.ino…`, `✍ writing…`.
- **Usage on every interaction**: each turn ends with a ledger divider
  (turns · tokens in/out · cost) whose *details* opens a turn summary —
  final summary rendered as markdown, usage grid, tools run. The footer
  keeps a Σ session total (per-call semantics verified against the Agent
  SDK docs, so nothing double-counts).
- **Markdown rendering** in assistant replies (GFM; raw HTML stays
  escaped).
- **Chat history**: every chat is saved automatically per sketch as a
  store-operation log and replayed read-only from 🕘 History —
  pixel-identical to the live transcript, delete per row, newest 50
  kept. Tool results persist verbatim under the app config dir; see the
  spec's retention note.
- **Project totals**: History leads with what the Assistant has cost the
  open sketch overall — chats, Σ cost, tokens in/out, turns, last chat.

## File explorer (merged from 0.9.0)

The 0.9.0 release's explorer lands on main: tree state in a store,
rename/delete/new-folder operations with disk-coherent refreshes.

## Under the hood

- ModemManager bench guidance: a udev `ID_MM_DEVICE_IGNORE` rule stops
  4-second AT-probes on every board enumeration (see the memory notes in
  the repo history; the rule itself is host configuration).
- Suites at 365 vitest + 373 cargo tests.

No bundle identifier change, so saved state stays where it is.
