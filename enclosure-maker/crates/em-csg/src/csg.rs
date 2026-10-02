use crate::node::Node;
use em_core::{Mesh, Polygon, Vec3};

/// A solid, represented as a set of polygons forming a closed (manifold)
/// surface. Boolean operations consume their operands by value and return a
/// new solid, mirroring the classic csg.js API.
#[derive(Clone, Debug, Default)]
pub struct Csg {
    pub(crate) polygons: Vec<Polygon>,
}

impl Csg {
    pub fn from_polygons(polygons: Vec<Polygon>) -> Self {
        Csg { polygons }
    }

    pub fn to_mesh(&self) -> Mesh {
        Mesh::from_polygons(&self.polygons)
    }

    pub fn union(self, other: Csg) -> Csg {
        let mut a = Node::new(self.polygons);
        let mut b = Node::new(other.polygons);
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        Csg::from_polygons(a.all_polygons())
    }

    pub fn subtract(self, other: Csg) -> Csg {
        let mut a = Node::new(self.polygons);
        let mut b = Node::new(other.polygons);
        a.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        a.invert();
        Csg::from_polygons(a.all_polygons())
    }

    pub fn intersect(self, other: Csg) -> Csg {
        let mut a = Node::new(self.polygons);
        let mut b = Node::new(other.polygons);
        a.invert();
        b.clip_to(&a);
        b.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        a.build(b.all_polygons());
        a.invert();
        Csg::from_polygons(a.all_polygons())
    }

    pub fn translate(mut self, v: Vec3) -> Self {
        for poly in &mut self.polygons {
            for vert in &mut poly.vertices {
                vert.pos = vert.pos + v;
            }
        }
        self.recompute_planes();
        self
    }

    /// Rotates by Euler angles given in degrees, applied in X, then Y, then Z order.
    pub fn rotate(mut self, degrees: Vec3) -> Self {
        let (sx, cx) = degrees.x.to_radians().sin_cos();
        let (sy, cy) = degrees.y.to_radians().sin_cos();
        let (sz, cz) = degrees.z.to_radians().sin_cos();

        let rotate_point = |p: Vec3| -> Vec3 {
            // Rotate around X
            let p = Vec3::new(p.x, p.y * cx - p.z * sx, p.y * sx + p.z * cx);
            // Rotate around Y
            let p = Vec3::new(p.x * cy + p.z * sy, p.y, -p.x * sy + p.z * cy);
            // Rotate around Z
            Vec3::new(p.x * cz - p.y * sz, p.x * sz + p.y * cz, p.z)
        };

        for poly in &mut self.polygons {
            for vert in &mut poly.vertices {
                vert.pos = rotate_point(vert.pos);
                vert.normal = rotate_point(vert.normal);
            }
        }
        self.recompute_planes();
        self
    }

    pub fn scale(mut self, v: Vec3) -> Self {
        // Normals transform by the inverse-transpose of the linear map; for a
        // diagonal scale matrix that's just the elementwise reciprocal.
        let flip = (v.x * v.y * v.z) < 0.0;
        for poly in &mut self.polygons {
            for vert in &mut poly.vertices {
                vert.pos = Vec3::new(vert.pos.x * v.x, vert.pos.y * v.y, vert.pos.z * v.z);
                vert.normal =
                    Vec3::new(vert.normal.x / v.x, vert.normal.y / v.y, vert.normal.z / v.z)
                        .normalize();
            }
            if flip {
                poly.flip();
            }
        }
        self.recompute_planes();
        self
    }

    pub fn mirror(mut self, normal: Vec3) -> Self {
        let n = normal.normalize();
        for poly in &mut self.polygons {
            for vert in &mut poly.vertices {
                let d = 2.0 * vert.pos.dot(n);
                vert.pos = vert.pos - n * d;
                let dn = 2.0 * vert.normal.dot(n);
                vert.normal = vert.normal - n * dn;
            }
            poly.flip();
        }
        self.recompute_planes();
        self
    }

    fn recompute_planes(&mut self) {
        for poly in &mut self.polygons {
            let rebuilt = Polygon::new(poly.vertices.clone());
            poly.plane = rebuilt.plane;
        }
    }

