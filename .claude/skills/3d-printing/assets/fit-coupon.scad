// Fit coupon for one printer-material pair. Sets tol, hole_comp, and the heat-set insert bore.
// Print with the same nozzle, layer height, walls, and filament as the real part.
// Plate flat on the bed, pin standing up, no supports.
//
// Export:
//   openscad -D 'part="plate"' -o fit-coupon-plate.stl fit-coupon.scad
//   openscad -D 'part="pin"'   -o fit-coupon-pin.stl   fit-coupon.scad
//
// Reading it (in this order):
//   Row B, middle: 3.00 mm nominal holes, labelled with the hole_comp candidate.
//     Test with a 3 mm drill shank. The smallest hole it slides into by hand is hole_comp.
//     If that differs from hole_comp below, set it and reprint before trusting row A.
//   Row A, back: holes for the printed pin, labelled with the per-side clearance.
//     Push the pin in from the top face. The label of the hole with the fit you want is tol.
//   Row C, front: blind bores for heat-set inserts, labelled with the bore diameter.
//     The smallest bore that takes the insert flush, without a cracked boss or a spinning
//     insert, is the bore for that insert.

part = "plate"; // plate | pin | all
// Export plate and pin as separate STLs. part="all" is two solids and fails check_stl.py.

hole_comp = 0.15;
pin_d = 6.0;
pin_h = 10.0;
clearances = [0.10, 0.15, 0.20, 0.25, 0.30, 0.40];
comp_trials = [0.00, 0.10, 0.15, 0.20, 0.30];
ref_d = 3.0;
insert_bores = [3.9, 4.0, 4.1, 4.2, 4.3];
insert_len = 5.7;
boss_od = 9.5;
boss_h = 9.0;

plate_t = 4.0;
corner_r = 3.0;
bed_chamfer = 0.8;
mouth_chamfer = 0.4;
insert_lead = 0.5;
boss_fillet = 1.0;
label_depth = 0.4;
label_size = 3.2;
font = "Liberation Sans:style=Bold";

pitch = 12.5;
margin = 6.0;
row_a = 47;
row_b = 31;
row_c = 14;

eps = 0.01;
$fn = 96;

cols = max(len(clearances), len(comp_trials), len(insert_bores));
plate_w = 2 * margin + (cols - 1) * pitch + boss_od;
plate_d = row_a + (pin_d / 2 + 0.5) + margin;

function col_x(i) = margin + boss_od / 2 + i * pitch;
function fmt2(v) = let(c = round(v * 100)) str(".", c < 10 ? "0" : "", c);
function fmt1(v) = let(t = round(v * 10)) str(floor(t / 10), ".", t % 10);

module outline(inset = 0) {
    offset(delta = -inset) offset(r = corner_r) offset(delta = -corner_r)
        square([plate_w, plate_d]);
}

module slab() {
    hull() {
        linear_extrude(eps) outline(bed_chamfer);
        translate([0, 0, bed_chamfer]) linear_extrude(plate_t - bed_chamfer) outline();
    }
}

module through_hole(d) {
    translate([0, 0, -eps]) cylinder(d = d, h = plate_t + 2 * eps);
    translate([0, 0, -eps]) cylinder(d1 = d + 2 * mouth_chamfer, d2 = d, h = mouth_chamfer + eps);
    translate([0, 0, plate_t - mouth_chamfer]) cylinder(d1 = d, d2 = d + 2 * mouth_chamfer, h = mouth_chamfer + eps);
}

module label(s, x, y) {
    translate([x, y, plate_t - label_depth])
        linear_extrude(label_depth + eps)
            text(s, size = label_size, font = font, halign = "center", valign = "center");
}

module boss() {
    cylinder(d = boss_od, h = boss_h);
    translate([0, 0, plate_t - eps]) cylinder(d1 = boss_od + 2 * boss_fillet, d2 = boss_od, h = boss_fillet);
}

module insert_bore(d) {
    depth = insert_len + 1;
    translate([0, 0, boss_h - depth]) cylinder(d = d, h = depth + eps);
    translate([0, 0, boss_h - insert_lead]) cylinder(d1 = d, d2 = d + 2 * insert_lead, h = insert_lead + eps);
}

module plate() {
    difference() {
        union() {
            slab();
            for (i = [0 : len(insert_bores) - 1]) translate([col_x(i), row_c, 0]) boss();
        }
        for (i = [0 : len(clearances) - 1]) {
            translate([col_x(i), row_a, 0]) through_hole(pin_d + 2 * clearances[i] + hole_comp);
            label(fmt2(clearances[i]), col_x(i), row_a - 7);
        }
        for (i = [0 : len(comp_trials) - 1]) {
            translate([col_x(i), row_b, 0]) through_hole(ref_d + comp_trials[i]);
            label(fmt2(comp_trials[i]), col_x(i), row_b - 6);
        }
        for (i = [0 : len(insert_bores) - 1]) {
            translate([col_x(i), row_c, 0]) insert_bore(insert_bores[i]);
            label(fmt1(insert_bores[i]), col_x(i), row_c - 8.5);
        }
    }
}

module pin() {
    chamfer = 0.5;
    hull() {
        cylinder(d = pin_d - 2 * chamfer, h = eps);
        translate([0, 0, chamfer]) cylinder(d = pin_d, h = pin_h - 2 * chamfer);
        translate([0, 0, pin_h - eps]) cylinder(d = pin_d - 2 * chamfer, h = eps);
    }
}

if (part == "plate") plate();
else if (part == "pin") pin();
else {
    plate();
    translate([plate_w + 8, plate_d / 2, 0]) pin();
}
