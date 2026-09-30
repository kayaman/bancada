// enclosure_lib.scad — reusable geometry for 3D-printed electronics enclosures
// v2.2 (rim band, wiring helpers). Target OpenSCAD 2021.01+. Units: mm.
//
// Usage:  use <enclosure_lib.scad>
// `use` imports modules and functions but NOT variables, so the project file stays
// the single source of truth for every dimension (and must define its own EPS).
// Pass parameters explicitly rather than relying on defaults.
//
// Conventions
//   * Bodies sit at the origin, +Z up, print-as-oriented unless noted.
//   * Modules named *_cut are cutters and already overshoot by EPS.
//   * Modules named *_2d are profiles; extrude them or pass them as children to
//     wall_cut() / wall_cut_chamfer().
//   * Face coordinates (face_frame): u, v span the face, w = depth INTO the part
//     from its outer surface.
//       front/back : u = world X, v = world Z      left/right : u = world Y, v = world Z
//       bottom/top : u = world X, v = world Y
//
// Index
//   fn   poly_d, tie_anchor_size
//   2D   rrect, rrect_inset, rrect_c, teardrop_2d, hole_2d, slot_pattern_2d,
//        hex_pattern_2d, keyhole_2d, lip_2d
//   3D   rbox, rbox_c, fillet_lin, fillet_ring, boss, insert_boss, pcb_standoff,
//        lip_tongue, rim_band, snap_hook, label, tie_anchor, cable_channel
//   cut  screw_cut, hex_nut_trap_cut, teardrop_cut, lip_groove_cut, keyhole_cut,
//        foot_recess_cut, label_cut, cable_exit_cut
//   face face_frame, wall_cut, wall_cut_chamfer
//   misc explode

EPS = 0.01;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

// Diameter to give an n-gon so its FLATS clear a round part of diameter d.
function poly_d(d, n) = d / cos(180 / n);

// ---------------------------------------------------------------------------
// 2D primitives
// ---------------------------------------------------------------------------

// Rounded rectangle, lower-left corner at origin. r = 0 gives a plain square.
module rrect(size, r = 2) {
    assert(r <= min(size[0], size[1]) / 2, "rrect: corner radius too large");
    if (r <= 0) square(size);
    else hull() for (x = [r, size[0] - r], y = [r, size[1] - r])
        translate([x, y]) circle(r = r);
}

// Rounded rectangle inset by d on all sides, concentric with rrect(size, r).
module rrect_inset(size, r = 2, d = 0) {
    translate([d, d]) rrect([size[0] - 2 * d, size[1] - 2 * d], max(r - d, 0.4));
}

// Rounded rectangle centred on the origin.
module rrect_c(size, r = 1) {
    translate([-size[0] / 2, -size[1] / 2]) rrect(size, min(r, min(size[0], size[1]) / 2));
}

// Teardrop for horizontal holes: circle with a 45° roof pointing +Y.
// In face coordinates +Y is v = up on side walls, so it prints unsupported there.
module teardrop_2d(d) {
    r = d / 2;
    union() {
        circle(r = r);
        rotate(45) square([r, r]);
    }
}

// Round hole profile, or a teardrop (roof toward +Y) when td = true.
module hole_2d(d, td = false) {
    if (td) teardrop_2d(d);
    else circle(d = d);
}

// Vertical stadium slots filling area = [w, h], centred along w.
// Keep pitch >= slot_w + wall: the webs are structure.
module slot_pattern_2d(area, slot_w = 2, pitch = 5) {
    n = floor((area[0] - slot_w) / pitch) + 1;
    x0 = (area[0] - ((n - 1) * pitch + slot_w)) / 2;
    if (n > 0 && area[1] >= slot_w)
        for (i = [0 : n - 1])
            translate([x0 + i * pitch, 0])
                hull() {
                    translate([slot_w / 2, slot_w / 2]) circle(d = slot_w);
                    translate([slot_w / 2, area[1] - slot_w / 2]) circle(d = slot_w);
                }
}

// Pointy-top honeycomb clipped to area = [w, h]; cell = across-flats opening.
// A vertex points up (+Y / v), so it prints unsupported on walls and bridges
// short spans on horizontal faces.
module hex_pattern_2d(area, cell = 4, web = 1.6) {
    dx = cell + web;
    dy = dx * sqrt(3) / 2;
    intersection() {
        square(area);
        for (j = [0 : ceil(area[1] / dy)], i = [-1 : ceil(area[0] / dx)])
            translate([i * dx + (j % 2) * dx / 2, j * dy])
                rotate(30) circle(d = cell / cos(30), $fn = 6);
    }
}

