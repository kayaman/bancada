// 100 x 60 x 60 mm enclosure with a cutout for a power supply cable/connector
// on the back wall, and a lid held down with 4 M4 screws into gusseted
// bosses (heat-set inserts in the base, clearance holes in the lid). Run
// with:
//   cargo run -p em-preview -- preview --script examples/psu_enclosure.rhai
//   cargo run -p em-preview -- export --script examples/psu_enclosure.rhai --output-dir out --parts base,lid

let width = param("Width", 100.0, 60.0, 160.0);
let depth = param("Depth", 60.0, 40.0, 100.0);
let height = param("Height", 60.0, 30.0, 100.0);
let wall = param("Wall thickness", 2.0, 1.2, 4.0);

let power_hole_w = param("Power hole width", 28.0, 10.0, 60.0);
let power_hole_h = param("Power hole height", 20.0, 8.0, 50.0);
let power_hole_z = param("Power hole vertical offset", -8.0, -20.0, 20.0);

// --- Base: hollow box with 4 gusseted M4 screw bosses -----------------

let outer = cuboid(width, depth, height);
let cavity = cuboid(width - 2.0 * wall, depth - 2.0 * wall, height).translate(0.0, 0.0, wall);
let base = outer.subtract(cavity);

let boss_height = height - wall;
let boss_z = -height / 2.0 + wall;
let margin = 8.0;

let corners = [
    [ width / 2.0 - margin,  depth / 2.0 - margin],
    [-(width / 2.0 - margin),  depth / 2.0 - margin],
    [ width / 2.0 - margin, -(depth / 2.0 - margin)],
    [-(width / 2.0 - margin), -(depth / 2.0 - margin)],
];

for corner in corners {
    let boss = screw_boss(4, boss_height, 4, 1.2).translate(corner[0], corner[1], boss_z);
    let bore = heat_set_bore(4).translate(corner[0], corner[1], height / 2.0);
    base = base.union(boss).subtract(bore);
}

// Power supply cutout on the back (-Y) wall, overshooting the wall
// thickness on both sides for a clean boolean cut.
let power_cutter = rounded_box(power_hole_w, wall + 4.0, power_hole_h, 2.0, 8)
    .translate(0.0, -depth / 2.0 + wall / 2.0, power_hole_z);
base = base.subtract(power_cutter);

// --- Lid: flat chamfered plate with M4 clearance holes over the bosses -

let lid = chamfered_box(width, depth, 2.0, 0.5);
for corner in corners {
    let hole = cylinder(2.3, 4.0, 24).translate(corner[0], corner[1], -2.0);
    lid = lid.subtract(hole);
}
lid = lid.translate(0.0, 0.0, height / 2.0 + 1.0);

emit("base", base);
emit("lid", lid);
view("assembly", base.union(lid));
