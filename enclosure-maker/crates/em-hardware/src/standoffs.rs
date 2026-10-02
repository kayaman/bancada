use crate::ThreadSize;
use em_primitives::{cylinder, Csg};

pub struct StandoffParams {
    pub thread: ThreadSize,
    /// Standoff height (>= 4.0 mm recommended for solder-tail clearance).
    pub height: f64,
    pub wall_thickness: f64,
}

/// A hollow PCB standoff: a post with a through bore sized for this
/// thread's clearance hole, base at Z=0, extending up to `height`.
pub fn pcb_standoff(params: &StandoffParams) -> Csg {
    let bore_r = params.thread.clearance_hole_mm() / 2.0;
    let outer_r = bore_r + params.wall_thickness;
    let outer = cylinder(outer_r, params.height, 24);
    let bore = cylinder(bore_r, params.height, 24);
    outer.subtract(bore)
}
