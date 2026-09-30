# Meshes and export

Read this when the user hands over an STL or 3MF, or when you are exporting a model you wrote.

## Reviewing a mesh someone else made

Check these in order and stop at the first one that fails. Each is cheaper than the next.

1. **Units.** STL carries no units, and every slicer assumes millimetres. A part that is 25.4× too small was drawn in inches. One that is 10× or 1000× off was drawn in centimetres or metres. Read the bounding box before anything else and say what you found.
2. **Envelope.** Compare the bounding box against the profile envelope in every orientation you might use, not just the one it arrived in.
3. **Closed, one solid, no island.** Open edges, flipped normals, a second shell, and a region that starts in mid-air are the defects Bambu Studio either offers to repair or prints as spaghetti. Run the checker below and quote its result. A repair can fill a hole the user wanted or drop a thin wall, so a repair prompt is a failed file, not a step to click.
4. **Bed face.** A part with no flat face covering most of its footprint needs a new orientation or a split. The slicer's lay-on-face tool finds candidates; it does not know which face is cosmetic.
5. **Walls and overhangs.** Slice it and read the layer preview. Walls that preview as a single line or as gap fill are thinner than the profile wall. Overhang highlighting (Bambu Studio and OrcaSlicer show it in the prepare view) finds faces past the limit.
6. **Holes and fits.** Measure functional holes in the mesh. A vertical hole drawn at exactly the screw diameter will not take the screw.

```bash
python3 scripts/check_stl.py part.stl
```

`ok` means one closed solid, consistent winding, no collapsed triangles, and no island whose cross-section loses the plastic under it. Anything else is a fail: open edges, non-manifold edges, extra solids, or floating islands. The script is standard-library Python. Quote the line in the reply. Do not claim the check ran when it did not.

A void inside that one solid is allowed. A second body is not, even when both sit on the bed. Bambu Studio keeps them as one object and will happily start the upper one in mid-air.

## Editing a mesh without the source

Small fixes do not need a rebuild. OpenSCAD imports a closed STL and applies booleans to it:

```openscad
difference() {
    import("bracket.stl");
    translate([12, 8, -0.01]) cylinder(d = 3.4 + 0.15, h = 10, $fn = 64);
}
```

That covers opening a hole, adding a chamfer by subtracting a wedge, cutting a split plane for the envelope, and adding dowel pockets. An import that fails `check_stl.py` makes CGAL fail or leave a skin. Close it in the source, or rebuild the part, before the boolean. The checker has to print `ok` on the result. Anything beyond local edits is a rebuild, and the user should decide whether it is worth it.

## One solid, fused, on the bed

Bambu Studio repairs a mesh that is not a closed 2-manifold, and it prints a disconnected shell as its own part. Both show up before any strength question matters.

Build every feature into the body:

- A boss, rib, pin, foot, or embossed letter overlaps the body by at least `eps` (0.01 mm). Touching along a face, an edge, or a point becomes a second shell or a non-manifold edge after the boolean.
- The neck that joins them is at least two extrusion widths. An overlap of 0.01 mm is one solid and still a hair that snaps off.
- Deboss text by subtracting it. Emboss by union. Letters placed on the surface without entering it are floating shells.
- Cutters in a `difference()` run past the faces they cut by `eps`. A cutter that ends on a face leaves a zero-thickness skin. That skin is what makes the slicer offer Repair.
- Two positive solids that only meet at a shared face get the same treatment: overlap them, or the export grows open edges.
- One file, one solid. A plate and a pin side by side, or a lid sitting on a base, is two solids. `assets/fit-coupon.scad` exports `plate` and `pin` separately for this reason. `part="all"` is a preview and fails the check.
- In print orientation the bed face is the plane z = 0, and every region above it still has plastic underneath, or it is a bridge no longer than the profile allows with walls at both ends. A lump that rejoins the body only at the top is floating. The checker flags the height where that cross-section loses its support.

OpenSCAD's `Simple: yes` on the console is necessary and not sufficient. The STL is the file Bambu Studio opens. Check that.

## What makes Bambu Studio offer Repair

| Defect | Usual cause in a model you wrote |
|---|---|
| Open edges | Cutter ended on a face; imported mesh was already open |
| Non-manifold edge | Three faces meet, or two solids were unioned along a shared face only |
| Inconsistent winding | A face flipped inside a boolean |
| Collapsed triangle | Vertices welded together by a zero-thickness skin |
| Extra solid | Feature placed against the body instead of into it; two parts in one file |
| Floating island | Region whose layer does not meet the layer below, often a bar that only connects higher up |

Do not send the user to the Repair button, to "Split to objects", or to auto-orient. Fix the source and export again.

## Exporting a model you wrote

- **One file per part**, each placed in its print orientation with the bed face on z = 0. The slicer should not need to rotate it, and it should not need Lay on face. The export command lists every part. Run `scripts/check_stl.py` on each file and keep the `ok` line.
- **Resolution.** A circle with `n` segments is a polygon inside the true circle, so a hole prints smaller before the plastic does anything. Across flats the diameter is `d · cos(180° / n)`. At `$fn = 32` that is about 0.5% under, at 64 about 0.1%. Use `$fa = 1` and `$fs = 0.2` or an explicit `$fn` ≥ 64 on functional holes for export. A low `$fn` in preview is fine.
- **Coincident faces.** Extend every subtracted solid past the faces it cuts by a small `eps` (0.01 mm). A difference that ends exactly on a face leaves a zero-thickness skin that some slicers print.
- **STL or 3MF.** STL is universal. 3MF carries units, several objects, and in Bambu Studio and OrcaSlicer a whole project with its settings. OpenSCAD writes 3MF only when it was built with lib3mf (`openscad --info` shows it); otherwise export STL and say so.
- **Render time.** OpenSCAD's CGAL backend on older releases takes minutes on parts with many holes or text. Newer builds with the Manifold backend are much faster. A slow export is not a broken model.

## Pausing to embed a part

For a captured magnet, nut, or insert that must sit inside the print, give the Z height of the first layer that closes the pocket, rounded up to a whole layer. That is the layer to pause before. In Bambu Studio and OrcaSlicer, right-click that layer on the preview slider and add a pause. PrusaSlicer uses the same slider with a colour-change or pause marker. Tell the user which parts go in and, for magnets, which way up.
