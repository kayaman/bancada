# Closures and fasteners

## Contents
1. Choosing a closure
2. Screwed lids (default)
3. Heat-set inserts
4. Self-tapping screws into plastic
5. Nuts, nut traps, clearance holes
6. Snap-fits
7. Sliding lids
8. Hinges
9. Threads and twist-lock (round enclosures)
10. Magnets
11. Split lines and alignment

## 1. Choosing a closure

| Closure | Opens | Sealing | Strength | Tolerance sensitivity | Pick when |
|---|---|---|---|---|---|
| Screwed lid + inserts, tongue-and-groove | often | good (with gasket) | high | low | **default**; anything that gets serviced |
| Screwed lid, self-tap | rarely | good | medium | low | cheap, few openings |
| Snap-fit | often | poor | low–medium | high | toys, quick access, no tools wanted |
| Friction lid (lip only) | often | poor | low | high | dust covers, prototypes |
| Sliding lid | often | poor | low | medium | battery doors, pencil-box style |
| Hinged | often | poor–medium | medium | medium | display/keypad covers |
| Threaded / bayonet | often | good (O-ring) | high | medium | round sensors, outdoor pods |
| Magnetic | very often | poor | low | low | access panels |

Check the profile first: the default PETG-CF profile forbids elastic features
(snap-fits, living hinges, flexing tabs) — use screws, tongue-and-groove and dowels.
Children/pet products: battery compartments must need a tool (screw) — see
environment-and-safety.md.

## 2. Screwed lids

The body and the lid must fit. A plate the same size as the body, resting on the
rim, is not a closure: it slides, and a zero gap binds after printing. See the
skill's "Body and lid must fit" rule. Summary:

- Screws along Z through the lid into gusseted insert columns in the base corners
  (template). Columns run floor to mating face and never pass through the board;
  the template only places one where a column zone can hold it, so a board with
  connectors on two adjacent walls gets three.
- Tongue on the base rim (about 1.0–1.2 mm wide × 2 mm tall). Groove in the lid,
  opened by the slip clearance on each face (house slip 0.25 mm per side; the
  published FDM band for a hand-seated lid is about 0.2–0.4 mm per side) and one
  slip deeper than the tongue. `lip_groove_cut` adds its `tol` argument to the
  width once, so pass `2 * tol` for a full slip on each flank.
- Keep at least one extrusion of plastic on both sides of the groove
  (`rim >= lip_w + 2 * ew + 2 * tol`). Do not close the gap to get there. When the
  wall is thinner, the template thickens the rim inward (`rim_band`: a band under
  the tongue with a 45° underside, so it prints without support). Outside the
  template, do the same or hang a locating lip from the lid inside the cavity,
  notched around the columns.
- A short 45° lead-in, and a small relief in the groove's inside corners. FDM
  rounds those corners and a sharp tongue binds.
- The lid prints mating face down, so the groove mouth is on the bed and its
  first layer spreads into the groove. The land beside it is one extrusion, so
  there is no room for a chamfer: keep the slicer's elephant-foot compensation on
  for the lid (Bambu Studio default 0.15 mm) and say so in the build notes.
- Spacing ≤ 60–80 mm between screws for a stiff lid; ≤ 50 mm for a gasket.
- Counterbores (head recess) need `top_t ≥ head height + 1.2`; otherwise let
  pan heads sit on top.
- Screws in the bottom (through the base floor into the lid) hide the
  hardware; bosses then hang from the lid.

## 3. Heat-set inserts (brass, knurled)

Hole sizes vary by vendor. The datasheet number is the hole after printing; CAD
is larger by the printed-hole shrink, often 0.2–0.3 mm. The hole itself is
straight even when the insert has a tapered pilot. Common CAD starting points
for short brass inserts:

| Thread | Hole Ø | Hole depth | Boss OD (min) |
|---|---|---|---|
| M2 | 3.2 | insert length + 1.0 (≈ 4) | 7 |
| M2.5 | 3.6 | ≈ 5 | 8 |
| M3 | 4.0–4.2 | 6.7 (for 5.7 mm inserts) | 9 (9.5 on PETG-CF) |
| M4 | 5.6 | ≈ 9 | 11 |

Rules: the bore is straight, not tapered. ≥ 2.5 mm of plastic around the bore on
brittle filled materials (≥ 2 mm otherwise); pocket depth = insert length + 1.0 mm
relief (displaced plastic needs somewhere to go, and a bottomed-out insert splits
the column). Leave the mouth square. A chamfer removes the plastic the top knurl
bites. If the insert will not start, `mouth` on `insert_boss()` may be at most
0.4 mm. Tie the column to the wall and the floor with filleted gussets. The boss
wall is solid perimeters, four to six loops, not infill. Keep inserts outside the
profile's antenna keepout. Install with the iron 10–20 °C above the spool's nozzle
temperature, melt about 90% of the way, then press flush and hold until the plastic sets.

## 4. Self-tapping into plastic

- Pilot Ø ≈ 0.8 × thread major: M2 → 1.6, M2.5 → 2.0–2.1, M3 → 2.4–2.5.
- Engagement ≥ 2 × diameter. Boss OD ≈ 2.2–2.5 × thread diameter.
- Plastite/PT screws hold far better than machine screws in PLA/PETG.
- Tolerates ~5–10 re-insertions before stripping.

