use crate::{Polygon, Vertex};

/// A flat triangle soup, ready for export or rendering.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub triangles: Vec<[Vertex; 3]>,
}

impl Mesh {
    pub fn from_polygons(polys: &[Polygon]) -> Self {
        let mut triangles = Vec::new();
        for poly in polys {
            // Fan-tessellate: works for the convex polygons this crate produces
            // (quads, N-gon caps of primitives, etc).
            for i in 1..poly.vertices.len() - 1 {
                triangles.push([
                    poly.vertices[0],
                    poly.vertices[i],
                    poly.vertices[i + 1],
                ]);
            }
        }
        Mesh { triangles }
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
}
