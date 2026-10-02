# enclosure-maker

A code-based CAD tool for 3D-printable models, with a Rust-native geometry
core and a `.rhai` scripting layer for near-instant live preview in the
browser.

## Quick start

```sh
# Live preview: watches the script (and any imported library files) and
# pushes updates to the browser
cargo run -p em-preview -- preview --script enclosure-maker/examples/basic_box.rhai
# then open http://localhost:8080

# One-shot STL export
cargo run -p em-preview -- export --script enclosure-maker/examples/basic_box.rhai --output part.stl
```

Edit the `.rhai` file while `preview` is running and the browser view updates
automatically — no recompile needed.

### In Bancada

Open an electronics project and select **Enclosure**. Review the generated
request, then send it to start designing with the project's board and bill
of materials. The preview, model controls, code editor, and assistant run
inside Bancada's window. Existing `main.rhai` projects reopen directly.

The enclosure source and `lib/` imports live in the electronics project's
own directory. Chat history and saved part transforms live under its
`.enclosure-maker/` directory. Export writes each printable part's STL to
Downloads through Bancada's desktop bridge. No separate enclosure-maker
app or launcher is required.

The CLI commands below run from the Bancada repository root; example paths
are prefixed with `enclosure-maker/`.

## Writing a script

```js
let base = cuboid(20.0, 20.0, 10.0);
let hole = cylinder(4.0, 12.0, 32).translate(0.0, 0.0, 0.0);
emit(base.subtract(hole));
```

