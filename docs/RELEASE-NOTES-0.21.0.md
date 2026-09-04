# bancada 0.21.0

0.20.0 gave Bancada a notion of the board. This one closes the older gap
underneath it: Bancada could **build and flash** ESP-IDF projects but never
**make** one, so every ESP-IDF project had to be born somewhere else and
opened. New Project now asks which platform you want first.

## Two platforms, one wizard

```
Platform
┌──────────────────────────┐ ┌──────────────────────────┐
│ Arduino                  │ │ ESP-IDF                  │
│ arduino-cli · a sketch   │ │ idf.py · a CMake project │
│ with a pinned profile    │ │ with a main component    │
└──────────────────────────┘ └──────────────────────────┘
```

Choosing ESP-IDF swaps the form: a **target chip** instead of a board, four
starters that compile on any target and need no wiring (Hello, Blink, Tasks,
NVS), and the same Devkit select 0.20.0 introduced. Profiles and the library
registry disappear entirely — they are arduino-cli concepts, an ESP-IDF
project's dependencies come from the IDF Component Manager's own registry, and
rendering those fields disabled would claim the form can do something it
cannot.

`create_idf_project` is a **sibling** of `create_project`, not a mode of it.
The two share a parent directory and a name and nothing else. A CMake project
name may not begin with a digit; an Arduino sketch name may — `2fast` is a
legal sketch folder and an illegal `project()` argument, and a test pins that
disagreement, because the wizard has to validate against the paradigm you
actually chose rather than against one blended rule.

## You do not need ESP-IDF installed to create an ESP-IDF project

`idf.py create-project` exists and using it would have been less code. But it
would mean a first project — exactly when someone is least likely to have a
working install — is the one you cannot make. So the tree is written directly:
`CMakeLists.txt`, `main/CMakeLists.txt`, `main/<name>.c`, `.gitignore`. Four
small files, instant, and testable without a toolchain.

The chosen chip goes into `sdkconfig.defaults` as `CONFIG_IDF_TARGET` rather
than through `idf.py set-target`, which would need that install. That is the
mechanism ESP-IDF itself provides for naming a target before the first
configure. **Building** still needs the real thing, and that is the right place
for a missing toolchain to be reported.

Scaffolding writes into a dot-prefixed staging directory and renames it into
place, so a failure part-way leaves nothing behind rather than a half-tree that
`detect_kind` would happily accept as a project.

### Proven, not assumed

Two claims above are load-bearing and neither was obvious, so
`core/tests/idf_scaffold_builds.rs` checks them against a real install
(opt-in, `BANCADA_IDF_LIVE=1`). Every starter is scaffolded and built —
all four, because they differ in what they pull in, and a template that
scaffolds cleanly and fails to *link* is exactly the bug worth catching.

Run against ESP-IDF v6.0.1 for this release, 264 s, all green:

```
hello: 142304 bytes    tasks: 142416 bytes
blink: 144144 bytes    nvs:   165216 bytes
```

No `set-target` ran anywhere in that test, and the assertion reads the target
back out of the `sdkconfig` the build *generated* — so the defaults file really
does carry it through. The second test greps the whole build log and asserts
`kconfgen` never complained about an unknown symbol, which is the entire reason
the board marker is a comment rather than a `CONFIG_` key. Both now verified
against IDF v6 rather than inherited on faith.

## MCP calls are legible in the Assistant log

An Espressif documentation search used to render as the generic fallback: a
collapsed blob titled `mcp__espressif-docs__search_espressif_sources({…})` with
its query buried in raw JSON. Five searches in a row were five identical lines
you had to expand one at a time to tell apart.

```
✓ ┃espressif-docs┃ search_espressif_sources   I2C pull-ups on ESP32-C6
```

Server, tool, and **what was asked** — visible without expanding, so a research
loop reads as a sequence. The subject is the only part allowed to ellipsise,
since it is the only part that differs between consecutive calls; the full
input and result are still there when expanded.

The wire name `mcp__<server>__<tool>` is *decomposed* rather than special-cased,
so this serves every MCP server Bancada ever offers rather than Espressif
alone. Bancada's own verify, upload and serial cards keep their specific
renderings; `board_pinout` and third-party tools land here.

The activity line benefits too: it drops the `mcp__…__` prefix, which is
identical on every call and crowds out what differs, and it learned `query`. So
the footer now reads `⚙ search_espressif_sources I2C pull-ups… · step 4`
instead of a bare tool name.

## Tests

```
cargo test --workspace   732 core · 84 src-tauri, 0 failed
BANCADA_IDF_LIVE=1 …     5 real ESP-IDF builds, 0 failed
npm test                 1260 passed, 78 files
tsc --noEmit             clean
vite build               clean
```

Run the Rust suite with `--test-threads=1`: `cli::tests` has a known parallel
flake that is not a real failure.

## Still not seen on hardware

Scaffolded projects now demonstrably **build**. Nothing has been **flashed**.
The bench pass owed by 0.20.0 stands, plus one more item:

5. Flash a scaffolded ESP-IDF Blink to the C6 and confirm the pin the board
   profile chose is the one that moves.

0.19.0's bench pass is still owed as well.
