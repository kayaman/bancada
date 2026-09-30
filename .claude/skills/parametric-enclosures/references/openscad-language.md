# OpenSCAD language essentials for enclosure work

Condensed from the official Language Reference and Cheat Sheet
(https://openscad.org/documentation.html, https://openscad.org/cheatsheet/).
Focus: what you need to write correct enclosure code and the mistakes that
actually happen.

## Contents
1. Mental model
2. Primitives and 2D→3D
3. Transformations
4. Booleans and hull/minkowski/offset
5. Variables, scope, functions, modules
6. Control flow and list comprehensions
7. Resolution: $fn / $fa / $fs
8. Modifiers and debugging
9. Customizer annotations
10. include vs use, libraries
11. Import/export formats
12. Pitfalls (→ openscad-conventions.md)

## 1. Mental model

OpenSCAD is a declarative CSG language. A file *describes* a solid; it doesn't
execute steps. Every statement at top level is implicitly unioned. Geometry is
built from primitives, placed by transformations, combined by booleans.
Preview (F5) is fast and approximate (OpenCSG); Render (F6) computes the real
mesh (CGAL, or Manifold in development snapshots) and is what gets exported.

## 2. Primitives and 2D→3D

3D: `cube(size, center)`, `cube([x,y,z])`, `sphere(r|d=)`,
`cylinder(h, r|d, center)`, `cylinder(h, r1|d1, r2|d2)` (cones/chamfers),
`polyhedron(points, faces, convexity)`.

2D: `square(size|[x,y], center)`, `circle(r|d=)`, `polygon(points, paths)`,
`text(text, size, font, halign, valign, spacing, direction, language, script)`,
`import("file.svg"|"file.dxf")`, `projection(cut=true|false)`.

Extrusion:
- `linear_extrude(height, center, convexity, twist, slices, scale)` — the
  workhorse. Build 2D profiles (rounded rectangles, cutout shapes, vent
  patterns) and extrude them; far faster and cleaner than 3D booleans.
- `rotate_extrude(angle, convexity)` — revolves a 2D shape in +X around Z
  (profile must be entirely at x ≥ 0). Good for knobs, round enclosures,
  O-ring grooves.

**Enclosure tip:** design in 2D whenever the feature is prismatic. A rounded
box is `linear_extrude(h) offset(r) square(...)`, not `minkowski()`.

## 3. Transformations

`translate([x,y,z])`, `rotate([ax,ay,az])` (applied X then Y then Z, about the
origin), `rotate(a, v=[x,y,z])`, `scale([x,y,z])`, `resize([x,y,z], auto)`,
`mirror([x,y,z])` (normal of the mirror plane; changes handedness — avoid for
parts with threads or text), `multmatrix(m)`, `color("name"|"#hex"|[r,g,b,a], alpha)`
(preview only, ignored by STL), `offset(r=|delta=, chamfer)` (2D only).

Order matters: transforms apply right-to-left (innermost first).
`translate(p) rotate(a) x` rotates x about the origin, then moves it.

## 4. Booleans and friends

- `union()`, `difference()` (first child minus all others), `intersection()`.
- `hull()` — convex hull of children. Fast. Use for stadium slots, rounded
  boxes from 4 cylinders, fillet-like transitions.
- `minkowski()` — very slow on non-trivial meshes; avoid in enclosures.
  Use `offset()` on 2D profiles or `hull()` of spheres/cylinders instead.
- `offset(r=+x)` grows with rounded corners, `offset(delta=x, chamfer=true)`
  grows with chamfers, negative shrinks. `offset(r=-a) offset(r=+a)` rounds
  inner corners; `offset(r=+a) offset(r=-a)` rounds outer ones.

## 5. Variables, scope, functions, modules

- Variables are **set once per scope**; the last assignment in a scope wins
  *everywhere in that scope* (assignment is hoisted). `x = 1; echo(x); x = 2;`
  prints 2 with a warning. Never "update" a variable; compute a new name.
- Scopes: file top level, each `module`/`function` body, each `{}` block of
  `for`/`if`/`let`. Inner scopes see outer variables.
- Special variables (`$fn`, `$fa`, `$fs`, `$preview`, `$children`, `$t`, and
  any user `$name`) are dynamically scoped: they pass down into called modules.
- `function name(args) = expr;` — pure expressions only. Use `let(a=…) expr`
  and the ternary `c ? a : b`; recursion for accumulation.
- `module name(args = defaults) { … }`; `children()`, `children(i)`,
  `$children` let modules wrap other geometry (e.g. `wall_cut(...) { shape(); }`).
- `assert(cond, "message")` stops evaluation with an error; `echo(...)` prints.
- Named arguments are clearer and survive refactors:
  `insert_boss(h = 10, bore = 4.1)`.
- Type tests: `is_undef`, `is_num`, `is_list`, `is_string`, `is_bool`,
  `is_function`. Function literals: `f = function(x) x * 2;`.

## 6. Control flow and list comprehensions

- `for (i = [0:n-1])`, `for (i = [start:step:end])`, `for (p = list)`,
  `for (i = …, j = …)` (nested). A `for` produces an implicit **union** of its
  iterations. `intersection_for()` intersects instead.
- Ranges with start > end are deprecated; guard with `if (n > 0)`.
- `if (…) { … } else { … }` creates or omits geometry.
- List comprehensions build data:
  `[for (h = holes) [h.x + ox, h.y + oy]]`,
  `[for (i = [0:n-1]) if (i % 2 == 0) i]`, `[for (…) let(x = …) f(x)]`,
  `[each list_a, each list_b]` (flatten), `concat(a, b)`.
- Dot indexing on vectors: `p.x`, `p.y`, `p.z`.
- Useful math: `len`, `min/max` (on lists too), `norm`, `cross`, `floor`,
  `ceil`, `round`, `sqrt`, `pow`, trig in **degrees**, `lookup(key, table)`
  for interpolation, `search()` for finding entries.

## 7. Resolution

Curves become polygons. `$fn` forces a segment count; `$fa` (min angle) and
`$fs` (min segment length, mm) scale with size and are the better global
default:

```
$fa = $preview ? 12 : 4;
$fs = $preview ? 1.0 : 0.4;   // ≈ nozzle width; finer adds nothing printable
```

Use `$fn = 6` deliberately for hex nuts. Small holes print undersized partly
because the polygon is inscribed in the circle — use `poly_d(d, n)` or add
hole compensation (see fdm-design-rules.md). Global `$fn = 100` on a part
with many holes makes renders crawl.

## 8. Modifiers and debugging

Prefix characters on a statement:
- `#` highlight (shows in preview, still part of the model) — perfect for
  seeing a cutting solid inside a `difference()`.
- `%` background/ghost (preview only, excluded from render and export) —
  use for the PCB phantom and mating parts.
- `!` show only this subtree.
- `*` disable this subtree.

`$preview` is true in F5 and false in F6/CLI export — use it to lower
resolution or show helpers only while designing. `render()` forces a
subtree to be computed as a mesh in preview (fixes preview artifacts and
speeds complex repeated parts). `echo()` values, `assert()` invariants.

## 9. Customizer annotations

The Customizer (Window → Customizer) exposes top-level variables whose value
is a **literal** (not an expression):

```
/* [Board] */                 // starts a tab/group
pcb_t = 1.6;                  // plain number field
wall = 2.0;   // [1.2:0.1:4]  // slider min:step:max
part = "base"; // [base, lid, assembly]   // dropdown
lip = true;                   // checkbox
pcb_size = [60, 40];          // vector field
/* [Hidden] */                // everything after this is hidden
```

Put all tunables above `/* [Hidden] */` and all derived values after it.
The same variables can be overridden from the CLI with `-D`.

## 10. include vs use, libraries

- `include <file.scad>` pastes the file: its variables, top-level geometry and
  modules all come in.
- `use <file.scad>` imports **only modules and functions**; top-level code
  and variables are ignored. That's why the template defines its own `EPS`.
- Search path: same directory, then the OpenSCAD library folder
  (`OPENSCADPATH`). In a chat delivery, either ship the library file
  alongside or inline the needed modules so a single file renders.
- Popular libraries (use only if the user has them): **BOSL2** (attachments,
  rounding, threads, hinges, snap joints), **MCAD** (bundled with some
  installs; nuts/bolts, bearings). Default to self-contained code.

## 11. Import/export

- Export (File → Export or CLI `-o`): STL (ASCII by default in 2021.01),
  3MF (keeps units, preferred by modern slicers), OFF, AMF, DXF/SVG for 2D,
  PNG image, `.echo` for console output. Export only after a Render.
- Import: `import("board.stl")` to check fit against a vendor model,
  `import("outline.dxf")` / `.svg` for PCB outlines from KiCad (export
  Edge.Cuts as DXF/SVG at 1:1 in mm), `surface()` for heightmaps.
- Imported STL can't be booleaned reliably if it isn't manifold; use it as
  a `%` reference, not as a cutting tool, unless it is clean.

## 12. Pitfalls

Project-level pitfalls (coincident faces, non-manifold output, scope surprises, `use`
not importing variables, fonts) are collected in `openscad-conventions.md`.
