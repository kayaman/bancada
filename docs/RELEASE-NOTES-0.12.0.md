# bancada 0.12.0

The Assistant closes the hardware loop — it can now flash the board (behind
a per-session arm switch), watch and type to the serial monitor, and reach
the web. And the workbench learns what it costs: every project's Assistant
spend is now banked permanently and browsable on a dashboard, profiles can
switch boards without leaving the toolbar, and the editor grows real tabs.

## Usage dashboard

- **📊 Usage in the toolbar**: a full-screen dashboard of Assistant token
  usage (in/out) and cost for **every** project, not just the open one —
  grand totals up top, one row per project with cost, tokens, turns,
  sessions, and last activity, sorted by spend.
- **Totals survive everything**: usage is banked into a new cumulative
  `usage.json` the moment each result lands, so pruning old chats (the
  50-per-project cap) or deleting them from History no longer erases what
  they cost. Existing chat logs are folded in once, automatically, on
  first contact.
- **Links to individual sessions**: expand a project to see each saved
  chat with its own cost and token counts; click one and it replays
  read-only right inside the dashboard — whatever project is currently
  open. Sessions whose files were pruned are honestly reported ("N older
  sessions pruned — still counted in the totals above") instead of faked.
- A corrupt usage record is refused, never silently emptied — the same
  protection the board fleet file already had — and a failed usage write
  can never break chat recording itself.

## Profiles and boards

- **＋ / ✎ in the toolbar**: add a profile for another board (libraries
  copied, platform pinned) or retarget the selected profile's board in
  place — no more hand-editing `sketch.yaml`. Retargeting keeps board
  options when the board is unchanged and refuses blank profile names.
- **Board picker opens up**: search results render as a proper listbox
  overlay instead of a cramped select.
- **Port/board mismatch warning**: flashing a profile at a board that
  isn't its target now warns first.
- **Board-required libraries are pinned** into new profiles (the Uno Q's
  RouterBridge stack, notably), so hermetic profile builds stop failing.

## Templates

- **Starter picker**: new projects can start from i2c-scan, wifi-scan, or
  board-info instead of Blink.
- **i2c-scan compiles everywhere**: the template's runtime pin override is
  now guarded to ESP32 cores and its output uses the portable Print API,
  so a project retargeted to an Uno or Uno Q still builds. Found the hard
  way; pinned by a template test.

## Editor

- **Tabs**: open files get a real tab strip — close, close-others,
  close-all, dirty markers, and a close-again-to-discard arm for unsaved
  tabs. Saving disarms a pending close.

## AI Assistant: flash, serial, and web

The Assistant closes the loop: it can now fix → verify → **flash** → watch
the boot output, without leaving the panel.

- **`upload` tool** — flashes through the same `compile -u` path as the
  Upload button. Structurally scoped: no port argument (it targets the
  board selected in the UI, with the session's own profile/FQBN), shares
  the build gate with your Verify/Upload, and stops the monitor for the
  flash like the manual flow.
- **Allow uploads switch** — flashing starts OFF every session. Until you
  arm it (panel footer, 🔒→🔓), the upload tool is refused and the agent
  is told to ask you. New sessions always start disarmed.
- **`serial_read` / `serial_send` tools** — the agent reads monitor output
  it hasn't seen (bounded scrollback, never replays pre-session backlog;
  auto-starts the monitor on the UI-selected port/baud) and can type lines
  to the board like the Monitor tab's send box. The Monitor tab stays in
  sync either way.
- **Web access** — `WebFetch`/`WebSearch` are enabled. Deliberate
  trade-off, stated plainly in the README: reads were never confined, and
  web access is an egress path — data the agent reads can leave the
  machine. `Bash` remains structurally out, and the scope stays
  untouchable.
- New transcript cards for flash and serial activity, "Flashing…"/"📡
  upload" status, and the write-confinement model (deny rules, guard hook,
  drift alarm) unchanged and re-verified.
- **Debug view**: a 🐛 toggle shows every raw agent event (including
  shapes the store ignores), tool cards expand with full input/result
  detail, and the footer tracks the live turn with elapsed time and full
  paths.

Verified end to end against a live session (argv, init tool set, MCP round
trip) and on hardware: arm-toggle refusal, armed flash, and boot-banner
read all exercised at the bench.

## Under the hood

- Linux installables: 256px hicolor icon, desktop menu category, install
  docs.
- Suites at 490 vitest + 514 cargo tests.

No bundle identifier change, so saved state — including the new
`usage.json` — stays where it is.
