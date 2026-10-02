use em_core::Vec2;

/// A closed 2D outline used as input to extrude/revolve operations. Points
/// must be wound counter-clockwise (as seen looking down the +Z axis, i.e.
/// the mathematically positive sense in the XY plane) for the resulting
/// solid's side faces to end up with outward-facing normals.
#[derive(Clone, Debug)]
pub struct Profile2D {
    pub points: Vec<Vec2>,
}

impl Profile2D {
    pub fn polygon(points: Vec<Vec2>) -> Self {
        assert!(points.len() >= 3, "a profile needs at least 3 points");
        Profile2D { points }
    }

    pub fn rect(w: f64, h: f64) -> Self {
        let (hw, hh) = (w * 0.5, h * 0.5);
        Profile2D::polygon(vec![
            Vec2::new(-hw, -hh),
            Vec2::new(hw, -hh),
            Vec2::new(hw, hh),
            Vec2::new(-hw, hh),
        ])
    }

    pub fn circle(r: f64, segments: u32) -> Self {
        let segments = segments.max(3);
        let points = (0..segments)
            .map(|i| {
                let theta = 2.0 * std::f64::consts::PI * (i as f64) / (segments as f64);
                Vec2::new(r * theta.cos(), r * theta.sin())
            })
            .collect();
        Profile2D::polygon(points)
    }

    /// A regular polygon with `sides` sides, vertex-up, circumradius `r`.
    pub fn regular_polygon(r: f64, sides: u32) -> Self {
        Profile2D::circle(r, sides)
    }
}
