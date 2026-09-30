# Slicer plan and failed prints

The slicer executes the design. It does not rescue a horizontal hole, a peel load, or a wall that is not a whole number of lines. Set the plan from the profile, then change one thing at a time when a print fails.

## Functional print plan (0.4 mm nozzle)

Use these unless the profile says otherwise. The house PETG-CF profile overrides walls, floors, bridges, and fan behavior.

| Setting | Working choice | Why |
|---|---|---|
| Layer height | 0.20 mm | Default. 0.12–0.16 mm when a vertical curve must look smooth. Up to about 75% of the nozzle (0.28 mm on a 0.4) for a draft. Stay at or above 0.16 mm on PETG-CF if the nozzle is collecting blobs |
| Walls | 4 for a normal functional part; 6 on house PETG-CF | Strength is in the perimeters. Raising infill from 20% to 80% does less than adding two walls, and it increases warp |
| Top / bottom | ≥ 4 solid layers | So infill does not show through and the skin can take a compressive load |
| Infill | 15–20% gyroid, or cubic when gyroid shakes the machine | The walls carry bending. Infill supports the top skins and helps in compression. See the infill section below |
| Supports | None | If a face was sacrificed on purpose, name it and paint support only there |
| Brim | When the profile says so, or the footprint is small | House rule: height > 2.5 × the smallest base dimension |
| Seam | A hidden corner, not a sliding face | The seam is a ridge |
| Elephant foot | Compensation around 0.2 mm if the slicer has it, after the CAD chamfer | Backup for the first-layer squish |
| Cooling | High on PLA. Modest on PETG, ABS, nylon, and filled PETG | Fan makes prettier overhangs and weaker layer bonds. On an open printer with PETG-CF, prefer the bond |

Outer walls slightly slower than infill. That is a surface setting, not a strength setting.

Arachne / variable line width can fill an awkward gap. Still design to `n * ew` so the part does not depend on it. Prusa's note on the perimeter generator still applies in Bambu Studio: Arachne is the better default, and Classic holds a concave corner or a groove width more accurately when that corner is the fit.

## Infill

Prusa's rule, and it matches what fails on a real part: bending strength is the wall loops. Adding two loops does more than raising infill from 20% to 50%, and dense infill warps. Infill's job is to hold the top skins up and to resist crushing. Most functional parts are fine at 15–20%. Above about 30% is for a pad that is actually crushed. 100% infill is rectilinear, slow, and a common way to make a lump warp. It is not how you make a part strong.

Pattern, from Bambu's own fill-pattern notes:

| Pattern | Use it when |
|---|---|
| Gyroid | The load direction is not one axis. Lines do not cross inside a layer, so the nozzle does not hit its own infill. It vibrates on a bed-slinger once the part is wide or the density is high, and the slice is slower |
| Cubic | You want the same kind of all-direction support and the machine is shaking, or the part is large. Lines cross, so keep the density modest or blobs at the crossings telegraph through the top skin |
| Cross Hatch, rectilinear, zig-zag | A cosmetic shell. Bambu calls Cross Hatch suitable for non-load-bearing parts |
| Lightning | A figurine. It only supports the top skin |
| Grid, dense triangles | Avoid on a functional part. Crossings scrape, and triangles bridge a long gap under the top layers |

On the house profile the default is gyroid at 15–20%. Switch that one part to cubic when the A1 Mini shakes. Do not switch the whole profile to Cross Hatch because a print was slow.

The first solid layer bridges across the infill, so a sparse pattern needs enough top shell. Four layers is the working minimum. The house profile's 3 mm floor and ceiling already cover it. A wide flat top still sags if the infill spacing is long. Add top layers, or a modifier that densifies only the last few millimetres under that face. Do not raise the infill of the whole part to fix one roof.

A fastener boss is solid perimeters, not a high infill percentage. Dense infill belongs under a crushing face, as a modifier on that volume only. Leave infill/wall overlap at the preset. PETG in particular needs the infill anchored into the inner wall. An overlap of zero leaves a shell with loose fill inside it.

