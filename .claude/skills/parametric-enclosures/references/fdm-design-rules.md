# FDM design rules for enclosures

Generic numbers for a 0.4 mm nozzle, 0.2 mm layers and a reasonably tuned printer.
**A manufacturing profile (`manufacturing-profiles.md`) overrides everything here** — use
this file for the reasoning behind the rules, for materials without a profile, and to
build a new profile.
Put every tolerance in a named parameter so one test print can retune the whole
model.

## Contents
1. Thickness and minimum features
2. Tolerances and fits
3. Holes
4. Overhangs, bridges, supports
5. Print orientation and strength
6. Edges, corners, first layer
7. Materials
8. Calibration print (recommend when fits matter)

## 1. Thickness and minimum features

| Feature | Minimum | Default | Why |
|---|---|---|---|
| Side wall | 1.2 mm (3 lines) | 2.0 mm | ≥ 2 perimeters each side of infill; stiff, opaque |
| Floor / lid plate | 1.2 mm (6 layers) | 2.0 mm | top surfaces need ≥ 4–5 solid layers to close |
| Rib / gusset | 0.8 mm (2 lines) | 1.2 mm | one-line features are fragile and may be dropped by the slicer |
| Screw boss wall | 2.0 mm | profile (2.5 mm PETG-CF) | see closures reference |
| Vertical pin | Ø 3 mm | — | thinner pins wobble and break at layer lines |
| Debossed text on top face | 0.4 mm deep, 5 mm tall, bold | 0.6 mm | 2–3 layers reads clearly |
| Embossed text on side wall | 0.6 mm proud, 5 mm tall | — | stroke ≥ 2 line widths |

Make wall thickness a multiple of the extrusion width (≈0.42–0.45 mm) to avoid
gap fill: 1.2 / 1.6 / 2.0 / 2.4 mm are all good.

## 2. Tolerances and fits (per side, printed part to printed part)

| Fit | Clearance | Use |
|---|---|---|
| Press / interference | 0.0–0.1 mm | pins that should stay |
| Snug / friction | 0.1–0.15 mm | friction lids, light-pipe inserts |
| Sliding | 0.2–0.3 mm | lid lips, sliding lids, button caps in holes |
| Loose / print-in-place | 0.35–0.5 mm | hinges and captured parts printed assembled |

A lid with no tongue, lip, or lap is not in this table. Matching the body's outer
size and resting on the rim does not fit. Open the female feature by the sliding
clearance on each face; do not leave a zero gap.

Printed part to a bought part: board-to-wall ≥ the slip tol (default 0.5 mm; board
outlines vary ±0.2 mm), connector bodies +0.5 mm per side, magnets and bearings
+0.1–0.2 mm on diameter.

## 3. Holes

- Vertical holes (axis along Z) print **undersized** by roughly 0.1–0.3 mm on
  diameter (inscribed polygons + extrusion squish). Add a `hole_comp`
  parameter (default 0.2 mm) to functional diameters, or use `poly_d()`.
- Horizontal holes (axis in XY) print oval with a droopy top. For Ø > ~5 mm,
  use a teardrop (hull of the circle and a point above at 45°) or a flat
  bridged top; for small ones, drill/ream if accuracy matters.
- Small holes (< 2 mm) are unreliable; print a pilot and drill.
- Holes through the first layer on the bed may close slightly from elephant's
  foot — add 0.2 mm or a 0.4 mm chamfer at the bottom.

## 4. Overhangs, bridges, supports

- Overhangs up to 45° from vertical print unsupported; 50–55° usually OK with
  good cooling; beyond that, redesign.
- Bridges (horizontal spans supported at both ends): ≤ 10 mm clean, ≤ 20 mm
  acceptable, longer sag. A lid plate is not a bridge if it prints on the bed.
- Replace unsupported horizontal ceilings over cutouts with 45° chamfers or
  arch/teardrop tops.
- Rectangular cutouts in vertical walls bridge their top edge: keep width
  modest or round/pointed the top.