A script produces one or more **named parts** by calling `emit`: either
`emit(solid)` (defaults to a part named `"part"` — the simple single-part
case above) or `emit("name", solid)` for multiple parts (see
[Multi-part scripts](#multi-part-scripts) below). `view("name", solid)` is a
preview-only composition and is not a printable object. Re-emitting an existing
name replaces it. Always write numeric literals with a decimal point
(`20.0`, not `20`) — Rhai integers and floats are distinct types and the
bindings expect floats.

### Primitives

| Function | Returns |
|---|---|
| `cuboid(x, y, z)` | box centered at the origin |
| `sphere(r, segments)` | sphere centered at the origin |
| `cylinder(r, h, segments)` | base centered at origin, extends up to `h` |
| `cone(r0, r1, h, segments)` | frustum, base radius `r0` at z=0, `r1` at z=h |
| `rounded_box(x, y, z, radius, segments)` | box with filleted edges/corners; `radius` <= half the smallest dimension. `(x,y,z)` is the *final outer* size |
| `chamfered_box(x, y, z, chamfer)` | box with flat 45°-cut edges; `chamfer` <= half the smallest dimension |

### 2D profiles + extrude/revolve/loft

| Function | Returns |
|---|---|
| `profile_rect(w, h)` | rectangle profile |
| `profile_circle(r, segments)` | circle profile |
| `profile_regular_polygon(r, sides)` | regular N-gon, circumradius `r` |
| `extrude_linear(profile, height)` | straight extrusion along +Z |
| `extrude_revolve(profile, degrees, segments)` | revolve around the Z axis; profile X = radius, Y = height |
| `loft2(bottom, bottom_z, top, top_z)` | connects two profiles at different heights into one solid; **profiles must have equal point counts** (point `i` in one connects to point `i` in the other — no resampling). A multi-section loft is just several `loft2` calls unioned together |

### Boolean ops, transforms, and patterns (methods on a solid)

`a.union(b)`, `a.subtract(b)`, `a.intersect(b)`,
`a.translate(x, y, z)`, `a.rotate(x, y, z)` (Euler degrees, X then Y then Z),
`a.scale(x, y, z)`, `a.mirror(nx, ny, nz)`.

`a.linear_pattern(x, y, z, count)` unions `count` copies of `a`, each further
offset by `(x,y,z)` (the first copy unshifted). `a.radial_pattern(count)`
unions `count` copies evenly rotated around the Z axis.

`a.shell(thickness)` hollows `a` into a wall of about `thickness`, by
subtracting an inward-scaled copy — **exact for a box or cylinder centered
on its own bounding box**, an approximation for other shapes (not a true
constant-offset surface). `a.shell_open(thickness, nx, ny, nz)` also opens
one face (e.g. `(0,0,1)` for an open top); note that since the cavity is
inset on *every* axis, opening a face removes the entire cap on that side
down to the cavity's ceiling, not just a thin slit — the new rim ends up at
`(that axis's half-extent) - thickness`, not at the original outer face.

`a.split(px, py, pz, nx, ny, nz)` cuts `a` along the plane through
`(px,py,pz)` perpendicular to `(nx,ny,nz)`, returning a 2-element array
`[piece_toward_normal, piece_away_from_normal]` — useful for printing a
part too big for your bed in sections. `a.split_with_dowels(px, py, pz, nx,
ny, nz, dowel_count, dowel_radius, dowel_length)` does the same but adds
alignment dowels at the cut: short pins protruding from the second piece,
matching (clearance-enlarged) sockets recessed into the first, so the
halves self-align when glued. Dowel placement is based on `a`'s bounding
box, evenly spaced along whichever in-plane direction is longer — a
reasonable default for roughly-convex parts, but check the result before
printing if your cross-section at the cut is very irregular. See
`examples/split_demo.rhai`.

### Hardware library

Dimensions follow standard heat-set insert and hex nut datasheets. Thread
size is passed as a plain integer: `2`, `3`, or `4` (for M2/M3/M4).

| Function | Returns |
|---|---|
| `heat_set_bore(thread_mm)` | blind insert cavity, straight bore, square mouth — opening at local Z=0, cuts into -Z |
| `screw_boss(thread_mm, height, gusset_count, wall)` | solid gusseted column, base at Z=0 |
| `hex_nut_trap(thread_mm, depth_extra)` | hex nut cavity — opening at local Z=0, cuts into -Z |
| `pcb_standoff(thread_mm, height, wall)` | hollow standoff with a through screw-clearance bore |
| `vent_slot_row(width, length, count, wall, through_depth)` | row of rounded-end vent slot cutters, through-axis Z |

`screw_boss` is structural only — subtract a `heat_set_bore` (translated to
the column's top) to add the insert cavity. See `examples/m3_enclosure.rhai`
for a complete base with four screw bosses and a vent row.

These sizes are fixed. Scripts pass the arguments in the table above; the
functions have no other overloads, and a script cannot read this table out
of the engine.

| Thread | Insert OD (the bore) | Insert length | Bore depth | Nut across flats | Nut height | Screw clearance |
|---|---|---|---|---|---|---|
| M2 | 3.2 | 3.5 | 4.5 | 4.0 | 1.6 | 2.4 |
| M3 | 4.1 | 5.7 | 6.7 | 5.5 | 2.4 | 3.4 |
| M4 | 5.1 | 6.8 | 7.8 | 7.0 | 3.2 | 4.5 |

Bore depth is insert length plus 1 mm of relief under the insert. The bore
mouth is square. `screw_boss` outer diameter is insert OD + 2 × `wall`, and
`gusset_count` is clamped to 2–4. A larger boss means a larger `wall`.
`hex_nut_trap` depth is nut height + `depth_extra`. `pcb_standoff`'s
through-bore is the screw clearance, and its outer diameter is that
clearance + 2 × `wall`. Circular features in these helpers use 24 segments.

## Multi-part scripts

A script can define several named printable parts and compose preview-only
views over them with boolean ops and transforms:

```js
emit("base", base);
emit("lid", lid);
view("assembly", base.union(lid));
view("exploded", base.union(lid.translate(0.0, 0.0, 30.0)));
let half_space = cuboid(1000.0, 1000.0, 1000.0).translate(500.0, 0.0, 0.0);
view("section", base.union(lid).intersect(half_space));
```

`emit` parts are what gets printed. `view` parts are for looking at a
composition (`--part assembly`) and are left out of the default preview and
out of the download. See `examples/enclosure_assembly.rhai` for a complete
version of this pattern. Without `--part`, `preview` shows the printable
parts together (for a single-part script, that's just the one part).

The preview **Export** button writes one binary STL per printable part. A
single part (or a selected `--part`) is one file. Two or more printable parts
are one `.stl` each, so a slicer keeps the lid and the body apart.

Batch export from the CLI still takes an explicit part list, since a script's
names mix printable output (`base`, `lid`) with preview-only views:

```sh
# Every part to its own STL file
cargo run -p em-preview -- export --script enclosure-maker/examples/enclosure_assembly.rhai \
  --output-dir out --parts base,lid

# Every part bundled into one .3mf (multi-object; opens as separate items in most slicers)
cargo run -p em-preview -- export --script enclosure-maker/examples/enclosure_assembly.rhai \
  --output out.3mf --parts base,lid
```

If a script has only one part, `--part`/`--parts` can be omitted — it's
picked automatically. `--format ascii` on `export` writes text STL instead
of binary (mainly useful for diffing models in version control); it has no
effect on `.3mf` output.

## Parametric controls

The preview's **Model controls** panel works without the assistant or code
editing. Select a named part from the list or click it in the viewport,
then use **Move** / **Rotate** handles or enter exact X/Y/Z values. Movement
is in millimeters; rotation is in degrees about the part's bounding-box
center, applied X then Y then Z. Optional snapping uses 1 mm and 15°.
**Reset**, **Undo**, and **Redo** apply to these part transforms. **Fit** and
the 3D/Top/Front/Right buttons control the camera.

Part transforms save automatically to
`.enclosure-maker/<script filename>.transforms.json`, keyed by part name.
They apply after the script evaluates, survive reopening and code edits,
and affect desktop/browser STL downloads and CLI STL/3MF exports. Keep
part names stable to preserve their controls. These transforms position
whole output parts; they do not change their internal features or the
script's preview-only assembly views.

**Dimensions** offers exact numeric inputs alongside parameter sliders.
Use **Save** to make those parameter values the script's defaults.
**Shape & hardware dimensions** also exposes literal numeric variables and
supported primitive/hardware arguments in the main script, including boss
height, wall thickness, gussets and M2/M3/M4 thread sizes. Editing one of
these writes only that literal back into the script and rebuilds the model.
Expressions remain expressions: edit their underlying variable/parameter,
or expose a `param(...)` for dimensions defined inside imported helpers.
An invalid dimension reports a script error while preserving the last
successful model.

`param(name, default, min, max)` declares a value as UI-adjustable and
returns its current value (an override sent from the browser, or `default`
if none is set yet):

```js
let width = param("Width", 60.0, 40.0, 120.0);
let wall = param("Wall thickness", 2.0, 1.2, 4.0);
```

While `preview` is running, every declared `param(...)` shows up as a slider
in the top-right panel of the browser page; moving one re-evaluates the
script with that override and pushes the updated mesh, debounced (~80ms
after you stop dragging) so it doesn't re-run the CSG pipeline on every
mouse-move tick. Overrides are per-server-process, shared across every
connected browser tab (a second tab connecting mid-session sees whatever the
current values are, not the script's original defaults), and persist across
a file-edit-triggered reload — editing unrelated code doesn't reset your
slider positions. `export` also accepts overrides on the command line,
repeatable:

```sh
cargo run -p em-preview -- export --script enclosure-maker/examples/enclosure_assembly.rhai \
  --output wide.stl --part base --param Width=100.0 --param "Wall thickness=3.0"
```

See `examples/enclosure_assembly.rhai` for width/depth/height/wall exposed
this way.

### Save and Export, from the browser

The top-right toolbar of the preview page includes:

- **Preview** renders the current editor code immediately, including pending
  edits, and re-runs it even if the code has not changed. Errors appear in the
  viewport status while the last successful model stays visible.

- **Export** downloads the current model without re-running CSG, as one
  binary STL per printable part. In the desktop app each file is written
  into the Downloads folder.
- **Save** rewrites the script's `param(...)` calls so your current slider
  positions become the new defaults — a surgical edit that only touches
  each changed default's numeric literal, leaving everything else in the
  file (formatting, comments, untouched params) byte-for-byte identical.
  Only shown when the script declares at least one `param(...)`; shows
  "Nothing to save" if no slider has been moved from its default yet.

## AI assistant

The chat panel in the bottom-left of the preview page can use Claude Code,
OpenAI Codex, or GitHub Copilot CLI to edit `.rhai` files directly in your
project. Choose the provider in the chat header; the matching `claude`,
`codex`, or `copilot` executable must be installed, authenticated, and on
`PATH`. Claude is confined by a `PreToolUse` hook and deny rules. Codex runs
in its workspace-write sandbox with approvals disabled, and Copilot is
limited to its read/write tools and normal path verification. All providers
are instructed to leave app metadata and version-control files alone. Any
edit is picked up by the normal file watcher, so the 3D view updates exactly
as it would if you'd edited the file yourself.

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

## Shared library files

Any `.rhai` file can be imported by another, resolved relative to the
*importing script's own directory* — not the process's working directory:

```js
import "lib/presets" as presets;
let base = presets::standard_base(60.0, 40.0, 20.0, 2.0);
```

Functions at a module's top level are exported automatically — Rhai's
`export` keyword is only for module-level variables, not `fn`. (`export` is
also a reserved word, so it can't be used as a binding or variable name in
your own scripts.) See `examples/lib/presets.rhai` for a small reusable
parts library, and `examples/enclosure_assembly.rhai` for a script that
imports it. While `preview` is running, editing an imported library file
triggers a reload just like editing the main script.

## Workspace layout

- `crates/em-core` — vectors, planes, mesh types
- `crates/em-csg` — BSP-tree boolean operations (union/subtract/intersect), bounds, patterns
- `crates/em-primitives` — primitives, 2D profiles, extrude/revolve/loft, shell
- `crates/em-hardware` — enclosure fastener/mounting library
- `crates/em-export` — binary/ASCII STL writers, 3MF writer
- `crates/em-script` — Rhai bindings over the crates above, multi-part `Scene`, `param(...)` schema
- `crates/em-agent` — confinement/protocol logic for the embedded AI assistant (bancada-derived design)
- `crates/em-preview` — lib + CLI: file watcher + bidirectional web server (mesh push, live parameter sliders, chat + persistent per-project history, save/export), single/batch export
- `crates/em-projects` — project scaffolding and board/BOM handoff
- `../src-tauri/src/enclosure_preview.rs` — Bancada's embedded preview and desktop exports

Run `cargo test --workspace` to run the geometry, export, and scripting test
suites.
