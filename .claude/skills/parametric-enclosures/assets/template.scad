// ============================================================================
// <PRODUCT> enclosure — v0.1
// Profile: Bambu Lab A1 Mini, 0.4 mm hardened nozzle, PETG-CF, 0.2 mm layers
//          (see references/manufacturing-profiles.md — change the Process group
//           when the profile changes)
// Parts:   base (floor on the plate) + lid (printed as modeled: mating face down,
//          so the screw counterbores open upward)
// Frame:   origin = outer bottom-left-front corner of the base, +Z up.
//          Board inputs are BOARD coordinates, as on the board's drawing
//          (origin = its bottom-left corner, top view). pcb_rot turns the board
//          CCW to choose which face each edge meets; to_world() converts.
// Layout:  the board sits pcb_clear (+ connector overhang) from every wall that
//          has a connector. Column zones take the sides without connectors: both
//          X ends if free, else front and back, else every free side; a corner
//          column exists where a zone can hold it. A sensor chamber takes the +X
//          end. Zones stretch to clear the antenna keepout; the lid split rises
//          above the tallest cutout. Wiring bays deepen the floor between the
//          board and the front or back wall for tie anchors and cable exits.
// Export:
//   openscad -D 'part="base"' -o base.stl <product>_enclosure.scad
//   openscad -D 'part="lid"'  -o lid.stl  <product>_enclosure.scad
// ============================================================================

use <enclosure_lib.scad>

/* [Part] */
part = "assembly";  // [assembly, exploded, base, lid, section, check]

/* [Project] */
product = "PRODUCT";
version = "v0.1";
lid_label = "";     // debossed on the lid top; "" = none

/* [Process] */
ew          = 0.42;   // extrusion width
layer       = 0.2;
wall_lines  = 6;      // [3:1:10] wall = wall_lines * ew
envelope    = 170;    // usable build envelope per axis
tol         = 0.25;   // calibrated slip fit for this printer/filament pair
hole_comp   = 0.15;   // added to the diameter of FUNCTIONAL holes only
max_bridge  = 5;      // mm
teardrop_min = 5;     // horizontal round holes >= this get a teardrop roof
antenna_keepout = 20; // mm, no inserts or screws inside this radius
elastic_ok  = false;  // snap-fits allowed by this material?

/* [PCB] */
pcb_x       = 50;                                     // ASSUMPTION
pcb_y       = 30;                                     // ASSUMPTION
pcb_t       = 1.6;
pcb_rot     = 0;      // [0, 90, 180, 270] CCW in top view: picks the face each board edge meets
pcb_clear   = 0.5;    // per side (>= tol; clones vary ±0.2)
comp_top    = 8;      // tallest component, top side  // ASSUMPTION
comp_bot    = 2;      // tallest component, bottom side
standoff_min = 5;     // raised automatically for comp_bot and keyhole heads
standoff_od = 6;
standoff_bore = 2.4;  // thread-forming pilot: 2.4 M3, 2.1 M2.5, 1.6 M2
mount_holes = [[3.5, 3.5], [46.5, 3.5], [3.5, 26.5], [46.5, 26.5]]; // ASSUMPTION
rail_w      = 1.5;    // mount_holes = []: rails under the front/back board edges
antenna     = [];     // [x, y] antenna centre; [] = none
heat_sources = [[25, 15]]; // regulator / MCU positions  // ASSUMPTION
heat_w      = 0.5;    // W total dissipation            // ASSUMPTION

/* [Cutouts] */
// [edge, pos, z, body_w, body_h, corner_r, user_facing, overmold, overhang]
//   edge:     board edge the connector sits on: "x0" (x = 0), "x1", "y0", "y1";
//             the wall it meets loses its column zone
//   pos:      connector centre along that edge (board y on x-edges, x on y-edges)
//   z:        connector centre above the board top
//   overmold: [w, h] of the mating plug's body, or [] — recesses the outer face
//             so the plug reaches a socket that sits behind a thick wall
//   overhang: how far the body sticks out past the board edge (0 = flush)
cutouts = [
    ["y0", 25, 1.6, 9.0, 3.2, 1.6, true, [12.5, 7.0], 0]  // USB-C // ASSUMPTION
];
cutout_clear    = 0.5;  // per side around connector bodies
user_port_extra = 0.25; // extra per side on user-facing ports
lead_in         = 1.0;  // 45° lead-in chamfer depth on user-facing ports
cutout_roof     = true; // 45° roof on openings whose top would bridge past max_bridge

/* [Wiring] */
// Floor between the board and the front/back wall for wires, connectors and tie
// anchors; a column zone on that side counts toward it. Keep it 0 on a face with
// connector cutouts: a socket behind a bay is out of reach.
bay_front   = 0;      // mm of floor between the board's front edge and the front wall
bay_back    = 0;      // mm of floor between the board's back edge and the back wall
// [face, pos, z, cable_d]
//   face: "front" | "back"
//   pos:  X of the exit centre from the board's left edge (board x when pcb_rot = 0)
//   z:    centre height above the floor
//   Each exit gets a hole of cable_d + exit_clear with anti-chafe chamfers, and a
//   zip-tie anchor on the floor behind it.
cable_exits = [];     // e.g. [["back", 25, 6, 4.5]]
exit_clear  = 0.4;    // added to the cable diameter
exit_ch     = 0.6;    // anti-chafe chamfer on both faces
tie_w       = 2.5;    // strap width: 2.5 small, 3.6 standard
tie_t       = 1.1;    // strap thickness
tie_len     = 5;      // anchor length along its tunnel
tie_offset  = 6;      // inner wall to anchor centre

