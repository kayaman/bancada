# FreeCAD trust model

What enclosure-maker's embedded session is actually granted when it talks
to FreeCAD, why that was accepted anyway, and what is deliberately not
claimed.

**This does not describe Bancada's own main Assistant panel.** That
subsystem, its tool surface, and its doctrine (`docs/architecture/agent-safety.md`
in the Bancada repository root) are completely untouched by anything in
this document. This page is about enclosure-maker's own, independent,
narrower embedded agent only.

---

## The precedent this reverses

Bancada's own `agent-safety.md` records a considered decision for ESP-IDF
sessions:

> **We deliberately do not use ESP-IDF's own `idf.py mcp-server`.** It
> exists, and this machine's install even ships it. Routing the agent at it
> would hand the model `flash_project(port)` — a raw port argument, outside
> the build gate — dissolving the property [that `upload` always flashes
> what `verify` built, never a raw caller-supplied port].

The pattern that document establishes: when a vendor's own MCP server
exposes a raw, unscoped capability, Bancada writes its *own* narrow server
instead — fixed schemas, no raw arguments, every tool's blast radius
decided structurally (`mcp__bancada__verify`, `upload`, `serial_read`,
`serial_send`, `board_pinout`). The one third-party server it does allow,
Espressif's documentation server, is explicitly admissible *because* "it is
retrieval only... does not execute code, modify files, or perform
actions."

**enclosure-maker does the opposite.** The embedded session is wired
directly to FreeCAD's own MCP server (`freecad-mcp`), and its single most
important tool — `execute_code` — is unrestricted Python execution inside
the FreeCAD process. This is a conscious, one-feature trade-off, not an
oversight or a drift from doctrine. It is written down here precisely
because it contradicts a documented precedent, and a security posture that
only lists its strengths is misleading.

## What's granted

- **Full FreeCAD Python**, via `mcp__freecad__execute_code` (and its
  `_async`/`_headless` siblings): `Part`, `Mesh`, `Draft`, `Sketcher`,
  arbitrary `import os`/`import subprocess`-level access to whatever the
  FreeCAD process itself can reach. There is no sandboxing, no restricted
  interpreter, no allowed-module list.
- Practically, this means the agent can read or write **any file the
  FreeCAD process's OS user can reach**, not just files inside the
  project directory. The `PreToolUse` file-write hook and deny rules in
  `crates/em-agent/guard.rs`/`settings.rs` govern Claude's own
  `Write`/`Edit`/`MultiEdit`/`NotebookEdit` tool calls — they have no
  visibility into, and no effect on, what a FreeCAD Python script executed
  via `execute_code` does.

## Why it was accepted anyway

Most of what an enclosure actually needs — fillets, true-offset shells,
lofts, screw bosses, heat-set-insert bores, hex nut traps — has no
representation in FreeCAD's handful of typed `create_object` types
(`Part::Box`, `Part::Cylinder`, `Part::Cut`, `PartDesign::Body`, a few
`Draft::` factories). It only exists as Python calls into `Part`/`Mesh`.
Restricting the session to the typed object tools and refusing
`execute_code` entirely (an option that was considered) would mean losing
nearly all real enclosure-design capability — the fastener/geometry
library enclosure-maker used to hand-roll in Rust existed for exactly this
reason, and FreeCAD's own Python is now where that logic lives instead,
written fresh by the model each time rather than called from a fixed
library.

## What is still true

- **`--restricted --strict-mcp-config`** still isolates the embedded
  session from the invoking user's own ambient Claude Code configuration —
  their installed plugins, skills, and any MCP server *they* registered.
  The session sees exactly one server: the `freecad` entry in the
  generated `--mcp-config` file (`em_agent::build_mcp_config_json`),
  nothing else.
- **The tool list is still a real boundary.** `Bash`, `Task`,
  `NotebookEdit`, `KillShell`, `BashOutput`, `WebFetch`, and `WebSearch`
  are not offered and are explicitly disallowed. The session cannot shell
  out directly, spawn subagents, or reach the network through Claude's own
  tools — only through whatever FreeCAD's Python interpreter itself can
  do, which is a real restriction in practice (no `requests`/`urllib`
  usage has been observed or is instructed for) even though it isn't a
  structural guarantee.
- **The `PreToolUse` hook and deny rules still run**, as free
  defense-in-depth, even though `Write`/`Edit` aren't in the offered tool
  list anymore. If a future change ever re-adds one of those tools, the
  hook is already there to catch an out-of-project path.

## What is not claimed

Unlike Bancada's build-gate/upload-arming model — which is a *structural*
guarantee (`upload` physically cannot take a raw port, a raw FQBN, or flash
anything `verify` didn't build) — there is no structural containment of
what `execute_code` can do once the session decides to call it. This is a
**trust decision about the model's judgment and the system prompt's
instructions**, backed by nothing stronger than "the model generally does
what it's asked and the system prompt tells it not to do anything else."
A sufficiently adversarial prompt injection reaching this session (from a
hostile BOM/board hand-off, say, or a hostile chat history file) could in
principle direct it to do anything the FreeCAD process's OS user can do.

## A known environmental quirk: Flatpak and `/tmp`

Verified live while building this refactor: if the user's FreeCAD is
installed via Flatpak (a common Linux distribution method, e.g. `flatpak
install org.freecad.FreeCAD`), Flatpak **always** virtualizes `/tmp` for a
sandboxed app, regardless of its other filesystem permissions (even
`filesystems=host` in its manifest). A path under `/tmp` that the
assistant's FreeCAD Python computes — e.g. `os.path.expanduser(...)`
never produces one, but a hand-typed `/tmp/...` path would — silently
resolves to the Flatpak's own private overlay
(`/run/user/<uid>/.flatpak/<app-id>/tmp/...`), not the real host `/tmp`.
Real home-directory paths (`~/Projects/...`, `~/Downloads/...`) are
genuinely shared with the host and behave as expected; this was confirmed
by writing a file to each location and checking it from outside the
sandbox. Since Bancada projects always live under a real home-directory
path, this is not a problem in normal use — it only bites a project root
deliberately pointed at `/tmp` (as a throwaway test setup might do), where
Bancada/the host and FreeCAD would silently disagree about what the
project directory even contains.
