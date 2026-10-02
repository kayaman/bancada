use crate::Profile2D;
use em_core::{Polygon, Vec2, Vec3, Vertex};
use em_csg::Csg;

const AXIS_EPSILON: f64 = 1e-9;

/// Extrudes a 2D profile (in the XY plane) straight up along +Z.
pub fn extrude_linear(profile: &Profile2D, height: f64) -> Csg {
    let n = profile.points.len();
    let at = |p: Vec2, z: f64| Vec3::new(p.x, p.y, z);

    let mut polygons = Vec::new();

    for k in 0..n {
        let kn = (k + 1) % n;
        let a = profile.points[k];
        let b = profile.points[kn];
        let d = Vec2::new(b.x - a.x, b.y - a.y);
        let normal = Vec3::new(d.y, -d.x, 0.0).normalize();

        polygons.push(Polygon::new(vec![
            Vertex::new(at(a, 0.0), normal),
            Vertex::new(at(b, 0.0), normal),
            Vertex::new(at(b, height), normal),
            Vertex::new(at(a, height), normal),
        ]));
    }

    // Bottom cap (outward -Z): reversed point order.
    polygons.push(Polygon::new(
        profile
            .points
            .iter()
            .rev()
            .map(|p| Vertex::new(at(*p, 0.0), Vec3::new(0.0, 0.0, -1.0)))
            .collect(),
    ));
    // Top cap (outward +Z): natural point order.
    polygons.push(Polygon::new(
        profile
            .points
            .iter()
            .map(|p| Vertex::new(at(*p, height), Vec3::new(0.0, 0.0, 1.0)))
            .collect(),
    ));

    Csg::from_polygons(polygons)
}

/// Revolves a 2D profile around the Z axis. The profile's X coordinate is
/// treated as radius (must be >= 0) and Y as height along Z. `degrees` of
/// 360 (or more) produces a closed solid of revolution; anything less adds
/// flat end caps at the start and end angle.
pub fn extrude_revolve(profile: &Profile2D, degrees: f64, segments: u32) -> Csg {
    let segments = segments.max(3);
    let full = degrees >= 360.0 - 1e-6;
    let sweep = degrees.to_radians();

    let point3d = |theta: f64, p: Vec2| Vec3::new(p.x * theta.cos(), p.x * theta.sin(), p.y);
    let normal3d = |theta: f64, n2d: Vec2| {
        Vec3::new(n2d.x * theta.cos(), n2d.x * theta.sin(), n2d.y)
    };

    let theta = |i: u32| -> f64 { sweep * (i as f64) / (segments as f64) };

    let n = profile.points.len();
    let mut polygons = Vec::new();

    let bands = segments;
    for i in 0..bands {
        let j = if full { (i + 1) % segments } else { i + 1 };
        let theta_i = theta(i);
        let theta_j = theta(j);

        for k in 0..n {
            let kn = (k + 1) % n;
            let a = profile.points[k];
            let b = profile.points[kn];
            let d = Vec2::new(b.x - a.x, b.y - a.y);
            let n2d = Vec2::new(d.y, -d.x);

            let a_on_axis = a.x.abs() < AXIS_EPSILON;
            let b_on_axis = b.x.abs() < AXIS_EPSILON;

            let a_i = Vertex::new(point3d(theta_i, a), normal3d(theta_i, n2d));
            let a_j = Vertex::new(point3d(theta_j, a), normal3d(theta_j, n2d));
            let b_i = Vertex::new(point3d(theta_i, b), normal3d(theta_i, n2d));
            let b_j = Vertex::new(point3d(theta_j, b), normal3d(theta_j, n2d));

            if a_on_axis && b_on_axis {
                continue; // degenerate edge lying entirely on the axis
            } else if a_on_axis {
                polygons.push(Polygon::new(vec![a_i, b_j, b_i]));
            } else if b_on_axis {
                polygons.push(Polygon::new(vec![a_i, a_j, b_i]));
            } else {
                polygons.push(Polygon::new(vec![a_i, a_j, b_j, b_i]));
            }
        }
    }

    if !full {
        // Start cap at theta=0: natural profile order gives outward normal
        // pointing "before" the sweep begins.
        polygons.push(Polygon::new(
            profile
                .points
                .iter()
                .map(|p| {
                    Vertex::new(point3d(0.0, *p), Vec3::new(0.0, -1.0, 0.0))
                })
                .collect(),
        ));
        // End cap at theta=sweep: reversed order gives outward normal
        // pointing "after" the sweep ends.
        let end_normal = Vec3::new(-sweep.sin(), sweep.cos(), 0.0);
        polygons.push(Polygon::new(
            profile
                .points
                .iter()
                .rev()
                .map(|p| Vertex::new(point3d(sweep, *p), end_normal))
                .collect(),
        ));
    }

    Csg::from_polygons(polygons)
}