/* [Fasteners] */
insert_bore = 4.1;    // M3 heat-set insert — check the datasheet
insert_len  = 5.7;
boss_od     = 9.5;
gussets     = 3;
screw_clear = 3.4;    // M3 (hole_comp added)
head_d      = 6.0;
head_h      = 3.0;

/* [Shell] */
floor_t     = 3.0;
ceil_t      = 3.0;
corner_r    = 3.0;
cb          = 0.8;    // bottom chamfer (elephant-foot relief)
ct          = 0.8;    // lid top chamfer
seam_ch     = 0.4;    // base top outer edge: a shadow line that hides the seam
inner_fillet = 1.0;   // wall/floor and column fillets

/* [Lip] */
lip_w       = 1.0;    // tongue width, >= 2 * ew
lip_h       = 2.0;
rim_h       = 1.5;    // thin walls: straight height of the inward rim band

/* [Ventilation] */
vents        = "auto"; // [auto, on, off]  auto = on above 0.3 W
vent_in_face = "auto"; // [auto, left, right, back, front]  auto = first face without connectors
vent_out_face = "auto"; // [auto, left, right, back, front] auto = opposite the inlet if free
vent_slot_w  = 2.0;

/* [Sensor chamber] */
sensor_chamber = false; // divided, vented chamber at the +X end
chamber_len  = 20;      // interior length along X
chamber_gap  = 3;       // air gap between the divider's two skins
wire_pass    = [6, 4];  // notch (w, depth) at the divider top for wires

/* [Mounting] */
feet        = true;   // rubber-foot recesses
foot_d      = 8;
foot_depth  = 1;
wall_mount  = "none"; // [none, ears, keyholes]
ear_len     = 12;     // ears: reach past each X-end wall
ear_w       = 14;
ear_t       = 4;
ear_hole    = 4.5;    // M4 / #8 wood screw, countersunk
ear_head_d  = 8.5;
keyhole     = [7, 3.5, 8]; // head Ø, shank Ø, travel; the slot runs toward the back face
keyhole_span = 0;     // centre distance between the two keyholes; 0 = half the board

/* [Safety] */
child_pet_safe = false; // toddlers/pets can reach it: ≤ 4 mm openings

/* [Hidden] */
EPS = 0.01;
$fa = $preview ? 6 : 2;
$fs = $preview ? 0.6 : 0.4;

// --- derived -------------------------------------------------------------

function max0(l) = len(l) == 0 ? 0 : max(l);

wall      = wall_lines * ew;
rim_w     = max(wall, lip_w + 2 * ew + 2 * tol);  // one extrusion of land each side
lip_inset = (rim_w - lip_w) / 2;
boss_zone = boss_od + tol;
boss_inset = wall + boss_od / 2 - 0.5;            // fuses the column into the corner
divider_t = 2 * wall + chamber_gap;

// board footprint after rotation, and board -> footprint mapping
fp_x = pcb_rot % 180 == 0 ? pcb_x : pcb_y;
fp_y = pcb_rot % 180 == 0 ? pcb_y : pcb_x;
function rot_pt(p) =
    pcb_rot == 0   ? p
  : pcb_rot == 90  ? [pcb_y - p[1], p[0]]
  : pcb_rot == 180 ? [pcb_x - p[0], pcb_y - p[1]]
  :                  [p[1], pcb_x - p[0]];
function rot_vec(v) =
    pcb_rot == 0 ? v : pcb_rot == 90 ? [-v[1], v[0]]
  : pcb_rot == 180 ? [-v[0], -v[1]] : [v[1], -v[0]];
function edge_pt(e, pos) =
    e == "x0" ? [0, pos] : e == "x1" ? [pcb_x, pos] : e == "y0" ? [pos, 0] : [pos, pcb_y];
function edge_normal(e) =
    e == "x0" ? [-1, 0] : e == "x1" ? [1, 0] : e == "y0" ? [0, -1] : [0, 1];
function vec_face(n) =
    n == [0, -1] ? "front" : n == [0, 1] ? "back" : n == [-1, 0] ? "left" : "right";
function cut_face(c) = vec_face(rot_vec(edge_normal(c[0])));
function cut_overhang(c) = len(c) > 8 ? c[8] : 0;
function cut_grow(c) = cutout_clear + (c[6] ? user_port_extra : 0);
function cut_round(c) = c[3] == c[4] && c[5] >= c[3] / 2;
// height of the 45° roof that keeps an opening's flat top within max_bridge
function roof_h(w, h, r) = let(re = min(r, w / 2, h / 2))
    cutout_roof && w - 2 * re > max_bridge ? (w - max_bridge) / 2 - re : 0;
