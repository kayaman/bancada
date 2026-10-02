// A base + lid enclosure. The printable parts are emit()'d (base, lid) so
// they stay separate objects. Preview compositions (assembly, exploded,
// section) are view()'d and are not included in the print download.
//
// Run with:
//   cargo run -p em-preview -- preview --script examples/enclosure_assembly.rhai
//   cargo run -p em-preview -- preview --script examples/enclosure_assembly.rhai --part assembly
//   cargo run -p em-preview -- export --script examples/enclosure_assembly.rhai --output-dir out --parts base,lid

import "lib/presets" as presets;

let width = param("Width", 60.0, 40.0, 120.0);
let depth = param("Depth", 40.0, 30.0, 80.0);
let height = param("Height", 20.0, 12.0, 40.0);
let wall = param("Wall thickness", 2.0, 1.2, 4.0);

let base = presets::standard_base(width, depth, height, wall);

// Lid: a chamfered plate, plus corner clearance holes lined up over the
// base's screw bosses.
let lid_plate = chamfered_box(width, depth, 2.0, 0.5);

let lid = lid_plate;

let margin = 6.0;
let corners = [
    [ width / 2.0 - margin,  depth / 2.0 - margin],
    [-(width / 2.0 - margin),  depth / 2.0 - margin],
    [ width / 2.0 - margin, -(depth / 2.0 - margin)],
    [-(width / 2.0 - margin), -(depth / 2.0 - margin)],
];
for corner in corners {
    let hole = cylinder(1.7, 4.0, 24).translate(corner[0], corner[1], -2.0);
    lid = lid.subtract(hole);
}
lid = lid.translate(0.0, 0.0, height / 2.0 + 1.0);

emit("base", base);
emit("lid", lid);
view("assembly", base.union(lid));
view("exploded", base.union(lid.translate(0.0, 0.0, 30.0)));

let half_space = cuboid(1000.0, 1000.0, 1000.0).translate(500.0, 0.0, 0.0);
view("section", base.union(lid).intersect(half_space));
