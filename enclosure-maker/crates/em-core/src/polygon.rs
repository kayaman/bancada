use crate::{Plane, Vertex};

#[derive(Clone, Debug)]
pub struct Polygon {
    pub vertices: Vec<Vertex>,
    pub plane: Plane,
}

impl Polygon {
    /// Builds a polygon from an ordered, coplanar, counter-clockwise (as seen
    /// from the outward face) list of vertices. The plane is derived from the
    /// first three vertices' positions.
    pub fn new(vertices: Vec<Vertex>) -> Self {
        assert!(vertices.len() >= 3, "a polygon needs at least 3 vertices");
        let plane = Plane::from_points(vertices[0].pos, vertices[1].pos, vertices[2].pos);
        Polygon { vertices, plane }
    }

    pub fn flip(&mut self) {
        self.vertices.reverse();
        for v in &mut self.vertices {
            v.flip();
        }
        self.plane.flip();
    }
}
