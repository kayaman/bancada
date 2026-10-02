use crate::{cuboid, cylinder, extrude_linear, sphere, Profile2D};
use em_core::{Vec2, Vec3};
use em_csg::Csg;

/// A box with filleted (rounded) edges and corners, centered at the origin.
/// `radius` must be at most half of the smallest dimension of `size`.
///
/// Built as the classic box-Minkowski-sum-ball decomposition: a core box, 3
/// axis-extended slabs, 12 edge cylinders, and 8 corner spheres, all unioned.
/// Full cylinders/spheres are used rather than the minimal quarter/octant
/// pieces — the extra material each contributes falls entirely inside the
/// core/slabs, so it doesn't change the union's outer boundary, and it's far
/// simpler (and no riskier) than hand-building partial primitives.
pub fn rounded_box(size: Vec3, radius: f64, segments: u32) -> Csg {
    let min_half = size.x.min(size.y).min(size.z) / 2.0;
    assert!(
        radius <= min_half,
        "rounded_box: radius ({radius}) must be <= half the smallest dimension ({min_half})"
    );
    assert!(radius > 0.0, "rounded_box: radius must be positive");

    let h = size * 0.5;
    let r = radius;

    // Core box, inset by r on every axis.
    let mut solid = cuboid(Vec3::new(size.x - 2.0 * r, size.y - 2.0 * r, size.z - 2.0 * r));

    // Three axis-extended slabs: full length on one axis, inset by r on the
    // other two.
    solid = solid.union(cuboid(Vec3::new(size.x, size.y - 2.0 * r, size.z - 2.0 * r)));
    solid = solid.union(cuboid(Vec3::new(size.x - 2.0 * r, size.y, size.z - 2.0 * r)));
    solid = solid.union(cuboid(Vec3::new(size.x - 2.0 * r, size.y - 2.0 * r, size.z)));

    // 12 edge cylinders. cylinder() spans [0, height] along +Z from a base
    // centered on the XY origin, so each is built along Z then rotated onto
    // its actual edge axis.
    let edge_len_z = size.z - 2.0 * r;
    let edge_len_x = size.x - 2.0 * r;
    let edge_len_y = size.y - 2.0 * r;

    for &(sx, sy) in &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        // Edges parallel to Z, at the 4 (x,y) inset corners.
        let cyl = cylinder(r, edge_len_z, segments)
            .translate(Vec3::new(0.0, 0.0, -edge_len_z / 2.0))
            .translate(Vec3::new(sx * (h.x - r), sy * (h.y - r), 0.0));
        solid = solid.union(cyl);
    }
    for &(sy, sz) in &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        // Edges parallel to X: build along Z, rotate 90 deg about Y to lay along X.
        let cyl = cylinder(r, edge_len_x, segments)
            .translate(Vec3::new(0.0, 0.0, -edge_len_x / 2.0))
            .rotate(Vec3::new(0.0, 90.0, 0.0))
            .translate(Vec3::new(0.0, sy * (h.y - r), sz * (h.z - r)));
        solid = solid.union(cyl);
    }
    for &(sx, sz) in &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        // Edges parallel to Y: build along Z, rotate -90 deg about X to lay along Y.
        let cyl = cylinder(r, edge_len_y, segments)
            .translate(Vec3::new(0.0, 0.0, -edge_len_y / 2.0))
            .rotate(Vec3::new(-90.0, 0.0, 0.0))
            .translate(Vec3::new(sx * (h.x - r), 0.0, sz * (h.z - r)));
        solid = solid.union(cyl);
    }

    // 8 corner spheres.
    for &(sx, sy, sz) in &[
        (1.0, 1.0, 1.0),
        (1.0, 1.0, -1.0),
        (1.0, -1.0, 1.0),
        (1.0, -1.0, -1.0),
        (-1.0, 1.0, 1.0),
        (-1.0, 1.0, -1.0),
        (-1.0, -1.0, 1.0),
        (-1.0, -1.0, -1.0),
    ] {
        let corner = Vec3::new(sx * (h.x - r), sy * (h.y - r), sz * (h.z - r));
        solid = solid.union(sphere(r, segments).translate(corner));
    }

    solid
}

/// A box with flat 45-degree chamfers cut into each of its 12 edges,
/// centered at the origin. Built by subtracting a triangular wedge cutter
/// along each edge from a sharp box — the same `extrude_linear`-of-a-right-
/// triangle recipe `em-hardware`'s `screw_boss` gussets use.
pub fn chamfered_box(size: Vec3, chamfer: f64) -> Csg {
    let min_half = size.x.min(size.y).min(size.z) / 2.0;
    assert!(
        chamfer <= min_half,
        "chamfered_box: chamfer ({chamfer}) must be <= half the smallest dimension ({min_half})"
    );
    assert!(chamfer > 0.0, "chamfered_box: chamfer must be positive");

    let h = size * 0.5;
    let mut solid = cuboid(size);

    // A wedge cutter: a right-triangle prism (right angle at the local
    // origin, legs of length `chamfer` along local +X/+Y), overshooting past
    // the box along its own Z so the boolean subtract cuts fully through
    // whichever box edge it's placed against.
    let overshoot = size.x.max(size.y).max(size.z) * 2.0;
    let wedge_profile = Profile2D::polygon(vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(chamfer, 0.0),
        Vec2::new(0.0, chamfer),
    ]);
    let wedge =
        || extrude_linear(&wedge_profile, overshoot).translate(Vec3::new(0.0, 0.0, -overshoot / 2.0));

    // Each corner sign combination needs the wedge's right-angle vertex
    // placed exactly at the box's edge with its two legs running inward
    // along the two adjacent faces. That requires a *reflection* (not just
    // rotation) for two of every four corners, hence `scale()` with
    // negative components (which correctly flips winding) rather than
    // `rotate()`. Each of the 3 transforms below was derived and hand
    // -verified against concrete corner coordinates.

    // 4 edges parallel to Z, cutting the (x,y) corners.
    for &(sx, sy) in &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let cutter = wedge()
            .scale(Vec3::new(-sx, -sy, 1.0))
            .translate(Vec3::new(sx * h.x, sy * h.y, 0.0));
        solid = solid.subtract(cutter);
    }
    // 4 edges parallel to X, cutting the (y,z) corners.
    for &(sy, sz) in &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let cutter = wedge()
            .scale(Vec3::new(sz, -sy, 1.0))
            .rotate(Vec3::new(0.0, 90.0, 0.0))
            .translate(Vec3::new(0.0, sy * h.y, sz * h.z));
        solid = solid.subtract(cutter);
    }
    // 4 edges parallel to Y, cutting the (x,z) corners.
    for &(sx, sz) in &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let cutter = wedge()
            .scale(Vec3::new(-sx, sz, 1.0))
            .rotate(Vec3::new(-90.0, 0.0, 0.0))
            .translate(Vec3::new(sx * h.x, 0.0, sz * h.z));
        solid = solid.subtract(cutter);
    }

    solid
}
