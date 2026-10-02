use crate::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub pos: Vec3,
    pub normal: Vec3,
}

impl Vertex {
    pub fn new(pos: Vec3, normal: Vec3) -> Self {
        Vertex { pos, normal }
    }

    pub fn interpolate(&self, other: &Self, t: f64) -> Self {
        Vertex {
            pos: self.pos.lerp(other.pos, t),
            normal: self.normal.lerp(other.normal, t),
        }
    }

    pub fn flip(&mut self) {
        self.normal = self.normal.negate();
    }
}