For a flat face someone looks at, set the top surface pattern to Monotonic. The sparse pattern stays gyroid or cubic. The bed face takes the plate texture, so the bottom pattern does not matter on the textured PEI.

## Setting names by slicer

Write the print plan with the name the user's slicer shows. The house printer is a Bambu, so default to Bambu Studio names.

| Plan term | Bambu Studio / OrcaSlicer | PrusaSlicer | Cura |
|---|---|---|---|
| Walls | Wall loops | Perimeters | Wall Line Count |
| Top / bottom | Top shell layers / Bottom shell layers | Solid layers: Top / Bottom | Top Layers / Bottom Layers |
| Infill | Sparse infill density, Sparse infill pattern | Fill density, Fill pattern | Infill Density, Infill Pattern |
| Supports | Enable support, plus support painting | Generate support material, plus paint-on supports | Generate Support, plus support blockers |
| Brim | Brim type | Brim width | Build Plate Adhesion Type |
| Seam | Seam position | Seam position | Z Seam Alignment |
| Elephant foot | Elephant foot compensation | Elephant foot compensation | Initial Layer Horizontal Expansion (negative) |
| Hole compensation | X-Y hole compensation | None for holes only; do it in CAD | Hole Horizontal Expansion |

Prefer `hole_comp` in the model over the slicer's hole compensation. A slicer value applies to every hole, including the ones that were already sized, and it is invisible to whoever opens the model next. Use one or the other, never both.

## Bambu Studio on the A1 Mini

This is the default slicer for the house profile. Write the print plan with these names. The mesh is already one solid on z = 0, so the prepare view is for checking, not for repairing or reorienting.

| | Use |
|---|---|
| Printer | Bambu Lab A1 mini 0.4 nozzle |
| Process | 0.20mm Standard @BBL A1M |
| Plate | Textured PEI Plate |
| Filament | The PETG-CF preset that matches the spool. A PLA preset left selected will print the carbon spool with the wrong temperature and fan |

Then set, and only set, what the profile overrides:

| Setting | Value on house PETG-CF |
|---|---|
| Layer height | 0.2 mm |
| Wall loops | 6 |
| Top shell thickness / Bottom shell thickness | 3 mm |
| Sparse infill density | 15–20% |
| Sparse infill pattern | Gyroid. Cubic if that part shakes the machine. Not Grid, not Lightning |
| Infill/wall overlap | leave the preset |
| Top surface pattern | Monotonic on a flat face someone looks at |
| Enable support | off |
| Brim type | No-brim, or Outer brim only (brim width 5 mm) when height > 2.5 × the smallest base side |
| Seam position | Back, on a hidden corner |
| Wall generator | Arachne. Classic when a groove or an inside corner is the fit |
| Precise wall | on when a fit matters |
| X-Y hole compensation | 0 |
| X-Y contour compensation | 0 |
| Elephant foot compensation | leave the preset, about 0.15 mm. The bed-edge chamfer already removed the foot from the mating profile |
| Detect thin wall | may stay on. The part must still be whole extrusion widths without it |
| Make overhangs printable | off. It rewrites the model |
| Order of walls | Inner/outer/inner |

On the send dialog, turn Flow dynamics calibration on for a functional fit. Feed PETG-CF from the external spool holder. The house profile already bans the AMS Lite for this filament.

After slicing, the preview is the last check: no travel move that starts a region with nothing under it, and no overhang highlight on a fit, seal, or sliding face. A warning triangle on the object, or a dialog that the model needs repair, means go back to `scripts/check_stl.py` and the CAD. The repaired mesh is a different part.

## Calibration order

Do this once per material, not once per part. Stop at the first step that explains the error.

