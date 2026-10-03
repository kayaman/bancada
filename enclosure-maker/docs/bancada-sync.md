# Enclosure-maker comparison — 2026-10-02

**Superseded.** This is a dated snapshot of the hand-rolled Rust CSG/Rhai
engine (em-core, em-csg, em-primitives, em-hardware, em-export, em-script)
and its live-preview iframe (model controls, parameter sliders, the
iframe/postMessage STL export bridge), all since deleted. That engine was
replaced by driving FreeCAD directly through FreeCAD's own MCP server — see
`enclosure-maker/README.md` and `enclosure-maker/docs/freecad-trust-model.md`
for the current architecture. Left below as a historical record of that
sync event, not as current fact.

Compared Bancada's integrated copy with the sibling `enclosure-maker` checkout
at `04dfd8b`, including its uncommitted model-controls and printing-guidance
changes. Fetched `origin`: its main branch had no commits missing from that
checkout. Experimental remote branches were not treated as released features.

| Area | Result in Bancada |
| --- | --- |
| Geometry, hardware, STL/3MF, Rhai imports | Already equivalent; retained Bancada's geometry panic handling. |
| Model controls | Added selectable parts, move/rotate handles, exact XYZ inputs, snapping, reset, undo/redo, fit and camera views. |
| Dimensions | Added exact parameter inputs, reset buttons and literal shape/hardware controls with stale-source protection. |
| Persistence and exports | Added per-script transforms under `.enclosure-maker/`; applied to desktop/browser STL and CLI STL/3MF. |
| Assistant | Added Claude session isolation, fixed hardware dimensions and printing guidance; retained all three providers and chat history. |
| Bancada integration | Preserved in-process preview, board/BOM handoff, `em-projects`, iframe download bridge and renderer/crash safeguards. Seeded chat opens automatically so design progress is visible. |
| Standalone app | Did not import the obsolete launcher or second Tauri app; Bancada owns project selection and the window. |

The original repository was left unchanged apart from refreshing Git remote
references. Bancada's existing uncommitted integration work was preserved.

Run `cargo test -p em-preview -p em-agent -p em-script -p em-projects`,
`cargo check -p bancada`, `npm test` and `npm run build` from the repository
root. WebSocket integration tests need permission to bind loopback ports.

Validation completed: 110 Rust tests passed (two opt-in assistant tests remain
ignored), all 1,324 frontend tests passed, and the production frontend build
and Bancada Rust check passed. A Chromium smoke test exercised exact movement
and rotation, undo/redo, parameter and hardware edits, transform persistence,
camera/snapping controls, and the iframe export message bridge. Exported STL
coordinates matched the transformed preview. The bridge's parent save response
was simulated; this was not a native WebKit Downloads-folder test. No live AI
provider session was started.
