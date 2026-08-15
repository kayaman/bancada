# Bancada architecture

How Bancada is put together, for someone about to change it.

Bancada is an Arduino workbench built with **Tauri 2 + Rust + React**. It does
not reimplement the toolchain — it drives `arduino-cli`, `esptool`, `git`, `gh`
and `claude` as subprocesses and parses their output.

Roughly 16k lines of Rust in `bancada-core`, a 6.7k-line Tauri layer, and 22k
lines of TypeScript.

---

## The layers

```
┌───────────────────────────────────────────────────────────────────────┐
│ React UI                                    src/                      │
│   App.tsx orchestrates · panels · CodeMirror · canvas instruments      │
│   pure-logic modules (src/*.ts) · scope/ · agent/ · obs/               │
└───────────────────────────────┬───────────────────────────────────────┘
                                │  src/api.ts — the ONLY IPC module
             103 invoke commands · 7 events · 3 Channels
┌───────────────────────────────┴───────────────────────────────────────┐
│ src-tauri  (crate `bancada`)                                          │
│   lib.rs — commands, AppState, threads, event emission                │
│   OWNS: processes · threads · mutexes · file paths · the clock        │
│   + a loopback MCP server the AI Assistant calls back into            │
└───────────────────────────────┬───────────────────────────────────────┘
                                │
┌───────────────────────────────┴───────────────────────────────────────┐
│ core  (crate `bancada-core`) — 24 modules, no Tauri, no UI            │
│   parsers · validators · policy · wire formats · argv builders        │
│   unit-testable headlessly; reusable from a CLI or another frontend   │
└───────────────────────────────┬───────────────────────────────────────┘
                                │  subprocesses (all resolved from PATH)
        arduino-cli · esptool · git · gh · claude
                       + optional Mouser HTTPS API
                                │
                          the board on the bench
```

---

## Where does my code go?

The question this doc set exists to answer. In order — take the first match.

| If it… | It goes in | Because |
|---|---|---|
| parses, validates, decides, or builds an argv | `core/src/<module>.rs` | it must be unit-testable with no window and no hardware |
| owns a process, a thread, a mutex, a file path, or the clock | `src-tauri/src/lib.rs` | `core` may not hold long-lived state |
| is frontend logic you could test without React | a `src/*.ts` module | vitest runs in **node** — no component is ever rendered |
| crosses the IPC boundary | `src/api.ts` + a case in `api.test.ts` | a camelCase/snake_case typo fails only at runtime |
| is a wire format | a `core` module **and** a doc section | protocols are documented as contracts here |
| is genuinely only rendering | a `.tsx` component | keep it thin |

The test for the first row: **could this function run in a unit test with no
window, no runtime, and no hardware?** If yes, it belongs in `core`.

---

## The documents

Read in this order the first time.

| Doc | Answers |
|---|---|
| [current-state diagram and review](current-state-diagram.md) | How every layer, datastore, circuit artifact, and external integration connects; strengths and current risks. |
| [system-context](system-context.md) | What does Bancada talk to, and why does it shell out to everything? |
| [conventions](conventions.md) | How does this project work — layering, testing, commits, releases? |
| [backend-modules](backend-modules.md) | What are the 24 core modules, and what does the Tauri layer add? |
| [ipc-contract](ipc-contract.md) | Every command, event, channel, and the MCP surface. |
| [frontend](frontend.md) | The shell, the three state tiers, the pure-logic tier. |
| [runtime-model](runtime-model.md) | `AppState`, the locks, the threads, shutdown, cancellation. |
| [persistence](persistence.md) | Every file Bancada writes, and the rules for adding one. |
| [agent-safety](agent-safety.md) | What the AI Assistant is actually confined to. |
| [data-flows](data-flows.md) | Seven actions traced end to end. **The best single page.** |

Elsewhere in `docs/`: [scope-architecture](../scope-architecture.md) (the
oscilloscope wire protocol in byte detail), [INSTALL](../INSTALL.md),
[hardware-smoke-tests](../hardware-smoke-tests.md).

**Design history** lives in `docs/superpowers/specs/` and `plans/` — one spec
and plan per feature, dated. Those are snapshots of intent at design time and
are not revised afterwards. For how the system works *now*, this set is the
authority.

---

## Five things that surprise people

Each is deliberate, and each looks like a bug until you know why.

1. **`renderFrame` returns the same object every call.** The scope engine reuses
   its `RenderFrame` and scratch arrays because it runs at 60 fps. A test pins
   this.
2. **Panels are hidden, never unmounted.** Unmounting would drop a live socket,
   channel or agent session on a tab switch.
3. **Stores are polled, not subscribed.** `AgentStore`, `ObsStore` and
   `ScopeEngine` expose a monotonic `version`. This decouples ingest rate from
   render rate. Do not "fix" it into `useState`.
4. **The build gate is non-blocking.** Contention is an error
   (`"build already in progress"`), never a queue.
5. **The agent's `PreToolUse` hook is Bancada's own binary** re-invoked as
   `bancada --agent-guard <dir>`, so the confinement policy is the same
   unit-tested Rust function — not a generated shell script.

---

## Known structural tensions

Named honestly, so a newcomer does not read them as intentional design and pile
on. No refactor is proposed here.

### `src-tauri/src/lib.rs` is 6,658 lines

All 103 commands live in one module. The seams are already visible — the handler
list is grouped by domain, and those groups are really seven independent session
subsystems (scope, agent + MCP, mqtt, device-proxy, git, fleet, chat + usage),
each with its own slot in `AppState` and its own threads.

The file is a flat namespace over a structure that already exists. Its 142-line
rustdoc header is doing the work a module tree would otherwise do.

**If you are adding a command:** put it with its domain group and keep the
`EmitFn` seam intact — it is the only reason ~69 unit tests can live in this
file.

### `src/App.tsx` is 2,217 lines

It owns nearly all cross-panel state and every subscription, and passes ~30
props to `Toolbar` alone. The mitigation is real — 17 pure-logic modules have
been extracted, and that is where the tested behaviour lives — but the
orchestration itself has no harness.

The clearest signal: `src/__tests__/conflicts.test.ts` reads `App.tsx` **as a
string** to assert that `refuseOnConflict(` appears before `api.compileSketch(`.
That is a workaround, documented as one, and it is the codebase saying this file
has outgrown its harness.

### The safety model was written three times

The README, the agent-panel spec and the `lib.rs` rustdoc each described the
agent confinement model with different emphases — a guaranteed drift.
[agent-safety](agent-safety.md) is now the single source and the others point
at it. **Keep it that way.**

### Stale comments outlive the code

The `build_gate` field comment said it serialised "the three sketch build
paths" long after the MCP `upload` tool made it four, and `rename_project`
later made it five. It has been corrected — but it stood wrong through two
additions, and that is the failure mode to watch for in a codebase that
documents this heavily. A count in prose is a claim that rots silently;
prefer naming the rule over counting the callers.

### No CI, and the version lives in four files

`Cargo.toml`, `package.json`, `src-tauri/tauri.conf.json` and `Cargo.lock` are
kept in sync by hand. The release ritual — bump, write release notes, run the
opt-in suites, commit, GPG-signed tag — was tribal knowledge until
[conventions §5](conventions.md#5-releases). For a single-maintainer project
this is a deliberate trade, but it is a trade, not an absence.