- Design for **zero supports**: supports scar the surfaces, waste time, and
  trap in small cavities. If a support is truly needed, say where and why.

## 5. Print orientation and strength

- Parts are weakest **between layers** (Z). Put tensile and bending loads in
  the XY plane.
- Base: floor on the bed. Lid: either face down prints without supports. The
  template prints it mating face down, so the screw counterbores open upward.
  Outer face down gives the better show face, but each counterbore then has a
  downward ledge: add a one-layer sacrificial bridge the user drills through.
- Snap-fit beams and flexure buttons: bending axis parallel to the layers, i.e.
  beam lying in XY when possible. A vertical cantilever snaps at its root.
- Screw bosses: screws along Z so tightening compresses rather than peels
  layers; inserts spread the load.
- Thin tall walls (> 60 mm, < 1.6 mm) wobble — add ribs or thickness.
- Large flat parts in ABS/ASA/PC warp: round corners (r ≥ 3 mm), use a brim,
  avoid long sharp-cornered walls.

## 6. Edges, corners, first layer

- Vertical outer corners: round (r 2–5 mm) — better looking, stronger,
  less warping.
- Bottom edges on the bed: 0.4–0.6 mm × 45° chamfer (hides elephant's foot).
  Don't fillet bottom edges; a fillet is an overhang that starts flat. Where a
  chamfer would eat a one-extrusion land (the lid's groove on the bed), leave the
  edge square and keep the slicer's elephant-foot compensation on.
- Any other edge parallel to the bed, including the top lip and the underside of
  a ledge: chamfer. A fillet there steps with the layers and, facing down, starts
  as a flat overhang.
- Vertical edges: fillet. A sharp corner rings. Inside a loaded corner, keep the
  wall thickness constant through the bend (outside radius = inside radius + wall).
- Inner corners where walls meet the floor: fillet, about 1 mm. That corner is
  supported by the floor, and a square corner there is where a brittle wall cracks.

## 7. Materials

| Material | Heat (≈ softening) | Strengths | Watch out | Enclosure fit |
|---|---|---|---|---|
| PLA | ~55–60 °C | easy, crisp, stiff | brittle snaps, creeps under screws, softens in cars/sun | indoor prototypes, low heat |
| PETG | ~75–80 °C | tough, good layer bond, some UV resistance | stringing, can fuse to smooth PEI | **default** for functional enclosures |
| ABS | ~95–100 °C | heat, post-processing | warps, fumes, needs enclosure | warm electronics |
| ASA | ~95–100 °C | ABS + UV stable | warps, fumes | **outdoors** |
| PC / PC-blends | ~110–140 °C | strong, hot | hard to print | high heat |
| TPU 95A | — | flexible, grippy | slow, no fine tolerances | gaskets, bumpers, feet, button membranes |
| Nylon (PA) | ~80–180 °C | tough, snap-fits | absorbs water (dry it) | hinges, clips |
| CF/GF-filled (PETG-CF, PA-CF…) | +10–20 °C over base | stiff, matte, dimensionally stable | abrasive (hardened nozzle), lower layer adhesion, CF can detune antennas nearby | stiff shells; keep CF away from antennas |

Shrink: ABS/ASA ≈ 0.5–0.8 %, PETG ≈ 0.2–0.4 %, PLA ≈ 0.2–0.3 %. Mostly
absorbed by clearances; matters for long parts with tight fits.

None of these common filaments are flame-rated (UL94 V-0) unless the spool says
so. See environment-and-safety.md for mains/battery implications.

## 8. Calibration print

When fits matter (sliding lid, snap-fits, press-fit magnets), offer a small
test coupon before the full print. It costs one short print and sets `tol`,
`hole_comp` and the insert bore for every future design on that
printer/material. If the 3d-printing skill is installed, use its
`assets/fit-coupon.scad`; otherwise print a strip of holes and pegs at 0.05 mm
steps of clearance.
