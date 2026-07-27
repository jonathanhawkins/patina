//! pat-0q2kx.225 — the project-wide global shader parameter store.
//!
//! Godot's global uniforms are shared by every shader that names them: a shader
//! declaring `global uniform vec4 tint;` reads whatever this store holds,
//! `RenderingServer.global_shader_parameter_set` mutates it at runtime, and all
//! referencing shaders update together. The values are seeded from
//! `project.godot`'s `[shader_globals]` section at boot.
//!
//! ## Why this lives in `gdvariant`
//!
//! Two very different crates must see the *same* store: `gdserver3d` resolves
//! `Global`-scoped uniforms while rendering, and `gdscript-interop` exposes the
//! `RenderingServer.global_shader_parameter_*` script surface. Their only shared
//! dependency that also has `Variant` — which the get/set API is defined in
//! terms of — is `gdvariant`.
//!
//! It cannot live in `gdresource` (where the `[shader_globals]` *parsing* is),
//! because `gdserver3d` does not depend on `gdresource`; a store there would be
//! invisible to the shader resolver, i.e. dead code. It cannot live in `gdcore`
//! either, which has no `Variant`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::Variant;

/// One global shader parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalShaderParameter {
    /// Godot's declared type string — `"color"`, `"vec4"`, `"float"`, … kept as
    /// written so `global_shader_parameter_get_type` can report it verbatim.
    pub type_name: String,
    /// The value set by the project file or `global_shader_parameter_set`.
    pub value: Variant,
    /// A `ShaderGlobalsOverride` node's value, which wins while it is active.
    ///
    /// Held separately rather than clobbering `value` so that reverting the
    /// override restores what the project or a script last set, instead of
    /// leaving the override's value behind permanently.
    pub override_value: Option<Variant>,
}

impl GlobalShaderParameter {
    /// The value a shader should read: the override if one is active.
    pub fn effective(&self) -> &Variant {
        self.override_value.as_ref().unwrap_or(&self.value)
    }
}

/// The process-global store, mirroring the `OnceLock<Mutex<..>>` idiom the
/// ClassDB uses. Process-global rather than thread-local because the renderer
/// and the script VM must observe the same values.
fn store() -> &'static Mutex<HashMap<String, GlobalShaderParameter>> {
    static STORE: OnceLock<Mutex<HashMap<String, GlobalShaderParameter>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `RenderingServer.global_shader_parameter_add`.
///
/// Re-adding an existing name replaces its declared type and value, matching
/// Godot — the project file is re-applied on reload without needing a remove.
pub fn add(name: &str, type_name: &str, default_value: Variant) {
    let mut guard = store().lock().expect("shader globals poisoned");
    guard.insert(
        name.to_string(),
        GlobalShaderParameter {
            type_name: type_name.to_string(),
            value: default_value,
            override_value: None,
        },
    );
}

/// `RenderingServer.global_shader_parameter_remove`.
pub fn remove(name: &str) {
    store()
        .lock()
        .expect("shader globals poisoned")
        .remove(name);
}

/// `RenderingServer.global_shader_parameter_set`.
///
/// Setting an unknown name is a no-op rather than an implicit add: Godot
/// requires the parameter to be declared (by the project file or an explicit
/// `add`) so that its *type* is known, and silently creating a typeless entry
/// would surface later as a shader binding failure with no link back to here.
pub fn set(name: &str, value: Variant) -> bool {
    let mut guard = store().lock().expect("shader globals poisoned");
    match guard.get_mut(name) {
        Some(param) => {
            param.value = value;
            true
        }
        None => false,
    }
}

/// `RenderingServer.global_shader_parameter_set_override`.
///
/// `None` clears the override and restores the underlying value.
pub fn set_override(name: &str, value: Option<Variant>) -> bool {
    let mut guard = store().lock().expect("shader globals poisoned");
    match guard.get_mut(name) {
        Some(param) => {
            param.override_value = value;
            true
        }
        None => false,
    }
}