1. **Dry the spool.** Wet filament imitates every other fault.
2. **Flow.** A single-wall cube should measure as the extrusion width you set. Fix flow before any clearance.
3. **Pressure advance** (linear advance). Corners stop bulging, holes get rounder. This is why a "calibrated" printer suddenly misses fits after a nozzle change.
4. **Temperature.** Only for a new material. Too cold splits layers; too hot strings and sags.
5. **Fit coupon** (`assets/fit-coupon.scad`) for `hole_comp`, then `tol`, then the insert bore. The header of that file says how to read each row.

A single tight hole is hole compensation, elephant's foot, or a worn nozzle. It is not a reason to redo the whole calibration or to scale the STL.

## Diagnosing a failed print

Name one cause, the observation that would confirm it, and one fix. Geometry first when both could be true.

| What the user sees | Most likely cause | Confirm | Fix |
|---|---|---|---|
| Popping, stringing, cloudy or rough surface | Wet filament | Sound, or it got worse after the spool sat out | Dry it. Then drop nozzle temperature about 5 °C if hairs remain |
| Splits along layer lines, part is brittle | Weak welds: wet, too much fan, too cold, too fast, or the load is in Z | The crack is flat and between layers, not through a wall | Dry, less fan, +5–10 °C, slower. If the crack is where the design peels layers, reorient. On open-frame PETG-CF, do not expect enclosed-printer Z strength |
| Corners lift, base warped | Shrinkage plus a sharp, wide base | Worse in ABS/ASA/nylon; starts at the corners | Brim, round the corners, enclosure. Do not fight ABS on the open A1 Mini |
| Base flange, parts will not seat | Elephant's foot | Only the first layer is fat | CAD chamfer, then slicer compensation. Less first-layer squish if adhesion allows |
| Screws and pins too tight, outside dimensions OK | Vertical holes print small | A drilled hole of the same CAD size fits | `hole_comp`, or drill. Do not scale the whole part |
| PETG stuck to the sheet, sheet at risk | Bare smooth PEI | The part is fused, not just stuck | Glue stick as a release layer. Prefer the textured plate |
| Clogs, or dimensions drifting over a spool of filled filament | Brass nozzle wearing, wet filament, or a 0.2 mm nozzle | Orifice looks oval, or fiber filament was the change | Hardened steel, ≥ 0.4 mm, 0.6 mm if clogs continue. Dry |
| Echoes after corners | Speed and acceleration | The model is fine; the ghost follows direction changes | Slower outer wall. Input shaping is a printer calibration |
| Fit face is scarred and undersize | Support was on a working face | The bad face was an overhang | Reorient or chamfer so that face is a bed face or a vertical wall |
| Layer shift | Mechanical: belt, nozzle crash, warped part catching the nozzle | The shift is a single step in X or Y | Check the machine. A catching overhang is still a geometry problem |
| Spaghetti after the first layers | Lost adhesion, or a feature printed in air | The first layer let go, or a mid-air island exists | Brim or a cleaner plate for adhesion. An island with no support is a modeling error. `check_stl.py` names the height |
| Bambu Studio offers Repair, or a warning triangle on the object | Open edges, flipped faces, a zero-thickness skin, or a non-manifold edge | `check_stl.py` reports open edges, non-manifold edges, or degenerate triangles | Fix the CAD: cutters past the surface, features overlapped into the body. Do not click Repair |
| A second lump, or a piece that starts in the air | Two solids in one file, or a feature joined only at a face or only higher up | `check_stl.py` reports extra solids or a floating island | One solid per file. Overlap the feature in, with a neck of at least two extrusion widths |

If the user sends a photo or a description that matches two rows, take the row that is fixed by drying or by orientation before the row that is fixed by a long calibration.

## What not to change

- Do not add a raft to hide a warped design.
- Do not raise infill to 100% to fix a layer split.
- Do not turn on supports for the whole part because one ledge is 50°.
- Do not recommend a different filament until the current one has been dried and the load path has been checked. A material swap is a real answer when the temperature or the toughness is wrong, and a stall when the part is wet.
