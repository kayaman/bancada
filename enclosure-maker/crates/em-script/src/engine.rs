use crate::bindings::register_all_bindings;
use em_core::Mesh;
use em_primitives::Csg;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("script error: {0}")]
    Rhai(#[from] Box<rhai::EvalAltResult>),
    #[error("I/O error reading script: {0}")]
    Io(#[from] std::io::Error),
    #[error("script did not call emit(...) on any solid")]
    NoParts,
}

/// The solids a script produced. `emit` entries are printable parts: a
/// script that calls `emit(csg)` (no name) gets one part named `"part"`; a
/// script that calls `emit("base", csg)` / `emit("lid", csg)` / etc. gets
/// one entry per name. `view` entries are preview-only compositions
/// (`assembly`, `exploded`, `section`) and are not printed. Re-emitting or
/// re-viewing an existing name replaces that entry rather than adding a
/// duplicate.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    parts: Vec<(String, Mesh)>,
    views: Vec<(String, Mesh)>,
}

impl Scene {
    /// A printable part, or a preview-only view of the same name if no
    /// printable part has it. Printable parts win when both exist.
    pub fn part(&self, name: &str) -> Option<&Mesh> {
        self.parts
            .iter()
            .chain(self.views.iter())
            .find(|(n, _)| n == name)
            .map(|(_, m)| m)
    }

    /// Printable parts, in emit order. These are what a slicer should receive
    /// as separate objects.
    pub fn printable_parts(&self) -> &[(String, Mesh)] {
        &self.parts
    }

    /// Printable part names, then preview-only view names.
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts
            .iter()
            .chain(self.views.iter())
            .map(|(n, _)| n.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Printable parts' triangles concatenated into one mesh for rendering.
    /// This is plain concatenation, not a boolean union, so the lid and body
    /// stay distinct solids. Preview-only views are left out.
    pub fn assembly_mesh(&self) -> Mesh {
        let mut triangles = Vec::new();
        for (_, mesh) in &self.parts {
            triangles.extend(mesh.triangles.iter().copied());
        }
        Mesh { triangles }
    }
}

/// A script-declared, UI-adjustable parameter (via the `param(...)` Rhai
/// binding). `value` is the override currently in effect, or `default` if
/// none was supplied for this evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub value: f64,
}

thread_local! {
    static PARTS: RefCell<Vec<(String, Csg, bool)>> = const { RefCell::new(Vec::new()) };
    static PARAMS: RefCell<Vec<ParamInfo>> = const { RefCell::new(Vec::new()) };
    static OVERRIDES: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

fn stash(name: String, csg: Csg, printable: bool) {
    PARTS.with(|parts| {
        let mut parts = parts.borrow_mut();
        if let Some(entry) = parts
            .iter_mut()
            .find(|(n, _, flag)| *n == name && *flag == printable)
        {
            entry.1 = csg;
        } else {
            parts.push((name, csg, printable));
        }
    });
}

pub(crate) fn stash_part(name: String, csg: Csg) {
    stash(name, csg, true);
}

pub(crate) fn stash_view(name: String, csg: Csg) {
    stash(name, csg, false);
}

/// Called by the `param(...)` Rhai binding: records this parameter's
/// declaration (for the UI schema) and returns the value to use --
/// whatever override is currently set for `name`, or `default` otherwise.
pub(crate) fn stash_param(name: String, default: f64, min: f64, max: f64) -> f64 {
    let value = OVERRIDES
        .with(|o| o.borrow().get(&name).copied())
        .unwrap_or(default);
    PARAMS.with(|params| {
        let mut params = params.borrow_mut();
        let info = ParamInfo {
            name: name.clone(),
            default,
            min,
            max,
            value,
        };
        if let Some(entry) = params.iter_mut().find(|p| p.name == name) {
            *entry = info;
        } else {
            params.push(info);
        }
    });
    value
}

pub struct ScriptEngine {
    engine: rhai::Engine,
}

impl ScriptEngine {
    pub fn new() -> Self {
        let mut engine = rhai::Engine::new();
        register_all_bindings(&mut engine);
        ScriptEngine { engine }
    }

    /// Like [`new`](Self::new), but anchors `import` resolution to `root`
    /// instead of the process's current working directory. Without this,
    /// `import "lib/presets" as presets;` inside a script only resolves
    /// correctly when the CLI happens to be invoked from that script's own
    /// directory -- CWD-relative resolution is otherwise silently fragile.
    pub fn with_import_root(root: impl AsRef<Path>) -> Self {
        let mut engine = rhai::Engine::new();
        register_all_bindings(&mut engine);
        engine.set_module_resolver(rhai::module_resolvers::FileModuleResolver::new_with_path(
            root.as_ref(),
        ));
        ScriptEngine { engine }
    }

    pub fn eval_file(&self, path: impl AsRef<Path>) -> Result<Scene, ScriptError> {
        self.eval_file_with_params(path, &HashMap::new())
            .map(|(scene, _)| scene)
    }

    pub fn eval_str(&self, src: &str) -> Result<Scene, ScriptError> {
        self.eval_str_with_params(src, &HashMap::new())
            .map(|(scene, _)| scene)
    }

    /// Like [`eval_file`](Self::eval_file), but `overrides` supplies values
    /// for any `param(name, ...)` calls by name (unmatched overrides are
    /// simply never looked up -- harmless if a script drops a parameter).
    /// Returns the produced [`Scene`] plus every parameter the script
    /// declared, in declaration order, each with its currently-effective
    /// value -- enough for a caller to render UI controls for them.
    pub fn eval_file_with_params(
        &self,
        path: impl AsRef<Path>,
        overrides: &HashMap<String, f64>,
    ) -> Result<(Scene, Vec<ParamInfo>), ScriptError> {
        let src = std::fs::read_to_string(path)?;
        self.eval_str_with_params(&src, overrides)
    }

    pub fn eval_str_with_params(
        &self,
        src: &str,
        overrides: &HashMap<String, f64>,
    ) -> Result<(Scene, Vec<ParamInfo>), ScriptError> {
        PARTS.with(|parts| parts.borrow_mut().clear());
        PARAMS.with(|params| params.borrow_mut().clear());
        OVERRIDES.with(|o| *o.borrow_mut() = overrides.clone());

        self.engine.run(src)?;

        let stashed = PARTS.with(|parts| std::mem::take(&mut *parts.borrow_mut()));
        let params = PARAMS.with(|params| std::mem::take(&mut *params.borrow_mut()));
        let mut parts = Vec::new();
        let mut views = Vec::new();
        for (name, csg, printable) in stashed {
            let mesh = csg.to_mesh();
            if printable {
                parts.push((name, mesh));
            } else {
                views.push((name, mesh));
            }
        }
        if parts.is_empty() {
            return Err(ScriptError::NoParts);
        }
        Ok((Scene { parts, views }, params))
    }
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}