// Keyhole: head opening at origin, slot runs toward -Y so the part hangs down
// onto the screw.
module keyhole_2d(d_big = 7, d_small = 3.5, len = 8) {
    circle(d = d_big);
    hull() {
        circle(d = d_small);
        translate([0, -len]) circle(d = d_small);
    }
}

// Tongue-and-groove ring inset from an outline of `size`.
module lip_2d(size, r = 2, inset = 1.2, w = 1.2) {
    difference() {
        rrect_inset(size, r, inset);
        rrect_inset(size, r, inset + w);
    }
}

// ---------------------------------------------------------------------------
// 3D primitives
// ---------------------------------------------------------------------------

module rbox(size, r = 2) {
    linear_extrude(size[2]) rrect([size[0], size[1]], r);
}

// Rounded box with 45° chamfers: cb on the plate side (elephant-foot relief),
// ct on top. Hull of three convex slabs — cheap and always manifold.
// Also useful as a cavity CUTTER with cb > 0: leaves a 45° fillet-substitute
// where the inner walls meet the floor.
module rbox_c(size, r = 2, cb = 0.8, ct = 0.8) {
    assert(size[2] > cb + ct, "rbox_c: chamfers exceed height");
    hull() {
        linear_extrude(EPS) rrect_inset([size[0], size[1]], r, cb);
        translate([0, 0, cb])
            linear_extrude(size[2] - cb - ct) rrect([size[0], size[1]], r);
        translate([0, 0, size[2] - EPS])
            linear_extrude(EPS) rrect_inset([size[0], size[1]], r, ct);
    }
}

// ---------------------------------------------------------------------------
// Fillets — material added at concave junctions
// ---------------------------------------------------------------------------

// Along a straight concave edge between the z=0 floor and a wall in the YZ plane
// at x=0; material toward +X/+Z, running along +Y for len. Overlaps the floor and
// the wall by EPS so the union fuses instead of touching.
module fillet_lin(r, len) {
    translate([0, len, 0]) rotate([90, 0, 0]) linear_extrude(len)
        difference() {
            translate([-EPS, -EPS]) square([r + EPS, r + EPS]);
            translate([r, r]) circle(r = r, $fn = 32);
        }
}

// Ring at the base of a cylinder of radius R standing on z=0. Reaches up to r
// inside the cylinder, so the union fuses whatever the cylinder's facet count,
// and EPS below z = 0 into the floor.
module fillet_ring(r, R) {
    x0 = max(R - r, 0);
    rotate_extrude($fn = 64)
        translate([x0, 0])
            difference() {
                translate([0, -EPS]) square([R + r - x0, r + EPS]);
                translate([R + r - x0, r]) circle(r = r, $fn = 32);
            }
}

// ---------------------------------------------------------------------------
// Bosses and standoffs
// ---------------------------------------------------------------------------

