// selftest.scad — renders every module in enclosure_lib.scad
//
// Run after any library change, BEFORE touching product files:
//   openscad -o selftest.stl selftest.scad
// Expected: exits 0, no WARNING lines, manifold STL.
// "may not be a valid 2-manifold" means the slicer would be guessing.

use <enclosure_lib.scad>

EPS = 0.01;
$fa = 1;
$fs = 0.4;
P = 40;  // grid pitch

module cell(ix, iy) { translate([ix * P, iy * P, 0]) children(); }

// Row 0 — 2D primitives and boxes
cell(0, 0) linear_extrude(2) rrect([25, 18], 3);
cell(1, 0) linear_extrude(2) rrect_inset([25, 18], 3, 2);
cell(2, 0) linear_extrude(2) teardrop_2d(8);
cell(3, 0) rbox([25, 18, 10], 3);
cell(4, 0) rbox_c([25, 18, 10], 3, 0.8, 0.8);
cell(5, 0) linear_extrude(2) rrect_c([20, 10], 2);
cell(6, 0) linear_extrude(2) rrect([20, 10], 0);

// Row 1 — fillets and patterns
cell(0, 1) union() { cube([20, 20, 2]); cube([2, 20, 12]); translate([2, 0, 2]) fillet_lin(1.5, 20); }
cell(1, 1) union() { cylinder(d = 10, h = 12); fillet_ring(1.5, 5); }
cell(2, 1) linear_extrude(2) slot_pattern_2d([30, 20], 2, 4.5);
cell(3, 1) linear_extrude(2) hex_pattern_2d([30, 30], 4, 1.6);
cell(4, 1) linear_extrude(2) keyhole_2d(7, 3.5, 8);
cell(5, 1) linear_extrude(2) lip_2d([28, 20], 3, 1.2, 1.2);

// Row 2 — bosses
cell(0, 2) insert_boss(h = 14, od = 9.5, bore = 4.1, insert_len = 5.7,
                       gussets = 3, gusset_t = 2.52);
cell(1, 2) insert_boss(h = 14, od = 9.5, gussets = 3, gusset_t = 2.52,
                       angle0 = 90, spread = 90);
cell(2, 2) boss(h = 16, od = 9.5, bore = 4.1, bore_depth = 6.7,
                gussets = 2, gusset_h = 10, gusset_l = 8, gusset_t = 2.52);
cell(3, 2) pcb_standoff(h = 6, od = 6, bore = 2.4, bore_depth = 6);
cell(4, 2) pcb_standoff(h = 8, od = 7, bore = 2.4, bore_depth = 6,
                        gussets = 3, gusset_t = 2.0);

// Row 3 — cutters through a plate
cell(0, 3) difference() { translate([-10, -10, 0]) cube([20, 20, 4]);
                          screw_cut(3.4, 4, "counterbore", 6.0, 3.0); }
cell(1, 3) difference() { translate([-10, -10, 0]) cube([20, 20, 4]);
                          screw_cut(3.4, 4, "countersink", 6.0); }
cell(2, 3) difference() { translate([-10, -10, 0]) cube([20, 20, 4]);
                          screw_cut(3.4, 4, "none"); }
cell(3, 3) difference() { translate([-10, -10, 0]) cube([20, 20, 4]);
                          hex_nut_trap_cut(5.8, 2.6); screw_cut(3.4, 4, "none"); }
cell(4, 3) difference() { translate([-10, -10, 0]) cube([20, 20, 10]);
                          translate([0, 0, 5]) rotate([90, 0, 0]) translate([0, 0, -10])
                              teardrop_cut(6, 20); }

// Row 4 — closure and mounting
cell(0, 4) lip_tongue([28, 20], 3, 1.2, 1.2, 2);
cell(1, 4) difference() { rbox([28, 20, 4], 3); lip_groove_cut([28, 20], 3, 1.2, 1.2, 2.25, 0.25); }
cell(2, 4) difference() { translate([-10, -14, 0]) cube([20, 24, 3]); keyhole_cut(7, 3.5, 8, 3); }
cell(3, 4) difference() { translate([-10, -10, 0]) cube([20, 20, 4]); foot_recess_cut(8, 1); }
cell(4, 4) difference() { translate([-14, -6, 0]) cube([28, 12, 3]);
                          translate([0, 0, 3]) label_cut("TEST v2", 4, 0.4); }
cell(5, 4) label("EMB", 6, 0.6);
cell(6, 4) snap_hook(12, 1.6, 6, 0.8, 2);
cell(7, 4) {
    o = [30, 24, 12]; w = 1.26;
    difference() {
        rbox(o, 3);
        translate([w, w, 1.2]) rbox([o[0] - 2 * w, o[1] - 2 * w, o[2]], 3 - w);
    }
    translate([0, 0, o[2]]) rim_band([o[0], o[1]], 3, w, 2.34, 1.5);
    translate([0, 0, o[2] - EPS]) lip_tongue([o[0], o[1]], 3, 0.67, 1.0, 2 + EPS);
}

// Row 5 — face frames on a hollow box (all six faces)
cell(0, 5) {
    o = [30, 30, 20]; w = 2;
    difference() {
        rbox(o, 2);
        translate([w, w, w]) cube([o[0] - 2 * w, o[1] - 2 * w, o[2]]);
        for (f = ["front", "back", "left", "right"]) {
            wall_cut(f, o, w) translate([15, 10]) rrect_c([8, 4], 1);
            wall_cut_chamfer(f, o, 1) translate([15, 10]) rrect_c([8, 4], 1);
        }
        wall_cut("bottom", o, w) translate([15, 15]) circle(d = 6);
    }
}

// Row 6 — wiring: anchors on a floor, a channel, cable exits through walls
cell(0, 6) union() {
    translate([-12, -8, -3]) cube([24, 16, 3]);
    translate([0, 0, -0.01]) tie_anchor(2.5, 1.1, 5, 1.68, 2.0, 0.5, 1.0);
}
cell(1, 6) union() {
    translate([-12, -8, -3]) cube([24, 16, 3]);
    translate([0, 0, -0.01]) tie_anchor(3.6, 1.3, 6, 1.68, 2.0, 0.5, 1.0);
}
cell(2, 6) union() {
    translate([-8, 0, -3]) cube([16, 30, 3]);
    translate([0, 0, -0.01]) cable_channel(30, 6, 5, 1.26, 1.0);
}
cell(3, 6) {
    o = [30, 30, 20]; w = 2.52;
    difference() {
        rbox(o, 2);
        translate([w, w, w]) cube([o[0] - 2 * w, o[1] - 2 * w, o[2]]);
        face_frame("front", o) translate([15, 10, 0]) cable_exit_cut(4.9, w, 0.6, false);
        face_frame("back", o) translate([15, 10, 0]) cable_exit_cut(6.4, w, 0.6, true);
        face_frame("left", o) translate([15, 10, 0]) cable_exit_cut(3.4, w, 0.6, false);
    }
}

echo("selftest: if you see this with no warnings above, the library rendered clean");
