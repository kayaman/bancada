use em_primitives::{cuboid, cylinder, Csg, Vec3};

pub struct VentSlotParams {
    /// Slot width (1.5-2.5 mm recommended).
    pub width: f64,
    /// Slot length, including the rounded ends (<= 25 mm recommended).
    pub length: f64,
    pub count: u32,
    /// Determines the pitch between slot centers: width + wall_thickness.
    pub wall_thickness: f64,
    /// Depth of the cutter through the wall (make this >= wall thickness).
    pub through_depth: f64,
}

/// A single capsule-shaped (rounded-end) vent slot, its long axis along X,
/// its through-cut axis along Z, centered on the origin.
fn slot(width: f64, length: f64, through_depth: f64) -> Csg {
    let half_depth = through_depth / 2.0;
    let cap_r = width / 2.0;
    let straight_len = (length - width).max(1e-6);

    let cap = cylinder(cap_r, through_depth, 16).translate(Vec3::new(0.0, 0.0, -half_depth));
    let body = cuboid(Vec3::new(straight_len, width, through_depth));

    body.union(cap.clone().translate(Vec3::new(-straight_len / 2.0, 0.0, 0.0)))
        .union(cap.translate(Vec3::new(straight_len / 2.0, 0.0, 0.0)))
}

/// A row of evenly-pitched vent slots, centered on the origin, arrayed
/// along X.
pub fn vent_slot_row(params: &VentSlotParams) -> Csg {
    if params.count == 0 {
        return Csg::default();
    }
    let pitch = params.width + params.wall_thickness;
    let total_span = pitch * (params.count as f64 - 1.0);

    let mut result = Csg::default();
    for i in 0..params.count {
        let x = -total_span / 2.0 + (i as f64) * pitch;
        let s = slot(params.width, params.length, params.through_depth).translate(Vec3::new(x, 0.0, 0.0));
        result = result.union(s);
    }
    result
}
