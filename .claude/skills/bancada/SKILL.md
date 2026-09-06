---
name: bancada
description: Working on the Bancada codebase — an Arduino/ESP-IDF bench workbench built with Tauri 2, Rust and React that drives arduino-cli, esptool, git, gh and claude as subprocesses. Use for any change in this repo: it gives the reading order for docs/architecture/, the four verification gates, the rules most easily broken from outside (layering, the single IPC module, lock order, serial ownership, agent containment), the commit and release ritual, and the documentation that is itself stale and must not be trusted.
---

# Working on Bancada

Bancada is a desktop Arduino workbench: editor, board and library managers,
serial monitor, software oscilloscope, observability panels, and an embedded
Claude Code assistant panel. Version and product story are in `README.md`.

**The defining principle** (`docs/architecture/system-context.md`): *Bancada
does not reimplement the toolchain — it drives the same engines the official
IDE uses.* Every capability spawns a binary resolved from `PATH` by bare name
(`arduino-cli`, `esptool`, `idf.py`, `git`, `gh`, `claude`) and parses its
`--json`. No libgit2, no Arduino SDK binding, no Anthropic SDK. A missing tool
is a first-class `Error::ToolMissing`, never a crash.

Two scope statements that are product law, not preference:

- **Bancada is a bench tool and stays one.** Fleet identity, firmware
  lifecycle and telemetry governance belong to the separate
  `bancada-platform` repo.
- **ESP-IDF support is build, flash, monitor and stops there.** No
  `menuconfig`, no component manager, no partition editor. Bancada reads two
  `sdkconfig` values and writes nothing back.

And an epistemic one: where a board pinout is not modelled, say **inferred**
or say nothing. *Unknown and safe are different answers.*

## The rules live in the docs — read them, don't guess

`docs/architecture/conventions.md` **is** this project's rules document.
Read it before your first change. Then, in this order:

| Read | For |
|---|---|
| `docs/architecture/README.md` | the layer map and "where does my code go?" |
| `docs/architecture/conventions.md` | layering, testing, docs, commits, releases, dependencies |
| `docs/architecture/data-flows.md` | seven end-to-end traces; the best single page |
| `docs/architecture/runtime-model.md` | threads, locks, the build gate, serial ownership |
| `docs/architecture/ipc-contract.md` | the frontend/Rust boundary |
| `docs/architecture/agent-safety.md` | the assistant's containment model |
| `docs/architecture/persistence.md`, `frontend.md`, `backend-modules.md` | as needed |

`docs/superpowers/` is **design history, not current truth**: `specs/` and
`plans/` are snapshots of intent at design time and are never revised after
implementation. `docs/architecture/` is the authority for how things work now.

There are no ADRs. The decision record is commit bodies, the specs' rejected
alternatives, and module rustdoc — `src-tauri/src/lib.rs` opens with a
151-line header doing the work a module tree would otherwise do. Rustdoc here
is a documentation surface, not API boilerplate.

## The loop

Tests first: write the test, watch it **FAIL**, implement, watch it **PASS**,
then commit. All four gates green after every task:

```bash
cargo test -p bancada-core --lib        # core unit suite
cargo check -p bancada                  # the Tauri layer compiles
npx tsc --noEmit && npx vitest run      # seconds
npm run build                           # tsc --noEmit && vite build
```

**Do not drop the fourth gate.** `vitest` never parses `styles.css`; an
unclosed CSS block once passed every other gate and only the bundler caught
it. There is **no CI and no linter** — these four commands are the whole
safety net, and `tsconfig` is strict with `noUnusedLocals`, so an unused
import is a hard error.

Slow and hardware-dependent suites are `#[ignore]`d on purpose so a plain
`cargo test` stays hermetic; each file's `//!` header gives its exact
invocation and its `BANCADA_*` env gate. Run them for a release, not per task.

## The rules easiest to break from outside

Each is documented; this is the short list of what an unfamiliar change
usually violates.

1. **Layering.** `bancada-core` has no Tauri and no UI. The test: *could this
   run in a unit test with no window, no runtime and no hardware?* Then it
   goes in `core`. Corollary: `settings`, `fleet`, `chatlog` and `usage` are
   path-agnostic and clock-free — the caller supplies the path *and* `now`.
   `src-tauri` is the only place that reads a clock or resolves a config dir.
2. **Logic never lives in a `.tsx`.** Extract to a `src/*.ts` module and call
   it, so it is testable.
