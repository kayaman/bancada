# Manufacturing profiles

A profile is the printer + material pair the part will actually be made on. It overrides
the generic numbers in SKILL.md and in `fdm-design-rules.md`. Load the default unless the
user names a different one; the template's Process group mirrors this table, so change
both together.

Contents:
- [Default: Bambu Lab A1 Mini + PETG-CF](#default-bambu-lab-a1-mini--petg-cf)
- [Adding a profile](#adding-a-profile)
- [Calibrating `tol`](#calibrating-tol)

---

## Default: Bambu Lab A1 Mini + PETG-CF

Do not ask the user to confirm this profile. Use it unless they name another.

**Machine.** Bed-slinger (Y-moving bed), open frame, textured PEI plate.
Build volume 180 × 180 × 180 mm → design envelope **170 × 170 × 170 mm**.
Nozzle 0.4 mm hardened steel, extrusion width `ew = 0.42`, layer height 0.2 mm.

**Material behaviour to design around.** PETG-CF is stiff and dimensionally stable but
brittle and notch-sensitive — far lower elongation at break than unfilled PETG. It is
anisotropic and weakest in tension across layer lines. Bridges and overhangs droop more
than PLA. Holes come out undersized and the first layer spreads. HDT is roughly
75–85 °C, so it is not a high-temperature material. The carbon fill attenuates 2.4 GHz
more than neutral plastics.

### Geometry overrides

| Parameter | Value | Why |
|---|---|---|
| `wall` | `6 * ew` = 2.52 mm | 6 perimeters; brittle material needs section, not ribs alone |
| floor / ceiling | 3.0 mm (15 layers) | |
| max bridge | 5 mm | PETG-CF sags where PLA holds |
| max overhang | 45°, target 40° | |
| teardrop threshold | horizontal holes ≥ 5 mm | |
| bottom chamfer | 0.8 mm × 45°, every plate-touching edge | elephant foot |
| `hole_comp` | +0.15 mm on diameter | functional holes only, not cosmetic cutouts |
| internal fillet | R ≥ 1.0 mm, **everywhere** | a square internal corner is a crack initiator here |
| min insert boss OD | 9.5 mm for M3 | ≥ 2.5 mm plastic around the insert |
| gussets per column | 3 minimum, always root-filleted | |
| antenna keepout | 20 mm | CF attenuation |
| `tol` | press 0.15 / slip 0.25 / loose 0.40 | calibrate once, then freeze |
| `lip_w` | 1.0 mm (≥ 2 × `ew`) | tongue that still leaves one extrusion of land beside the groove on a 6-line wall |

Wall must always be an integer multiple of `ew`. Do not round it to a pretty number.

### No elastic features

Snap fits, cantilever latches, living hinges and flexing tabs are **forbidden**. They
work in PLA and PETG, and they crack on the second open in PETG-CF. All closures are
screwed; all alignment is tongue-and-groove or dowel pins.

If a brief explicitly demands a clip: arm length ≥ 10 × thickness, root fillet ≥ 1.5 mm,
deflection ≤ 1.5% of arm length, and state the fatigue risk in the build notes. For the
general snap-fit method (strain formula, orientation) see `closures-and-fasteners.md` §6.

### Load paths

Never let a screw load a single layer plane in tension. The pull-out path runs through
the column body, not through a flat lid flange — that flange is a stack of layer bonds
being peeled apart.

### Envelope and orientation

- Assert every part fits 170 × 170 × 170 mm. If it does not, split it and join with M3
  bolts plus ≥ 2 dowel pins (Ø 4 mm printed pockets, 0.20 mm clearance). Never specify
  glue: PETG bonds poorly with cyanoacrylate and has no practical solvent weld.
- Each part needs a flat face covering ≥ 60% of its footprint to print on, support-free.
- Bed-slinger: if a part's height exceeds 2.5 × its smallest footprint dimension,
  reorient it, widen the base, or split it — and call for a brim in the build notes.
- The bottom face takes the plate texture. Never put a gasket seat, a sealing surface or
  fine debossed detail there.

### Operator notes to include in BUILD NOTES

- Hardened nozzle is mandatory; CF abrades brass fast.
- Dry the filament before any long print — roughly 65 °C for 8 h. PETG is hygroscopic
  and the CF load makes it worse; wet filament produces brittle layer bonds, which is
  exactly the failure you do not want in a screw column.
- Do not run CF through the AMS Lite — feed from the spool holder to avoid abrading the
  filament path.
- Glue stick on the textured PEI acts as a release agent, not an adhesive. PETG bonds to
  bare PEI hard enough to damage the sheet.

---

## Adding a profile

When the user names a different printer or material, write a new section with the same
headings and the same override table. The fields that must be filled: build envelope,
`ew`, layer height, wall, max bridge, max overhang, teardrop threshold, `hole_comp`,
minimum internal fillet, insert boss OD, antenna keepout, `tol` triple, `lip_w`, and
whether elastic features are allowed.

Common variations worth knowing:

- **0.6 mm hardened nozzle on the same A1 Mini.** Set `ew = 0.62`, `wall_lines = 4`
  (wall 2.48 mm) and `lip_w = 1.24` (two lines); fewer, fatter perimeters actually
  improve part strength with filled materials. A centred tongue now needs
  `lip_w + 2·ew + 2·tol` = 2.98 mm of rim, so the template adds a rim band under it
  by itself. Recompute everything derived from `ew`; do not carry over the 0.4 mm
  numbers.
- **Unfilled PETG or PLA.** Elastic features become allowed; internal fillet can drop to
  R 0.5 mm; antenna keepout back to 15 mm; 3–4 wall lines are normal, and the
  template's rim band keeps the tongue-and-groove printable on them. PLA's ~55 °C HDT
  makes it unsuitable for anything in a car, a window, or near a regulator.
- **Unknown printer/material.** Use the generic values in `fdm-design-rules.md` and its
  materials table, and say which numbers are unverified.
- **ASA / ABS.** Enclosed printer only. Add shrinkage compensation (~0.5–0.7%) to all
  outer dimensions and expect warped large flat faces — design with ribs and avoid broad
  unbroken bottoms.

## Calibrating `tol`

Print one fit-test coupon per printer-material pair and pick the clearance that gives
the fit you want by hand. The 3d-printing skill's `assets/fit-coupon.scad` measures
`hole_comp`, `tol` (holes for a printed pin at 0.10–0.40 mm per side) and the insert
bore in one print; without it, print a plate with pins at 0.10, 0.15, 0.20, 0.25, 0.30
and 0.40 mm clearance. Feed the numbers into every project and do not change them per
project. This single number is the most common cause of assemblies that worked last
time and do not fit now.