## 5. Nuts and clearance holes

| Thread | Clearance hole | Head Ø (socket/pan) | Hex nut AF / thickness | Nut trap AF / depth |
|---|---|---|---|---|
| M2 | 2.4 | 3.8 | 4.0 / 1.6 | 4.2 / 1.8 |
| M2.5 | 2.9 | 4.5 | 5.0 / 2.0 | 5.2 / 2.2 |
| M3 | 3.4 | 5.5–5.7 | 5.5 / 2.4 | 5.7–5.8 / 2.6 |
| M4 | 4.5 | 7.0 | 7.0 / 3.2 | 7.2 / 3.4 |

Nut trap: `cylinder(d = poly_d(af_trap, 6), $fn = 6)` (library: `hex_nut_trap_cut(af, depth)`).
A trap opening on a vertical wall (side-slide nut slot) avoids a bridged
ceiling. For a trap in a floor, the hole above it bridges — add a sacrificial
0.2 mm layer the user drills through, or accept a small bridge.

## 6. Snap-fits (cantilever)

Only when the profile allows elastic features (not PETG-CF; see
manufacturing-profiles.md for the exception rules if a brief demands one).

Geometry: beam length L, thickness t (in bending direction), hook depth y
(deflection needed to pass the catch). For a constant rectangular section:

```
strain ε = 1.5 · y · t / L²          (keep below the material limit)
```

| Material | Allowable ε (repeated use) |
|---|---|
| PLA | ≤ 1 % (brittle; single-use snaps up to ~2 %) |
| PETG, ABS, ASA | ≤ 2 % |
| Nylon | ≤ 3–4 % |

Example: t = 1.6, y = 0.8 → L ≥ sqrt(1.5·0.8·1.6 / 0.02) ≈ 9.8 mm for PETG.
Make L 12–15 mm to be safe.

Design rules:
- Beam in the XY plane when possible (bending across layers breaks beams).
  If it must stand vertically, go thicker/longer and use PETG/nylon.
- Root fillet ≥ 0.5 × t; tapering the beam (t at root → 0.5 t at tip) spreads
  the strain more evenly — or simply make the beam longer.
- Lead-in angle ~30–45°; retention face 90° = permanent, 45–60° = releasable.
- Catch windows in the opposite wall: hook height + 0.3 mm clearance.
- Provide a release slot for a screwdriver if the face angle is 90°.
- Library: `snap_hook(length, thick, width, hook_depth, hook_h)`.

## 7. Sliding lids

- Rails: 45° dovetail or a rectangular groove 1.5–2 mm deep in the walls,
  clearance 0.25 mm per side.
- Print the lid flat (lid plate on the bed); print grooves with 45° upper
  flanks so the base needs no supports.
- Add a detent bump (0.3–0.5 mm) near the closed position and a finger grip.

## 8. Hinges

- Pin hinge with separate pin: knuckle OD ≥ pin Ø + 3; pin hole = pin + 0.3.
  A 1.75 mm filament strand or an M2/M3 screw makes a good pin.
- Print-in-place hinge: 0.4–0.5 mm clearance, cone-shaped pin tips (45°)
  so the knuckles self-support; print with the hinge axis parallel to the bed.
- Living hinges only in PP/TPU/nylon, never PLA/PETG.
- Add a stop to limit opening angle.

## 9. Threads and twist-lock

- Don't hand-write thread profiles; use BOSL2 `threaded_rod()/threaded_nut()`
  if the user has it, or a bayonet (2–3 L-slots with a detent bump), which is
  easy to model and prints well.
- Printed threads: pitch ≥ 2 mm, trapezoidal/buttress profile, 0.3–0.4 mm
  radial clearance, first thread chamfered.
- O-ring in a face groove for sealing (see environment-and-safety.md).

## 10. Magnets

- Round neodymium magnets: pocket Ø = magnet + 0.1–0.2 mm (press) or +0.3 with
  glue; depth = magnet + 0.2 mm.
- Fully captured magnets: pause the print at the pocket's top layer, drop the
  magnet in, resume (tell the user the layer height/Z to pause at).
- Mark polarity; opposite poles on the two parts.
- Keep magnets away from compasses/hall sensors and ≥ 15 mm from antennas.
- Products for children/pets: magnets must be fully enclosed, never
  glued-in-a-pocket where they can come loose.

## 11. Split lines and alignment

- Horizontal split with the lid on top is the default: each half prints
  without supports.
- Split just above the tallest side connector so all side cutouts live in the
  base; or split through the connector and make matching half-notches in
  base and lid.
- Deep enclosures: a mid split (two shells) keeps each part short and gives
  full access to the board.
- Alignment is not optional. Tongue-and-groove (default; also helps sealing),
  an internal lip, or a stepped lap. A butt joint fails the fit rule. Dowel pins
  (printed Ø4 pockets, 0.20 mm clearance) are for parts split to fit the envelope,
  in addition to the lid register, not instead of it.
- Parts too big for the envelope: split and join with M3 bolts + ≥ 2 dowels,
  never glue (PETG doesn't bond reliably).
