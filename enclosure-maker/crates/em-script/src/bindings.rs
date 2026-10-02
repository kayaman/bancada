use em_hardware::{
    heat_set_bore, hex_nut_trap, pcb_standoff, screw_boss, vent_slot_row, HeatSetBoreParams,
    ScrewBossParams, StandoffParams, ThreadSize, VentSlotParams,
};
use em_primitives::{
    chamfered_box, cone, cuboid, cylinder, extrude_linear, extrude_revolve, loft2, rounded_box,
    shell, sphere, split, split_with_dowels, Csg, Profile2D, Vec3,
};
use rhai::{Array, Dynamic, Engine, EvalAltResult};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Catch native geometry panics before they unwind into Rhai. Its native-call
/// argument backup can panic again during unwinding, aborting the process even
/// when the caller has its own catch_unwind around the whole evaluation.
fn geometry_result<T>(build: impl FnOnce() -> T) -> Result<T, Box<EvalAltResult>> {
    catch_unwind(AssertUnwindSafe(build)).map_err(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("geometry evaluation panicked");
        message.into()
    })
}

// Keep the panic boundary inside every native binding, including ones whose
// return value is already a Rhai Result. The explicit return-type forms retain
// the type annotations used for hardware errors and split arrays.
macro_rules! register_geometry {
    ($engine:expr, $name:expr, |$($arg:ident: $ty:ty),*| -> Result<$out:ty, $err:ty $(,)?> $body:block $(,)?) => {
        $engine.register_fn($name, |$($arg: $ty),*| {
            geometry_result(|| -> Result<$out, $err> { $body }).and_then(|result| result)
        });
    };
    ($engine:expr, $name:expr, |$($arg:ident: $ty:ty),*| -> $out:ty $body:block $(,)?) => {
        $engine.register_fn($name, |$($arg: $ty),*| geometry_result(|| -> $out { $body }));
    };
    ($engine:expr, $name:expr, |$($arg:ident: $ty:ty),*| $body:expr $(,)?) => {
        $engine.register_fn($name, |$($arg: $ty),*| geometry_result(|| $body));
    };
}

fn thread_from_mm(mm: i64) -> Result<ThreadSize, Box<EvalAltResult>> {
    match mm {
        2 => Ok(ThreadSize::M2),
        3 => Ok(ThreadSize::M3),
        4 => Ok(ThreadSize::M4),
        other => Err(format!("unsupported thread size M{other}: expected 2, 3, or 4").into()),
    }
}