function recess_size(c) = c[7] + [2 * tol, 2 * tol];
function cut_half_w(c) = max(c[3] / 2 + cut_grow(c), len(c[7]) == 2 ? recess_size(c)[0] / 2 : 0);
function cut_bot_off(c) = max(c[4] / 2 + cut_grow(c), len(c[7]) == 2 ? recess_size(c)[1] / 2 : 0);
function cut_top_off(c) = let(w = c[3] + 2 * cut_grow(c), h = c[4] + 2 * cut_grow(c))
    max(cut_round(c) ? (w >= teardrop_min ? w / sqrt(2) : w / 2)
                     : h / 2 + roof_h(w, h, c[5] + cut_grow(c)),
        len(c[7]) == 2 ? recess_size(c)[1] / 2 + roof_h(recess_size(c)[0], recess_size(c)[1], 1) : 0);

// Connector faces decide where the column zones go: both X ends when they are
// free of connectors, else front and back when those are free, else every free
// side (a corner column needs a zone on at least one of its two sides).
cut_faces = [for (c = cutouts) cut_face(c)];
function has(l, v) = len([for (x = l) if (x == v) 1]) > 0;
function face_oh(f) = max0([for (c = cutouts) if (cut_face(c) == f) cut_overhang(c)]);
con_l  = has(cut_faces, "left");
con_r  = has(cut_faces, "right");
con_f  = has(cut_faces, "front");
con_b  = has(cut_faces, "back");
layout = !con_l && !con_r ? "x" : !con_f && !con_b ? "y" : "mixed";
oh_l = face_oh("left");
oh_r = face_oh("right");
oh_f = face_oh("front");
oh_b = face_oh("back");

// heights: board stack, then the lid split above the tallest cutout
standoff_h = max(standoff_min, comp_bot + 1,
                 wall_mount == "keyholes" ? comp_bot + 3.5 : 0);
pcb_z    = floor_t + standoff_h;
pcb_top  = pcb_z + pcb_t;
cut_top  = max0([for (c = cutouts) pcb_top + c[2] + cut_top_off(c) + (c[6] ? lead_in : 0)]);
base_h   = max(pcb_top + comp_top + 2, cut_top + 1);  // mating face
cav_z    = base_h - floor_t;
lid_h    = ceil_t + lip_h + tol;

// column zones before the antenna stretch
zl0 = layout != "y" && !con_l ? boss_zone : 0;
zr0 = sensor_chamber ? divider_t + chamber_len : layout != "y" && !con_r ? boss_zone : 0;
zf0 = layout != "x" && !con_f ? boss_zone : 0;
zb0 = layout != "x" && !con_b ? boss_zone : 0;

// Antenna: stretch each zone until the inserts it holds clear the keepout. X
// zones first, against the unstretched Y layout; then Y zones for corners only a
// Y zone holds. The keepout assert below catches anything left over.
ant = len(antenna) == 2 ? rot_pt(antenna) : undef;   // footprint coordinates
kk  = antenna_keepout * antenna_keepout;
yf0      = max(zf0, bay_front);   // a wiring bay deepens the zone on its side
yb0      = max(zb0, bay_back);
outer_y0 = 2 * wall + yf0 + yb0 + 2 * pcb_clear + oh_f + oh_b + fp_y;
ant_y0   = is_undef(ant) ? 0 : wall + yf0 + pcb_clear + oh_f + ant[1];
function x_need(yc, far) = let(dy = ant_y0 - yc)
    sqrt(max(0, kk - dy * dy)) - pcb_clear - (far ? fp_x - ant[0] : ant[0]) + boss_od / 2;
left_zone  = is_undef(ant) || zl0 < boss_zone ? zl0
           : max(zl0, x_need(boss_inset, false), x_need(outer_y0 - boss_inset, false));
right_zone = is_undef(ant) || zr0 < boss_zone ? zr0
           : max(zr0, x_need(boss_inset, true), x_need(outer_y0 - boss_inset, true));
cav_x   = left_zone + right_zone + 2 * pcb_clear + oh_l + oh_r + fp_x;
outer_x = cav_x + 2 * wall;

ant_x  = is_undef(ant) ? 0 : wall + left_zone + pcb_clear + oh_l + ant[0];
y_only = [for (k = [[boss_inset, left_zone], [outer_x - boss_inset, right_zone]])
          if (k[1] < boss_zone) k[0]];
function y_need(xc, far) = let(dx = ant_x - xc)
    sqrt(max(0, kk - dx * dx)) - pcb_clear - (far ? fp_y - ant[1] : ant[1]) + boss_od / 2;
front_zone = is_undef(ant) || zf0 < boss_zone ? zf0
           : max(zf0, max0([for (xc = y_only) y_need(xc, false)]));
back_zone  = is_undef(ant) || zb0 < boss_zone ? zb0
           : max(zb0, max0([for (xc = y_only) y_need(xc, true)]));
gap_f   = max(front_zone, bay_front);
gap_b   = max(back_zone, bay_back);
cav_y   = gap_f + gap_b + 2 * pcb_clear + oh_f + oh_b + fp_y;
outer_y = cav_y + 2 * wall;

outer   = [outer_x, outer_y, base_h];
ears_on = wall_mount == "ears";
span_x  = outer_x + (ears_on ? 2 * ear_len : 0);

