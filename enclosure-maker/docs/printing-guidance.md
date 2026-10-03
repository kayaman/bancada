# Dimension and printing guidance for enclosure-maker

Maintained in the parametric-enclosures skill at references/enclosure-maker-agent.md;
copy this resource verbatim into the app's docs/printing-guidance.md when updating it.
This is focused guidance from parametric-enclosures and 3d-printing. It grants no
additional tools or filesystem access. User choices override house defaults.

## Native source and adjustable dimensions

Keep the existing project and its stable named parts rather than starting
over. Expose important independent dimensions as clearly-named, adjustable
values: wall/floor, bore diameter, insert length, relief, boss height and
radial wall, gusset dimensions, clearances and hole positions. Derive mating
features from shared inputs rather than repeating a number in two places.
Use only APIs and conventions documented for this host; don't introduce a
different modeling engine or scripting language mid-project.

Use mm. Identify nominal size, per-side clearance, diameter compensation and
final CAD size. A circular female feature may use nominal male diameter +
2 * per-side clearance + diameter compensation. A coupon-selected insert
bore is already final CAD size: don't apply the diameter compensation a
second time on top of it. Rectangular lid clearances apply per face. Choose
one owner for hole compensation, CAD or slicer, and document it.

## House profile and fasteners

Unless the user/project specifies another profile: A1 Mini, PETG-CF, 0.4 mm hardened
nozzle, 0.42 mm nominal extrusion width, 0.2 mm layers, 170 mm usable design envelope
per axis (180 mm machine volume). House starting targets: 2.52 mm shell wall,
3 mm floor/ceiling, 5 mm bridge, 40° target/45° maximum overhang from vertical,
0.25 mm per-side slip clearance and +0.15 mm general hole-diameter compensation.
These are uncalibrated design targets, not measured machine limits or strength
certification. Check actual slicer perimeter coverage. For a changed nozzle or
material, recompute dependent dimensions and revalidate fits.

Thread size alone does not specify an insert. Use the actual manufacturer/product
for bore shape, diameter, length, mouth treatment and installation guidance. The
house M3 example starts at final CAD bore 4.1 mm, length 5.7 mm, relief 1 mm, square
mouth, boss OD at least 9.5 mm and radial wall at least 2.5 mm. These are assumptions.
Compute effective boss OD as max(requested minimum, profile minimum, final bore +
2 * required radial wall), then derive placement clearances from it. Check material
beneath blind bores, screw engagement/bottom clearance and matching lid-hole centres.
Tie loaded columns into the shell with gussets and suitable root reinforcement.

A host's own fastener/dimension reference can fix bore sizes; if those are
unsuitable for a specific case, build the feature from primitives with
explicit dimensions instead of guessing at an undocumented variant. Make
sure a circular hole's final mesh is fine enough to stay functionally round
on export (check the mesher's tessellation/deflection quality, not just
whatever default it ships with), while preserving intentionally
polygonal features like hex nut traps. A raw shell/hollow operation on
complex geometry is not proof of constant wall thickness. A lid needs a
locating register and explicit clearance. Avoid support on mating surfaces;
favour screwed closures for house PETG-CF and assess load direction across
layers.

## Print orientation and verification

Keep each printable part as its own distinct, named object; use separate,
clearly-marked non-printable geometry only for visualizing an assembly,
exploded view, or section — never ship that alongside the printable output.
Keep print orientation distinct from assembly placement, and bed faces at
z=0. Part placement lives directly on the object itself (its transform in
the document) — keep it consistent with the intended print orientation, and
remember that placement also applies at export time, so double-check a part
still sits on or above the bed after any reposition.

Validate critical dimensions in source, sections and actual exports. Inspect closed
mesh topology, intended body count, bed position and envelope, then slicer layers
for thin walls, islands, bridge spans and overhangs. Sampled mesh checks do not prove
self-intersection freedom, strength or physical fit. A successful preview is not an
STL validation. Prefer correcting generated source to blindly accepting mesh repair.

Only claim checks actually performed with available tools. If this host has only
file-editing tools, use its live preview and report CLI/mesh/slicer/physical checks
as unverified. Do not call missing tools or relax permissions to claim completion.
Record printed calibration results with printer, nozzle, material, orientation and
slicer settings; without measurements, say the values are uncalibrated.
