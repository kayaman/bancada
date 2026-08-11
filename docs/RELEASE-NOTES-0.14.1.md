# bancada 0.14.1

A fixes-only release: a three-lens bug hunt over everything shipped in
0.12–0.14 (session lifecycle, UI state, Rust core and the git feature),
every finding fixed and adversarially re-reviewed. Nothing new to learn —
things that could bite quietly now don't.

## Data that stopped disappearing

- **sketch.yaml keys Bancada doesn't model survive every profile
  operation.** `arduino-cli board attach`'s `default_port`/`default_fqbn`,
  profile-level `programmer:`, `port_config:` and friends used to be
  silently deleted by any add/retarget/library change. They round-trip
  verbatim now.
- **Continuing an old chat no longer marks it for deletion.** The 50-chat
  cap pruned by filename, so a just-continued old chat (old name, newest
  content) was the first thing deleted. Pruning ranks by recency now.
- **Checkpoint commits keep your identity.** Your configured git
  name/email authors your checkpoints; Bancada's built-in identity is only
  a fallback so checkpointing still works on machines with none — and
  gpg-signing/hooks can no longer hang or abort a checkpoint.

## Assistant lifecycle

- **Stop is just stop**: interrupting a session that was still resuming
  used to respawn it and re-send the prompt. Never again.
- Rapid continue/new-session/project-switch sequences can no longer merge
  two chats' transcripts, resurrect a discarded one, bind a session to the
  wrong project, or record a dying session's last words into another
  chat's file.
- The upload-arm switch and the backend can no longer disagree in either
  direction — a continued session always starts disarmed, in both places.
- Renaming or deleting a file with an unresolved assistant conflict now
  carries the conflict along (rename) or clears it (delete), instead of
  clobbering the assistant's fix or wedging the panel behind a banner that
  named a file that no longer existed.

## Smaller but visible

- ✎ (change board) on a profile whose FQBN carries options — like the
  ESP32-C6's `CDCOnBoot=cdc` — shows the board instead of a blank picker.
- Non-ASCII filenames (`leitura-térmica.ino`) appear correctly in the git
  pill and checkpoint messages instead of C-quoted escapes, and tracked
  secrets with accented paths are flagged again.
- The usage dashboard can no longer show one project's sessions under
  another when you expand two projects quickly.
- Switching the toolbar profile while the board form is open resets the
  form to the newly selected profile.