pcb_origin = [wall + left_zone + pcb_clear + oh_l, wall + gap_f + pcb_clear + oh_f, pcb_z];
divider_x0 = pcb_origin[0] + fp_x + pcb_clear;
chamber_x0 = divider_x0 + divider_t;

function to_world(p) = [pcb_origin[0], pcb_origin[1]] + rot_pt(p);
function cut_centre(c) = let(p = to_world(edge_pt(c[0], c[1])), f = cut_face(c))
    [f == "front" || f == "back" ? p[0] : p[1], pcb_top + c[2]];
function cut_rect(c) = let(m = c[6] ? lead_in : 0, p = cut_centre(c))
    [p[0] - cut_half_w(c) - m, p[0] + cut_half_w(c) + m,
     p[1] - cut_bot_off(c) - m, p[1] + cut_top_off(c) + m];

// Corner columns wherever a zone can hold one: [x, y, first-rib angle]. The ribs
// run along the wall of the zone that holds the column, away from the board.
corners = [
    [boss_inset,           boss_inset,           left_zone,  front_zone, 90,  180],
    [boss_inset,           outer_y - boss_inset, left_zone,  back_zone,  90,  0],
    [outer_x - boss_inset, boss_inset,           right_zone, front_zone, 270, 180],
    [outer_x - boss_inset, outer_y - boss_inset, right_zone, back_zone,  270, 0]
];
screw_pts = [for (k = corners) if (k[2] >= boss_zone || k[3] >= boss_zone)
             [k[0], k[1], k[2] >= boss_zone ? k[4] : k[5]]];

vents_on   = vents == "on" || (vents == "auto" && heat_w > 0.3);
vent_pitch = vent_slot_w + wall;
vent_m     = boss_inset + boss_od / 2 + 1.5;   // keep vents clear of the columns
function opposite(f) = f == "left" ? "right" : f == "right" ? "left" : f == "front" ? "back" : "front";
vent_free  = [for (f = ["left", "right", "front", "back"])
              if (!has(cut_faces, f) && !(sensor_chamber && f == "right")) f];
vin_face   = vent_in_face != "auto" ? vent_in_face : len(vent_free) > 0 ? vent_free[0] : "left";
vout_alt   = [for (f = vent_free) if (f != vin_face) f];
vout_face  = vent_out_face != "auto" ? vent_out_face
           : has(vent_free, opposite(vin_face)) ? opposite(vin_face)
           : len(vout_alt) > 0 ? vout_alt[0] : vin_face;
band_h     = child_pet_safe ? min(0.25 * cav_z, 4) : 0.25 * cav_z;  // chimney bands
vent_lo    = max(floor_t + 1, ears_on ? ear_t + inner_fillet + 0.5 : 0);
vent_hi    = base_h - band_h - 1.5;

sensor_pos = [chamber_x0 + chamber_len / 2, outer_y / 2];

// no mount holes: rails under the front/back edges, and a stop on every side
// that has a zone or a bay (a wall already stops the board on the others)
rails_on = len(mount_holes) == 0;
rail_y   = [pcb_origin[1] + 0.5, pcb_origin[1] + fp_y - 0.5 - rail_w];
stop_h   = pcb_top + 1.5 - floor_t;
fp_cx    = pcb_origin[0] + fp_x / 2;
fp_cy    = pcb_origin[1] + fp_y / 2;
stops    = concat(   // [x, y, size x, size y]
    left_zone > 0                     ? [[pcb_origin[0] - pcb_clear - 2, fp_cy - 2, 2, 4]] : [],
    right_zone > 0 && !sensor_chamber ? [[divider_x0, fp_cy - 2, 2, 4]] : [],
    gap_f > 0                         ? [[fp_cx - 2, pcb_origin[1] - pcb_clear - 2, 4, 2]] : [],
    gap_b > 0                         ? [[fp_cx - 2, pcb_origin[1] + fp_y + pcb_clear, 4, 2]] : []);

// keyholes: on the board centreline, slot toward the back face (+Y), so the
// front face and its cables point down on the wall
key_span = keyhole_span > 0 ? keyhole_span : fp_x / 2;
key_pts  = [for (s = [-1, 1])
            [pcb_origin[0] + fp_x / 2 + s * key_span / 2,
             pcb_origin[1] + fp_y / 2 - keyhole[2] / 2]];
function seg_dist(p, a, b) =
    let(ab = b - a, t = max(0, min(1, ((p - a) * ab) / (ab * ab)))) norm(p - (a + t * ab));

