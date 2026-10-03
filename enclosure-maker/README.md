# enclosure-maker

A chat-driven CAD assistant for 3D-printable enclosures. The embedded
assistant designs by calling FreeCAD directly through FreeCAD's own MCP
server — there is no scripting language and no custom geometry engine here.
FreeCAD's own document and GUI window are the model.

## Quick start

1. Install [FreeCAD](https://www.freecad.org/) and keep it open, with its
   MCP RPC addon running (`uvx freecad-mcp`, or however your FreeCAD/addon
   setup starts it — see
   [freecad-trust-model.md](docs/freecad-trust-model.md) for what this
   addon actually exposes).
2. Serve the chat assistant for a project directory:

   ```sh
   cargo run -p em-preview -- chat --project-dir ~/Projects/my-enclosure --port 8080
   # then open http://localhost:8080
   ```

   A "project" is a directory containing `model.FCStd` — FreeCAD's own
   document format. If the directory doesn't have one yet, send the
   assistant a message and it will create one (from a small starter
   template, `assets/starter.FCStd`) the first time it needs to.

There is no live 3D preview in the browser page. FreeCAD's own window,
kept open on your desktop, is the live model — the chat page only carries
the conversation.

### In Bancada

Open an electronics project and select **Enclosure**. Review the generated
request (board, bill of materials), then send it to start designing with
that context. The chat runs inside Bancada's window; FreeCAD's own window,
elsewhere on your desktop, is where the model actually appears. Existing
`model.FCStd` projects reopen directly. No separate enclosure-maker app or
launcher is required.

Chat history lives under the project's own `.enclosure-maker/chats/`
directory, alongside `model.FCStd` itself.

## What the assistant can do

The embedded session talks to FreeCAD through a fixed set of MCP tools:
document/object lifecycle (`create_document`, `create_object`,
`edit_object`, `delete_object`, `get_object`, `get_objects`,
`list_documents`, `reload_document`), code execution (`execute_code`,
`execute_code_async`, `get_async_status`, `execute_code_headless`),
introspection (`get_view`, `get_rpc_status`), and the parts library
(`get_parts_list`, `insert_part_from_library`) — see
`crates/em-agent/src/args.rs`'s `TOOLS` constant for the authoritative,
current list.

In practice, almost everything real goes through **`execute_code`**: it
hands the model full, unrestricted FreeCAD Python (`Part`, `Mesh`, `Draft`,
`Sketcher`, ...). `create_object` only covers a handful of built-in types
(`Part::Box`, `Part::Cylinder`, `Part::Cut`, `PartDesign::Body`, a few
`Draft::` factories) — fillets, shells, lofts, and fastener shapes like
screw bosses or heat-set-insert bores all come from the model writing
ordinary FreeCAD Python, the same way a person scripting FreeCAD by hand
would.

**This means the embedded session has no sandboxed geometry capability —
it has FreeCAD's own, unsandboxed Python interpreter.** That is a
deliberate, consciously-accepted trade-off, not an oversight. Read
[docs/freecad-trust-model.md](docs/freecad-trust-model.md) for what that
means, why it was accepted, and what is (and isn't) still true about the
session's containment.

The system prompt (`crates/em-preview/src/agent_session.rs`'s
`design_assistant_prompt`) carries the fastener dimension table a human
might also want for reference:

| Thread | Insert OD (bore) | Insert length | Bore depth | Nut across-flats | Nut height | Screw clearance |
|---|---|---|---|---|---|---|
| M2 | 3.2 | 3.5 | 4.5 | 4.0 | 1.6 | 2.4 |
| M3 | 4.1 | 5.7 | 6.7 | 5.5 | 2.4 | 3.4 |
| M4 | 5.1 | 6.8 | 7.8 | 7.0 | 3.2 | 4.5 |

All in mm. Bore depth is insert length plus 1 mm of relief. A screw boss's
outer diameter is insert OD + 2 × wall. A hex nut trap's depth is nut
height + clearance. These aren't a callable library the way they used to
be — they're guidance the assistant applies by writing ordinary
`Part.makeCylinder`/`Part.makeBox` calls plus booleans.

Export happens the same way: the assistant calls FreeCAD's own
`Mesh.export(...)` / `Import.export(...)` from `execute_code`, writing
straight to a filesystem path (the project directory, or the user's
Downloads folder by default) — there's no browser download and no Tauri
bridge involved.

## AI assistant

The chat page can use Claude Code, OpenAI Codex, or GitHub Copilot CLI.
Choose the provider in the chat header; the matching `claude`, `codex`, or
`copilot` executable must be installed, authenticated, and on `PATH`. Only
**Claude** currently gets FreeCAD's MCP server wired into its session
(`--mcp-config`, built per-session from `em_agent::build_mcp_config_json`);
Codex and Copilot are selectable but don't yet have an equivalent MCP
config path implemented for their own CLIs.

Claude's session is isolated from your own ambient Claude Code
configuration (`--restricted --strict-mcp-config`) and sees exactly one MCP
server: FreeCAD's. A `PreToolUse` hook and deny rules still run as
defense-in-depth against `Write`/`Edit`/`MultiEdit`/`NotebookEdit`, even
though none of those tools are offered to the session anymore (there's no
script file to write) — see `crates/em-agent/src/guard.rs`.

Chat history is persisted per project, under
`<project>/.enclosure-maker/chats/` — one `.ndjson` file per thread, moving
with the project if you copy, move, or back it up. It survives closing the
app and reopening the project later:

- **New** starts a fresh thread; the next message you send opens a new
  chat file and a new session with the selected provider.
- **History** lists every past thread for this project (most recently
  active first) with its opening message as the title. Clicking one
  replays its full transcript into the chat log and *continues* it —
  the next message you send resumes the same underlying provider session
  natively, with its full prior context, rather than starting cold. The
  provider is saved with the thread and restored when you reopen it.
- The small `×` on a history entry deletes that thread's file permanently.

## Known limitations

- **No live 3D preview.** FreeCAD's own GUI window is the model view. If
  you want to see the part, look at FreeCAD, not the chat page.
- **Flatpak FreeCAD and `/tmp`.** If FreeCAD is installed via Flatpak
  (common on Linux), Flatpak always sandboxes `/tmp` regardless of its
  other filesystem permissions: a path under `/tmp` that the assistant
  computes lands in the Flatpak's own private overlay, not the real host
  `/tmp`. Real paths like `~/Projects` or `~/Downloads` are unaffected.
  Since Bancada projects always live under a real home-directory path,
  this only bites if a project root is deliberately pointed at `/tmp` (as
  a throwaway test setup might do).
- **Only Claude gets the FreeCAD MCP server.** See "AI assistant" above.

## Workspace layout

- `crates/em-agent` — confinement/protocol logic for the embedded AI
  assistant, plus the FreeCAD `--mcp-config` payload (bancada-derived
  design)
- `crates/em-preview` — lib + CLI: chat-only web server (websocket chat,
  persistent per-project history), the system prompt that teaches the
  assistant to drive FreeCAD
- `crates/em-projects` — project scaffolding (the `model.FCStd` starter
  asset) and board/BOM handoff
- `assets/starter.FCStd` — the blank FreeCAD document copied into a new
  project
- `../src-tauri/src/enclosure_preview.rs` — Bancada's embedded chat server
  and desktop integration

Run `cargo test --workspace` (from the Bancada repository root) for the
confinement, protocol, and project-scaffolding test suites.