// Gusseted screw column — the load-bearing element, deliberately opinionated:
//   ribs are tapered by a cone, so every layer sits inside the one below;
//   rib/column junctions are filleted in plan by a morphological closing;
//   the column/floor junction gets a fillet ring.
// h           column height (floor to mating face)
// od          column outer diameter
// bore        blind hole Ø (insert bore or pilot); 0 = none
// bore_depth  from the top face; >= h makes it a through hole
// gussets     rib count; rib i points at angle0 + i*spread (spread default 360/gussets)
// gusset_h/l  rib height at the column / reach from the column wall (default 0.7h, = h: 45°)
// gusset_t    rib thickness (use the wall thickness)
// mouth       lead-in at the bore mouth. 0 on a heat-set insert: a chamfer
//             removes the plastic the top knurl should bite.
module boss(h, od, bore = 0, bore_depth = 0,
            gussets = 3, gusset_h = 0, gusset_l = 0, gusset_t = 2.52,
            angle0 = 0, spread = 0, fillet_r = 1.0, mouth = 0) {

    gh = (gusset_h > 0) ? gusset_h : 0.7 * h;
    gl = (gusset_l > 0) ? gusset_l : gh;
    sp = (spread > 0) ? spread : (gussets > 0 ? 360 / gussets : 0);

    assert(od > bore + 2, "boss: bore too close to the outer wall");
    assert(gh <= h, "boss: ribs taller than the column");
    assert(gh >= gl, "boss: ribs reach further than they rise; raise gusset_h or shorten gusset_l");

    difference() {
        union() {
            cylinder(d = od, h = h);
            if (fillet_r > 0) fillet_ring(fillet_r, od / 2);
            if (gussets > 0 && gl > 0)
                intersection() {
                    linear_extrude(gh)
                        difference() {
                            offset(r = -fillet_r) offset(r = fillet_r)
                                union() {
                                    circle(d = od);
                                    for (i = [0 : gussets - 1])
                                        rotate(angle0 + i * sp)
                                            translate([0, -gusset_t / 2])
                                                square([od / 2 + gl, gusset_t]);
                                }
                            circle(d = od - 1);
                        }
                    cylinder(r1 = od / 2 + gl, r2 = od / 2, h = gh);
                }
        }
        if (bore > 0) {
            bz = bore_depth >= h ? -EPS : h - bore_depth;
            translate([0, 0, bz])
                cylinder(d = bore, h = h - bz + EPS);
            if (mouth > 0)
                translate([0, 0, h - mouth])
                    cylinder(d1 = bore, d2 = bore + 2 * mouth, h = mouth + EPS);
        }
    }
}

// Heat-set insert column. bore/insert_len from the insert datasheet (defaults:
// common M3 4.6 × 5.7 mm). Depth = insert_len + 1.0 relief so the insert never
// bottoms out and splits the column.
module insert_boss(h, od = 9.5, bore = 4.1, insert_len = 5.7,
                   gussets = 3, gusset_h = 0, gusset_l = 0, gusset_t = 2.52,
                   angle0 = 0, spread = 0, fillet_r = 1.0, mouth = 0) {
    boss(h = h, od = od, bore = bore, bore_depth = insert_len + 1.0,
         gussets = gussets, gusset_h = gusset_h, gusset_l = gusset_l,
         gusset_t = gusset_t, angle0 = angle0, spread = spread,
         fillet_r = fillet_r, mouth = mouth);
}

// PCB standoff: same family, light duty.
module pcb_standoff(h, od = 6, bore = 2.4, bore_depth = 6,
                    gussets = 0, gusset_t = 2.52, fillet_r = 1.0) {
    boss(h = h, od = od, bore = bore, bore_depth = min(bore_depth, h),
         gussets = gussets, gusset_t = gusset_t, fillet_r = fillet_r, mouth = 0.5);
}

// ---------------------------------------------------------------------------
// Closures
// ---------------------------------------------------------------------------

// Raised tongue on the base's mating face.
module lip_tongue(size, r = 2, inset = 1.2, w = 1.2, h = 2) {
    linear_extrude(h) lip_2d(size, r, inset, w);
}

// Inward thickening of a thin wall's rim so a tongue-and-groove still leaves one
// extrusion of land each side. Ring from inset `wall` to inset `rim_w`, h tall,
// with a 45° chamfer below it so it prints without support. Top at z = 0: place
// it at the mating face.
module rim_band(size, r = 2, wall = 1.6, rim_w = 2.5, h = 1.5) {
    ch = rim_w - wall;
    translate([0, 0, -h - ch])
        difference() {
            linear_extrude(h + ch) rrect_inset(size, r, wall - EPS);
            hull() {
                translate([0, 0, -EPS]) linear_extrude(EPS) rrect_inset(size, r, wall);
                translate([0, 0, ch]) linear_extrude(h + EPS) rrect_inset(size, r, rim_w);
            }
        }
}

// Matching groove cutter for the lid underside (cuts from z=0 upward by h).
// `tol` is added to the groove width once and split across both flanks, so each
// side only gains tol/2. For a per-side slip clearance, pass 2 * slip as `tol`
// and pass h = tongue height + slip so the tongue does not bottom out.
module lip_groove_cut(size, r = 2, inset = 1.2, w = 1.2, h = 2, tol = 0.25) {
    translate([0, 0, -EPS])
        linear_extrude(h + EPS)
            lip_2d(size, r, inset - tol / 2, w + tol);
}

