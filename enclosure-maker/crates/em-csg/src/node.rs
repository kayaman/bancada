use em_core::{Plane, PolySplit, Polygon};

/// A node in a BSP tree built from a set of polygons. Follows the classic
/// csg.js algorithm: each node holds a splitting plane, the polygons that lie
/// exactly on it, and front/back subtrees for everything else.
pub(crate) struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polygons: Vec<Polygon>,
}

impl Node {
    pub(crate) fn new(polygons: Vec<Polygon>) -> Self {
        let mut node = Node::empty();
        node.build(polygons);
        node
    }

    fn empty() -> Self {
        Node {
            plane: None,
            front: None,
            back: None,
            polygons: Vec::new(),
        }
    }

    pub(crate) fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some(plane) = &mut self.plane {
            plane.flip();
        }
        if let Some(front) = &mut self.front {
            front.invert();
        }
        if let Some(back) = &mut self.back {
            back.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// Removes all polygon area that lies in front of every splitting plane in
    /// this tree (i.e. keeps only the parts of `polygons` that are inside the
    /// solid represented by this tree).
    pub(crate) fn clip_polygons(&self, polygons: Vec<Polygon>) -> Vec<Polygon> {
        let Some(plane) = &self.plane else {
            return polygons;
        };

        let mut front = Vec::new();
        let mut back = Vec::new();
        for poly in &polygons {
            match plane.split_polygon(poly) {
                PolySplit::CoplanarFront(p) | PolySplit::Front(p) => front.push(p),
                PolySplit::CoplanarBack(p) | PolySplit::Back(p) => back.push(p),
                PolySplit::Spanning { front: f, back: b } => {
                    front.push(f);
                    back.push(b);
                }
                PolySplit::None => {}
            }
        }

        let mut front = match &self.front {
            Some(node) => node.clip_polygons(front),
            None => front,
        };

        let back = match &self.back {
            Some(node) => node.clip_polygons(back),
            None => Vec::new(),
        };

        front.extend(back);
        front
    }

    pub(crate) fn clip_to(&mut self, other: &Node) {
        self.polygons = other.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(front) = &mut self.front {
            front.clip_to(other);
        }
        if let Some(back) = &mut self.back {
            back.clip_to(other);
        }
    }

    pub(crate) fn all_polygons(&self) -> Vec<Polygon> {
        let mut polygons = self.polygons.clone();
        if let Some(front) = &self.front {
            polygons.extend(front.all_polygons());
        }
        if let Some(back) = &self.back {
            polygons.extend(back.all_polygons());
        }
        polygons
    }

    pub(crate) fn build(&mut self, polygons: Vec<Polygon>) {
        if polygons.is_empty() {
            return;
        }
        if self.plane.is_none() {
            self.plane = Some(polygons[0].plane.clone());
        }
        let plane = self.plane.clone().unwrap();

        let mut front = Vec::new();
        let mut back = Vec::new();
        for poly in &polygons {
            // Coplanar pieces (both orientations) join this node's own polygons.
            match plane.split_polygon(poly) {
                PolySplit::CoplanarFront(p) | PolySplit::CoplanarBack(p) => {
                    self.polygons.push(p)
                }
                PolySplit::Front(p) => front.push(p),
                PolySplit::Back(p) => back.push(p),
                PolySplit::Spanning { front: f, back: b } => {
                    front.push(f);
                    back.push(b);
                }
                PolySplit::None => {}
            }
        }

        if !front.is_empty() {
            self.front
                .get_or_insert_with(|| Box::new(Node::empty()))
                .build(front);
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(|| Box::new(Node::empty()))
                .build(back);
        }
    }
}
