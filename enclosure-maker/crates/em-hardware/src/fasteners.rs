use crate::ThreadSize;
use em_primitives::{cone, cylinder, extrude_linear, Csg, Profile2D, Vec2, Vec3};

pub struct HeatSetBoreParams {
    pub thread: ThreadSize,
}

/// A blind cavity sized for a heat-set insert. The square mouth sits at
/// local Z=0 and the bore runs straight down (into -Z). Place this cutter's
/// origin on the face where the insert is pressed in, then subtract it.
/// A positive lead-in, if one is ever set, is a cone above the straight bore.
pub fn heat_set_bore(params: &HeatSetBoreParams) -> Csg {
    let r = params.thread.insert_od_mm() / 2.0;
    let bore_depth = params.thread.bore_depth_mm();
    let chamfer = params.thread.lead_in_chamfer_mm();
    let total_height = bore_depth + chamfer;

    let bore = cylinder(r, bore_depth, 24);
    let solid = if chamfer > 0.0 {
        let lead_in = cone(r, r + chamfer, chamfer, 24).translate(Vec3::new(0.0, 0.0, bore_depth));
        bore.union(lead_in)
    } else {
        bore
    };

    solid.translate(Vec3::new(0.0, 0.0, -total_height))
}

pub struct ScrewBossParams {
    pub thread: ThreadSize,
    /// Column height, floor to the mating face.
    pub height: f64,
    /// Number of triangular gussets (2-4).
    pub gusset_count: u32,
    pub wall_thickness: f64,
}

/// A solid, gusseted screw-boss column: a cylindrical post sized for this
/// thread's insert plus wall thickness, braced by triangular gussets so it
/// never acts as an unsupported lever arm. Does not include the insert
/// cavity itself — subtract [`heat_set_bore`] (translated to the column's
/// top) to finish it.
pub fn screw_boss(params: &ScrewBossParams) -> Csg {
    let boss_radius = params.thread.boss_min_od_mm(params.wall_thickness) / 2.0;
    let mut solid = cylinder(boss_radius, params.height, 24);

    let gusset_count = params.gusset_count.clamp(2, 4);
    let gusset_height = params.height * 0.7;

    for i in 0..gusset_count {
        let angle = 360.0 * (i as f64) / (gusset_count as f64);
        solid = solid.union(gusset(boss_radius, gusset_height, params.wall_thickness, angle));
    }

    solid
}

/// A single triangular gusset: a wedge of thickness `wall_thickness`,
/// running from the boss surface outward, placed at the given azimuth
/// (degrees) around the Z axis. Its radial reach is capped at the boss
/// radius (so gussets on tall columns don't balloon outward) while its
/// vertical rise stays `gusset_height` — the resulting slope is always
/// >= 45 deg from horizontal, so it prints unsupported either way.
fn gusset(boss_radius: f64, gusset_height: f64, wall_thickness: f64, angle_degrees: f64) -> Csg {
    let run = boss_radius.min(gusset_height);
    // Right triangle in a (radial, height) plane: vertical leg against the
    // boss, horizontal leg along the floor, hypotenuse between them.
    let profile = Profile2D::polygon(vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(run, 0.0),
        Vec2::new(0.0, gusset_height),
    ]);

    extrude_linear(&profile, wall_thickness)
        .translate(Vec3::new(0.0, 0.0, -wall_thickness / 2.0))
        .rotate(Vec3::new(90.0, 0.0, 0.0))
        .translate(Vec3::new(boss_radius, 0.0, 0.0))
        .rotate(Vec3::new(0.0, 0.0, angle_degrees))
}

/// A hexagonal nut-trap cavity, sized to this thread's nut across-flats.
/// `depth_extra` adds clearance beyond the nut's nominal height. Like
/// [`heat_set_bore`], its opening sits at local Z=0 and it cuts into -Z.
pub fn hex_nut_trap(thread: ThreadSize, depth_extra: f64) -> Csg {
    let circumradius = thread.nut_af_mm() / 3f64.sqrt();
    let depth = thread.nut_height_mm() + depth_extra;
    let profile = Profile2D::regular_polygon(circumradius, 6);
    extrude_linear(&profile, depth).translate(Vec3::new(0.0, 0.0, -depth))
}
