# Conventions

The rules this project actually follows. They were real long before they were
written down here — they lived in module rustdoc, in the `## Global Constraints`
block of each plan under `docs/superpowers/plans/`, and in git history. This
page consolidates them so a contributor does not have to reverse-engineer them.

---

## 1. The layering rule

> `bancada-core` contains **no** Tauri or UI code, so it can be unit tested
> headlessly and reused from a CLI or a different frontend later.
> — `core/src/lib.rs:3-4`

This is the load-bearing rule. Concretely:

| Belongs in `core/` | Belongs in `src-tauri/` |
|---|---|
| Parsing, validation, policy, wire formats | Owning a child process |
| Building an argv | Spawning it, streaming its output |
| Deciding *whether* something is allowed | Emitting events, holding a `Channel` |
| Pure transforms over data | Anything holding a `Mutex` in `AppState` |
| Anything you can test with `tempfile` | Anything needing an `AppHandle` |

The test of whether you got it right: **could this function run in a unit test
with no window, no runtime, and no hardware?** If yes, it belongs in `core`.

`core` may still touch the filesystem and spawn processes — `cli.rs`, `git.rs`
and `esptool.rs` all do. What it may not do is own long-lived state or know that
a UI exists.

### The injection corollary

Four modules — `settings`, `fleet`, `chatlog`, `usage` — are additionally
**path-agnostic and clock-free**. The caller supplies both the file path and
`now`. That is what makes them deterministic under test, and it is why
`src-tauri` is the only place that reads a clock or resolves a config directory.

Follow this for any new persistent state. A function that calls
`SystemTime::now()` internally cannot be tested for date-boundary behaviour.

**The frontend follows the same rule**, with `localStorage` in the place of a
config directory: `notifications.ts` takes `now` on every call, `buildHistory.ts`
takes an injected `KVStorage` **and** a `now`, `serialPrefs.ts` takes a
`StorageLike`, and `serialStore.ts` takes the timestamp with the line. A `Map`
satisfies the storage shape structurally, so the tests hand in one and never
touch a browser API. `App.tsx` is where `Date.now()` and `window.localStorage`
actually appear — with one deliberate exception, `StatusBar`, which owns the
app's only ticking clock because a module cannot own one.

---

## 2. Testing

### Rust: unit tests live beside the code

Every `core` module carries a `#[cfg(test)] mod tests`. The Tauri layer has one
too (`src-tauri/src/lib.rs`), with ~82 tests — which is possible only because
the code is written against an injectable `EmitFn` rather than calling
`AppHandle::emit` directly. That seam is the single reason the listener and
reader threads are testable without a Tauri app. **Preserve it.**

`cli.rs` tests use a **stub-script trick**: a fake `arduino-cli` shell script on
a temp `PATH`, so argv construction and output parsing are tested without the
real engine.

### The opt-in suites

Anything needing network, hardware, an installed core, or a live `claude` is
`#[ignore]`d with a reason string, so a plain `cargo test` stays fast and
hermetic:

```bash
cargo test -p bancada-core --test core_list_real    -- --ignored  # arduino-cli
cargo test -p bancada-core --test fleet_real        -- --ignored  # attached board
cargo test -p bancada-core --test gh_fetch          -- --ignored  # network + git
cargo test -p bancada-core --test scaffold_compiles -- --ignored  # installed core
cargo test -p bancada-core --test new_project_builds -- --ignored # installed core
BANCADA_AGENT_LIVE=1 cargo test -p bancada-core --test agent_live -- --ignored
```

Some carry a second gate: `agent_live` needs `BANCADA_AGENT_LIVE=1` on top of
`--ignored`, and the compile suites honour `BANCADA_TEST_FQBN` (default
`arduino:avr:uno`).

These are the **only** tests that prove a scaffolded library, a fetched library
or a new project actually *compile*, rather than merely producing the expected
text. Run them before a release.

### TypeScript: node by default, jsdom per file

`vitest.config.ts` sets `environment: "node"` and collects
`src/**/__tests__/**/*.test.{ts,tsx}`. Node stays the default because most
logic lives in plain `.ts` modules and doesn't need a DOM. A `.tsx` test opts
into jsdom itself: `// @vitest-environment jsdom` as the file's first line,
plus `afterEach(cleanup)` from `@testing-library/react` (there's no
`environmentMatchGlobs` — it was removed in vitest 3, so the opt-in is
per-file, not per-glob).

A `.tsx` test renders a **leaf component only** — props in, DOM out. The five
that exist are `BottomTabBar`, `ToastStack`, `StatusBar`, `BuildConsole` and
`SerialMonitor`; they are the pattern to copy. `App.tsx` is still never
rendered; the `App.tsx?raw` source-text tests remain the harness for its wiring
(see the tensions section in the [index](README.md)). Assert on roles, labels
and text, not class names — except where the class *is* the contract (e.g.
`.active`).

This is the constraint that shapes the entire frontend. Because a component
still can't carry untested logic of its own, anything worth testing is
extracted into a plain `.ts` module first — which is why `src/` has ~29
root-level logic modules and why two components export pure helpers
(`fallbackFqbnLabel`, `exchangeRow`) that exist only to be testable.

So: **if you are about to put logic in a component, put it in a module and call
it from the component instead.** See [frontend](frontend.md).

Two patterns worth knowing:

- **IPC contract tests** (`src/__tests__/api.test.ts`) mock `@tauri-apps/api`
  and assert the exact `(command, argKeys)` pair for every wrapper. A
  camelCase/snake_case typo fails only at runtime otherwise.
- **Source-text assertions** (`src/__tests__/conflicts.test.ts`) read `App.tsx`
  *as a string* to assert call ordering. This is an explicit workaround for
  `App.tsx` having no harness — see the tensions section in the
  [index](README.md).

### The loop

Tests come first: write the test, watch it **FAIL**, implement, watch it
**PASS**, then commit. Every plan in `docs/superpowers/plans/` is structured
this way, step by step.

Suites green after every task:

```bash
cargo test -p bancada-core --lib
cargo check -p bancada
npx tsc --noEmit && npx vitest run
npm run build
```

`npm run build` (`tsc --noEmit && vite build`) is the fourth gate for one
reason: **vitest never parses `styles.css`**. An unclosed CSS block passed
every test gate on this branch and only the bundler noticed.

---

## 3. Documentation

**Wire protocols are documented as contracts, and the code points back at the
doc.** `docs/scope-architecture.md` is the model: numbered sections, byte-offset
tables, exact signatures — and `core/src/scope.rs`, `src/scope/*.ts` and the
firmware README all cite it by section number. When you change a wire format,
the doc is part of the change.

**Known limitations are stated, not hidden.** Every spec has a
`## Known limitations` section, and the rustdoc for the agent safety model
explicitly names what is *not* enforced. Prefer an honest limitation to a
reassuring omission.

**Diagrams are ASCII box-drawing.** No mermaid, no SVG, no images — they render
in a terminal, in a diff, and to an agent reading the file. Tables carry wire
formats and state matrices; fenced `rust`/`ts` blocks carry exact interfaces.

**Design happens in `docs/superpowers/`.** A feature gets a spec
(`specs/YYYY-MM-DD-<feature>-design.md`: Problem → Design → Known limitations →
Testing) and then a plan (`plans/YYYY-MM-DD-<feature>.md`: numbered tasks, each
with files, exact interfaces, and TDD steps). Those are **snapshots of intent at
design time** and are not revised afterwards — so for how the system works
*now*, this `docs/architecture/` set is the authority, not the specs.

---

## 4. Commits

- Conventional-commit subjects, lowercase and descriptive:
  `feat:`, `fix:`, `test:`, `docs:`, `refactor:`.
- The one exception is a release commit: `Release 0.15.0`.
- Every commit ends with the co-authorship trailer used throughout the repo:

  ```
  Co-Authored-By: Claude <model> <noreply@anthropic.com>
  ```

- **Shared checkout:** other sessions may be working in this repository at the
  same time. Stage only the files you named — never `git add -A`. Re-check
  `git log`/`git status` before merging or tagging, because another session's
  commits interleave with yours.

---

## 5. Releases

There is no CI and no release script. The ritual is manual and must be done in
this order:

1. **Bump the version in four files, by hand** — they are kept in sync only by
   discipline:
   - `Cargo.toml` (`[workspace.package] version`)
   - `package.json`
   - `src-tauri/tauri.conf.json`
   - `Cargo.lock` (via any `cargo` command)
2. **Write `docs/RELEASE-NOTES-X.Y.Z.md`** — user-facing, product-announcement
   voice: a short narrative lede, then `##` sections named after feature areas,
   bullets in full sentences, and an `## Under the hood` / `## Fixes` /
   `## Known limitations` section where they apply.
3. **Run the opt-in suites** (§2) plus `docs/hardware-smoke-tests.md`. They are
   the pre-release gate that CI would otherwise be.
4. **Commit** `Release X.Y.Z` — touching exactly those four files plus the new
   release notes.
5. **Tag** a GPG-signed annotated tag `vX.Y.Z`, message
   `bancada X.Y.Z — <one-line summary>`.

Bundles are built with `npm run tauri build` and land in
`target/release/bundle/{rpm,deb,appimage}/`.

---

## 6. Dependencies

New dependencies are added reluctantly, and **the manifest comment explaining
why is part of the change**. Existing examples set the bar:

- `serialport` — `default-features = false` to avoid a libudev build dependency
- `ureq` — plain HTTP on purpose; bench devices do not serve TLS
- `rumqttc` — `default-features = false`; the sync client is used deliberately,
  and the comment notes it drags tokio in transitively
- `trash` — Explorer deletes go to the OS trash, never `fs::remove`

The frontend is deliberately thin: React, zustand, CodeMirror 6, react-markdown.
**No charting library, no icon package, no date library, no lodash.** The scope
renderer, the spectrum display, the FFT and the unified diff are all
hand-written, because each was smaller than the dependency would have been.

Dev dependencies for the component harness (see above):

- `jsdom` — the DOM implementation a `.tsx` test's jsdom environment runs against
- `@testing-library/react` — renders components and unmounts them via `cleanup`
- `@testing-library/dom` — RTL 16's `screen`/`getByRole` queries; a peer dep of `@testing-library/react`
- `@testing-library/user-event` — realistic click/type/keyboard interactions, not raw DOM events

---

## See also

- [system-context](system-context.md) — the external-engine principle these follow from
- [frontend](frontend.md) — what the node-environment test constraint produced
- [`docs/hardware-smoke-tests.md`](../hardware-smoke-tests.md) — the pre-release hardware pass
