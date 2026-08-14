# Bancada documentation

## Start here

| If you want to… | Read |
|---|---|
| **install and run Bancada** | [INSTALL.md](INSTALL.md) |
| **understand or change the code** | [architecture/](architecture/README.md) |
| **work on the oscilloscope** | [scope-architecture.md](scope-architecture.md) |
| **do the pre-release hardware pass** | [hardware-smoke-tests.md](hardware-smoke-tests.md) |

## Architecture

[`architecture/`](architecture/README.md) describes the system as it stands.
Start with its index, or jump straight to
[data-flows](architecture/data-flows.md) if you learn best by tracing an action
end to end.

- [README](architecture/README.md) — layer map, "where does my code go?", known tensions
- [current-state diagram and review](architecture/current-state-diagram.md) — elaborate runtime map, external integrations, circuit synchronization, and findings
- [system-context](architecture/system-context.md) — external engines, network peers, constraints
- [conventions](architecture/conventions.md) — layering, testing, commits, releases
- [backend-modules](architecture/backend-modules.md) — the 22 core modules and the Tauri layer
- [ipc-contract](architecture/ipc-contract.md) — 99 commands, 7 events, 3 channels, the MCP surface
- [frontend](architecture/frontend.md) — shell, state tiers, the pure-logic tier
- [runtime-model](architecture/runtime-model.md) — `AppState`, locks, threads, shutdown
- [persistence](architecture/persistence.md) — every file Bancada writes
- [agent-safety](architecture/agent-safety.md) — the AI Assistant's confinement model
- [data-flows](architecture/data-flows.md) — seven traces through every layer

## Subsystem contracts

- [scope-architecture.md](scope-architecture.md) — the oscilloscope wire
  protocol, byte by byte: framing, CRC, the control plane, the Rust→TS envelope,
  and the TypeScript engine contract. Cited by name from `core/src/scope.rs`,
  `src/scope/*.ts` and the firmware README.

## Operations

- [INSTALL.md](INSTALL.md) — building the bundles, installing the engines, and
  the serial-port setup (udev / ModemManager) an end user needs.
- [hardware-smoke-tests.md](hardware-smoke-tests.md) — the manual pre-release
  pass against real hardware. This project has no CI; this is what stands in for
  it.

## Release notes

Newest first. One file per release, user-facing.

- [0.17.0](RELEASE-NOTES-0.17.0.md) — boards remember their project, new starters, serial monitor fixes
- [0.16.0](RELEASE-NOTES-0.16.0.md) — project lifecycle, flash provenance, one project menu
- [0.15.1](RELEASE-NOTES-0.15.1.md) — architecture documentation
- [0.15.0](RELEASE-NOTES-0.15.0.md) — device browser
- [0.14.1](RELEASE-NOTES-0.14.1.md) — durability and assistant lifecycle fixes
- [0.14.0](RELEASE-NOTES-0.14.0.md) — the git pill
- [0.13.0](RELEASE-NOTES-0.13.0.md) — continue an AI session, guard rails
- [0.12.0](RELEASE-NOTES-0.12.0.md) — usage dashboard, profiles and boards, editor tabs
- [0.11.0](RELEASE-NOTES-0.11.0.md) — brand, projects, assistant
- [0.10.0](RELEASE-NOTES-0.10.0.md) — bottom panel, port recognition, file explorer
- [0.8.0](RELEASE-NOTES-0.8.0.md) — AI Assistant panel, board picker

## Design history

`superpowers/` holds one design spec and one implementation plan per feature,
dated:

- `superpowers/specs/YYYY-MM-DD-<feature>-design.md` — Problem → Design →
  Known limitations → Testing, plus the decisions taken and the ones rejected.
- `superpowers/plans/YYYY-MM-DD-<feature>.md` — numbered tasks with exact
  interfaces and TDD steps.

These are **snapshots of intent at design time** and are not revised after
implementation. They are the best record of *why* something was done — including
what was tried and dropped. For how the system works *now*, use
[architecture/](architecture/README.md).
