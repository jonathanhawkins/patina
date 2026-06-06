//! Script instance traits and supporting types for scripting interop.
//!
//! This module defines the `ScriptInstance` trait that every scripting backend
//! (GDScript, Rust-native, etc.) must implement, along with metadata structs
//! for method and property introspection.

use std::fmt;

use gdvariant::variant::VariantType;
use gdvariant::Variant;

/// Bitflags describing method characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodFlags(u8);

impl MethodFlags {
    /// A regular instance method.
    pub const NORMAL: Self = Self(0b001);
    /// A virtual method (overridable by subclasses).
    pub const VIRTUAL: Self = Self(0b010);
    /// A const method (does not mutate the object).
    pub const CONST: Self = Self(0b100);

    /// Returns `true` if `self` contains all the flags in `other`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for MethodFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl fmt::Display for MethodFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.contains(Self::NORMAL) {
            parts.push("NORMAL");
        }
        if self.contains(Self::VIRTUAL) {
            parts.push("VIRTUAL");
        }
        if self.contains(Self::CONST) {
            parts.push("CONST");
        }
        if parts.is_empty() {
            write!(f, "(none)")
        } else {
            write!(f, "{}", parts.join(" | "))
        }
    }
}

/// Metadata describing a single method on a script.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// The method name.
    pub name: String,
    /// Names of the method's arguments.
    pub argument_names: Vec<String>,
    /// The return type of the method.
    pub return_type: VariantType,
    /// Method flags.
    pub flags: MethodFlags,
}

impl fmt::Display for MethodInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}({}) -> {} [{}]",
            self.name,
            self.argument_names.join(", "),
            self.return_type,
            self.flags,
        )
    }
}

/// Metadata describing a single property on a script.
#[derive(Debug, Clone)]
pub struct ScriptPropertyInfo {
    /// The property name.
    pub name: String,
    /// The type of the property value.
    pub property_type: VariantType,
    /// The default value for this property.
    pub default_value: Variant,
}

/// Errors that can occur when interacting with a script instance.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScriptError {
    /// The requested method does not exist on the script.
    #[error("method not found: '{0}'")]
    MethodNotFound(String),

    /// The wrong number of arguments were passed to a method.
    #[error("invalid argument count: expected {expected}, got {got}")]
    InvalidArgCount {
        /// Expected number of arguments.
        expected: usize,
        /// Actual number of arguments provided.
        got: usize,
    },

    /// The requested property does not exist on the script.
    #[error("property not found: '{0}'")]
    PropertyNotFound(String),

    /// A type mismatch occurred during a script operation.
    #[error("type error: {0}")]
    TypeError(String),

    /// No script is attached to the given object.
    #[error("no script attached to object")]
    NoScript,
}

/// Trait implemented by all script instances.
///
/// Each scripting backend provides its own concrete type that implements this
/// trait, allowing the engine to call methods, get/set properties, and
/// introspect the script's API uniformly.
pub trait ScriptInstance {
    /// Calls a method by name with the given arguments.
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError>;

    /// Gets a property value by name, or `None` if it does not exist.
    fn get_property(&self, name: &str) -> Option<Variant>;

    /// Sets a property value by name. Returns `true` if the property existed.
    fn set_property(&mut self, name: &str, value: Variant) -> bool;

    /// Returns metadata for all methods exposed by this script.
    fn list_methods(&self) -> Vec<MethodInfo>;

    /// Returns metadata for all properties exposed by this script.
    fn list_properties(&self) -> Vec<ScriptPropertyInfo>;

    /// Returns the human-readable name of this script.
    fn get_script_name(&self) -> &str;

    /// Returns `true` if the script defines a method with the given name.
    fn has_method(&self, name: &str) -> bool {
        self.list_methods().iter().any(|m| m.name == name)
    }

