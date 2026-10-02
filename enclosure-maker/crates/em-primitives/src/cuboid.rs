use em_core::{Polygon, Vec3, Vertex};
use em_csg::Csg;

/// An axis-aligned box centered at the origin.
pub fn cuboid(size: Vec3) -> Csg {
    let h = size * 0.5;
    let corner = |sx: f64, sy: f64, sz: f64| Vec3::new(sx * h.x, sy * h.y, sz * h.z);
    let face = |verts: [Vec3; 4], normal: Vec3| {
        Polygon::new(verts.into_iter().map(|p| Vertex::new(p, normal)).collect())
    };

    let polygons = vec![
        face(
            [
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, -1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
                corner(-1.0, 1.0, -1.0),
            ],
            Vec3::new(-1.0, 0.0, 0.0),
        ),
        face(
            [
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, -1.0, 1.0),
            ],
            Vec3::new(1.0, 0.0, 0.0),
        ),
        face(
            [
                corner(-1.0, -1.0, -1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, -1.0, 1.0),
                corner(-1.0, -1.0, 1.0),
            ],
            Vec3::new(0.0, -1.0, 0.0),
        ),
        face(
            [
                corner(-1.0, 1.0, -1.0),
                corner(-1.0, 1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, 1.0, -1.0),
            ],
            Vec3::new(0.0, 1.0, 0.0),
        ),
        face(
            [
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, -1.0, -1.0),
            ],
            Vec3::new(0.0, 0.0, -1.0),
        ),
        face(
            [
                corner(-1.0, -1.0, 1.0),
                corner(1.0, -1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            Vec3::new(0.0, 0.0, 1.0),
        ),
    ];
    Csg::from_polygons(polygons)
}
