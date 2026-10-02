// A box with a chamfered lid lip and a through-hole — the "hello world" of
// enclosure-maker. Run with:
//   cargo run -p em-preview -- preview --script examples/basic_box.rhai
//   cargo run -p em-preview -- export --script examples/basic_box.rhai --output basic_box.stl

let width = 40.0;
let depth = 30.0;
let height = 15.0;

let body = cuboid(width, depth, height);

let hole = cylinder(4.0, height + 2.0, 32)
    .translate(0.0, 0.0, -height / 2.0 - 1.0);

emit(body.subtract(hole));