// wiring: each exit goes through the front/back wall, a tie anchor sits behind it
tie_side    = 4 * ew;
tie_roof    = 10 * layer;
tie_clear   = 0.5;
anchor_size = tie_anchor_size(tie_w, tie_t, tie_len, tie_side, tie_roof, tie_clear, inner_fillet);
rib_reach   = 0.7 * (base_h - floor_t);   // insert_boss default: ribs reach as far as they rise
split_clear = 1 + (rim_w > wall + EPS ? rim_h + rim_w - wall : 0);
function bay(f) = f == "front" ? bay_front : f == "back" ? bay_back : 0;
function side_gap(f) = f == "front" ? gap_f : gap_b;
function exit_d(e) = e[3] + exit_clear;
function exit_td(e) = exit_d(e) >= teardrop_min;
function exit_x(e) = pcb_origin[0] + e[1];
function exit_r(e) = exit_d(e) / 2 + exit_ch;
function exit_top(e) = exit_td(e) ? (exit_d(e) + 2 * exit_ch) / sqrt(2) : exit_r(e);
function exit_ext(e) = max(exit_r(e), anchor_size[0] / 2);
function anchor_xy(e) = [exit_x(e), e[0] == "front" ? wall + tie_offset : outer_y - wall - tie_offset];
function exit_wall_xy(e) = [exit_x(e), e[0] == "front" ? wall : outer_y - wall];
// columns on the front or back wall as [x, keep-out half-width]; a column held by
// that wall's zone has a rib running along the wall toward the middle
function rib_along(p) = let(a = p[0] < outer_x / 2 ? 0 : 180)
    len([for (i = [0 : max(gussets - 1, 0)]) if (gussets > 0 && (p[2] + 90 * i) % 360 == a) 1]) > 0;
function wall_cols(f) = [for (p = screw_pts) if (f == "front" ? p[1] < outer_y / 2 : p[1] > outer_y / 2)
                         [p[0], boss_od / 2 + 1 + (rib_along(p) ? rib_reach : 0)]];

// fasteners for the BOM
std_len = [3, 4, 5, 6, 8, 10, 12, 14, 16, 20, 25, 30];
function pick_len(max_l) = max0([for (l = std_len) if (l <= max_l) l]);
lid_screw   = pick_len(lid_h - head_h + insert_len + 1.0 - 0.5);
pcb_thread  = standoff_bore <= 1.7 ? "M2" : standoff_bore <= 2.2 ? "M2.5" : "M3";
pcb_screw   = pick_len(pcb_t + standoff_h - 0.5);

// --- validate ------------------------------------------------------------

assert(pcb_rot == 0 || pcb_rot == 90 || pcb_rot == 180 || pcb_rot == 270,
       "pcb_rot must be 0, 90, 180 or 270");
assert(wall >= 3 * ew, str("wall ", wall, " is under 3 perimeters"));
assert(lip_w >= 2 * ew, "tongue narrower than two extrusions");
assert(lip_inset - tol >= ew - EPS, "land beside the groove is under one extrusion");
assert(boss_od >= insert_bore + 2 * 2.5, "column OD too small around the insert");
assert(standoff_h >= 4, "standoff too short to clear solder tails");
assert(pcb_clear >= tol, "board clearance below the fit tolerance");
assert(min(cav_x, cav_y) >= boss_od + 2, "cavity too small for corner columns");
assert(len(screw_pts) >= 3,
       str("connectors on ", len(screw_pts) < 2 ? "four" : "three", " sides leave only ",
           len(screw_pts), " corner column(s): bring one connector out through the lid, ",
           "move it to a free side, or place a column by hand"));
assert(!sensor_chamber || !con_r, "a connector faces the sensor chamber's +X wall — change pcb_rot");
assert(!sensor_chamber || (vin_face != "right" && vout_face != "right"),
       "main-cavity vents can't use the +X face: the sensor chamber is there");
assert(!sensor_chamber || chamber_len >= boss_zone + 5,
       "sensor chamber too short to hold the corner columns and a sensor");
assert(max(span_x, outer_y, base_h) <= envelope,
       "base exceeds the build envelope — split it (bolts + dowels, no glue)");
