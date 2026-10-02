use crate::{cuboid, cylinder};
use em_core::Vec3;
use em_csg::Csg;

/// A rotation (Euler XYZ degrees, matching `Csg::rotate`'s convention) that
/// maps the local +Z axis onto `normal`. Derived from spherical coordinates:
/// a pure Y-rotation by `theta` (the angle from +Z) followed by a pure
/// Z-rotation by `phi` (the azimuth in the XY plane) carries (0,0,1) to
/// exactly `(sin theta cos phi, sin theta sin phi, cos theta)` -- the
/// standard spherical-to-Cartesian form for a unit vector.
fn align_z_to(normal: Vec3) -> Vec3 {
    let n = normal.normalize();
    let theta = n.z.clamp(-1.0, 1.0).acos();
    let phi = n.y.atan2(n.x);
    Vec3::new(0.0, theta.to_degrees(), phi.to_degrees())
}

/// A half-space {x : dot(x - point, normal) >= 0}, approximated by a huge
/// box (bounded, not truly infinite, but far larger than any reasonable
/// part) with one face on the plane through `point` perpendicular to
/// `normal`.
fn half_space_cutter(point: Vec3, normal: Vec3) -> Csg {
    const BIG: f64 = 1e5;
    let euler = align_z_to(normal);
    cuboid(Vec3::new(BIG, BIG, BIG)).translate(Vec3::new(0.0, 0.0, BIG / 2.0)).rotate(euler).translate(point)
}

/// A cylinder based at `base`, extending along `normal` by `length` --
/// `cylinder()`'s local Z axis (its own extrusion direction) rotated onto
/// `normal` before being moved into position, so the rotation (which fixes
/// the origin) doesn't disturb the base point.
fn pin_cylinder(base: Vec3, normal: Vec3, radius: f64, length: f64) -> Csg {
    let euler = align_z_to(normal);
    cylinder(radius, length, 24).rotate(euler).translate(base)
}

/// Splits `solid` into two pieces along the plane through `point`
/// perpendicular to `normal`: `a` is the side `normal` points toward (i.e.
/// `dot(x - point, normal) >= 0`), `b` is the other side.
pub fn split(solid: &Csg, point: Vec3, normal: Vec3) -> (Csg, Csg) {
    let cutter = half_space_cutter(point, normal);
    let a = solid.clone().intersect(cutter.clone());
    let b = solid.clone().subtract(cutter);
    (a, b)
}

/// Like [`split`], but adds `dowel_count` alignment dowels at the cut:
/// short pins protruding from `b`'s cut face, and matching (clearance
/// -enlarged) sockets recessed into `a`'s cut face, so the pieces
/// self-align when reassembled. Pins are placed evenly along whichever
/// in-plane direction has the larger extent, based on `solid`'s axis
/// -aligned bounding box -- an approximation that works well for
/// roughly-convex parts, but can misjudge available material for very
/// irregular cross-sections at the cut (check the result before printing).
pub fn split_with_dowels(
    solid: &Csg,
    point: Vec3,
    normal: Vec3,
    dowel_count: u32,
    dowel_radius: f64,
    dowel_length: f64,
) -> (Csg, Csg) {
    let (mut a, mut b) = split(solid, point, normal);
    if dowel_count == 0 {
        return (a, b);
    }

    // Standard FDM slip-fit clearance (matches common heat-set-insert/dowel
    // practice: a socket cut a little larger than the pin that goes in it).
    const CLEARANCE: f64 = 0.2;

    for pos in dowel_positions(solid, point, normal, dowel_count) {
        let pin = pin_cylinder(pos, normal, dowel_radius, dowel_length);
        let socket = pin_cylinder(pos, normal, dowel_radius + CLEARANCE, dowel_length);
        b = b.union(pin);
        a = a.subtract(socket);
    }
    (a, b)
}

fn dowel_positions(solid: &Csg, point: Vec3, normal: Vec3, count: u32) -> Vec<Vec3> {
    let n = normal.normalize();
    // Any vector not parallel to `n` works as a seed for building an
    // orthonormal in-plane basis (u, v) via two cross products.
    let reference = if n.z.abs() < 0.9 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(1.0, 0.0, 0.0) };
    let u = reference.cross(n).normalize();
    let v = n.cross(u);

    let (min, max) = solid.bounds();
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ];

    let (mut u_min, mut u_max) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut v_min, mut v_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for c in corners {
        let rel = c - point;
        let pu = rel.dot(u);
        let pv = rel.dot(v);
        u_min = u_min.min(pu);
        u_max = u_max.max(pu);
        v_min = v_min.min(pv);
        v_max = v_max.max(pv);
    }

    let (long_dir, long_min, long_max, short_dir, short_mid) = if (u_max - u_min) >= (v_max - v_min) {
        (u, u_min, u_max, v, (v_min + v_max) / 2.0)
    } else {
        (v, v_min, v_max, u, (u_min + u_max) / 2.0)
    };

    // Keep pins away from the edges of the bounding box.
    let margin = (long_max - long_min) * 0.2;
    let usable_min = long_min + margin;
    let usable_max = long_max - margin;

    (0..count)
        .map(|i| {
            let coord = if count == 1 {
                (usable_min + usable_max) / 2.0
            } else {
                usable_min + (i as f64 / (count as f64 - 1.0)) * (usable_max - usable_min)
            };
            point + long_dir * coord + short_dir * short_mid
        })
        .collect()
}
