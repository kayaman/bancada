# House profile

A profile is the printer plus the material the part will actually be made on. It overrides generic numbers in `fdm.md` and `fits-and-fasteners.md`.

Use the default below without asking. If the user names another printer or material, copy this structure, fill the same fields, and recompute every dimension that came from extrusion width. Do not keep 0.4 mm numbers on a 0.6 mm nozzle.

## Default: Bambu Lab A1 Mini + PETG-CF

**Machine.** Bed-slinger, open frame, textured PEI. Build volume 180 × 180 × 180 mm, so the design envelope is **170 × 170 × 170 mm**. Nozzle 0.4 mm hardened steel. Extrusion width `ew = 0.42` mm. Layer height 0.2 mm.

Bambu recommends a 0.6 mm hardened nozzle for this filament to cut clogs. Stay on 0.4 mm for design numbers unless the user says the nozzle changed. If the complaint is clogging or a worn orifice, suggest 0.6 mm and recompute: `ew = 0.62`, wall = 4 × ew.

**Material.** PETG-CF is stiff and dimensionally stable, and it is brittle and notch-sensitive. Elongation is far below unfilled PETG. It is weakest in tension across layer lines. Bridges and overhangs sag more than PLA. Holes print small. The first layer spreads. Heat deflection is roughly 75–85 °C, so this is not a high-temperature material. The carbon fill attenuates 2.4 GHz more than unfilled plastic; keep that in mind near antennas (the enclosure skill uses a 20 mm keepout).

### Geometry overrides

| Parameter | Value | Why |
|---|---|---|
| Wall | `6 * ew` = 2.52 mm | Six perimeters. A brittle material needs section, not ribs alone |
| Floor / ceiling | 3.0 mm (15 layers at 0.2) | Closes the surface and adds bending stiffness |
| Max bridge | 5 mm | PETG-CF sags where PLA still spans |
| Max overhang | 45° from vertical, target 40° | |
| Teardrop threshold | Horizontal holes ≥ 5 mm | |
| Bed-parallel edges | Chamfer. Bed edge 0.8 mm × 45° | A fillet on the bed, or under a lip, starts as a flat overhang |
| Vertical corners | Fillet, R ≥ 1.0 mm | Sharp corners ring, and a square inside corner starts the crack. Keep the wall thickness through the bend |
| `hole_comp` | +0.15 mm on diameter | Functional holes only. Insert bores use the coupon, often +0.2–0.3 mm over the datasheet |
| M3 insert boss | OD ≥ 9.5 mm | ≥ 2.5 mm of plastic around a ~4.0–4.2 mm bore |
| `tol` per side | press 0.15 / slip 0.25 / loose 0.40 | Freeze after one coupon. Do not retune per part |

Wall thickness is always an integer multiple of `ew`. Do not round 2.52 to 2.5.

### No elastic features

Snap fits, cantilever latches, living hinges, and flexing tabs crack on the second cycle in PETG-CF. Closures are screwed. Alignment is a tongue, a lip, or dowel pins.

If a brief explicitly demands a clip, the arm length is at least 10 × thickness, the root fillet is at least 1.5 mm, and deflection stays under 1.5% of arm length. State the fatigue risk in the print plan. The strain formula is in `fits-and-fasteners.md`. Prefer switching to unfilled PETG instead of arguing the clip into this material.

### Envelope, orientation, joining

- Assert every part fits 170 × 170 × 170 mm. If it does not, split it and join with M3 bolts plus at least two dowel pins (Ø 4 mm pockets, 0.20 mm clearance per side). Do not specify glue. PETG bonds poorly with cyanoacrylate and has no practical solvent weld.
- Each part needs a flat face covering at least 60% of its footprint.
- If height exceeds 2.5 × the smallest footprint dimension, reorient, widen the base, or split, and call for a brim. A bed-slinger tips tall skinny parts.
- The bottom face takes the plate texture. Do not put a gasket seat, a sealing lip, or fine debossed detail on it.

### Operator notes for the print plan

- Hardened nozzle is mandatory. Carbon fiber abrades brass within a spool, and a worn nozzle silently changes every hole.
- Dry before a long print: about 65 °C for 8 h in a dryer or convection oven. PETG is hygroscopic and the fiber load makes wet filament worse. Wet filament makes brittle layer bonds, which is the failure mode of a loaded part.
- Do not run this filament through the AMS Lite. Feed from the spool holder so the fiber does not abrade the tubes.
- Glue stick on textured PEI is a release agent, not an adhesive. PETG can bond to bare smooth PEI hard enough to tear the sheet. The A1 Mini's textured plate is the right surface; still mention release if they swap to a smooth plate.
- Keep the part fan modest. Overcooling is a common reason PETG-CF splits between layers. Bambu's own note for this filament is that an open-frame printer and a high fan both hurt Z strength, so design the load path accordingly instead of expecting enclosed-printer layer bonds.
- Slice in Bambu Studio with printer `Bambu Lab A1 mini 0.4 nozzle`, process `0.20mm Standard @BBL A1M`, plate `Textured PEI Plate`, and the PETG-CF filament preset. The setting list is in `slicer-and-troubleshooting.md`. The STL is already one solid on z = 0; Studio should not offer Repair and should not be asked to auto-orient.

## Other profiles worth having ready

Fill the same override table when the user names one of these. Until they do, do not silently switch off PETG-CF.

- **Unfilled PETG or PLA on the same printer.** Elastic features are allowed. Internal fillet can drop to R 0.5 mm. PLA softens around 55–60 °C and does not belong in a car, a window, or next to a heat source. Bridges can go to about 10 mm before sag matters.
- **0.6 mm hardened nozzle.** `ew = 0.62`, wall = 4 × ew = 2.48 mm. Fewer, wider perimeters are stronger in filled materials and clog less.
- **ASA or ABS.** Enclosed printer only. The A1 Mini is open, so say that this profile cannot print them well. Expect shrinkage on the order of 0.5–0.8% and lifted corners on large flat parts. Round corners, add a brim, avoid a broad unbroken base.
- **Unknown printer.** Use `fdm.md` generic values and say which numbers are unverified.

## Calibrating `tol`

Print `assets/fit-coupon.scad` once per printer-material pair, with the same walls, layer height, and filament as the real part. It has three rows: 3 mm holes that set `hole_comp` against a drill shank, holes for a printed pin at each per-side clearance, and blind bores for the M3 insert. Read them in that order, because the clearance row already includes `hole_comp`. The hole that gives the fit you want by hand sets `tol` for every later part.

For a different printer, nozzle, or insert, edit the parameters at the top of the file (`hole_comp`, `clearances`, `insert_bores`, `insert_len`, `boss_od`) and keep the layout. A hole that is tight on one part is almost never a reason to change `tol` globally; check hole compensation and elephant's foot first.