assert(max(outer_x, outer_y, lid_h) <= envelope, "lid exceeds the build envelope");
assert(!child_pet_safe || vent_slot_w <= 4, "child/pet safe: vent slots must be ≤ 4 mm");
for (c = cutouts) {
    f = cut_face(c);
    r = cut_rect(c);
    s = face_span(f);
    assert(r[3] <= base_h - 1 + EPS, str("cutout crosses the lid split: ", c));
    assert(r[2] > floor_t, str("cutout cuts into the floor: ", c));
    assert(bay(f) == 0, str("cutout on the ", f, " face behind a ", bay(f), " mm wiring bay: ",
                            "no plug reaches its socket — put the bay on the other side"));
    if (vents_on)
        for (b = [[vin_face, vent_lo], [vout_face, vent_hi]])
            if (b[0] == f)
                assert(r[1] < s[0] || r[0] > s[1] || r[3] < b[1] || r[2] > b[1] + band_h,
                       str("cutout ", c, " overlaps the vent band on the ", f,
                           " face — move the vents to another face"));
}
for (e = cable_exits) {
    x  = exit_x(e);
    zc = floor_t + e[2];
    s  = face_span(e[0]);
    assert(e[0] == "front" || e[0] == "back",
           str("cable exit on '", e[0], "': exits go through the front or back wall"));
    assert(side_gap(e[0]) + pcb_clear >= tie_offset + anchor_size[1] / 2 + 1,
           str("bay_", e[0], " too shallow for the tie anchor: needs ",
               r2(tie_offset + anchor_size[1] / 2 + 1 - pcb_clear), " mm"));
    assert(tie_offset >= anchor_size[1] / 2 + 1, "tie anchor touches the wall: raise tie_offset");
    for (k = wall_cols(e[0]))
        assert(abs(x - k[0]) >= exit_ext(e) + k[1],
               str("cable exit or its anchor runs into a corner column or its rib: ", e));
    assert(e[2] - exit_r(e) >= inner_fillet, str("cable exit cuts into the floor: ", e));
    assert(zc + exit_top(e) <= base_h - split_clear, str("cable exit crosses the lid split: ", e));
    if (sensor_chamber)
        assert(x + exit_ext(e) < divider_x0 - 1 || x - exit_ext(e) > chamber_x0 + 1,
               str("cable exit or its anchor hits the sensor chamber divider: ", e));
    if (!is_undef(ant))
        for (p = [anchor_xy(e), exit_wall_xy(e)])
            assert(norm(p - to_world(antenna)) >= antenna_keepout,
                   str("cable run inside the antenna keepout: ", e));
    if (rails_on)
        assert(abs(x - fp_cx) >= anchor_size[0] / 2 + 2.5
               || side_gap(e[0]) - 2 > tie_offset + anchor_size[1] / 2,
               str("tie anchor hits the board stop on the ", e[0], " side: move the exit or deepen the bay"));
    if (vents_on)
        for (b = [[vin_face, vent_lo], [vout_face, vent_hi]])
            if (b[0] == e[0])
                assert(x + exit_r(e) < s[0] || x - exit_r(e) > s[1]
                       || zc + exit_top(e) < b[1] || zc - exit_r(e) > b[1] + band_h,
                       str("cable exit ", e, " overlaps the vent band on the ", e[0], " face"));
}
for (p = screw_pts) if (!is_undef(ant))
    assert(norm([p[0], p[1]] - to_world(antenna)) >= antenna_keepout - 0.01,
           "insert column inside the antenna keepout");
if (sensor_chamber) for (h = heat_sources)
    assert(norm(sensor_pos - to_world(h)) >= 15,
           "sensor chamber within 15 mm of a heat source — lengthen the chamber");
if (wall_mount == "keyholes")
    for (k = key_pts) {
        assert(k[0] - keyhole[0] / 2 >= pcb_origin[0] && k[0] + keyhole[0] / 2 <= pcb_origin[0] + fp_x
               && k[1] - keyhole[0] / 2 >= pcb_origin[1] + (rails_on ? 1 + rail_w : 0)
               && k[1] + keyhole[2] + keyhole[1] / 2 <= pcb_origin[1] + fp_y - (rails_on ? 1 + rail_w : 0),
               "keyhole leaves the board footprint — use wall_mount = \"ears\" or set keyhole_span");
        for (m = mount_holes)
            assert(seg_dist(to_world(m), k, k + [0, keyhole[2]])
                   >= standoff_od / 2 + keyhole[0] / 2 + 1,
                   "keyhole collides with a PCB standoff — set keyhole_span or use ears");
    }

// --- report --------------------------------------------------------------

function r2(x) = round(x * 100) / 100;
echo(str("outer: ", r2(span_x), " x ", r2(outer_y), " x ", r2(base_h + lid_h), " mm",
         ears_on ? " (with ears)" : ""));
echo(str("cavity: ", r2(cav_x), " x ", r2(cav_y), " x ", r2(cav_z), " mm; column layout ",
         layout, ", ", len(screw_pts), " columns"));
echo(str("wall: ", wall, " mm (", wall_lines, " lines)",
         rim_w > wall + EPS ? str("; rim band ", rim_w, " mm under the tongue") : ""));
echo(str("BOM: ", len(screw_pts), " x M3 heat-set insert ", insert_bore, " x ", insert_len,
         " mm; ", len(screw_pts), " x M3 x ", lid_screw, " mm socket head (lid)"));
if (!rails_on)
    echo(str("BOM: ", len(mount_holes), " x ", pcb_thread, " x ", pcb_screw,
             " mm thread-forming screw for plastic (board)"));
if (feet) echo(str("BOM: 4 x rubber bumper, Ø", foot_d - 0.5, " x ", 3 * foot_depth, " mm"));
if (ears_on) echo("BOM: 2 x M4 / #8 countersunk wood screw (ears)");
if (wall_mount == "keyholes")
    echo(str("BOM: 2 x pan-head wall screw, head Ø ", keyhole[0] - 1.5, "–", keyhole[0] - 0.5,
             " mm, shank ≤ ", keyhole[1] - 0.3, " mm"));
if (len(cable_exits) > 0)
    echo(str("BOM: ", len(cable_exits), " x cable tie, ", tie_w, " mm wide"));
echo(str("largest dimension vs envelope: ", max(span_x, outer_y, base_h), " / ", envelope));
if (rails_on)
    echo("NOTE: no mount holes — board rests on rails between end stops; the lid limits lift to 2 mm");
if (wall_mount == "keyholes")
    echo("NOTE: floor label skipped — the keyholes use the floor under the board");
for (e = cable_exits)
    echo(str("NOTE: cable exit ", e, " runs straight over its anchor at z = ",
             r2(anchor_size[2] + e[3] / 2)));
