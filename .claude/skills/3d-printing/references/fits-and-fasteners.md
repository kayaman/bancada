# Fits and fasteners

Clearances are per side unless a row says diameter. The house profile's `tol` (press 0.15 / slip 0.25 / loose 0.40 mm) replaces the generic table when that profile is in force. One coupon in `assets/fit-coupon.scad` sets `tol` for the printer-material pair; after that, leave it alone.

Vertical gaps only exist in whole layers. A 0.25 mm step at 0.2 mm layers becomes 0.2 or 0.4. Design Z clearances as a multiple of the layer height.

## Fit table (generic 0.4 mm nozzle, per side)

| Fit | Clearance | Use |
|---|---|---|
| Press | 0.00–0.10 mm | Pins and magnets that must stay. Too much interference splits a brittle boss |
| Snug | 0.10–0.15 mm | Friction lids, light press of a cap |
| Slip | 0.20–0.30 mm | Sliding lids, shafts, buttons in holes |
| Loose / print-in-place | 0.30–0.50 mm | Hinges and parts printed already assembled. Below about 0.3 mm a 0.4 mm nozzle tends to fuse them |

Printed part against a bought part: leave the slip clearance, and a little more when the bought part's own tolerance is unknown. A lead-in chamfer on the pin or the hole mouth stops the edge catching on layer lines.

Elephant's foot eats a fit that includes the bed edge. Chamfer that edge or keep the mating diameter off the first layer.

## Body and lid

A lid that is a copy of the body's outline, sitting on the rim, does not fit. Nothing locates it sideways. A zero gap in CAD becomes a bind or a rattle after printing, because the first layer spreads and inside corners come out round.

Give the pair a continuous register: a tongue in a groove, a lip that drops inside the opening, or a stepped lap. A butt joint is not a fit.

Fix the male feature and open only the female, by the slip clearance on each face. On the house profile that is 0.25 mm per side. Published FDM practice for a lid that seats by hand is about 0.2–0.4 mm per side (tighter binds, past about 0.5 mm it rattles and the seam leaks light). Leave the depth one slip deeper than the tongue so it does not bottom out. Vertical gaps still have to land on a whole layer.

The wall has to afford that gap. Plastic left on both sides of the groove stays at least one extrusion wide: wall ≥ tongue width + two extrusion widths + two slip gaps. If it does not, narrow the tongue toward 1.0 mm or add a perimeter. Do not close the clearance to save the land. A short 45° lead-in lets the lid find the opening. Relieve the groove's inside corners so the tongue's corner has somewhere to go.

Both parts share one set of dimensions. A screw hole that is merely near its boss misses. In the assembled position the two solids do not intersect, a section shows the gap, and the lid cannot shift sideways without the register stopping it. Chamfer whichever mating edge is printed on the bed.

## Heat-set inserts

Use inserts for anything that will be unscrewed more than a few times. The hole diameter is that insert's datasheet number, not a universal M3 number. The datasheet is the hole you want after printing. Printed holes come out small, so the CAD diameter is larger. CNC Kitchen's insert tests put that offset at about 0.2–0.3 mm for a small insert: their M3 datasheet hole of 4.0 mm wanted about 4.2 mm in CAD, and pull-out there was still about 90% of the tightest hole. Past that the hold falls off quickly. Confirm on a coupon in the same material, walls, and orientation. The hole that lets the pilot pre-seat, and leaves no burr under the insert, is the one to keep.

Starting CAD diameters for common short brass inserts, when the datasheet is not in front of you. These already include a typical printed-hole shrink. A coupon replaces the column.

| Thread | CAD hole Ø | Blind depth | Min plastic around the bore |
|---|---|---|---|
| M2 | 3.1–3.2 | length + 1 mm | 2.0 mm |
| M2.5 | 3.6–3.8 | length + 1 mm | 2.0 mm |
| M3 | 4.0–4.2 | length + 1 mm (about 6.7 for a 5.7 mm insert) | 2.0 mm; 2.5 mm in PETG-CF (boss OD ≥ 9.5) |
| M4 | 5.4–5.6 | length + 1 mm | 2.5 mm |
| M5 | 6.4–6.8 | length + 1 mm | 2.8 mm |

The bore is straight. A tapered hole grips on a ring and splits above it. The extra millimetre of depth is where displaced plastic goes. An insert that bottoms out splits the boss. A through hole is only as long as the insert. Leave the mouth square. A chamfer there removes the plastic the top knurl should bite. If the insert will not start, a lead-in no bigger than about 0.4 mm.