// Cantilever snap hook on z=0, beam in XZ, hook toward +X.
// Strain (rectangular beam) = 1.5 * hook_depth * thick / length^2 — check it
// against the material limit. Forbidden on the default PETG-CF profile.
module snap_hook(length = 12, thick = 1.6, width = 6, hook_depth = 0.8, hook_h = 2) {
    rotate([90, 0, 0])
        linear_extrude(height = width, center = true)
            polygon([
                [0, 0], [thick, 0],
                [thick, length - hook_h],
                [thick + hook_depth, length - hook_h],
                [thick + hook_depth, length - 0.7 * hook_h],
                [thick, length],
                [0, length]
            ]);
}

// ---------------------------------------------------------------------------
// Cutters
// ---------------------------------------------------------------------------

// Clearance hole through a plate of thickness t; head recess cut from the top.
// type: "counterbore" | "countersink" | "none"
module screw_cut(d = 3.4, t = 3, type = "counterbore", head_d = 6.0, head_h = 3.0) {
    translate([0, 0, -EPS]) cylinder(d = d, h = t + 2 * EPS);
    if (type == "counterbore")
        translate([0, 0, t - head_h]) cylinder(d = head_d, h = head_h + EPS);
    if (type == "countersink")
        translate([0, 0, t - head_d / 2])
            cylinder(d1 = d, d2 = head_d, h = head_d / 2 + EPS);
}

// Hex nut pocket from z=0 up to depth; af = across flats incl. clearance.
module hex_nut_trap_cut(af = 5.8, depth = 2.6) {
    translate([0, 0, -EPS]) cylinder(d = poly_d(af, 6), h = depth + EPS, $fn = 6);
}

// Horizontal-hole cutter, axis +Z, roof point toward +Y.
module teardrop_cut(d, h) {
    translate([0, 0, -EPS]) linear_extrude(h + 2 * EPS) teardrop_2d(d);
}

// Keyhole through a plate of thickness t.
module keyhole_cut(d_big = 7, d_small = 3.5, len = 8, t = 3) {
    translate([0, 0, -EPS]) linear_extrude(t + 2 * EPS) keyhole_2d(d_big, d_small, len);
}

// Rubber-foot recess cut upward into a bottom face lying on z = 0.
module foot_recess_cut(d = 8, depth = 1) {
    translate([0, 0, -EPS]) cylinder(d = d, h = depth + EPS);
}

// Debossed text cutter: occupies z = [-depth, EPS], so place it at the surface.
module label_cut(txt, size = 4, depth = 0.4, font = "Liberation Sans:style=Bold") {
    translate([0, 0, -depth])
        linear_extrude(depth + EPS)
            text(txt, size = size, font = font, halign = "center", valign = "center");
}

// Embossed text body on z = [0, depth].
module label(txt, size = 5, depth = 0.6, font = "Liberation Sans:style=Bold") {
    linear_extrude(depth)
        text(txt, size = size, font = font, halign = "center", valign = "center");
}

// ---------------------------------------------------------------------------
// Wiring — tie points, channels, cable exits
// ---------------------------------------------------------------------------

// Outer size [x, y, z] of tie_anchor() called with the same arguments.
function tie_anchor_size(tie_w = 2.5, tie_t = 1.1, len = 5, side = 1.68,
                         roof = 2.0, clear = 0.5, flare = 1.0) =
    [len + 2 * flare, tie_w + clear + 2 * side + 2 * flare, tie_t + clear + roof];

// Zip-tie anchor standing on z = 0, centred on the origin. The bundle runs along
// Y over it; the strap threads a tunnel along X and closes around the bundle.
// The roof bridges only tie_w + clear, and a 45° flare at the foot stands in for
// the root fillet. Open underneath: place it on a floor.
module tie_anchor(tie_w = 2.5, tie_t = 1.1, len = 5, side = 1.68,
                  roof = 2.0, clear = 0.5, flare = 1.0) {
    s = tie_anchor_size(tie_w, tie_t, len, side, roof, clear, flare);
    body = [len, s[1] - 2 * flare, s[2]];
    tunnel = [s[0] + 2, tie_w + clear, tie_t + clear + EPS];
    difference() {
        union() {
            translate([-body[0] / 2, -body[1] / 2, 0]) cube(body);
            hull() {
                translate([-s[0] / 2, -s[1] / 2, 0]) cube([s[0], s[1], EPS]);
                translate([-body[0] / 2, -body[1] / 2, 0]) cube([body[0], body[1], flare]);
            }
        }
        translate([-tunnel[0] / 2, -tunnel[1] / 2, -EPS]) cube(tunnel);
    }
}

