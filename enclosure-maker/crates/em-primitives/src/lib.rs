mod cuboid;
mod cylinder;
mod extrude;
mod loft;
mod profile2d;
mod rounded_box;
mod shell;
mod split;
mod sphere;

pub use cuboid::cuboid;
pub use cylinder::{cone, cylinder};
pub use extrude::{extrude_linear, extrude_revolve};
pub use loft::loft2;
pub use profile2d::Profile2D;
pub use rounded_box::{chamfered_box, rounded_box};
pub use shell::shell;
pub use split::{split, split_with_dowels};
pub use sphere::sphere;

pub use em_core::{Vec2, Vec3};
pub use em_csg::Csg;

#[cfg(test)]
mod tests {
    use super::*;

    /// Signed volume of a closed triangle mesh via the divergence theorem.
    /// A correctly-wound (outward-facing) watertight mesh gives a positive
    /// result matching the shape's true volume; wrong winding or gaps in
    /// the mesh throw this off, which is what makes it a good smoke test
    /// for the primitive constructors above.
    fn signed_volume(csg: &Csg) -> f64 {
        let mesh = csg.to_mesh();
        let mut total = 0.0;
        for tri in &mesh.triangles {
            let [a, b, c] = *tri;
            total += a.pos.dot(b.pos.cross(c.pos));
        }
        total / 6.0
    }

    fn assert_close(actual: f64, expected: f64, rel_tol: f64) {
        let diff = (actual - expected).abs();
        assert!(
            diff <= expected.abs() * rel_tol,
            "expected ~{expected}, got {actual} (diff {diff})"
        );
    }

    #[test]
    fn cuboid_volume() {
        let v = signed_volume(&cuboid(Vec3::new(2.0, 3.0, 4.0)));
        assert_close(v, 24.0, 1e-9);
    }

    #[test]
    fn cylinder_volume() {
        let v = signed_volume(&cylinder(2.0, 5.0, 64));
        assert_close(v, std::f64::consts::PI * 4.0 * 5.0, 0.01);
    }

    #[test]
    fn cone_volume() {
        let v = signed_volume(&cone(2.0, 0.0, 5.0, 64));
        assert_close(v, std::f64::consts::PI * 4.0 * 5.0 / 3.0, 0.01);
    }

    #[test]
    fn sphere_volume() {
        let v = signed_volume(&sphere(3.0, 48));
        assert_close(v, 4.0 / 3.0 * std::f64::consts::PI * 27.0, 0.02);
    }

    #[test]
    fn extrude_linear_matches_cuboid() {
        let profile = Profile2D::rect(2.0, 3.0);
        let v = signed_volume(&extrude_linear(&profile, 4.0));
        assert_close(v, 24.0, 1e-9);
    }

    #[test]
    fn extrude_revolve_full_matches_pappus_theorem() {
        // A 2x2 square cross-section centered at radius 9, swept fully
        // around the Z axis: V = 2*pi*R*area (Pappus's centroid theorem).
        let profile = Profile2D::polygon(vec![
            Vec2::new(8.0, -1.0),
            Vec2::new(10.0, -1.0),
            Vec2::new(10.0, 1.0),
            Vec2::new(8.0, 1.0),
        ]);
        let v = signed_volume(&extrude_revolve(&profile, 360.0, 64));
        let expected = 2.0 * std::f64::consts::PI * 9.0 * 4.0;
        assert_close(v, expected, 0.01);
    }

    #[test]
    fn extrude_revolve_partial_is_half_of_full() {
        let profile = Profile2D::polygon(vec![
            Vec2::new(8.0, -1.0),
            Vec2::new(10.0, -1.0),
            Vec2::new(10.0, 1.0),
            Vec2::new(8.0, 1.0),
        ]);
        let v = signed_volume(&extrude_revolve(&profile, 180.0, 64));
        let expected = std::f64::consts::PI * 9.0 * 4.0;
        assert_close(v, expected, 0.01);
    }

