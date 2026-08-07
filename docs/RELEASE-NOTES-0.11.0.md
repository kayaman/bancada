# bancada 0.11.0

Bancada gets its face and its memory: the new brand mark lands in the app,
projects can be cloned, and the workbench remembers where you've been.

## Brand

- **New mark everywhere**: the redrawn bench + signal-pulse logo (teal
  benchtop, amber lamp) ships as SVG masters with a full icon set — window,
  taskbar, and launcher icons, plus a favicon for browser dev.
- **In-app lockup**: the toolbar now leads with the mark, a "bancada"
  wordmark, and the app version.
- **Top-panel polish**: controls grouped as project | board | build with
  thin separators, the port picker and rescan button paired, proper
  tooltips and screen-reader labels, and the toolbar height pinned.

## Projects

- **⧉ Clone**: copy any local sketch into a new project — pick source,
  name, and location. The clone is built next to its destination and lands
  in one atomic rename; it gets a fresh git repo with a credentials-safe
  `.gitignore` before any file can ever be committed. The main `.ino` is
  renamed to match, library paths in `sketch.yaml` are re-pointed so the
  copy still finds every library (byte-faithful copy — comments, custom
  keys, and profile order survive), symlinks are recreated rather than
  followed, and anything non-fatal surfaces as a warning instead of
  failing the clone.
- **Recent projects**: a ▾ next to the Open button lists the ten most
  recently opened projects, newest first — click to reopen. Dead entries
  prune themselves. **Ctrl+O** opens the folder picker; **Ctrl+S** still
  saves (now also with CapsLock on).
- **Settings can no longer eat each other**: a long-standing bug where any
  settings write silently erased the fields it didn't mention (opening a
  file wiped the remembered New-Project location) is fixed at the root —
  the overwrite API is gone, replaced by narrow, serialized mutations.

## Assistant

- **Tokens and cost are scoped per project**: switching or creating a
  project is a hard session boundary — the Σ chip, transcript, and chat
  recording all start fresh, and a brand-new project shows zero. Chats
  keep landing in the right project's history, and a switched-away chat
  replays with an honest "Session ended — project switched" line.

## Under the hood

- Reusable popover menu extracted from the file-tree context menu (which
  behaves exactly as before).
- Suites at 411 vitest + 462 cargo tests.

No bundle identifier change, so saved state stays where it is.
