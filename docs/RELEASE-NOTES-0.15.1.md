# bancada 0.15.1

A documentation release. The application is unchanged — nothing in this cut
alters what Bancada does at the bench. What changes is that the codebase now
describes itself: there is a written architecture, and the README no longer
tells a version-0.7 story.

## The project can now be read

`docs/architecture/` is new — an index plus nine documents aimed at whoever
picks this codebase up next, which is usually a future you.

Until now the only whole-system picture was a thirteen-line diagram in the
README naming three of the twenty-two core modules. Everything else was
either a single-subsystem contract (`scope-architecture.md`) or one of
fifty-two dated feature specs that record what was *intended* at design time
and are never revised afterwards. Neither answers "where does this code go?"

- **[Where does my code go?](architecture/README.md)** — the decision table
  that should have existed all along, plus the layer map and an honest list
  of the structural tensions in the tree.
- **[data-flows](architecture/data-flows.md)** — six actions traced end to
  end: Verify, starting the monitor, an ADC sample becoming a pixel on the
  scope, a full agent turn including its MCP `verify` round trip, a git
  commit, and a device-browser request. The best single page for getting
  oriented.
- **[ipc-contract](architecture/ipc-contract.md)** — all 91 commands, 7
  events, 3 channels and the loopback MCP surface, in one place for the
  first time.
- **[runtime-model](architecture/runtime-model.md)** — the serial
  single-owner rule, the non-blocking build gate, every thread and how it
  stops, and the two deadlocks the design exists to avoid.
- Plus **system-context**, **conventions**, **backend-modules**,
  **frontend** and **persistence**.

## The safety model is stated once

The AI Assistant's confinement model had been written three times — in the
README, in the agent-panel spec, and in the `lib.rs` rustdoc — each with a
different emphasis. That is how a security statement drifts into being wrong.

[agent-safety](architecture/agent-safety.md) is now the single source: all
four enforcement layers, why each exists because the one above it cannot
express something, and a plain statement of what is **not** enforced. The
README keeps the three limits a user needs before deciding to use the panel
and points at the doc for the rest.

## The README caught up

It had stopped describing the product. Now fixed:

- The stack diagram reflects the actual architecture rather than three of
  twenty-two modules.
- Six shipped features it never mentioned are documented — the oscilloscope,
  the MQTT and WebSocket panels, the device browser, the usage dashboard,
  the git pill and editor tabs.
- The repo-layout tree gains `docs/` and `firmware/`; a reader previously had
  no way to know the project ships firmware at all.
- "Git integration" is retired from the Roadmap. It shipped in 0.14.0.
- `docs/` has an index. It was a flat directory that only `INSTALL.md` was
  ever linked from.

## Fixes

- **`cargo test --workspace` compiles again.** It had been failing since
  `AgentCfg` gained `resume_session_id`: the opt-in `agent_live` initializer
  was never updated, so the whole workspace test build errored even though
  the library suites passed. An opt-in suite that does not compile is not a
  gate, and these suites stand in for CI here. Caught by running the gate
  before cutting this release.
- `DeviceBrowse` drops a port field nothing read.

## Under the hood

The architecture docs were verified mechanically against the source rather
than by reading: every one of the 91 commands in `generate_handler!` is
documented and none invented, all 7 event names match in both directions
(Rust emit sites and the `api.ts` listeners), every cited path exists and
every internal link resolves.

That pass also turned up a stale comment now noted in the docs rather than
silently corrected: the `build_gate` field says it serialises "the three
sketch build paths", but there are four call sites — the MCP `upload` tool
was added after the comment was written.

## Notes

Verified on this cut: `cargo test --workspace` 584 passed / 15 ignored,
`vitest` 558 passed, `tsc --noEmit` clean, and the opt-in suites that do not
need an attached board — `core_list_real`, `gh_fetch`, `scaffold_compiles`
and `new_project_builds` — all green against a real `arduino-cli`.

Not run for this release: `fleet_real` and the
[hardware smoke tests](hardware-smoke-tests.md), which need a board on the
bench, and `agent_live`, which spends tokens. A documentation release does
not change any code path they cover.
