use crate::Profile2D;
use em_core::{Polygon, Vec3, Vertex};
use em_csg::Csg;

/// Connects two profiles at different heights into one solid. The profiles
/// must have equal point counts (index `i` in `bottom` connects to index `i`
/// in `top` -- no re-triangulation or resampling), and both should share the
/// same winding convention as `Profile2D`'s other constructors.
///
/// Each side face is emitted as **two triangles, not one 4-vertex polygon**:
/// unlike a plain extrude (where both rings share one profile, so the
/// resulting quad is a true parallelogram), two *different* profiles'
/// corresponding points generally don't lie in a common plane, and
/// `Polygon`'s plane is derived from only its first 3 vertices -- a
/// non-planar quad could get inconsistently classified during a later
/// boolean op. Splitting into triangles sidesteps this (3 points always
/// define an exact plane), the same approach `sphere()` uses for its
/// non-planar curved patches.
///
/// A multi-section loft is just multiple `loft2` calls unioned together.
pub fn loft2(bottom: &Profile2D, bottom_z: f64, top: &Profile2D, top_z: f64) -> Csg {
    assert_eq!(
        bottom.points.len(),
        top.points.len(),
        "loft2: profiles must have equal point counts ({} vs {})",
        bottom.points.len(),
        top.points.len()
    );
    let n = bottom.points.len();
    let at = |profile: &Profile2D, i: usize, z: f64| {
        let p = profile.points[i];
        Vec3::new(p.x, p.y, z)
    };

    let mut polygons = Vec::new();

    for i in 0..n {
        let j = (i + 1) % n;
        let b0 = at(bottom, i, bottom_z);
        let b1 = at(bottom, j, bottom_z);
        let t0 = at(top, i, top_z);
        let t1 = at(top, j, top_z);

        // Two triangles covering the same quad a plain extrude would emit
        // as one polygon: [b0, b1, t1, t0], split along the b0-t1 diagonal.
        let tri = |a: Vec3, b: Vec3, c: Vec3| {
            let normal = (b - a).cross(c - a).normalize();
            Polygon::new(vec![
                Vertex::new(a, normal),
                Vertex::new(b, normal),
                Vertex::new(c, normal),
            ])
        };
        polygons.push(tri(b0, b1, t1));
        polygons.push(tri(b0, t1, t0));
    }

    // Caps: reversed order for the bottom (outward -Z-ish), natural order
    // for the top -- same convention as `extrude_linear`.
    polygons.push(Polygon::new(
        bottom
            .points
            .iter()
            .rev()
            .map(|p| Vertex::new(Vec3::new(p.x, p.y, bottom_z), Vec3::new(0.0, 0.0, -1.0)))
            .collect(),
    ));
    polygons.push(Polygon::new(
        top.points
            .iter()
            .map(|p| Vertex::new(Vec3::new(p.x, p.y, top_z), Vec3::new(0.0, 0.0, 1.0)))
            .collect(),
    ));

    Csg::from_polygons(polygons)
}
