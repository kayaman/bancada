// A small enclosure base: open-top box, 4 gusseted M3 screw bosses with
// heat-set insert bores, and a row of vent slots on the front wall. Run with:
//   cargo run -p em-preview -- preview --script examples/m3_enclosure.rhai
//   cargo run -p em-preview -- export --script examples/m3_enclosure.rhai --output enclosure.stl

let width = 60.0;
let depth = 40.0;
let height = 20.0;
let wall = 2.0;

let outer = cuboid(width, depth, height);
let cavity = cuboid(width - 2.0 * wall, depth - 2.0 * wall, height).translate(0.0, 0.0, wall);
let shell = outer.subtract(cavity);

let boss_height = height - wall;
let boss_z = -height / 2.0 + wall;
let margin = 6.0;

let corners = [
    [ width / 2.0 - margin,  depth / 2.0 - margin],
    [-(width / 2.0 - margin),  depth / 2.0 - margin],
    [ width / 2.0 - margin, -(depth / 2.0 - margin)],
    [-(width / 2.0 - margin), -(depth / 2.0 - margin)],
];

let result = shell;
for corner in corners {
    let boss = screw_boss(3, boss_height, 4, 1.2).translate(corner[0], corner[1], boss_z);
    let bore = heat_set_bore(3).translate(corner[0], corner[1], height / 2.0);
    result = result.union(boss).subtract(bore);
}

// Vent row on the front (-Y) wall, overshooting the wall thickness on both
// sides for a clean boolean cut.
let vent_cutter_depth = wall + 2.0;
let vents = vent_slot_row(2.0, 15.0, 5, 2.0, vent_cutter_depth)
    .rotate(90.0, 0.0, 0.0)
    .translate(0.0, -depth / 2.0 + wall / 2.0, 0.0);
result = result.subtract(vents);

emit(result);