    /// Resolves `@onready` variables by evaluating their default expressions.
    /// Called by the scene tree just before `_ready` fires.
    /// Default implementation is a no-op.
    fn resolve_onready(&mut self) -> Result<(), ScriptError> {
        Ok(())
    }

    /// Inject scene-tree access so the script can call get_node / emit_signal.
    fn set_scene_access(&mut self, _access: Box<dyn SceneAccess>, _node_id: u64) {}

    /// Remove scene-tree access after a script callback finishes.
    fn clear_scene_access(&mut self) {}
}

/// Trait providing scene-tree operations to a running script.
///
/// The engine crate (`gdscene`) implements this trait on an accessor that wraps
/// a raw pointer to the `SceneTree`, breaking the circular borrow that would
/// otherwise prevent a script from calling back into the tree during execution.
pub trait SceneAccess {
    /// Resolve a node by path relative to `from`.
    fn get_node(&self, from: u64, path: &str) -> Option<u64>;
    /// Return the parent of `node`, if any.
    fn get_parent(&self, node: u64) -> Option<u64>;
    /// Return the children of `node`.
    fn get_children(&self, node: u64) -> Vec<u64>;
    /// Read a property from `node`.
    fn get_node_property(&self, node: u64, prop: &str) -> Variant;
    /// Write a property on `node`.
    fn set_node_property(&mut self, node: u64, prop: &str, value: Variant);
    /// Emit a signal on `node`.
    fn emit_signal(&mut self, node: u64, signal: &str, args: &[Variant]);
    /// Connect a signal on `source` to `method` on `target`.
    fn connect_signal(&mut self, source: u64, signal: &str, target: u64, method: &str);
    /// Return the name of `node`, if it exists.
    fn get_node_name(&self, node: u64) -> Option<String>;

    // -- Runtime node creation/deletion ------------------------------------

    /// Create a new node of the given class (not yet in the tree).
    /// Returns the raw ObjectId of the new node.
    fn create_node(&mut self, _class_name: &str, _name: &str) -> Option<u64> {
        None
    }

    /// Add `child_id` as a child of `parent_id` in the scene tree.
    fn add_child(&mut self, _parent_id: u64, _child_id: u64) -> bool {
        false
    }

    /// Mark a node for deferred deletion (removed at end of frame).
    fn queue_free(&mut self, _node_id: u64) {}

    /// Return the class name of a node.
    fn get_class(&self, _node: u64) -> Option<String> {
        None
    }

    // -- Input methods (used by the `Input` singleton in GDScript) ----------

    /// Returns `true` if any key mapped to `action` is currently held.
    fn is_input_action_pressed(&self, _action: &str) -> bool {
        false
    }

    /// Returns `true` if any key mapped to `action` was just pressed this frame.
    fn is_input_action_just_pressed(&self, _action: &str) -> bool {
        false
    }

    /// Returns `true` if the raw key `key` is currently held.
    fn is_input_key_pressed(&self, _key: &str) -> bool {
        false
    }

    /// Returns the global mouse position as (x, y).
    fn get_global_mouse_position(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    /// Returns `true` if the given mouse button index is pressed.
    fn is_mouse_button_pressed(&self, _button_index: i64) -> bool {
        false
    }

    /// Returns a direction Vector2 from four input actions.
    fn get_input_vector(&self, neg_x: &str, pos_x: &str, neg_y: &str, pos_y: &str) -> (f32, f32) {
        let mut x: f32 = 0.0;
        let mut y: f32 = 0.0;
        if self.is_input_action_pressed(neg_x) {
            x -= 1.0;
        }
        if self.is_input_action_pressed(pos_x) {
            x += 1.0;
        }
        if self.is_input_action_pressed(neg_y) {
            y -= 1.0;
        }
        if self.is_input_action_pressed(pos_y) {
            y += 1.0;
        }
        // Normalize if diagonal
        let len = (x * x + y * y).sqrt();
        if len > 0.0 {
            (x / len, y / len)
        } else {
            (0.0, 0.0)
        }
    }
}
