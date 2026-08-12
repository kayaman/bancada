# bancada 0.16.0

Projects get a life cycle — rename them, duplicate them, publish them — and
every flash now leaves a record of what actually reached the board. The
toolbar has been rebuilt around a single project affordance, and a board is
finally called by the name you gave it rather than by whichever `/dev/tty…`
the kernel happened to hand out.

## Rename a project

A project's name used to be permanent. Arduino binds the folder name to the
main `.ino` basename, and the file explorer refuses to rename that file on
purpose — so the only way out was to duplicate under a new name and abandon
the original, losing its history, its Assistant chats and its cost record.

**Project ▸ Rename** moves the folder and its main `.ino` together, and
carries across everything keyed to the old path: the chat transcripts, the
recorded token spend, the recent-projects entry. It shows you exactly what
will move before you commit to it.

It is refused while an Assistant session is live. That session pins the old
path in four places that cannot be rewritten after it starts — its working
directory, its system prompt, its permission rules and its guard hook — and
while containment does still fail closed, the layer that has to hold even
when hooks are switched off would be anchoring a directory that no longer
exists. Stop the session first; the message says so.

## Publish to GitHub, properly

The git pill could already create a repository, but only a private one, named
only, and only if the project was already under git.

Now it asks for visibility and a description, and initializes a repository
first when the project has none — publishing an unversioned sketch is one
action rather than two trips. It **refuses** to publish publicly when a
credential file is already tracked, and names the file: `.gitignore` does not
untrack what is already in the index.

## Every flash leaves a trace

A flash is the moment source becomes hardware, and it used to leave nothing
behind. Days later the only way to answer "what is running on this board?"
was to remember.

Now a successful flash checkpoints the project and writes an annotated
`flash/2026-08-12T1430` tag recording the port, the profile or FQBN, the board
and the Bancada version — then pushes it. One tag per distinct code state, not
one per flash, so an afternoon of iterating leaves a readable history rather
than a log.

Nothing in that path can fail a flash. Every step reports to the Build console
and gets out of the way — the push especially, because a bench is often
offline.

The agent's flashes are tagged the same way. The record is of what reached the
board, not of who sent it.

## A toolbar with one job per group

The project group had grown to six controls, and three glyphs each meant two
different things — `＋` was New Project, Create profile *and* add profile.

`📁 <name> ▾` is now the single project affordance: it names what is open and
holds Open, Recent, New, Duplicate and Rename. Usage moved to the right, where
it belongs — it reports on every project, not the open one, whatever its old
tooltip claimed.

**Clone is now Duplicate**, everywhere you read it.

The bar also could not survive a narrow window. With no wrapping and no
scrolling, Verify and Flash were pushed past the right edge and clipped, with
no way to reach them — a long project name alone could do it. They are now the
last things to yield, and a long name ellipsises instead.

## Boards are called what you call them

`/dev/ttyACM0` is whichever board enumerated first: the least stable thing
about a board, and the only thing most of the app showed. Five different
naming conventions had grown up around it.

There is one now — **name first, path second** — and the name is the nickname
you gave the board. The Fleet panel always knew which physical board was on
which port; nothing used it. Two ESP32 boards it correctly called *bench
probe* and *mesh node 3* both appeared in the picker as the same string.

That string was also wrong. When arduino-cli matches on a family-wide USB
vid/pid it offers an umbrella entry plus an arbitrary member of the family, so
a plain ESP32-S3 dev board was confidently labelled **"Ozobot DRVKit"**. The
fleet registry has refused that for as long as it has existed — being wrong is
worse than being silent when the name is what tells two boards apart — and the
toolbar was the last place still printing it.

## Fixes

- **The usage dashboard drilled into the wrong key.** A project's chat history
  was addressed by re-hashing a *display* path recovered from old transcripts.
  After a rename, deleting `usage.json` made every row expand to an empty
  session list with un-openable chats, silently. Rows now carry the key they
  are stored under.
- **New Project could run twice.** Enter in the name field bypassed the
  disabled button, so two quick presses fired two creations at the same path.
- **The git pill's popover was announced as a menu** while containing text
  inputs, so screen readers entered menu navigation and skipped the fields.
- Verify, Flash and the profile board-change button were disabled without
  saying why. They say why now.
- Nine icon-only buttons were missing an accessible name.

## Under the hood

Four modules had grown their own copy of the same "is this allowed, and if not
why" type — two of them colliding on the name. They share one now.

The docs assert a lot of numbers, and numbers rot: eleven were wrong, including
five module line counts. The `build_gate` comment claimed to serialise "the
three sketch build paths" while the MCP upload tool had made it four and
`rename_project` made it five.

Two conventions that were practised but never written down now are: when to
disable a control rather than hide it, and the ARIA contract for anything that
opens a menu.

## Notes

Verified on this cut: `cargo test --workspace` 623 passed, `vitest` 627
passed, `tsc --noEmit` clean, and **every** opt-in suite green against a real
`arduino-cli` and an attached ESP32 — `core_list_real`, `gh_fetch`,
`fleet_real`, `scaffold_compiles`, `new_project_builds`.

Not run: `agent_live`, which spends tokens, and the manual
[hardware smoke tests](hardware-smoke-tests.md). The flash-tagging path in
particular has been proven by its unit tests and by reading, not by flashing a
board and inspecting the resulting tag — that is the check worth doing first
on this cut.