if (max(outer_x, outer_y) > 100)
    echo("WARNING: span > 100 mm — add a column per ~70 mm (plan it against the board keepout)");
if (base_h > 2.5 * min(outer_x, outer_y))
    echo("WARNING: tall part on a bed-slinger — use a brim or reorient");
if (heat_w > 1)
    echo("WARNING: > 1 W — maximise open vent area or add a fan (environment-and-safety.md)");
if (!cutout_roof) for (c = cutouts) if (2 * cut_half_w(c) > max_bridge)
    echo(str("NOTE: cutout top spans ", 2 * cut_half_w(c), " mm, over the ", max_bridge,
             " mm bridge limit — it will sag; cutout_roof = true prints it clean"));

// --- helpers -------------------------------------------------------------

// Rounded rectangle, centred, plus a 45° roof when its flat top would bridge
// past max_bridge. Stays convex, so wall_cut_chamfer() can follow it.
module roofed_rect(size, r) {
    w = size[0];
    h = size[1];
    re = min(r, w / 2, h / 2);
    rh = roof_h(w, h, r);
    rrect_c(size, re);
    if (rh > 0)
        polygon([[-w / 2, h / 2 - re], [w / 2, h / 2 - re],
                 [max_bridge / 2, h / 2 + rh], [-max_bridge / 2, h / 2 + rh]]);
}

// 2D profile of a cutout grown by g per side (face coordinates, centred).
module cut_profile(c, g) {
    w = c[3] + 2 * g;
    h = c[4] + 2 * g;
    if (cut_round(c)) {
        if (w >= teardrop_min) teardrop_2d(w); else circle(d = w);
    } else roofed_rect([w, h], c[5] + g);
}

// Slot band on a face between u0..u1 at height v0 with height h.
module vent_band(face, u0, u1, v0, h) {
    if (u1 - u0 > vent_slot_w && h >= vent_slot_w)
        wall_cut(face, outer, rim_w)
            translate([u0, v0]) slot_pattern_2d([u1 - u0, h], vent_slot_w, vent_pitch);
}

function face_span(f) =
    (f == "left" || f == "right") ? [vent_m, outer_y - vent_m]
  : [vent_m, sensor_chamber ? divider_x0 - 1.5 : outer_x - vent_m];

// --- parts ---------------------------------------------------------------

module shell() {
    difference() {
        rbox_c([outer_x, outer_y, base_h], corner_r, cb, seam_ch);
        translate([wall, wall, floor_t])
            rbox_c([cav_x, cav_y, cav_z + 1], max(corner_r - wall, 0.4), inner_fillet, 0);
    }
}

// Mounting ear on the +X end wall; mirrored for the -X end.
module ear() {
    r = min(3, ear_w / 2 - 0.5);
    hx = max(ear_head_d / 2 + 1.5, ear_len / 2);
    difference() {
        hull() {
            translate([-wall, 0, 0]) cube([EPS, ear_w, ear_t]);
            for (y = [r, ear_w - r]) {
                translate([ear_len - r, y, 0]) cylinder(r = r - cb, h = EPS);
                translate([ear_len - r, y, cb]) cylinder(r = r, h = ear_t - cb);
            }
        }
        translate([hx, ear_w / 2, 0])
            screw_cut(d = ear_hole + hole_comp, t = ear_t, type = "countersink", head_d = ear_head_d);
    }
    translate([0, 0, ear_t - EPS]) fillet_lin(inner_fillet, ear_w);
}

module base() {
    union() {
        difference() {
            shell();

            // connector cutouts, lead-ins and plug recesses
            for (c = cutouts) {
                f = cut_face(c);
                g = cutout_clear + (c[6] ? user_port_extra : 0);
                wall_cut(f, outer, rim_w)
                    translate(cut_centre(c)) cut_profile(c, g);
                if (len(c[7]) == 2 && wall > 1.0 + EPS)
                    face_frame(f, outer)
                        translate([0, 0, -1]) linear_extrude(wall - 1.0 + 1)
                            translate(cut_centre(c)) roofed_rect(recess_size(c), 1);
                else if (c[6] && lead_in > 0)
                    wall_cut_chamfer(f, outer, lead_in)
                        translate(cut_centre(c)) cut_profile(c, g);
            }

            // cable exits, chamfered on both faces so the jacket can't chafe
            for (e = cable_exits)
                face_frame(e[0], outer) translate([exit_x(e), floor_t + e[2], 0])
                    cable_exit_cut(exit_d(e), wall, exit_ch, exit_td(e));

            // chimney: inlet low on one face, outlet high on another
            if (vents_on) {
                si = face_span(vin_face);
                vent_band(vin_face, si[0], si[1], vent_lo, band_h);
                so = face_span(vout_face);
                vent_band(vout_face, so[0], so[1], vent_hi, band_h);
            }

            // sensor chamber: its own bottom-in / top-out vents on the +X face
            if (sensor_chamber) {
                vent_band("right", vent_m, outer_y - vent_m, vent_lo, band_h);
                vent_band("right", vent_m, outer_y - vent_m, vent_hi, band_h);
            }

            // feet, keyholes, internal ID label
            if (feet)
                for (x = [12, outer_x - 12], y = [12, outer_y - 12])
                    translate([x, y, 0]) foot_recess_cut(foot_d, foot_depth);
            if (wall_mount == "keyholes")
                for (k = key_pts)
                    translate([k[0], k[1], 0]) rotate(180)
                        keyhole_cut(keyhole[0], keyhole[1], keyhole[2], floor_t);
            else
                translate([fp_cx, fp_cy, floor_t])
                    label_cut(str(product, " BASE ", version), 4, 0.4);
        }

