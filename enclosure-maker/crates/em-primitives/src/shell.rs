use crate::cuboid;
use em_core::Vec3;
use em_csg::Csg;

/// Hollows `solid` by subtracting an inward-scaled copy of itself, leaving a
/// wall of (approximately) `thickness`. If `open_face` is given (e.g.
/// `Vec3::new(0.0, 0.0, 1.0)` for an open top), the entire cap on that side
/// is removed, down to the level of the (inset-on-every-axis) cavity's
/// ceiling — **not just a thin slit at the outer surface**. Since the cavity
/// is inset uniformly on all 3 axes, everything above the cavity's ceiling
/// is one solid slab (there's no separate "cap" vs "wall" material up
/// there), so opening a face necessarily drops that face's rim down to the
/// cavity height. A box shelled with `thickness=2` on a `height=10` box and
/// opened on top ends up with walls reaching `height/2 - thickness`, not
/// `height/2` — call `shell` with a shape that's already been built to its
/// intended final rim height if that matters for your design, rather than
/// expecting the opening to preserve the original solid's full height.
///
/// This scales per-axis about `solid`'s own bounding-box center so each axis
/// shrinks by `2*thickness` — **exact for a box or cylinder centered on its
/// own bounding box**, but only an approximation for other shapes (it is not
/// a true constant-offset surface; concave or off-center solids can end up
/// with uneven wall thickness). Good enough for straightforward enclosure
/// shapes; not a substitute for a real offset algorithm.
pub fn shell(solid: Csg, thickness: f64, open_face: Option<Vec3>) -> Csg {
    let (min, max) = solid.bounds();
    let center = (min + max) * 0.5;
    let size = max - min;

    assert!(
        thickness > 0.0 && thickness * 2.0 < size.x.min(size.y).min(size.z),
        "shell: thickness ({thickness}) must be positive and less than half the smallest bounding-box dimension"
    );

    let scale = Vec3::new(
        (size.x - 2.0 * thickness) / size.x,
        (size.y - 2.0 * thickness) / size.y,
        (size.z - 2.0 * thickness) / size.z,
    );

    // Scale about the solid's own center: translate to origin, scale, translate back.
    let inner = solid
        .clone()
        .translate(center * -1.0)
        .scale(scale)
        .translate(center);

    let mut hollowed = solid.subtract(inner);

    if let Some(normal) = open_face {
        let n = normal.normalize();
        // A cutter box, oversized in the other two axes so it fully covers
        // that face. Along `n` it starts a bit past the *inner* cavity
        // surface (not at the object's center -- that would also remove the
        // tops of the side walls, not just the cap) and extends outward
        // well past the outer surface.
        let overshoot = size.x.max(size.y).max(size.z) * 4.0;
        // Just past the inner cavity surface (which sits `thickness` in from
        // the face) -- enough to guarantee the cut goes all the way through
        // the wall without also eating into the side walls' height.
        let inward_margin = thickness * 1.02;
        let span = overshoot + inward_margin;
        let cutter_size = Vec3::new(
            if n.x.abs() > 0.5 { span } else { size.x + overshoot },
            if n.y.abs() > 0.5 { span } else { size.y + overshoot },
            if n.z.abs() > 0.5 { span } else { size.z + overshoot },
        );
        let face_center = center + Vec3::new(n.x * size.x / 2.0, n.y * size.y / 2.0, n.z * size.z / 2.0);
        let cutter_center = face_center + n * ((overshoot - inward_margin) / 2.0);
        let cutter = cuboid(cutter_size).translate(cutter_center);
        hollowed = hollowed.subtract(cutter);
    }

    hollowed
}
