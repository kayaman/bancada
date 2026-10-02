/// Metric thread sizes with dimensional data for heat-set inserts, hex nuts,
/// and clearance holes, per the enclosure design rules (insert datasheet
/// nominal values + a 1 mm relief under the insert. The mouth stays square:
/// a chamfer removes the plastic the top knurl should bite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadSize {
    M2,
    M3,
    M4,
}

impl ThreadSize {
    /// Heat-set insert outer diameter (mm) — sizes the blind bore.
    pub fn insert_od_mm(self) -> f64 {
        match self {
            ThreadSize::M2 => 3.2,
            ThreadSize::M3 => 4.1,
            ThreadSize::M4 => 5.1,
        }
    }

    /// Heat-set insert length (mm).
    pub fn insert_length_mm(self) -> f64 {
        match self {
            ThreadSize::M2 => 3.5,
            ThreadSize::M3 => 5.7,
            ThreadSize::M4 => 6.8,
        }
    }

    /// Blind bore depth (mm): insert length plus a 1.0 mm relief so the
    /// insert never bottoms out.
    pub fn bore_depth_mm(self) -> f64 {
        self.insert_length_mm() + 1.0
    }

    /// Lead-in at the bore mouth. Zero: the mouth stays square. A chamfer
    /// here removes the plastic the top knurl should bite.
    pub fn lead_in_chamfer_mm(self) -> f64 {
        match self {
            ThreadSize::M2 | ThreadSize::M3 | ThreadSize::M4 => 0.0,
        }
    }

    /// A sensible default wall thickness (mm) around the boss for this size.
    pub fn default_wall_mm(self) -> f64 {
        match self {
            ThreadSize::M2 => 1.2,
            ThreadSize::M3 => 1.2,
            ThreadSize::M4 => 1.5,
        }
    }

    /// Minimum boss outer diameter (mm) for a given wall thickness: bore +
    /// 2x wall.
    pub fn boss_min_od_mm(self, wall_thickness: f64) -> f64 {
        self.insert_od_mm() + 2.0 * wall_thickness
    }

    /// Hex nut across-flats width (mm).
    pub fn nut_af_mm(self) -> f64 {
        match self {
            ThreadSize::M2 => 4.0,
            ThreadSize::M3 => 5.5,
            ThreadSize::M4 => 7.0,
        }
    }

    /// Hex nut height (mm).
    pub fn nut_height_mm(self) -> f64 {
        match self {
            ThreadSize::M2 => 1.6,
            ThreadSize::M3 => 2.4,
            ThreadSize::M4 => 3.2,
        }
    }

    /// Clearance hole diameter (mm) for a screw of this size passing
    /// through a mating part.
    pub fn clearance_hole_mm(self) -> f64 {
        match self {
            ThreadSize::M2 => 2.4,
            ThreadSize::M3 => 3.4,
            ThreadSize::M4 => 4.5,
        }
    }
}