3. **`src/api.ts` is the only frontend file importing
   `@tauri-apps/api/core` or `/event`.** (Other submodules are fine —
   `Toolbar.tsx` imports `/app` for `getVersion`.) Frontend camelCase and
   Rust snake_case mismatches fail *only at runtime*, so **adding a command
   means adding its case to `src/__tests__/api.test.ts`**.
4. **Serial has exactly one owner** — monitor or scope. Acquiring evicts the
   other, but `free_port_for_flash` refuses when the scope owns the port: a
   user's measurement is never killed for a flash.
5. **Lock order: `build_gate` (try-only) → `serial` → nothing.** `serial` is a
   leaf, held only across bounded-short operations and never across a compile,
   upload or wait. The MCP `serial_read` tool deliberately inverts this with a
   *try*-lock; converting it to a blocking `lock()` is a deadlock, not a
   slower version of the same thing.
6. **The build gate never queues.** Contention returns "build already in
   progress".
7. **Every `AppState` mutex uses `unwrap_or_else(|e| e.into_inner())`**, never
   `.unwrap()`. In the exit handler a poison panic would orphan the `claude`
   child and leak its 0600 temp files.
8. **Nothing auto-restarts** — not MQTT, not the agent child. Reconnection
   policy is the frontend's.
9. **An event that ends something carries what it is ending** (a session id or
   pid), so a straggler cannot kill a newer session.
10. **UI disables and says why in `title`**; it does not hide a merely
    unavailable control. The reason is computed in the pure-logic tier so it
    is testable. Hide only what would be meaningless.
11. **One stylesheet, `src/styles.css`.** No CSS modules, no Tailwind, no
    CSS-in-JS. Inline `style` only for computed geometry and data-driven
    colour.
12. **Dependencies are added reluctantly**, and the manifest comment
    explaining why is part of the change. No charting library, no icon pack,
    no date library. The FFT, the scope renderer and the unified diff are
    hand-written.

## Things that must change together

- **Agent containment**: `BUILTIN_TOOLS`, `EXPECTED_TOOLS` and the
  `--allowedTools` literal in `agent_args` change in the **same commit**.
  Policy is a pure, tested function in `core/src/agent.rs`, never inlined into
  `src-tauri`. Touching containment needs a new `path_is_confined` case.
  `docs/architecture/agent-safety.md` is the **single source** — the README
  and the spec point at it; keep it that way, because the model was once
  written three times and drifted.
- **Project rename** fans out: the folder, its main `.ino`, the chat-log key
  (`chatlog::rename_key`), the usage key (`UsageStore::rename_project_key`),
  the fleet pointer (`Fleet::repoint_project`) and the recents entry, in
  place. It is refused while an assistant session is live, and it takes the
  build gate.
- **Wire formats**: `docs/scope-architecture.md` is cited by section number
  from `core/src/scope.rs`, `src/scope/*.ts` and the firmware README.
  Documentation is part of that change.
- **Testability seams**: `src-tauri` is unit-testable only because it is
  written against an injectable `EmitFn` rather than calling
  `AppHandle::emit`. Preserve that seam.

## Tests

Rust unit tests sit beside the code in `#[cfg(test)] mod tests`. For a new
engine wrapper, copy the **stub-script trick** in `core/src/cli.rs`: a fake
`arduino-cli` on a temp `PATH`, so argv construction and parsing are tested
without the real engine. Captured toolchain output lives in
`core/src/testdata/` and is `include_str!`'d.

TypeScript tests live in a `__tests__/` directory beside the code, named
`<module>.test.ts`. A `.tsx` test opts into jsdom with
`// @vitest-environment jsdom` as its **first line** plus `afterEach(cleanup)`,
renders a **leaf component only**, and asserts on roles, labels and text
rather than class names — unless the class *is* the contract. `App.tsx` is
never rendered; its wiring is asserted by reading the file as source text,
which is a documented workaround, not a pattern to spread.

## Commits and releases

Conventional prefix, then a sentence describing **user-visible behaviour in
the present indicative** — the new state of the world from the user's seat,
not what you did:

```
feat: A Setup panel tells a new machine what it is missing, and installs arduino-cli
fix: The window no longer draws as garbage stripes on nouveau
```