// Two ribs along +Y bounding an open channel, centred on x = 0, standing on z = 0.
// gap is measured between the rib faces above the flare. Keeps a bundle off hot
// parts and out of the lid seam; hold the bundle with tie_anchor(), because a
// printed clip is an elastic feature that not every profile allows.
module cable_channel(len, gap, h, rib = 1.26, flare = 1.0) {
    assert(h > flare, "cable_channel: ribs shorter than their flare");
    for (sx = [-1, 1])
        translate([sx * (gap + rib) / 2, 0, 0]) {
            translate([-rib / 2, 0, 0]) cube([rib, len, h]);
            hull() {
                translate([-rib / 2 - flare, 0, 0]) cube([rib + 2 * flare, len, EPS]);
                translate([-rib / 2, 0, 0]) cube([rib, len, flare]);
            }
        }
}

// Cable exit through a plate of thickness t along +Z: Ø d with a 45° anti-chafe
// chamfer of ch on both faces. td = true adds a roof toward +Y for holes at or
// above the teardrop threshold. On a wall:
//   face_frame(face, outer) translate([u, v, 0]) cable_exit_cut(d, wall, ch, td);
module cable_exit_cut(d, t, ch = 0.6, td = false) {
    assert(t > 2 * ch, "cable_exit_cut: chamfers exceed the wall");
    translate([0, 0, -EPS]) linear_extrude(t + 2 * EPS) hole_2d(d, td);
    hull() {
        translate([0, 0, -EPS]) linear_extrude(EPS) hole_2d(d + 2 * ch, td);
        translate([0, 0, ch]) linear_extrude(EPS) hole_2d(d, td);
    }
    hull() {
        translate([0, 0, t - ch]) linear_extrude(EPS) hole_2d(d, td);
        translate([0, 0, t]) linear_extrude(EPS) hole_2d(d + 2 * ch, td);
    }
}

// ---------------------------------------------------------------------------
// Face frames — cut features into any face of a box of size `outer`
// ---------------------------------------------------------------------------

// Maps face coordinates (u, v, w) to world. w = depth into the part.
module face_frame(face, outer) {
    X = outer[0]; Y = outer[1]; Z = outer[2];
    M = (face == "front")  ? [[1,0,0,0],[0,0, 1,0],[0,1,0,0],[0,0,0,1]]
      : (face == "back")   ? [[1,0,0,0],[0,0,-1,Y],[0,1,0,0],[0,0,0,1]]
      : (face == "left")   ? [[0,0, 1,0],[1,0,0,0],[0,1,0,0],[0,0,0,1]]
      : (face == "right")  ? [[0,0,-1,X],[1,0,0,0],[0,1,0,0],[0,0,0,1]]
      : (face == "bottom") ? [[1,0,0,0],[0,1,0,0],[0,0, 1,0],[0,0,0,1]]
      : (face == "top")    ? [[1,0,0,0],[0,1,0,0],[0,0,-1,Z],[0,0,0,1]]
      : undef;
    assert(!is_undef(M), str("face_frame: unknown face '", face, "'"));
    multmatrix(M) children();
}

// Cut 2D children straight through a wall of thickness `wall`.
module wall_cut(face, outer, wall, extra = 1) {
    face_frame(face, outer)
        translate([0, 0, -extra]) linear_extrude(wall + 2 * extra) children();
}

// 45° lead-in chamfer of depth ch around a CONVEX 2D child on the outer surface,
// so a plug finds the hole without looking. Use alongside wall_cut().
module wall_cut_chamfer(face, outer, ch = 1) {
    face_frame(face, outer)
        hull() {
            translate([0, 0, -EPS]) linear_extrude(2 * EPS) offset(delta = ch) children();
            translate([0, 0, ch]) linear_extrude(EPS) children();
        }
}

// ---------------------------------------------------------------------------
// Preview helper
// ---------------------------------------------------------------------------

// Offset children along +Z for exploded views. Never export through this.
module explode(dz = 30) {
    translate([0, 0, dz]) children();
}
