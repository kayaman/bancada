use em_core::{Polygon, Vec3, Vertex};
use em_csg::Csg;

const MIN_RADIUS: f64 = 1e-9;

/// A right cylinder, base centered at the origin, extending up to `height`
/// along +Z.
pub fn cylinder(radius: f64, height: f64, segments: u32) -> Csg {
    frustum(radius, radius, height, segments)
}

/// A (possibly truncated) cone: base radius `bottom_r` at z=0, top radius
/// `top_r` at z=`height`. Either radius may be 0 for a sharp point.
pub fn cone(bottom_r: f64, top_r: f64, height: f64, segments: u32) -> Csg {
    frustum(bottom_r, top_r, height, segments)
}

fn frustum(bottom_r: f64, top_r: f64, height: f64, segments: u32) -> Csg {
    let segments = segments.max(3);
    let angle = |i: u32| 2.0 * std::f64::consts::PI * (i as f64) / (segments as f64);

    let bottom = |i: u32| {
        let (s, c) = angle(i).sin_cos();
        Vec3::new(bottom_r * c, bottom_r * s, 0.0)
    };
    let top = |i: u32| {
        let (s, c) = angle(i).sin_cos();
        Vec3::new(top_r * c, top_r * s, height)
    };

    let mut polygons = Vec::new();

    for i in 0..segments {
        let j = i + 1;
        let b0 = bottom(i);
        let b1 = bottom(j);
        let t0 = top(i);
        let t1 = top(j);

        let side_normal = |theta: f64| {
            let slope = (bottom_r - top_r) / height.max(1e-9);
            Vec3::new(theta.cos(), theta.sin(), slope).normalize()
        };
        let n0 = side_normal(angle(i));
        let n1 = side_normal(angle(j));

        if bottom_r < MIN_RADIUS {
            // Apex at the bottom: side collapses to a triangle.
            polygons.push(Polygon::new(vec![
                Vertex::new(b0, n0),
                Vertex::new(t1, n1),
                Vertex::new(t0, n0),
            ]));
        } else if top_r < MIN_RADIUS {
            // Apex at the top: side collapses to a triangle.
            polygons.push(Polygon::new(vec![
                Vertex::new(b0, n0),
                Vertex::new(b1, n1),
                Vertex::new(t0, n0),
            ]));
        } else {
            polygons.push(Polygon::new(vec![
                Vertex::new(b0, n0),
                Vertex::new(b1, n1),
                Vertex::new(t1, n1),
                Vertex::new(t0, n0),
            ]));
        }
    }

    if bottom_r >= MIN_RADIUS {
        // Bottom cap, outward normal -Z: reverse winding relative to
        // increasing-theta order.
        let verts: Vec<Vertex> = (0..segments)
            .rev()
            .map(|i| Vertex::new(bottom(i), Vec3::new(0.0, 0.0, -1.0)))
            .collect();
        polygons.push(Polygon::new(verts));
    }
    if top_r >= MIN_RADIUS {
        let verts: Vec<Vertex> = (0..segments)
            .map(|i| Vertex::new(top(i), Vec3::new(0.0, 0.0, 1.0)))
            .collect();
        polygons.push(Polygon::new(verts));
    }

    Csg::from_polygons(polygons)
}