pub(crate) fn register_all_bindings(engine: &mut Engine) {
    engine.register_type_with_name::<Csg>("Csg");
    engine.register_type_with_name::<Profile2D>("Profile2D");

    // --- Primitives ---
    register_geometry!(engine, "cuboid", |x: f64, y: f64, z: f64| cuboid(
        Vec3::new(x, y, z)
    ));
    register_geometry!(engine, "sphere", |r: f64, segments: i64| sphere(
        r,
        segments.max(3) as u32
    ));
    register_geometry!(engine, "cylinder", |r: f64, h: f64, segments: i64| {
        cylinder(r, h, segments.max(3) as u32)
    });
    register_geometry!(engine, "cone", |r0: f64, r1: f64, h: f64, segments: i64| {
        cone(r0, r1, h, segments.max(3) as u32)
    });

    // --- 2D profiles ---
    register_geometry!(engine, "profile_rect", |w: f64, h: f64| Profile2D::rect(
        w, h
    ));
    register_geometry!(engine, "profile_circle", |r: f64, segments: i64| {
        Profile2D::circle(r, segments.max(3) as u32)
    });
    register_geometry!(engine, "profile_regular_polygon", |r: f64, sides: i64| {
        Profile2D::regular_polygon(r, sides.max(3) as u32)
    });
    register_geometry!(
        engine,
        "extrude_linear",
        |profile: Profile2D, height: f64| { extrude_linear(&profile, height) }
    );
    register_geometry!(
        engine,
        "extrude_revolve",
        |profile: Profile2D, degrees: f64, segments: i64| {
            extrude_revolve(&profile, degrees, segments.max(3) as u32)
        },
    );

    // --- Csg boolean ops and transforms (methods: receiver is &mut Csg) ---
    register_geometry!(engine, "union", |a: &mut Csg, b: Csg| a.clone().union(b));
    register_geometry!(engine, "subtract", |a: &mut Csg, b: Csg| a
        .clone()
        .subtract(b));
    register_geometry!(engine, "intersect", |a: &mut Csg, b: Csg| a
        .clone()
        .intersect(b));
    register_geometry!(
        engine,
        "translate",
        |a: &mut Csg, x: f64, y: f64, z: f64| { a.clone().translate(Vec3::new(x, y, z)) }
    );
    register_geometry!(engine, "rotate", |a: &mut Csg, x: f64, y: f64, z: f64| {
        a.clone().rotate(Vec3::new(x, y, z))
    });
    register_geometry!(engine, "scale", |a: &mut Csg, x: f64, y: f64, z: f64| {
        a.clone().scale(Vec3::new(x, y, z))
    });
    register_geometry!(engine, "mirror", |a: &mut Csg, x: f64, y: f64, z: f64| {
        a.clone().mirror(Vec3::new(x, y, z))
    });

    // --- Hardware library ---
    register_geometry!(engine, "heat_set_bore", |thread_mm: i64| -> Result<
        Csg,
        Box<EvalAltResult>,
    > {
        Ok(heat_set_bore(&HeatSetBoreParams {
            thread: thread_from_mm(thread_mm)?,
        }))
    });
    register_geometry!(engine, "screw_boss", |thread_mm: i64,
                                              height: f64,
                                              gusset_count: i64,
                                              wall: f64|
     -> Result<
        Csg,
        Box<EvalAltResult>,
    > {
        Ok(screw_boss(&ScrewBossParams {
            thread: thread_from_mm(thread_mm)?,
            height,
            gusset_count: gusset_count.clamp(2, 4) as u32,
            wall_thickness: wall,
        }))
    },);
    register_geometry!(engine, "hex_nut_trap", |thread_mm: i64,
                                                depth_extra: f64|
     -> Result<
        Csg,
        Box<EvalAltResult>,
    > {
        Ok(hex_nut_trap(thread_from_mm(thread_mm)?, depth_extra))
    },);
    register_geometry!(engine, "pcb_standoff", |thread_mm: i64,
                                                height: f64,
                                                wall: f64|
     -> Result<
        Csg,
        Box<EvalAltResult>,
    > {
        Ok(pcb_standoff(&StandoffParams {
            thread: thread_from_mm(thread_mm)?,
            height,
            wall_thickness: wall,
        }))
    },);
    register_geometry!(
        engine,
        "vent_slot_row",
        |width: f64, length: f64, count: i64, wall: f64, through_depth: f64| {
            vent_slot_row(&VentSlotParams {
                width,
                length,
                count: count.max(0) as u32,
                wall_thickness: wall,
                through_depth,
            })
        },
    );

    // --- New operations ---
    //
    // `em_primitives::rounded_box`/`chamfered_box`/`shell` panic on an
    // out-of-range radius/chamfer/thickness (and have Rust-level tests
    // asserting exactly that), so their contract can't change -- but a
    // panic crossing into Rhai's own call machinery corrupts its
    // `ArgBackup` bookkeeping and crashes the whole embedded engine (and
    // the process with it). `register_geometry!` catches it right at this
    // boundary (see `geometry_result` above), before it ever reaches Rhai's
    // own unwinding, and reports the same message as a normal script error.
    register_geometry!(
        engine,
        "rounded_box",
        |x: f64, y: f64, z: f64, radius: f64, segments: i64| {
            rounded_box(Vec3::new(x, y, z), radius, segments.max(3) as u32)
        }
    );
    register_geometry!(
        engine,
        "chamfered_box",
        |x: f64, y: f64, z: f64, chamfer: f64| { chamfered_box(Vec3::new(x, y, z), chamfer) }
    );
    register_geometry!(engine, "shell", |solid: &mut Csg, thickness: f64| shell(
        solid.clone(),
        thickness,
        None
    ));
    register_geometry!(
        engine,
        "shell_open",
        |solid: &mut Csg, thickness: f64, nx: f64, ny: f64, nz: f64| {
            shell(solid.clone(), thickness, Some(Vec3::new(nx, ny, nz)))
        },
    );
    register_geometry!(
        engine,
        "loft2",
        |bottom: Profile2D, bottom_z: f64, top: Profile2D, top_z: f64| loft2(
            &bottom, bottom_z, &top, top_z
        ),
    );
    register_geometry!(
        engine,
        "linear_pattern",
        |solid: &mut Csg, x: f64, y: f64, z: f64, count: i64| {
            solid
                .clone()
                .linear_pattern(Vec3::new(x, y, z), count.max(0) as u32)
        }
    );
    register_geometry!(engine, "radial_pattern", |solid: &mut Csg, count: i64| {
        solid.clone().radial_pattern(count.max(0) as u32)
    });
    // Both return a 2-element array [a, b] rather than emitting directly --
    // stays composable with everything else (union/subtract/pattern/emit),
    // same as every other operation here.
    register_geometry!(engine, "split", |solid: &mut Csg,
                                         px: f64,
                                         py: f64,
                                         pz: f64,
                                         nx: f64,
                                         ny: f64,
                                         nz: f64|
     -> Array {
        let (a, b) = split(solid, Vec3::new(px, py, pz), Vec3::new(nx, ny, nz));
        vec![Dynamic::from(a), Dynamic::from(b)]
    },);
    register_geometry!(engine, "split_with_dowels", |solid: &mut Csg,
                                                     px: f64,
                                                     py: f64,
                                                     pz: f64,
                                                     nx: f64,
                                                     ny: f64,
                                                     nz: f64,
                                                     dowel_count: i64,
                                                     dowel_radius: f64,
                                                     dowel_length: f64|
     -> Array {
        let (a, b) = split_with_dowels(
            solid,
            Vec3::new(px, py, pz),
            Vec3::new(nx, ny, nz),
            dowel_count.max(0) as u32,
            dowel_radius,
            dowel_length,
        );
        vec![Dynamic::from(a), Dynamic::from(b)]
    },);

    // --- Parameters ---
    // Declares a UI-adjustable numeric parameter and returns its current
    // value: an override sent from the browser's slider panel, or `default`
    // if none is set yet. Also records (name, default, min, max) so the
    // preview server can report a schema for rendering the sliders.
    register_geometry!(
        engine,
        "param",
        |name: &str, default: f64, min: f64, max: f64| {
            crate::engine::stash_param(name.to_string(), default, min, max)
        }
    );

    // --- Output ---
    // Named `emit` rather than `export`: the latter is a Rhai keyword
    // (module export syntax) and can't be used as a function name. Two
    // overloads: `emit(solid)` defaults to part name "part" (keeps
    // single-part scripts unchanged); `emit("name", solid)` for multi-part
    // scripts.
    register_geometry!(engine, "emit", |csg: Csg| {
        crate::engine::stash_part("part".to_string(), csg);
    });
    register_geometry!(engine, "emit", |name: &str, csg: Csg| {
        crate::engine::stash_part(name.to_string(), csg);
    });
    // Preview-only composition (assembly, exploded, section). Stored apart
    // from emit() so it can be opened with --part but is not a printable
    // object.
    register_geometry!(engine, "view", |name: &str, csg: Csg| {
        crate::engine::stash_view(name.to_string(), csg);
    });
}
