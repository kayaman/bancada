use crate::{Polygon, Vec3, Vertex};

pub const PLANE_EPSILON: f64 = 1e-5;

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitType {
    Coplanar,
    Front,
    Back,
    Spanning,
}

/// Result of classifying+splitting one polygon against a plane. Kept as an
/// owned enum (rather than pushing into caller-provided buckets) so callers
/// can route the pieces without needing multiple simultaneous mutable
/// borrows of the same output vector.
pub enum PolySplit {
    CoplanarFront(Polygon),
    CoplanarBack(Polygon),
    Front(Polygon),
    Back(Polygon),
    Spanning { front: Polygon, back: Polygon },
    /// Degenerate spanning result (a sliver collapsed by epsilon clipping).
    None,
}

#[derive(Clone, Debug)]
pub struct Plane {
    pub normal: Vec3,
    pub w: f64,
}

impl Plane {
    pub fn new(normal: Vec3, w: f64) -> Self {
        Plane { normal, w }
    }

    /// Builds a plane from three points, assuming counter-clockwise winding
    /// (as seen from the side the normal points towards).
    pub fn from_points(a: Vec3, b: Vec3, c: Vec3) -> Self {
        let normal = (b - a).cross(c - a).normalize();
        Plane {
            w: normal.dot(a),
            normal,
        }
    }

    pub fn flip(&mut self) {
        self.normal = self.normal.negate();
        self.w = -self.w;
    }

    fn vertex_type(&self, v: Vec3) -> u8 {
        let t = self.normal.dot(v) - self.w;
        if t < -PLANE_EPSILON {
            BACK
        } else if t > PLANE_EPSILON {
            FRONT
        } else {
            COPLANAR
        }
    }

    pub fn classify_polygon(&self, poly: &Polygon) -> SplitType {
        let mut poly_type = COPLANAR;
        for v in &poly.vertices {
            poly_type |= self.vertex_type(v.pos);
        }
        match poly_type {
            COPLANAR => SplitType::Coplanar,
            FRONT => SplitType::Front,
            BACK => SplitType::Back,
            _ => SplitType::Spanning,
        }
    }

    /// Classifies and, if needed, splits `poly` against this plane.
    pub fn split_polygon(&self, poly: &Polygon) -> PolySplit {
        let mut poly_type = COPLANAR;
        let types: Vec<u8> = poly
            .vertices
            .iter()
            .map(|v| {
                let t = self.vertex_type(v.pos);
                poly_type |= t;
                t
            })
            .collect();

        match poly_type {
            COPLANAR => {
                if self.normal.dot(poly.plane.normal) > 0.0 {
                    PolySplit::CoplanarFront(poly.clone())
                } else {
                    PolySplit::CoplanarBack(poly.clone())
                }
            }
            FRONT => PolySplit::Front(poly.clone()),
            BACK => PolySplit::Back(poly.clone()),
            _ => {
                let mut f: Vec<Vertex> = Vec::new();
                let mut b: Vec<Vertex> = Vec::new();
                let n = poly.vertices.len();
                for i in 0..n {
                    let j = (i + 1) % n;
                    let ti = types[i];
                    let tj = types[j];
                    let vi = &poly.vertices[i];
                    let vj = &poly.vertices[j];

                    if ti != BACK {
                        f.push(*vi);
                    }
                    if ti != FRONT {
                        b.push(*vi);
                    }

                    if (ti | tj) == SPANNING {
                        let denom = self.normal.dot(vj.pos - vi.pos);
                        let t = (self.w - self.normal.dot(vi.pos)) / denom;
                        let mid = vi.interpolate(vj, t);
                        f.push(mid);
                        b.push(mid);
                    }
                }
                match (f.len() >= 3, b.len() >= 3) {
                    (true, true) => PolySplit::Spanning {
                        front: Polygon::new(f),
                        back: Polygon::new(b),
                    },
                    (true, false) => PolySplit::Front(Polygon::new(f)),
                    (false, true) => PolySplit::Back(Polygon::new(b)),
                    (false, false) => PolySplit::None,
                }
            }
        }
    }
}
