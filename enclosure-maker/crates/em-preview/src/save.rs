use regex::Regex;
use std::collections::HashMap;

/// Rewrites every `param("Name", <default>, min, max)` call in `source`
/// whose name is a key in `overrides`, replacing just the default-value
/// literal with the override's current value. Returns the rewritten source
/// and the list of names actually found and baked in (a name in
/// `overrides` with no matching `param(...)` call in the source is simply
/// not in this list -- e.g. because the script changed since the override
/// was set).
///
/// Only the numeric literal's byte range is touched; everything else
/// (whitespace, the call's own formatting, surrounding code) is copied
/// through unchanged.
///
/// Only ever called from a user-triggered "Save" click, so compiling the
/// regex fresh on each call (rather than caching it behind a `LazyLock`)
/// costs nothing worth avoiding.
pub fn bake_param_defaults(
    source: &str,
    overrides: &HashMap<String, f64>,
) -> (String, Vec<String>) {
    let param_call = Regex::new(r#"param\s*\(\s*"([^"]*)"\s*,\s*(-?\d+(?:\.\d+)?)"#).unwrap();
    let mut result = String::with_capacity(source.len());
    let mut last_end = 0;
    let mut baked = Vec::new();

    for caps in param_call.captures_iter(source) {
        let name = &caps[1];
        let Some(&value) = overrides.get(name) else {
            continue;
        };
        let literal = caps.get(2).unwrap();
        result.push_str(&source[last_end..literal.start()]);
        result.push_str(&format_rhai_float(value));
        last_end = literal.end();
        baked.push(name.to_string());
    }
    result.push_str(&source[last_end..]);
    (result, baked)
}

/// Formats a value as a Rhai float literal -- always with a decimal point,
/// since `20` and `20.0` are different types to Rhai and every binding in
/// this project expects the latter.
fn format_rhai_float(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bakes_only_overridden_names() {
        let source = "let w = param(\"Width\", 60.0, 40.0, 120.0);\nlet d = param(\"Depth\", 40.0, 30.0, 80.0);\n";
        let mut overrides = HashMap::new();
        overrides.insert("Width".to_string(), 90.0);

        let (rewritten, baked) = bake_param_defaults(source, &overrides);
        assert_eq!(baked, vec!["Width"]);
        assert!(rewritten.contains("param(\"Width\", 90.0, 40.0, 120.0)"));
        assert!(
            rewritten.contains("param(\"Depth\", 40.0, 30.0, 80.0)"),
            "untouched param call should be byte-identical"
        );
    }

    #[test]
    fn formats_whole_numbers_with_decimal_point() {
        let source = "param(\"Count\", 4.0, 1.0, 10.0)";
        let mut overrides = HashMap::new();
        overrides.insert("Count".to_string(), 7.0);
        let (rewritten, _) = bake_param_defaults(source, &overrides);
        assert!(rewritten.contains("param(\"Count\", 7.0, 1.0, 10.0)"));
    }

    #[test]
    fn preserves_fractional_values_and_unusual_spacing() {
        let source = "param( \"Wall\" ,  2.0, 1.2, 4.0)";
        let mut overrides = HashMap::new();
        overrides.insert("Wall".to_string(), 2.75);
        let (rewritten, _) = bake_param_defaults(source, &overrides);
        assert_eq!(rewritten, "param( \"Wall\" ,  2.75, 1.2, 4.0)");
    }

    #[test]
    fn override_with_no_matching_call_is_not_baked() {
        let source = "param(\"Width\", 60.0, 40.0, 120.0)";
        let mut overrides = HashMap::new();
        overrides.insert("Nonexistent".to_string(), 1.0);
        let (rewritten, baked) = bake_param_defaults(source, &overrides);
        assert!(baked.is_empty());
        assert_eq!(rewritten, source);
    }

    #[test]
    fn negative_default_is_matched_and_replaced() {
        let source = "param(\"Offset\", -5.0, -10.0, 10.0)";
        let mut overrides = HashMap::new();
        overrides.insert("Offset".to_string(), -2.5);
        let (rewritten, baked) = bake_param_defaults(source, &overrides);
        assert_eq!(baked, vec!["Offset"]);
        assert_eq!(rewritten, "param(\"Offset\", -2.5, -10.0, 10.0)");
    }
}