        // lid columns, clipped to the outer envelope so wall-bound ribs merge in
        intersection() {
            rbox_c([outer_x, outer_y, base_h], corner_r, cb, seam_ch);
            for (p = screw_pts)
                translate([p[0], p[1], floor_t - EPS])
                    insert_boss(h = base_h - floor_t + EPS, od = boss_od,
                                bore = insert_bore, insert_len = insert_len,
                                gussets = gussets, gusset_t = wall,
                                angle0 = p[2], spread = 90, fillet_r = inner_fillet);
        }

        // a zip-tie anchor behind each cable exit, so a pull stops there
        for (e = cable_exits)
            translate(concat(anchor_xy(e), [floor_t - EPS]))
                tie_anchor(tie_w, tie_t, tie_len, tie_side, tie_roof, tie_clear, inner_fillet);

        // board support: standoffs on the mount holes, or rails and end stops
        for (m = mount_holes)
            translate(concat(to_world(m), [floor_t - EPS]))
                pcb_standoff(h = standoff_h + EPS, od = standoff_od,
                             bore = standoff_bore, bore_depth = standoff_h,
                             fillet_r = inner_fillet);
        if (rails_on) {
            for (y = rail_y)
                translate([pcb_origin[0] + 2, y, floor_t - EPS])
                    cube([fp_x - 4, rail_w, standoff_h + EPS]);
            for (s = stops)
                translate([s[0], s[1], floor_t - EPS]) cube([s[2], s[3], stop_h + EPS]);
        }

        // sensor chamber divider: two skins with an air gap (thermal break)
        if (sensor_chamber)
            difference() {
                for (x = [divider_x0, divider_x0 + wall + chamber_gap])
                    translate([x, wall - EPS, floor_t - EPS])
                        cube([wall, cav_y + 2 * EPS, cav_z + EPS]);
                translate([divider_x0 - 1, outer_y / 2 - wire_pass[0] / 2, base_h - wire_pass[1]])
                    cube([divider_t + 2, wire_pass[0], wire_pass[1] + 1]);
            }

        // thin walls: rim band so the tongue keeps one extrusion of land each side
        if (rim_w > wall + EPS)
            translate([0, 0, base_h])
                rim_band([outer_x, outer_y], corner_r, wall, rim_w, rim_h);

        // wall-mount ears on both X ends
        if (ears_on) {
            translate([outer_x, outer_y / 2 - ear_w / 2, 0]) ear();
            translate([0, outer_y / 2 - ear_w / 2, 0]) mirror([1, 0, 0]) ear();
        }

        // tongue on the mating face
        translate([0, 0, base_h - EPS])
            lip_tongue([outer_x, outer_y], corner_r, lip_inset, lip_w, lip_h + EPS);
    }
}

// Lid in print orientation: mating face on the plate, origin at its underside.
// Its bed edge gets no chamfer: that would eat the land beside the groove, so
// keep the slicer's elephant-foot compensation on for this part.
module lid() {
    difference() {
        rbox_c([outer_x, outer_y, lid_h], corner_r, 0, ct);
        lip_groove_cut([outer_x, outer_y], corner_r, lip_inset, lip_w, lip_h + tol, 2 * tol);
        for (p = screw_pts)
            translate([p[0], p[1], 0])
                screw_cut(d = screw_clear + hole_comp, t = lid_h,
                          type = "counterbore", head_d = head_d, head_h = head_h);
        if (lid_label != "")
            translate([outer_x / 2, outer_y / 2, lid_h]) label_cut(lid_label, 6, 0.6);
    }
}

module lid_world() { translate([0, 0, base_h]) lid(); }

// Preview-only phantoms (% is never rendered or exported).
module ghosts() {
    %translate(pcb_origin) color("green", 0.5) cube([fp_x, fp_y, pcb_t]);
    if (!is_undef(ant))
        %translate(concat(to_world(antenna), [pcb_top]))
            color("red", 0.2) cylinder(r = antenna_keepout, h = 0.2);
}

// --- selector ------------------------------------------------------------

if (part == "base") base();
else if (part == "lid") lid();
else if (part == "exploded") {
    color("DimGray") base();
    color("Silver") explode(base_h + 25) lid();
    ghosts();
}
else if (part == "section") {           // front half removed: inspect fits
    difference() {
        union() { base(); lid_world(); }
        translate([-ear_len - 1, -1, -1])
            cube([span_x + 2, outer_y / 2 + 1, base_h + lid_h + 2]);
    }
    ghosts();
}
else if (part == "check")               // must render EMPTY; any solid = interference
    intersection() { base(); translate([0, 0, 2 * EPS]) lid_world(); }
else {                                  // assembly
    color("DimGray") base();
    color("Silver", 0.85) lid_world();
    ghosts();
}
