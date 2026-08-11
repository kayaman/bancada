# bancada 0.13.0

Assistant sessions stop being disposable. History was already there to read —
now any saved chat can be **continued**: the conversation picks up with its
past in context, natively when the CLI still remembers it, and from a
distilled summary when it doesn't. Either way it's the same chat file, the
same transcript, one History entry.

## Continue an AI session

- **▶ Continue this chat** — on every History row and in the replay view's
  back bar. The old transcript loads into the live panel, and your next
  message resumes the conversation instead of starting from zero.
- **Native resume first**: the panel re-attaches to the CLI's own session
  (`--resume` with the session id every saved chat already recorded), so the
  assistant remembers everything — decisions, file contents it read, the
  reasoning behind them — at full fidelity and zero extra cost.
- **Automatic fallback**: when the CLI's transcript is gone (cleaned up,
  different machine), the panel silently starts a fresh session carrying a
  bounded facts block distilled from the saved chat — your recent requests,
  the last answer, files touched, the last build/upload outcome. No error to
  dismiss, no flapping; the first reply just arrives with the context it
  could recover.
- **One chat, one file**: a continued session appends to the same history
  file it came from. History keeps showing a single conversation, and the
  usage ledger keeps counting into the same totals.

## Guard rails

- Upload arming **never** survives into a continued session — the arm
  switch starts OFF, every time, and the UI can't drift from the backend
  even if you flip it mid-continue.
- Confinement is per-spawn: a resumed session gets the same fresh settings,
  deny rules, and pre-tool guard as a new one. Restored context restores
  memory, not permissions.
- Continuing while a session is running behaves like "New session": the old
  one is closed cleanly (and its chat file gets its closing record) before
  the continuation starts. Rapid continue/new/switch sequences can't leak an
  orphan CLI process or write into the wrong chat — every respawn re-checks
  that its moment hasn't passed.

## Fixes

- A session id recorded in an old chat is validated before it ever reaches
  the CLI's command line, and a malformed one degrades to the facts
  fallback instead of failing the continue.
