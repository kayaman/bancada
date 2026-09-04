# bancada 0.21.1

**0.21.0 let you create an ESP-IDF project and then refused to rename or
duplicate it.** Both operations were written when a project could only be a
sketch, and neither had been revisited when the second paradigm arrived. The
Project menu offered them anyway, so the first thing you could do to a
brand-new ESP-IDF project was hit this:

```
/home/you/Projects/blink_node is not a sketch folder —
expected /home/you/Projects/blink_node/blink_node.ino
```

An error about a missing `.ino` in a project that never had one, from a menu
item that should not have been reachable — or, better, should have worked.

## The fix

Both `rename_project` and `clone_project` now fork on `detect_kind`: the same
detection the toolbar, the Verify button and the build backend already use, so
a project is renamed by the rules of the paradigm it actually is.

Nothing downstream changed. Chat history, usage totals, fleet repointing and
the recents entry are all keyed by **path**, not by paradigm, so the entire
state-carrying half of a rename works for ESP-IDF projects untouched. That is
the dividend of `detect_kind` being the single place the question is answered.

### An ESP-IDF project's name lives in CMakeLists.txt

`project(<name>)` is to an ESP-IDF project what `<name>.ino` is to a sketch:
`<name>.elf` is built from it, so moving only the directory leaves a project
whose output is named after its old self. They move together.

The rewrite happens **before** the directory move, and is rolled back if the
move fails. That is the mirror of what the Arduino rename does — there, every
in-directory edit precedes the single irreversible step; here, the fallible
step goes first so a failure leaves the project exactly where it was. Same
principle, opposite conclusion, because the risky step is a different one.

`main/<name>.c` is deliberately **not** renamed. Unlike `arduino-cli`, which
only recognises `Foo/Foo.ino` as a sketch, ESP-IDF names its sources in
`main/CMakeLists.txt` and does not care what they are called.

### Duplicating drops what names the original

`build/`, the generated `sdkconfig`, `sdkconfig.old` and `managed_components`
all carry the source project's name inside them, so copying them across would
hand you stale artefacts under the wrong name. `sdkconfig.defaults` **is**
copied — it is hand-written, committed, and carries both the target and the
board marker, which is exactly what should survive a duplicate. `.git` is
skipped for the reason the Arduino clone skips it: a copy gets a fresh
repository, never the original's history.

## The dialog was describing an operation that would not happen

Two smaller things, both in the Rename pane, both the same kind of mistake.

`checkProjectName` applied Arduino's rules to every project, so `2fast` and
`my.app` passed the pane and failed in the backend — a round trip ending in an
error the pane could have shown instantly. It now takes the project kind: no
leading digit or `-` for a CMake name, no `.`, and a 64-character limit rather
than arduino-lint's 63.

And the pane *promised* `old.ino → new.ino` for every project. For an ESP-IDF
one, no source file is renamed at all. It now says what will actually happen:

```
Becomes   /home/you/Projects/new_node
project(new_node) in CMakeLists.txt — no source file is renamed
```

Showing the wrong thing is worse than showing less.

`DuplicateProject` needed no change: it does no client-side validation and
relies on the backend, which now forks correctly.

## Tests

```
cargo test --workspace   739 core · 84 src-tauri, 0 failed
npm test                 1266 passed, 78 files
tsc --noEmit             clean
vite build               clean
```

Run the Rust suite with `--test-threads=1`: `cli::tests` has a known parallel
flake that is not a real failure.

## Unchanged from 0.21.0

The bench passes owed by 0.19.0, 0.20.0 and 0.21.0 are all still owed. Nothing
in this release has been on hardware either — though rename and duplicate are
filesystem operations with unit tests covering the failure paths (a rewrite
that fails moves nothing; an occupied destination is refused with the source
untouched), which is the level of proof they need.
