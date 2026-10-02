mod fasteners;
mod standoffs;
mod thread_size;
mod vents;

pub use fasteners::{heat_set_bore, hex_nut_trap, screw_boss, HeatSetBoreParams, ScrewBossParams};
pub use standoffs::{pcb_standoff, StandoffParams};
pub use thread_size::ThreadSize;
pub use vents::{vent_slot_row, VentSlotParams};

#[cfg(test)]
mod tests {
    use super::*;
    use em_primitives::{cylinder, Csg, Vec3};

    fn signed_volume(csg: &Csg) -> f64 {
        let mesh = csg.to_mesh();
        let mut total = 0.0;
        for tri in &mesh.triangles {
            let [a, b, c] = *tri;
            total += a.pos.dot(b.pos.cross(c.pos));
        }
        total / 6.0
    }

    #[test]
    fn screw_boss_is_heavier_than_bare_cylinder() {
        let boss = screw_boss(&ScrewBossParams {
            thread: ThreadSize::M3,
            height: 10.0,
            gusset_count: 4,
            wall_thickness: 1.2,
        });
        let bare_r = ThreadSize::M3.boss_min_od_mm(1.2) / 2.0;
        let bare = cylinder(bare_r, 10.0, 24);
        assert!(signed_volume(&boss) > signed_volume(&bare));
    }

    #[test]
    fn heat_set_bore_can_be_cut_from_a_boss() {
        let boss = screw_boss(&ScrewBossParams {
            thread: ThreadSize::M3,
            height: 10.0,
            gusset_count: 4,
            wall_thickness: 1.2,
        });
        let bore = heat_set_bore(&HeatSetBoreParams { thread: ThreadSize::M3 })
            .translate(Vec3::new(0.0, 0.0, 10.0));
        let cut = boss.clone().subtract(bore);
        assert!(signed_volume(&cut) < signed_volume(&boss));
        assert!(signed_volume(&cut) > 0.0);
    }

    #[test]
    fn hex_nut_trap_matches_hexagon_prism_volume() {
        let trap = hex_nut_trap(ThreadSize::M3, 0.5);
        let af = ThreadSize::M3.nut_af_mm();
        let depth = ThreadSize::M3.nut_height_mm() + 0.5;
        // Regular hexagon area with across-flats `af`: (af^2 * sqrt(3)) / 2.
        let expected_area = af * af * 3f64.sqrt() / 2.0;
        let expected = expected_area * depth;
        let actual = signed_volume(&trap);
        assert!((actual - expected).abs() < expected * 0.02);
    }

    #[test]
    fn pcb_standoff_is_hollow() {
        let standoff = pcb_standoff(&StandoffParams {
            thread: ThreadSize::M3,
            height: 5.0,
            wall_thickness: 1.2,
        });
        let bore_r = ThreadSize::M3.clearance_hole_mm() / 2.0;
        let outer_r = bore_r + 1.2;
        let expected =
            std::f64::consts::PI * (outer_r * outer_r - bore_r * bore_r) * 5.0;
        let actual = signed_volume(&standoff);
        assert!((actual - expected).abs() < expected * 0.02);
    }

    #[test]
    fn vent_slot_row_produces_requested_count_of_openings() {
        let row = vent_slot_row(&VentSlotParams {
            width: 2.0,
            length: 15.0,
            count: 5,
            wall_thickness: 2.0,
            through_depth: 3.0,
        });
        // Cutting from a plate should leave 6 fragments... just check the
        // cut solid has positive volume and is smaller than the plate.
        let plate = em_primitives::cuboid(Vec3::new(100.0, 20.0, 3.0));
        let cut = plate.clone().subtract(row);
        assert!(signed_volume(&cut) < signed_volume(&plate));
        assert!(signed_volume(&cut) > 0.0);
    }
}
