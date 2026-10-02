// Splitting a tall part into two printable halves with self-aligning
// dowels at the cut, for when a model is taller than your printer's bed.
//
// Run with:
//   cargo run -p em-preview -- preview --script examples/split_demo.rhai --part assembly
//   cargo run -p em-preview -- export --script examples/split_demo.rhai --output-dir out --parts top,bottom

let width = 30.0;
let depth = 30.0;
let height = 80.0;

let tower = rounded_box(width, depth, height, 3.0, 24);

// Cut at mid-height (z=0, since the box is centered on the origin), plane
// normal pointing up: split()'s first returned piece is the side the
// normal points away from (the top half here), the second is the bottom.
let halves = tower.split_with_dowels(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 3, 2.0, 5.0);
let top = halves[0];
let bottom = halves[1];

emit("top", top);
emit("bottom", bottom);
// A quick visual check that the dowels actually line up: the two halves
// pushed back together should look identical to the original tower.
view("assembly", top.union(bottom));
