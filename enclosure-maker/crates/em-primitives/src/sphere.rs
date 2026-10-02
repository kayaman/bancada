use em_core::{Polygon, Vec3, Vertex};
use em_csg::Csg;

/// A UV sphere centered at the origin. `segments` controls the number of
/// longitude divisions; latitude divisions are derived from it.
pub fn sphere(radius: f64, segments: u32) -> Csg {
    let slices = segments.max(3);
    let stacks = (segments / 2).max(2);

    let point = |i: u32, j: u32| -> Vec3 {
        if i == 0 {
            return Vec3::new(0.0, 0.0, radius);
        }
        if i == stacks {
            return Vec3::new(0.0, 0.0, -radius);
        }
        let phi = std::f64::consts::PI * (i as f64) / (stacks as f64);
        let theta = 2.0 * std::f64::consts::PI * (j as f64) / (slices as f64);
        let (sp, cp) = phi.sin_cos();
        let (st, ct) = theta.sin_cos();
        Vec3::new(radius * sp * ct, radius * sp * st, radius * cp)
    };
    let normal = |p: Vec3| p.normalize();

    let mut polygons = Vec::new();

    for i in 0..stacks {
        for j in 0..slices {
            let jn = j + 1;
            let a = point(i, j);
            let b = point(i + 1, j);
            let c = point(i + 1, jn);
            let d = point(i, jn);

            let va = Vertex::new(a, normal(a));
            let vb = Vertex::new(b, normal(b));
            let vc = Vertex::new(c, normal(c));
            let vd = Vertex::new(d, normal(d));

            // Every patch is triangulated explicitly (rather than left as a
            // quad) because spherical quads aren't exactly planar, and the
            // BSP tree assumes each polygon's vertices share one plane.
            if i == 0 {
                // North pole band: a and d coincide at the pole.
                polygons.push(Polygon::new(vec![va, vb, vc]));
            } else if i + 1 == stacks {
                // South pole band: b and c coincide at the pole.
                polygons.push(Polygon::new(vec![va, vb, vd]));
            } else {
                polygons.push(Polygon::new(vec![va, vb, vc]));
                polygons.push(Polygon::new(vec![va, vc, vd]));
            }
        }
    }

    Csg::from_polygons(polygons)
}