Install with a soldering iron 10–20 °C above the spool's nozzle temperature, about 245 °C for unfilled PETG. Melt the insert about 90% of the way, then press it flush with a flat tool and hold it until the plastic sets. Inserts creep back out while the plastic is soft. Press straight. Do not twist. An M5 insert needs a thick tip or the brass never gets hot. A spinning insert means the hole was large or the knurl never bit. A burr under the insert, or a screw that binds at the bottom, means the hole was small. A cracked boss means the wall was thin, the hole was small, or the plastic was brittle. On PETG-CF, take the easy-install end of the hole range. A tight hole in a brittle boss splits before it holds better.

Perimeters around the boss do the holding, not infill. The wall of the boss is solid rings: four to six perimeters, with no infill gap inside that wall. Tie the boss into a wall or a floor with a filleted gusset. A boss stuck on the side of a thin wall is a lever with a layer line at the root.

Put the screw axis along Z when you can, so the pull is trying to yank the insert out of a column of perimeters rather than peeling a flange.

## Self-tapping screws

For parts opened rarely.

- Pilot ≈ 0.8 × major diameter. M3 → about 2.4–2.5 mm. M4 → about 3.2 mm.
- Engagement ≥ 2 × diameter. Boss outer diameter about 2.2–2.5 × major diameter.
- Thread-forming screws for plastic hold better than a machine screw driven into a pilot.
- Expect a handful of reassemblies, then stripped threads. That is the moment to switch to an insert.

## Nuts and clearance holes

| Thread | Clearance hole | Hex nut across flats / thickness | Printed trap across flats / depth |
|---|---|---|---|
| M3 | 3.4 | 5.5 / 2.4 | 5.8 / 2.6 |
| M4 | 4.5 | 7.0 / 3.2 | 7.3 / 3.4 |
| M5 | 5.5 | 8.0 / 4.0 | 8.3 / 4.2 |

A nut slot that opens on a vertical face prints clean. A hexagonal pocket under a horizontal ceiling has to bridge; keep that span inside the profile's bridge limit or open the pocket from the side.

## Snap fits

Not on the house PETG-CF profile: use screws, or switch to unfilled PETG. If the user insists, the house profile's clip limits apply. Allowed in PLA (barely), PETG, ABS, ASA, nylon, and TPU.

For a rectangular cantilever, strain is about:

```
ε = 1.5 · y · t / L²
```

`y` is how far the hook must deflect, `t` is thickness in the bending direction, `L` is arm length. Keep repeated-use strain under about 1% for PLA, 2% for PETG/ABS/ASA, and 3% for nylon. Example: t = 1.6, y = 0.8, ε = 0.02 gives L ≈ 10 mm, so make the arm 12–15 mm.

- Lay the arm in XY so it bends across lines, not along a single layer.
- Root fillet ≥ 0.5 × t. A taper toward the tip spreads strain.
- Lead-in face around 30–45°. A 90° return face is permanent; 45–60° can be opened again.
- Leave a way to release a permanent catch, or the user will pry the arm off.

Living hinges that fold flat are a polypropylene (and sometimes nylon or TPU) feature. PLA, PETG, and PETG-CF crack. Do not call a thin PETG bridge a living hinge.

## Printed threads and pins

- Printed threads: pitch at least 2 mm, trapezoidal profile, about 0.3–0.4 mm radial clearance, chamfer the first turn. Fine machine threads below about M5 belong in an insert.
- Print the thread axis along Z when cosmetics and fit matter. A horizontal thread is a stack of overhangs.
- Print-in-place hinges: 0.4–0.5 mm clearance, hinge axis parallel to the bed, conical knuckle ends so they self-support.
- A separate pin can be a screw, a bit of 1.75 mm filament, or a cut length of rod. Size the knuckle hole to the pin plus the slip clearance.

## Magnets

Pocket diameter = magnet + 0.1–0.2 mm for a press, or +0.3 mm if it will be glued. Depth = magnet thickness + 0.2 mm. Chamfer the mouth.

Fully captured magnets: pause at the layer that closes the pocket, drop the magnet in, resume. Tell the user the Z height; how to set the pause is in `mesh-and-export.md`. Magnet polarity is part of the design when two parts meet; mark it in the reply.

On a brittle material, a press fit that is too tight cracks the pocket. Prefer the loose pocket plus a drop of glue, or a thin printed cap, over a heroic interference.
