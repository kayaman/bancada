// Shared library of reusable enclosure parts, imported by other scripts via
// e.g. `import "lib/presets" as presets;` (path resolved relative to the
// importing script's own directory). Functions at a module's top level are
// exported automatically in Rhai -- no `export` keyword needed for `fn`
// (that keyword is only for module-level variables).

// A hollow, open-top box, ready to have a lid unioned on top.
fn standard_base(width, depth, height, wall) {
    let outer = cuboid(width, depth, height);
    outer.shell_open(wall, 0.0, 0.0, 1.0)
}

// A flat lid sized to sit on a `standard_base` of the same width/depth, with
// clearance holes lined up over the base's 4 corner bosses.
fn standard_lid(width, depth, thickness) {
    let plate = cuboid(width, depth, thickness);
    let margin = 6.0;
    let corners = [
        [ width / 2.0 - margin,  depth / 2.0 - margin],
        [-(width / 2.0 - margin),  depth / 2.0 - margin],
        [ width / 2.0 - margin, -(depth / 2.0 - margin)],
        [-(width / 2.0 - margin), -(depth / 2.0 - margin)],
    ];

    let result = plate;
    for corner in corners {
        let hole = cylinder(1.7, thickness + 2.0, 24).translate(corner[0], corner[1], -thickness / 2.0 - 1.0);
        result = result.subtract(hole);
    }
    result
}