    #[test]
    fn rounded_box_volume_matches_minkowski_sum_formula() {
        // rounded_box's `size` is the FINAL outer envelope, so the standard
        // box-grown-by-a-ball formula must be applied to the *inset* core
        // dimensions (size - 2r), not `size` itself -- growing that core by
        // r back out gives exactly `size` again.
        let (a, b, c) = (20.0, 15.0, 10.0);
        let r = 2.0;
        let (ca, cb, cc) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
        let v = signed_volume(&rounded_box(Vec3::new(a, b, c), r, 24));
        let expected = ca * cb * cc
            + 2.0 * r * (ca * cb + cb * cc + cc * ca)
            + std::f64::consts::PI * r * r * (ca + cb + cc)
            + (4.0 / 3.0) * std::f64::consts::PI * r * r * r;
        assert_close(v, expected, 0.02);
    }

    #[test]
    #[should_panic(expected = "radius")]
    fn rounded_box_rejects_radius_exceeding_half_min_dimension() {
        rounded_box(Vec3::new(10.0, 10.0, 10.0), 6.0, 16);
    }

    #[test]
    fn chamfered_box_volume_is_between_loose_bounds() {
        let size = Vec3::new(20.0, 15.0, 10.0);
        let chamfer = 2.0;
        let v = signed_volume(&chamfered_box(size, chamfer));
        let sharp_volume = size.x * size.y * size.z;
        let shrunk_volume =
            (size.x - 2.0 * chamfer) * (size.y - 2.0 * chamfer) * (size.z - 2.0 * chamfer);
        assert!(v < sharp_volume, "chamfered box should be smaller than the sharp box");
        assert!(
            v > shrunk_volume,
            "chamfered box should be larger than the box shrunk by 2*chamfer per axis"
        );
    }

    #[test]
    #[should_panic(expected = "chamfer")]
    fn chamfered_box_rejects_chamfer_exceeding_half_min_dimension() {
        chamfered_box(Vec3::new(10.0, 10.0, 10.0), 6.0);
    }

    #[test]
    fn shell_of_a_closed_box_is_exact() {
        let size = Vec3::new(20.0, 15.0, 10.0);
        let t = 2.0;
        let box_solid = cuboid(size);
        let hollowed = shell(box_solid, t, None);
        let expected = size.x * size.y * size.z
            - (size.x - 2.0 * t) * (size.y - 2.0 * t) * (size.z - 2.0 * t);
        assert_close(signed_volume(&hollowed), expected, 1e-9);
    }

    #[test]
    fn shell_with_open_face_removes_that_walls_material() {
        let size = Vec3::new(20.0, 15.0, 10.0);
        let t = 2.0;
        let closed = shell(cuboid(size), t, None);
        let open_top = shell(cuboid(size), t, Some(Vec3::new(0.0, 0.0, 1.0)));
        assert!(
            signed_volume(&open_top) < signed_volume(&closed),
            "opening a face should remove material"
        );
        // shell() insets the cavity on all 3 axes, so the entire top cap is
        // one solid slab (not a separate "wall" - above the cavity ceiling,
        // the full cross-section is solid). Opening the top removes that
        // whole slab, so the new rim sits at the former cavity ceiling
        // (size.z/2 - thickness), not at the original outer face.
        let (_, max) = open_top.bounds();
        let expected_rim = size.z / 2.0 - t;
        assert!(
            max.z <= expected_rim + 1e-6 && max.z > expected_rim - t * 0.5,
            "expected rim near {expected_rim}, got {}",
            max.z
        );
    }

    #[test]
    #[should_panic(expected = "thickness")]
    fn shell_rejects_thickness_exceeding_half_min_dimension() {
        shell(cuboid(Vec3::new(10.0, 10.0, 10.0)), 6.0, None);
    }