/// `RenderingServer.global_shader_parameter_get` — the effective value, or
/// `Nil` when the parameter is not declared.
pub fn get(name: &str) -> Variant {
    store()
        .lock()
        .expect("shader globals poisoned")
        .get(name)
        .map(|p| p.effective().clone())
        .unwrap_or(Variant::Nil)
}

/// `RenderingServer.global_shader_parameter_get_type`.
pub fn get_type(name: &str) -> Option<String> {
    store()
        .lock()
        .expect("shader globals poisoned")
        .get(name)
        .map(|p| p.type_name.clone())
}

/// `RenderingServer.global_shader_parameter_get_list`, sorted.
///
/// Sorted because the backing map iterates arbitrarily; an unstable list would
/// make both the inspector and any test that reads it nondeterministic.
pub fn get_list() -> Vec<String> {
    let guard = store().lock().expect("shader globals poisoned");
    let mut names: Vec<String> = guard.keys().cloned().collect();
    names.sort();
    names
}

/// Whether a parameter is declared.
pub fn contains(name: &str) -> bool {
    store()
        .lock()
        .expect("shader globals poisoned")
        .contains_key(name)
}

/// Drops every parameter — used when loading a different project, and by tests
/// that need a clean store.
pub fn clear() {
    store().lock().expect("shader globals poisoned").clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resets the process-global store.
    ///
    /// SAFE because this repo runs its tests with nextest, which gives **each
    /// test its own process**, so one test's `clear()` cannot race another's
    /// data. Under a single-process runner with parallel threads these would
    /// interfere. If the runner ever changes, switch to per-test unique names
    /// and drop the clear.
    fn fresh() {
        clear();
    }

    #[test]
    fn add_get_and_type_roundtrip() {
        fresh();
        add("tint", "color", Variant::Int(7));
        assert_eq!(get("tint"), Variant::Int(7));
        assert_eq!(get_type("tint").as_deref(), Some("color"));
        assert!(contains("tint"));
    }

    #[test]
    fn set_requires_a_declared_parameter() {
        fresh();
        // Undeclared: refused, and stays absent rather than being created with
        // an unknown type.
        assert!(!set("ghost", Variant::Int(1)));
        assert!(!contains("ghost"));
        assert_eq!(get("ghost"), Variant::Nil);

        add("ghost", "float", Variant::Float(0.0));
        assert!(set("ghost", Variant::Float(2.5)));
        assert_eq!(get("ghost"), Variant::Float(2.5));
    }

    #[test]
    fn override_wins_and_reverting_restores_the_underlying_value() {
        fresh();
        add("wind", "float", Variant::Float(1.0));
        set("wind", Variant::Float(3.0));

        assert!(set_override("wind", Some(Variant::Float(9.0))));
        assert_eq!(get("wind"), Variant::Float(9.0), "override wins");

        assert!(set_override("wind", None));
        assert_eq!(
            get("wind"),
            Variant::Float(3.0),
            "clearing the override restores the last set value, not the \
             original default",
        );
    }

    #[test]
    fn re_adding_replaces_type_and_value() {
        fresh();
        add("x", "float", Variant::Float(1.0));
        add("x", "color", Variant::Int(2));
        assert_eq!(get_type("x").as_deref(), Some("color"));
        assert_eq!(get("x"), Variant::Int(2));
        assert_eq!(
            get_list().len(),
            1,
            "re-adding must not duplicate the entry"
        );
    }

    #[test]
    fn list_is_sorted_and_remove_works() {
        fresh();
        for n in ["zeta", "alpha", "mid"] {
            add(n, "float", Variant::Float(0.0));
        }
        assert_eq!(get_list(), vec!["alpha", "mid", "zeta"]);

        remove("mid");
        assert_eq!(get_list(), vec!["alpha", "zeta"]);
        assert!(!contains("mid"));
    }
}