    /// Axis-aligned bounding box as `(min, max)`. Panics if called on an
    /// empty solid (no polygons).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f64::INFINITY);
        let mut max = Vec3::splat(f64::NEG_INFINITY);
        for poly in &self.polygons {
            for v in &poly.vertices {
                min = Vec3::new(min.x.min(v.pos.x), min.y.min(v.pos.y), min.z.min(v.pos.z));
                max = Vec3::new(max.x.max(v.pos.x), max.y.max(v.pos.y), max.z.max(v.pos.z));
            }
        }
        assert!(min.x.is_finite(), "bounds() called on an empty solid");
        (min, max)
    }

    /// Unions `count` copies of `self`, each further offset by `step` (the
    /// first copy is unshifted).
    pub fn linear_pattern(self, step: Vec3, count: u32) -> Csg {
        let mut result = Csg::default();
        for i in 0..count {
            let offset = step * (i as f64);
            result = result.union(self.clone().translate(offset));
        }
        result
    }

    /// Unions `count` copies of `self`, evenly rotated around the Z axis
    /// (the first copy is unrotated).
    pub fn radial_pattern(self, count: u32) -> Csg {
        let mut result = Csg::default();
        for i in 0..count {
            let angle = 360.0 * (i as f64) / (count as f64);
            result = result.union(self.clone().rotate(Vec3::new(0.0, 0.0, angle)));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use em_core::Vertex;

    /// Minimal axis-aligned box builder, kept local to these tests so em-csg
    /// doesn't need a dev-dependency on em-primitives (which depends on em-csg).
    fn test_cuboid(center: Vec3, size: Vec3) -> Csg {
        let h = size * 0.5;
        let corner = |sx: f64, sy: f64, sz: f64| {
            Vec3::new(center.x + sx * h.x, center.y + sy * h.y, center.z + sz * h.z)
        };
        let face = |verts: [Vec3; 4], normal: Vec3| {
            Polygon::new(
                verts
                    .into_iter()
                    .map(|p| Vertex::new(p, normal))
                    .collect(),
            )
        };
        let polygons = vec![
            // -X
            face(
                [
                    corner(-1.0, -1.0, -1.0),
                    corner(-1.0, -1.0, 1.0),
                    corner(-1.0, 1.0, 1.0),
                    corner(-1.0, 1.0, -1.0),
                ],
                Vec3::new(-1.0, 0.0, 0.0),
            ),
            // +X
            face(
                [
                    corner(1.0, -1.0, -1.0),
                    corner(1.0, 1.0, -1.0),
                    corner(1.0, 1.0, 1.0),
                    corner(1.0, -1.0, 1.0),
                ],
                Vec3::new(1.0, 0.0, 0.0),
            ),
            // -Y
            face(
                [
                    corner(-1.0, -1.0, -1.0),
                    corner(1.0, -1.0, -1.0),
                    corner(1.0, -1.0, 1.0),
                    corner(-1.0, -1.0, 1.0),
                ],
                Vec3::new(0.0, -1.0, 0.0),
            ),
            // +Y
            face(
                [
                    corner(-1.0, 1.0, -1.0),
                    corner(-1.0, 1.0, 1.0),
                    corner(1.0, 1.0, 1.0),
                    corner(1.0, 1.0, -1.0),
                ],
                Vec3::new(0.0, 1.0, 0.0),
            ),
            // -Z
            face(
                [
                    corner(-1.0, -1.0, -1.0),
                    corner(-1.0, 1.0, -1.0),
                    corner(1.0, 1.0, -1.0),
                    corner(1.0, -1.0, -1.0),
                ],
                Vec3::new(0.0, 0.0, -1.0),
            ),
            // +Z
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

    #[test]
    fn union_of_overlapping_cuboids_spans_both() {
        let a = test_cuboid(Vec3::ZERO, Vec3::splat(10.0));
        let b = test_cuboid(Vec3::new(5.0, 0.0, 0.0), Vec3::splat(10.0));
        let result = a.union(b);
        assert!(!result.polygons.is_empty());
        let (min, max) = result.bounds();
        assert!((min.x - -5.0).abs() < 1e-6);
        assert!((max.x - 10.0).abs() < 1e-6);
    }

    #[test]
    fn subtract_removes_overlap_region() {
        let a = test_cuboid(Vec3::ZERO, Vec3::splat(10.0));
        let b = test_cuboid(Vec3::ZERO, Vec3::splat(4.0));
        let result = a.subtract(b);
        assert!(!result.polygons.is_empty());
        // Outer bounds unchanged; the mesh should now contain more polygons
        // than a plain cuboid (6) because a cavity was cut into it.
        let (min, max) = result.bounds();
        assert!((min.x - -5.0).abs() < 1e-6);
        assert!((max.x - 5.0).abs() < 1e-6);
        assert!(result.polygons.len() > 6);
    }

    #[test]
    fn intersect_keeps_only_overlap_region() {
        let a = test_cuboid(Vec3::ZERO, Vec3::splat(10.0));
        let b = test_cuboid(Vec3::new(5.0, 0.0, 0.0), Vec3::splat(10.0));
        let result = a.intersect(b);
        assert!(!result.polygons.is_empty());
        let (min, max) = result.bounds();
        assert!((min.x - 0.0).abs() < 1e-6);
        assert!((max.x - 5.0).abs() < 1e-6);
    }

    #[test]
    fn linear_pattern_spans_expected_extent() {
        let unit = test_cuboid(Vec3::ZERO, Vec3::splat(2.0));
        let row = unit.linear_pattern(Vec3::new(5.0, 0.0, 0.0), 4);
        let (min, max) = row.bounds();
        assert!((min.x - -1.0).abs() < 1e-6);
        assert!((max.x - 16.0).abs() < 1e-6); // last copy centered at x=15, half-width 1
    }

    #[test]
    fn radial_pattern_of_one_copy_is_unchanged() {
        let unit = test_cuboid(Vec3::new(5.0, 0.0, 0.0), Vec3::splat(2.0));
        let ring = unit.clone().radial_pattern(1);
        assert_eq!(ring.bounds(), unit.bounds());
    }
}