    #[test]
    fn loft2_between_circles_matches_frustum_volume() {
        let bottom = Profile2D::circle(4.0, 48);
        let top = Profile2D::circle(2.0, 48);
        let lofted = loft2(&bottom, 0.0, &top, 5.0);
        let cone_equiv = cone(4.0, 2.0, 5.0, 48);
        assert_close(signed_volume(&lofted), signed_volume(&cone_equiv), 1e-6);
    }

    #[test]
    fn loft2_between_identical_rects_matches_extrude_linear() {
        let profile = Profile2D::rect(6.0, 4.0);
        let lofted = loft2(&profile, 0.0, &profile, 3.0);
        let extruded = extrude_linear(&profile, 3.0);
        assert_close(signed_volume(&lofted), signed_volume(&extruded), 1e-9);
    }

    #[test]
    #[should_panic(expected = "point counts")]
    fn loft2_rejects_mismatched_point_counts() {
        let a = Profile2D::circle(4.0, 16);
        let b = Profile2D::circle(4.0, 20);
        loft2(&a, 0.0, &b, 5.0);
    }

    #[test]
    fn split_axis_aligned_conserves_volume_and_positions_pieces() {
        let solid = cuboid(Vec3::new(20.0, 15.0, 10.0));
        let total = signed_volume(&solid);
        let (a, b) = split(&solid, Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0));

        assert_close(signed_volume(&a) + signed_volume(&b), total, 1e-9);

        let (a_min, _) = a.bounds();
        let (_, b_max) = b.bounds();
        assert!(a_min.z > -1e-6, "piece a (positive side) should start at z=0, got min.z={}", a_min.z);
        assert!(b_max.z < 1e-6, "piece b (negative side) should end at z=0, got max.z={}", b_max.z);
    }

    #[test]
    fn split_at_arbitrary_angle_conserves_volume() {
        let solid = sphere(5.0, 48);
        let total = signed_volume(&solid);
        // A tilted, off-center plane -- exercises the general rotation path,
        // not just the axis-aligned special case.
        let normal = Vec3::new(1.0, 1.0, 0.5).normalize();
        let (a, b) = split(&solid, Vec3::new(0.3, -0.2, 0.1), normal);
        assert_close(signed_volume(&a) + signed_volume(&b), total, 1e-6);
    }

    #[test]
    fn split_off_the_solid_entirely_gives_one_empty_piece() {
        let solid = cuboid(Vec3::new(10.0, 10.0, 10.0));
        let total = signed_volume(&solid);
        // Plane far above the whole solid: everything should land in b.
        let (a, b) = split(&solid, Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 0.0, 1.0));
        assert_close(signed_volume(&b), total, 1e-9);
        assert!(signed_volume(&a) < 1e-6);
    }

    #[test]
    fn split_with_dowels_adds_pin_volume_to_b_and_removes_socket_volume_from_a() {
        let solid = cuboid(Vec3::new(20.0, 15.0, 10.0));
        let (plain_a, plain_b) = split(&solid, Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0));
        let (dowel_a, dowel_b) = split_with_dowels(&solid, Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 2, 1.5, 4.0);

        assert!(
            signed_volume(&dowel_b) > signed_volume(&plain_b),
            "b should gain volume from the protruding pins"
        );
        assert!(
            signed_volume(&dowel_a) < signed_volume(&plain_a),
            "a should lose volume to the (clearance-enlarged) sockets"
        );
    }

    #[test]
    fn split_with_dowels_zero_count_matches_plain_split() {
        let solid = cuboid(Vec3::new(20.0, 15.0, 10.0));
        let (plain_a, plain_b) = split(&solid, Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0));
        let (dowel_a, dowel_b) = split_with_dowels(&solid, Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 0, 1.5, 4.0);
        assert_close(signed_volume(&dowel_a), signed_volume(&plain_a), 1e-9);
        assert_close(signed_volume(&dowel_b), signed_volume(&plain_b), 1e-9);
    }
}