Prefixes in use: `feat`, `fix`, `docs`, `test`, `style`, `refactor`, `polish`,
`chore`, with optional `(ui)` and `(core)` scopes. `conventions.md` says
lowercase; **recent commits capitalise the first word** — follow the shape and
match the surrounding history. Bodies are long and explanatory: the symptom,
the mechanism, why the alternative was rejected, how it was verified. Nothing
enforces any of this, so it is on you.

**Shared checkout: stage only the files you named. Never `git add -A`.** Other
sessions commit into this repo concurrently, and the tree is often dirty with
someone else's work. Re-check `git status` and `git log` before merging or
tagging. Never stage `.claude/worktrees/` — it is a full checkout with its own
`target/`.

Releases are a manual, ordered ritual with no script and no CI: bump the
version **by hand in four files** (`Cargo.toml` workspace, `package.json`,
`src-tauri/tauri.conf.json`, `Cargo.lock`), write
`docs/RELEASE-NOTES-X.Y.Z.md` in product-announcement voice, run the opt-in
suites, commit `Release X.Y.Z` touching exactly those files, and tag a
GPG-signed annotated `vX.Y.Z`. There is no `CHANGELOG.md`; the release notes
are the changelog. Full steps in `conventions.md` §5.

## Traps

- **Startup order in `run()` is load-bearing.** `handle_agent_guard_argv()`
  runs first and returns early (the binary re-invokes itself as the agent's
  `PreToolUse` hook), then `ensure_webkit_renderer_works()` and
  `ensure_user_bins_on_path()` run **before any thread exists** — the one
  moment `set_var` is sound. Anything added above `tauri::Builder` must
  respect that. The bad-renderer list is `nouveau` alone, deliberately.
- **ESP-IDF discovery never uses `IDF_PATH`.** A windowed app has no shell
  environment. It reads `~/.espressif/tools/eim_idf.json`, then
  `~/.espressif/idf-env.json`.
- **Ports are keyed by identity, not name** (`core::ports::port_key` = name +
  vid:pid:serial). `/dev/ttyACM0` is the least stable thing about a board.
  `confidentBoardName` is not `visibleBoard`: a hidden sibling in
  `matching_boards` means the match was family-wide on USB vid/pid, which is
  how an ESP32-S3 once got labelled "Ozobot DRVKit". Machine-facing strings
  keep the bare address.
- **Stopping the monitor means the port is free** only because the reader
  thread is joined. `kill_child` sends SIGTERM and waits before SIGKILL,
  because SIGKILL alone left a grandchild holding the tty.
- **Panels are hidden, never unmounted** — unmounting drops a live socket,
  channel or agent session on a tab switch.
- **Stores are polled, not subscribed**, via a monotonic `version`. Do not
  "fix" that into `useState`. `renderFrame` deliberately returns the same
  object every call; a test pins it.
- **Vite watch ignores `target/` and `.claude/`** — chokidar would otherwise
  exhaust inotify before serving a page. Add any new build-output directory to
  `vite.config.ts`.
- **Onboard LEDs are not portable.** S3 and C3 devkits usually have a WS2812
  where `digitalWrite` does nothing; the Blink starter branches on
  `RGB_BUILTIN` / `LED_BUILTIN`.

## Documentation that is stale — do not trust it

This codebase names stale comments as its own failure mode: *a count in prose
is a claim that rots silently; prefer naming the rule over counting the
callers.* Known drift, verified:

- **`docs/boards/*.md` are generated**, but their own instructions describe a
  sibling project. In this repo the source is `KNOWN_BOARDS` / `SOC_PINS` in
  `core/src/boardprofile.rs`, there is **no `bidf` binary and no regeneration
  command**, and the gate is `cargo test -p bancada-core boardprofile::`.
  Edit the table, never the page, then hand-reconcile the page to
  `render_markdown` output. `docs/boards/README.md` also cites a
  "conventions §7" that does not exist.
- **`docs/hardware-smoke-tests.md` is a plan, not an implementation.** The
  `core/tests/hardware.rs` and fixtures it describes do not exist. Its
  principles are still worth following for hardware work: opt-in never
  default, serial never parallel (`--test-threads=1` is not optional),
  self-verifying via a serial heartbeat rather than "watch the LED", and **the
  suite erases the board — use a dedicated spare**.
- **Counts drift constantly.** The command count is given as 96, 109 and 111
  in three different files; module counts disagree with `core/src/lib.rs`.
  Never cite a count; name the rule.
- **The release-notes index in `docs/README.md` stops at 0.19.0** while later
  notes exist on disk.
- **`coverage/` is committed to git** and is not ignored.
