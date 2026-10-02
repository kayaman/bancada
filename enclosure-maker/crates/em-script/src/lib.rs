mod bindings;
mod engine;

pub use engine::{ParamInfo, Scene, ScriptEngine, ScriptError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_a_simple_box_with_a_hole() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                let base = cuboid(20.0, 20.0, 10.0);
                let hole = cylinder(4.0, 10.0, 24).translate(0.0, 0.0, 0.0);
                emit(base.subtract(hole));
                "#,
            )
            .expect("script should evaluate");
        // Single-arg emit() defaults to part name "part" -- unchanged
        // behavior for pre-multi-part scripts.
        let mesh = scene.part("part").expect("default part should exist");
        assert!(mesh.triangle_count() > 0);
    }

    #[test]
    fn missing_export_is_an_error() {
        let engine = ScriptEngine::new();
        let result = engine.eval_str("let x = cuboid(1.0, 1.0, 1.0);");
        assert!(matches!(result, Err(ScriptError::NoParts)));
    }

    #[test]
    fn hardware_bindings_work() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                let boss = screw_boss(3, 10.0, 4, 1.2);
                let bore = heat_set_bore(3).translate(0.0, 0.0, 10.0);
                emit(boss.subtract(bore));
                "#,
            )
            .expect("script should evaluate");
        assert!(scene.part("part").unwrap().triangle_count() > 0);
    }

    #[test]
    fn unsupported_thread_size_reports_a_script_error() {
        let engine = ScriptEngine::new();
        let result = engine.eval_str("emit(heat_set_bore(5));");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_geometry_returns_script_errors_and_the_engine_can_evaluate_again() {
        let engine = ScriptEngine::new();
        let cases = [
            // A variable as the first argument exercises Rhai's argument
            // backup, whose destructor used to turn this into a process abort.
            (
                "let width = 10.0; emit(rounded_box(width, 10.0, 10.0, 6.0, 8));",
                "rounded_box: radius",
            ),
            (
                "let width = 10.0; emit(chamfered_box(width, 10.0, 10.0, 6.0));",
                "chamfered_box: chamfer",
            ),
            (
                "let solid = cuboid(10.0, 10.0, 10.0); emit(solid.shell(6.0));",
                "shell: thickness",
            ),
            (
                "let solid = cuboid(10.0, 10.0, 10.0); emit(solid.shell_open(6.0, 0.0, 0.0, 1.0));",
                "shell: thickness",
            ),
            (
                "let bottom = profile_rect(10.0, 10.0); let top = profile_circle(3.0, 8); emit(loft2(bottom, 0.0, top, 5.0));",
                "loft2: profiles must have equal point counts",
            ),
            (
                "let solid = cuboid(10.0, 10.0, 10.0).linear_pattern(0.0, 0.0, 0.0, 0); emit(solid.shell(1.0));",
                "bounds() called on an empty solid",
            ),
        ];
        for (source, message) in cases {
            let error = engine.eval_str(source).unwrap_err();
            assert!(matches!(error, ScriptError::Rhai(_)));
            assert!(error.to_string().contains(message), "{error}");
            let scene = engine.eval_str("emit(cuboid(10.0, 10.0, 10.0));").unwrap();
            assert!(scene.part("part").unwrap().triangle_count() > 0);
        }
    }

    #[test]
    fn script_can_catch_a_geometry_error_and_reuse_the_first_argument() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                let width = 10.0;
                try { rounded_box(width, 10.0, 10.0, 6.0, 8); }
                catch (error) { }
                emit(cuboid(width, 10.0, 10.0));
                "#,
            )
            .unwrap();
        assert!(scene.part("part").unwrap().triangle_count() > 0);
    }

    #[test]
    fn multi_part_emit_produces_named_parts_and_assembly() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                emit("base", cuboid(20.0, 20.0, 5.0));
                emit("lid", cuboid(20.0, 20.0, 2.0).translate(0.0, 0.0, 10.0));
                "#,
            )
            .expect("script should evaluate");
        let base = scene.part("base").expect("base part");
        let lid = scene.part("lid").expect("lid part");
        assert!(scene.part("nonexistent").is_none());
        let assembly = scene.assembly_mesh();
        assert_eq!(
            assembly.triangle_count(),
            base.triangle_count() + lid.triangle_count()
        );
        assert_eq!(scene.printable_parts().len(), 2);
    }

    #[test]
    fn view_is_preview_only_and_left_out_of_the_printable_assembly() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                let base = cuboid(20.0, 20.0, 5.0);
                let lid = cuboid(20.0, 20.0, 2.0).translate(0.0, 0.0, 10.0);
                emit("base", base);
                emit("lid", lid);
                view("assembly", base.union(lid));
                "#,
            )
            .expect("script should evaluate");
        let base = scene.part("base").unwrap().triangle_count();
        let lid = scene.part("lid").unwrap().triangle_count();
        let assembly_view = scene.part("assembly").unwrap().triangle_count();
        assert!(assembly_view > 0);
        assert_eq!(scene.assembly_mesh().triangle_count(), base + lid);
        assert_eq!(
            scene
                .printable_parts()
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "lid"]
        );
        assert!(scene.part_names().any(|n| n == "assembly"));
    }

    #[test]
    fn view_without_emit_is_an_error() {
        let engine = ScriptEngine::new();
        let result = engine.eval_str(r#"view("assembly", cuboid(1.0, 1.0, 1.0));"#);
        assert!(matches!(result, Err(ScriptError::NoParts)));
    }

    #[test]
    fn re_emitting_the_same_name_replaces_rather_than_duplicates() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                emit("base", cuboid(20.0, 20.0, 5.0));
                emit("base", cuboid(10.0, 10.0, 5.0));
                "#,
            )
            .expect("script should evaluate");
        assert_eq!(scene.part_names().count(), 1);
        // The replacement (smaller box) should be what's retained.
        let cuboid_10 = engine.eval_str("emit(cuboid(10.0, 10.0, 5.0));").unwrap();
        assert_eq!(
            scene.part("base").unwrap().triangle_count(),
            cuboid_10.part("part").unwrap().triangle_count()
        );
    }

    #[test]
    fn param_without_override_uses_default_and_reports_schema() {
        let engine = ScriptEngine::new();
        let (_scene, params) = engine
            .eval_str_with_params(
                r#"
                let width = param("Width", 60.0, 20.0, 200.0);
                emit(cuboid(width, 10.0, 10.0));
                "#,
                &std::collections::HashMap::new(),
            )
            .expect("script should evaluate");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Width");
        assert_eq!(params[0].default, 60.0);
        assert_eq!(params[0].min, 20.0);
        assert_eq!(params[0].max, 200.0);
        assert_eq!(params[0].value, 60.0);
    }

    #[test]
    fn param_override_changes_both_value_and_geometry() {
        let engine = ScriptEngine::new();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("Width".to_string(), 100.0);

        let (scene, params) = engine
            .eval_str_with_params(
                r#"
                let width = param("Width", 60.0, 20.0, 200.0);
                emit(cuboid(width, 10.0, 10.0));
                "#,
                &overrides,
            )
            .expect("script should evaluate");

        assert_eq!(params[0].value, 100.0);
        assert_eq!(
            params[0].default, 60.0,
            "default stays the script's own value"
        );

        let mesh = scene.part("part").unwrap();
        let mut max_x = f64::NEG_INFINITY;
        for tri in &mesh.triangles {
            for v in tri {
                max_x = max_x.max(v.pos.x);
            }
        }
        assert!(
            (max_x - 50.0).abs() < 1e-6,
            "half of overridden width 100 is 50, got {max_x}"
        );
    }

    #[test]
    fn unmatched_override_is_harmless() {
        let engine = ScriptEngine::new();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("NonexistentParam".to_string(), 999.0);
        let result = engine.eval_str_with_params("emit(cuboid(1.0, 1.0, 1.0));", &overrides);
        assert!(result.is_ok());
    }

    #[test]
    fn split_returns_an_indexable_array_of_two_solids() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                let solid = cuboid(20.0, 15.0, 10.0);
                let halves = solid.split(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
                emit("top", halves[0]);
                emit("bottom", halves[1]);
                "#,
            )
            .expect("script should evaluate");
        assert!(scene.part("top").unwrap().triangle_count() > 0);
        assert!(scene.part("bottom").unwrap().triangle_count() > 0);
    }

    #[test]
    fn split_with_dowels_is_callable_from_a_script() {
        let engine = ScriptEngine::new();
        let scene = engine
            .eval_str(
                r#"
                let solid = cuboid(20.0, 15.0, 10.0);
                let halves = solid.split_with_dowels(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2, 1.5, 4.0);
                emit("a", halves[0]);
                emit("b", halves[1]);
                "#,
            )
            .expect("script should evaluate");
        assert!(scene.part("a").unwrap().triangle_count() > 0);
        assert!(scene.part("b").unwrap().triangle_count() > 0);
    }
}
