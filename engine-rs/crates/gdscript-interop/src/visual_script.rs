//! VisualScript compatibility stub.
//!
//! VisualScript was deprecated and removed in Godot 4, but projects migrated
//! from Godot 3 may still reference `.vs` files or `VisualScript` resources.
//! This stub implements [`ScriptInstance`] so the engine can load such scenes
//! without crashing — method calls return errors and properties are inert.

use crate::bindings::{MethodInfo, SceneAccess, ScriptError, ScriptInstance, ScriptPropertyInfo};
use gdvariant::Variant;

/// A no-op [`ScriptInstance`] for deprecated VisualScript resources.
///
/// All method calls return [`ScriptError::MethodNotFound`] and property
/// access returns `None` / `false`. This is intentional — VisualScript
/// is non-functional in Godot 4 and this stub exists solely to prevent
/// load-time failures.
#[derive(Debug, Clone)]
pub struct VisualScriptStub {
    script_path: String,
}

impl VisualScriptStub {
    /// Creates a stub for the given `.vs` resource path.
    pub fn new(script_path: impl Into<String>) -> Self {
        Self {
            script_path: script_path.into(),
        }
    }

    /// Returns the original `.vs` resource path.
    pub fn script_path(&self) -> &str {
        &self.script_path
    }
}

impl ScriptInstance for VisualScriptStub {
    fn call_method(&mut self, name: &str, _args: &[Variant]) -> Result<Variant, ScriptError> {
        Err(ScriptError::MethodNotFound(format!(
            "VisualScript is deprecated; method '{}' is unavailable",
            name
        )))
    }

    fn get_property(&self, _name: &str) -> Option<Variant> {
        None
    }

    fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
        false
    }

    fn list_methods(&self) -> Vec<MethodInfo> {
        Vec::new()
    }

    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        Vec::new()
    }

    fn get_script_name(&self) -> &str {
        "VisualScript"
    }

    fn has_method(&self, _name: &str) -> bool {
        false
    }

    fn set_scene_access(&mut self, _access: Box<dyn SceneAccess>, _node_id: u64) {
        // No-op: VisualScript stubs do not interact with the scene tree.
    }

    fn resolve_onready(&mut self) -> Result<(), ScriptError> {
        // No-op: nothing to resolve.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_visual_script_name() {
        let stub = VisualScriptStub::new("res://scripts/old.vs");
        assert_eq!(stub.get_script_name(), "VisualScript");
        assert_eq!(stub.script_path(), "res://scripts/old.vs");
    }

    #[test]
    fn stub_method_calls_return_error() {
        let mut stub = VisualScriptStub::new("res://test.vs");
        let result = stub.call_method("_ready", &[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            ScriptError::MethodNotFound(msg) => {
                assert!(
                    msg.contains("_ready"),
                    "error should mention the method name"
                );
                assert!(
                    msg.contains("deprecated"),
                    "error should mention deprecation"
                );
            }
            other => panic!("expected MethodNotFound, got {other:?}"),
        }
    }

    #[test]
    fn stub_properties_are_inert() {
        let mut stub = VisualScriptStub::new("res://test.vs");
        assert_eq!(stub.get_property("speed"), None);
        assert!(!stub.set_property("speed", Variant::Float(10.0)));
    }

    #[test]
    fn stub_lists_are_empty() {
        let stub = VisualScriptStub::new("res://test.vs");
        assert!(stub.list_methods().is_empty());
        assert!(stub.list_properties().is_empty());
        assert!(!stub.has_method("_process"));
    }
}
