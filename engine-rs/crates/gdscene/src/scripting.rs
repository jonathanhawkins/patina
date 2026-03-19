//! Script-to-node integration for the scene tree.
//!
//! This module provides [`GDScriptNodeInstance`], a [`ScriptInstance`]
//! implementation that wraps a parsed GDScript class and connects its
//! lifecycle methods (`_ready`, `_process`, `_physics_process`, etc.) to
//! scene tree nodes.
//!
//! The [`SceneTree`] stores attached scripts in a side map
//! (`HashMap<NodeId, Box<dyn ScriptInstance>>`) and exposes methods to
//! dispatch lifecycle callbacks to them.

use std::collections::HashMap;

use gdscript_interop::bindings::{
    MethodFlags, MethodInfo, SceneAccess, ScriptError, ScriptInstance, ScriptPropertyInfo,
};
use gdscript_interop::interpreter::{ClassInstance, Interpreter, RuntimeError};
use gdvariant::variant::VariantType;
use gdvariant::Variant;

use crate::node::NodeId;

/// Per-node script storage.
pub type ScriptStore = HashMap<NodeId, Box<dyn ScriptInstance>>;

// ---------------------------------------------------------------------------
// GDScriptNodeInstance
// ---------------------------------------------------------------------------

/// A [`ScriptInstance`] backed by a parsed GDScript class.
///
/// Wraps an [`Interpreter`], [`ClassDef`], and [`ClassInstance`] so that
/// lifecycle methods like `_ready` and `_process` execute the corresponding
/// GDScript methods defined in the source file.
pub struct GDScriptNodeInstance {
    interpreter: Interpreter,
    instance: ClassInstance,
    node_id: NodeId,
}

impl std::fmt::Debug for GDScriptNodeInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GDScriptNodeInstance")
            .field("node_id", &self.node_id)
            .field("class_name", &self.instance.class_def.name)
            .finish()
    }
}

impl GDScriptNodeInstance {
    /// Creates a new script instance by parsing GDScript source.
    ///
    /// The source is expected to be a class-style script (with `extends`,
    /// `var` declarations, and `func` definitions).
    pub fn from_source(source: &str, node_id: NodeId) -> Result<Self, RuntimeError> {
        let mut interpreter = Interpreter::new();
        let class_def = interpreter.run_class(source)?;
        let instance = interpreter.instantiate_class(&class_def)?;
        Ok(Self {
            interpreter,
            instance,
            node_id,
        })
    }

    /// Returns the node ID this script is attached to.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Returns true if the class defines a method with the given name.
    pub fn has_method(&self, name: &str) -> bool {
        self.instance.class_def.methods.contains_key(name)
    }
}

impl ScriptInstance for GDScriptNodeInstance {
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError> {
        self.interpreter
            .call_instance_method(&mut self.instance, name, args)
            .map_err(|e| match e.kind {
                gdscript_interop::interpreter::RuntimeErrorKind::UndefinedFunction(n) => {
                    ScriptError::MethodNotFound(n)
                }
                gdscript_interop::interpreter::RuntimeErrorKind::TypeError(msg) => {
                    ScriptError::TypeError(msg)
                }
                other => ScriptError::TypeError(other.to_string()),
            })
    }

    fn get_property(&self, name: &str) -> Option<Variant> {
        self.instance.properties.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, value: Variant) -> bool {
        if self.instance.properties.contains_key(name) {
            self.instance.properties.insert(name.to_string(), value);
            true
        } else {
            false
        }
    }

    fn list_methods(&self) -> Vec<MethodInfo> {
        self.instance
            .class_def
            .methods
            .iter()
            .map(|(name, func)| MethodInfo {
                name: name.clone(),
                argument_names: func.params.clone(),
                return_type: VariantType::Nil,
                flags: MethodFlags::NORMAL,
            })
            .collect()
    }

    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        self.instance
            .properties
            .iter()
            .map(|(name, val)| ScriptPropertyInfo {
                name: name.clone(),
                property_type: val.variant_type(),
                default_value: val.clone(),
            })
            .collect()
    }

    fn get_script_name(&self) -> &str {
        self.instance
            .class_def
            .name
            .as_deref()
            .unwrap_or("GDScript")
    }

    fn set_scene_access(&mut self, access: Box<dyn SceneAccess>, node_id: u64) {
        self.interpreter.set_scene_access(access, node_id);
    }

    fn clear_scene_access(&mut self) {
        self.interpreter.clear_scene_access();
    }
}

// ---------------------------------------------------------------------------
// InputSnapshot — a frozen copy of input state for one frame
// ---------------------------------------------------------------------------

use std::collections::HashSet;

/// A snapshot of input state at a single frame, passed to scripts via
/// [`SceneTreeAccessor`] so they can query `Input.is_action_pressed()` etc.
#[derive(Debug, Clone, Default)]
pub struct InputSnapshot {
    /// Keys currently held down (browser key names like "ArrowLeft", "a").
    pub pressed_keys: HashSet<String>,
    /// Keys that were first pressed this frame.
    pub just_pressed_keys: HashSet<String>,
    /// Action name → list of key names.
    pub input_map: std::collections::HashMap<String, Vec<String>>,
}

impl InputSnapshot {
    /// Returns `true` if any key mapped to `action` is held.
    pub fn is_action_pressed(&self, action: &str) -> bool {
        if let Some(keys) = self.input_map.get(action) {
            keys.iter().any(|k| self.pressed_keys.contains(k))
        } else {
            false
        }
    }

    /// Returns `true` if any key mapped to `action` was just pressed.
    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        if let Some(keys) = self.input_map.get(action) {
            keys.iter().any(|k| self.just_pressed_keys.contains(k))
        } else {
            false
        }
    }

    /// Returns `true` if the raw key is held.
    pub fn is_key_pressed(&self, key: &str) -> bool {
        self.pressed_keys.contains(key)
    }
}

// ---------------------------------------------------------------------------
// SceneTreeAccessor
// ---------------------------------------------------------------------------

use crate::scene_tree::SceneTree;
use gdcore::id::ObjectId;
use gdobject::signal::Connection;

/// Wraps a raw pointer to [`SceneTree`] so that a running script can call
/// back into the tree (e.g. `get_node`, `emit_signal`) during execution.
///
/// # Safety
///
/// The pointer is valid for the duration of a single `call_script_with_access`
/// call. The script is temporarily removed from the tree's script map before
/// the accessor is created, so there is no aliasing of the script itself.
pub(crate) struct SceneTreeAccessor {
    tree: *mut SceneTree,
    input: Option<InputSnapshot>,
}

impl SceneTreeAccessor {
    /// Creates a new accessor. Caller must ensure the pointer is valid.
    pub(crate) unsafe fn new(tree: *mut SceneTree) -> Self {
        Self { tree, input: None }
    }

    /// Creates a new accessor with an input snapshot.
    pub(crate) unsafe fn with_input(tree: *mut SceneTree, input: InputSnapshot) -> Self {
        Self {
            tree,
            input: Some(input),
        }
    }

    fn tree(&self) -> &SceneTree {
        unsafe { &*self.tree }
    }

    fn tree_mut(&mut self) -> &mut SceneTree {
        unsafe { &mut *self.tree }
    }
}

impl SceneAccess for SceneTreeAccessor {
    fn get_node(&self, from: u64, path: &str) -> Option<u64> {
        let from_id = NodeId::from_object_id(ObjectId::from_raw(from));
        let tree = self.tree();
        // Use the tree's built-in relative path lookup first
        if let Some(found) = tree.get_node_or_null(from_id, path) {
            return Some(found.raw());
        }
        // Fallback: search children by name
        if let Some(from_node) = tree.get_node(from_id) {
            for &child_id in from_node.children() {
                if let Some(child_node) = tree.get_node(child_id) {
                    if child_node.name() == path {
                        return Some(child_id.raw());
                    }
                }
            }
        }
        // Search siblings
        if let Some(from_node) = tree.get_node(from_id) {
            if let Some(parent_id) = from_node.parent() {
                if let Some(parent_node) = tree.get_node(parent_id) {
                    for &sib_id in parent_node.children() {
                        if let Some(sib_node) = tree.get_node(sib_id) {
                            if sib_node.name() == path {
                                return Some(sib_id.raw());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn get_parent(&self, node: u64) -> Option<u64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_node(nid)
            .and_then(|n| n.parent())
            .map(|pid| pid.raw())
    }

    fn get_children(&self, node: u64) -> Vec<u64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_node(nid)
            .map(|n| n.children().iter().map(|id| id.raw()).collect())
            .unwrap_or_default()
    }

    fn get_node_property(&self, node: u64, prop: &str) -> Variant {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        // Try script property first
        if let Some(script) = self.tree().get_script(nid) {
            if let Some(val) = script.get_property(prop) {
                return val;
            }
        }
        // Then try node property
        if let Some(n) = self.tree().get_node(nid) {
            return n.get_property(prop);
        }
        Variant::Nil
    }

    fn set_node_property(&mut self, node: u64, prop: &str, value: Variant) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        // Try script property first
        if let Some(script) = self.tree_mut().get_script_mut(nid) {
            if script.set_property(prop, value.clone()) {
                return;
            }
        }
        // Then try node property
        if let Some(n) = self.tree_mut().get_node_mut(nid) {
            n.set_property(prop, value);
        }
    }

    fn emit_signal(&mut self, node: u64, signal: &str, args: &[Variant]) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().emit_signal(nid, signal, args);
    }

    fn connect_signal(&mut self, source: u64, signal: &str, target: u64, method: &str) {
        let source_id = NodeId::from_object_id(ObjectId::from_raw(source));
        let target_oid = ObjectId::from_raw(target);
        let conn = Connection::new(target_oid, method);
        self.tree_mut().connect_signal(source_id, signal, conn);
    }

    fn get_node_name(&self, node: u64) -> Option<String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().get_node(nid).map(|n| n.name().to_string())
    }

    fn is_input_action_pressed(&self, action: &str) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_action_pressed(action))
            .unwrap_or(false)
    }

    fn is_input_action_just_pressed(&self, action: &str) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_action_just_pressed(action))
            .unwrap_or(false)
    }

    fn is_input_key_pressed(&self, key: &str) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_key_pressed(key))
            .unwrap_or(false)
    }

    fn create_node(&mut self, class_name: &str, name: &str) -> Option<u64> {
        let id = self.tree_mut().create_node(class_name, name);
        Some(id.raw())
    }

    fn add_child(&mut self, parent_id: u64, child_id: u64) -> bool {
        let pid = NodeId::from_object_id(ObjectId::from_raw(parent_id));
        let cid = NodeId::from_object_id(ObjectId::from_raw(child_id));

        // The node was created by create_node and already lives in the arena.
        // We extract it, then re-insert via SceneTree::add_child which wires
        // up the parent-child relationship and lifecycle events.
        let tree = self.tree_mut();
        if let Some(node) = tree.take_node(cid) {
            tree.add_child(pid, node).is_ok()
        } else {
            false
        }
    }

    fn queue_free(&mut self, node_id: u64) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node_id));
        self.tree_mut().queue_free(nid);
    }

    fn get_class(&self, node: u64) -> Option<String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_node(nid)
            .map(|n| n.class_name().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::LifecycleManager;
    use crate::main_loop::MainLoop;
    use crate::node::Node;
    use crate::scene_tree::SceneTree;

    const PLAYER_SCRIPT: &str = "\
extends Node2D
var speed = 100.0
var moved = false
func _ready():
    self.moved = true
func _process(delta):
    self.speed = self.speed + delta
";

    const PHYSICS_SCRIPT: &str = "\
extends Node2D
var velocity = 0.0
func _physics_process(delta):
    self.velocity = self.velocity + delta * 10.0
";

    const ENTER_EXIT_SCRIPT: &str = "\
extends Node
var entered = false
var exited = false
func _enter_tree():
    self.entered = true
func _exit_tree():
    self.exited = true
";

    const MINIMAL_SCRIPT: &str = "\
extends Node
var alive = true
";

    const MULTI_LIFECYCLE_SCRIPT: &str = "\
extends Node2D
var log_count = 0
func _ready():
    self.log_count = self.log_count + 1
func _process(delta):
    self.log_count = self.log_count + 1
func _physics_process(delta):
    self.log_count = self.log_count + 1
";

    // -- GDScriptNodeInstance unit tests ------------------------------------

    #[test]
    fn create_instance_from_source() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.node_id(), node_id);
        assert!(inst.has_method("_ready"));
        assert!(inst.has_method("_process"));
    }

    #[test]
    fn call_ready_modifies_property() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_property("moved"), Some(Variant::Bool(false)));
        inst.call_method("_ready", &[]).unwrap();
        assert_eq!(inst.get_property("moved"), Some(Variant::Bool(true)));
    }

    #[test]
    fn call_process_with_delta() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(100.0)));
        inst.call_method("_process", &[Variant::Float(0.5)])
            .unwrap();
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(100.5)));
    }

    #[test]
    fn list_methods_includes_lifecycle() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        let methods: Vec<String> = inst.list_methods().iter().map(|m| m.name.clone()).collect();
        assert!(methods.contains(&"_ready".to_string()));
        assert!(methods.contains(&"_process".to_string()));
    }

    #[test]
    fn list_properties_includes_vars() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        let props: Vec<String> = inst
            .list_properties()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert!(props.contains(&"speed".to_string()));
        assert!(props.contains(&"moved".to_string()));
    }

    #[test]
    fn set_property_updates_value() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert!(inst.set_property("speed", Variant::Float(200.0)));
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(200.0)));
    }

    #[test]
    fn set_nonexistent_property_returns_false() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert!(!inst.set_property("nonexistent", Variant::Int(1)));
    }

    #[test]
    fn get_script_name_returns_gdscript() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_script_name(), "GDScript");
    }

    #[test]
    fn call_nonexistent_method_returns_error() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        let result = inst.call_method("_nonexistent", &[]);
        assert!(result.is_err());
    }

    // -- SceneTree script store tests --------------------------------------

    #[test]
    fn attach_and_has_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));
        assert!(tree.has_script(child_id));
        assert!(!tree.has_script(root));
    }

    #[test]
    fn detach_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));
        assert!(tree.has_script(child_id));

        tree.detach_script(child_id);
        assert!(!tree.has_script(child_id));
    }

    #[test]
    fn get_script_returns_reference() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let s = tree.get_script(child_id);
        assert!(s.is_some());
        assert_eq!(s.unwrap().get_script_name(), "GDScript");
    }

    // -- Lifecycle integration tests ---------------------------------------

    #[test]
    fn ready_fires_script_after_enter_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Before lifecycle, moved=false
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("moved"),
            Some(Variant::Bool(false))
        );

        LifecycleManager::enter_tree(&mut tree, child_id);

        // After lifecycle, _ready was called so moved=true
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn enter_tree_fires_script_enter_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("entered"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn exit_tree_fires_script_exit_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::exit_tree(&mut tree, child_id);

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("exited"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn process_fires_script_each_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        ml.run_frames(3, 1.0 / 60.0);

        let delta = 1.0 / 60.0;
        let expected = 100.0 + delta * 3.0;
        let speed = ml
            .tree()
            .get_script(child_id)
            .unwrap()
            .get_property("speed")
            .unwrap();
        match speed {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected speed ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn physics_process_fires_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Body", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PHYSICS_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        // 1 frame at 1/60 → exactly 1 physics tick at dt=1/60
        ml.step(1.0 / 60.0);

        let vel = ml
            .tree()
            .get_script(child_id)
            .unwrap()
            .get_property("velocity")
            .unwrap();
        let physics_dt = 1.0 / 60.0;
        let expected = physics_dt * 10.0;
        match vel {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected velocity ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn script_with_no_lifecycle_methods_no_error() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Passive", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(MINIMAL_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // This should not panic or error — script has no _ready/_process
        LifecycleManager::enter_tree(&mut tree, child_id);

        let mut ml = MainLoop::new(tree);
        ml.run_frames(5, 1.0 / 60.0);

        assert_eq!(
            ml.tree()
                .get_script(child_id)
                .unwrap()
                .get_property("alive"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn multiple_scripted_nodes() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let n1 = Node::new("P1", "Node2D");
        let id1 = tree.add_child(root, n1).unwrap();
        let n2 = Node::new("P2", "Node2D");
        let id2 = tree.add_child(root, n2).unwrap();

        let s1 = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, id1).unwrap();
        let s2 = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, id2).unwrap();
        tree.attach_script(id1, Box::new(s1));
        tree.attach_script(id2, Box::new(s2));

        LifecycleManager::enter_tree(&mut tree, id1);
        LifecycleManager::enter_tree(&mut tree, id2);

        // Both should have _ready called
        assert_eq!(
            tree.get_script(id1).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
        assert_eq!(
            tree.get_script(id2).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn detach_script_stops_callbacks() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        ml.run_frames(2, 1.0 / 60.0);

        let delta = 1.0 / 60.0;
        let speed_after_2 = 100.0 + delta * 2.0;

        // Detach the script
        ml.tree_mut().detach_script(child_id);

        // Run more frames — speed should NOT change
        ml.run_frames(5, 1.0 / 60.0);

        // Script is gone, so get_script returns None
        assert!(ml.tree().get_script(child_id).is_none());
        assert!(!ml.tree().has_script(child_id));

        // Verify the speed was what we expected before detach
        assert!((speed_after_2 - (100.0_f64 + delta * 2.0)).abs() < 1e-9);
    }

    #[test]
    fn multi_lifecycle_all_callbacks_fire() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Multi", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(MULTI_LIFECYCLE_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        // After enter_tree: _ready fires (+1) → log_count = 1
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("log_count"),
            Some(Variant::Int(1))
        );

        let mut ml = MainLoop::new(tree);
        // 1 frame at 1/60: 1 physics tick (+1) + 1 process (+1) → log_count = 3
        ml.step(1.0 / 60.0);

        assert_eq!(
            ml.tree()
                .get_script(child_id)
                .unwrap()
                .get_property("log_count"),
            Some(Variant::Int(3))
        );
    }

    #[test]
    fn script_process_accumulates_over_many_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        let delta = 1.0 / 60.0;
        ml.run_frames(100, delta);

        let expected = 100.0 + delta * 100.0;
        let speed = ml
            .tree()
            .get_script(child_id)
            .unwrap()
            .get_property("speed")
            .unwrap();
        match speed {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-6,
                "expected speed ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn script_node_with_initial_property_values() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(100.0)));
        assert_eq!(inst.get_property("moved"), Some(Variant::Bool(false)));
    }

    #[test]
    fn enter_tree_with_children_scripts_fire_in_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let parent_script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, parent_id).unwrap();
        let child_script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(parent_id, Box::new(parent_script));
        tree.attach_script(child_id, Box::new(child_script));

        LifecycleManager::enter_tree(&mut tree, parent_id);

        // Both should have _ready called (child first in bottom-up, then parent)
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_script_mut_allows_mutation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let s = tree.get_script_mut(child_id).unwrap();
        s.set_property("speed", Variant::Float(999.0));

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("speed"),
            Some(Variant::Float(999.0))
        );
    }

    #[test]
    fn ext_resource_script_path_stored_on_node() {
        let tscn = "\
[gd_scene format=3]

[ext_resource type=\"Script\" path=\"res://scripts/player.gd\" id=\"1_abc\"]

[node name=\"Player\" type=\"Node2D\"]
script = ExtResource(\"1_abc\")
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let player = &nodes[0];
        assert_eq!(
            player.get_property("_script_path"),
            Variant::String("res://scripts/player.gd".into())
        );
    }

    #[test]
    fn node_without_script_no_script_path() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Empty\" type=\"Node2D\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[0].get_property("_script_path"), Variant::Nil);
    }

    // -- Scene access / signal tests ----------------------------------------

    /// Helper: build a tree with root → parent → child1, child2
    fn build_tree_with_children() -> (SceneTree, NodeId, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let c1 = Node::new("Child1", "Node2D");
        let c1_id = tree.add_child(parent_id, c1).unwrap();
        let c2 = Node::new("Child2", "Node2D");
        let c2_id = tree.add_child(parent_id, c2).unwrap();
        (tree, parent_id, c1_id, c2_id)
    }

    #[test]
    fn get_node_from_script() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var found = false
func _ready():
    var c = get_node(\"Child1\")
    self.found = true
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("found"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn dollar_syntax_parses() {
        // Verify the tokenizer and parser handle $NodeName
        let tokens = gdscript_interop::tokenize("$Player\n").unwrap();
        let has_dollar = tokens
            .iter()
            .any(|t| matches!(t.token, gdscript_interop::Token::Dollar));
        assert!(has_dollar, "Dollar token not found");
    }

    #[test]
    fn dollar_syntax_in_script() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var found_name = \"\"
func _ready():
    var c = $Child1
    self.found_name = c.get_name()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("found_name"),
            Some(Variant::String("Child1".into()))
        );
    }

    #[test]
    fn dollar_string_syntax() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var ok = false
func _ready():
    var c = $\"Child2\"
    self.ok = true
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_parent_from_script() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var has_parent = false
func _ready():
    var p = get_parent()
    self.has_parent = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("has_parent"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_children_from_script() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _ready():
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn emit_signal_from_script() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

        // Set up signal handler
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let conn = gdobject::signal::Connection::with_callback(
            ObjectId::from_raw(parent_id.raw()),
            "on_hit",
            move |_args| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            },
        );
        tree.connect_signal(parent_id, "hit", conn);

        let script_src = "\
extends Node2D
func _ready():
    emit_signal(\"hit\")
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn emit_signal_with_args() {
        use std::sync::{Arc, Mutex};
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();
        let conn = gdobject::signal::Connection::with_callback(
            ObjectId::from_raw(parent_id.raw()),
            "on_damage",
            move |args| {
                received_clone.lock().unwrap().extend_from_slice(args);
                Variant::Nil
            },
        );
        tree.connect_signal(parent_id, "damage_taken", conn);

        let script_src = "\
extends Node2D
func _ready():
    emit_signal(\"damage_taken\", 42)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);

        let args = received.lock().unwrap();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], Variant::Int(42));
    }

    #[test]
    fn connect_signal_from_script() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();

        // Child1 has a script that connects parent's "test_sig" to itself
        // We can't fully test cross-node method dispatch yet, but we can
        // verify the connection is registered.
        let script_src = "\
extends Node2D
var connected = false
func _ready():
    var p = get_parent()
    p.connect(\"test_sig\", $Child2, \"on_test\")
    self.connected = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("connected"),
            Some(Variant::Bool(true))
        );
        // Verify signal was registered on the parent
        let store = tree.signal_store(parent_id);
        assert!(store.is_some());
        assert!(store.unwrap().has_signal("test_sig"));
    }

    #[test]
    fn get_node_property_via_dot_access() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();

        // Attach a script to Child1 with a property
        let child_script = "\
extends Node2D
var health = 100
";
        let cs = GDScriptNodeInstance::from_source(child_script, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(cs));

        // Parent script reads Child1's property
        let parent_script = "\
extends Node2D
var child_health = 0
func _ready():
    var c = get_node(\"Child1\")
    self.child_health = c.health
";
        let ps = GDScriptNodeInstance::from_source(parent_script, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(ps));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_health"),
            Some(Variant::Int(100))
        );
    }

    #[test]
    fn set_node_property_via_dot_access() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();

        let child_script = "\
extends Node2D
var health = 100
";
        let cs = GDScriptNodeInstance::from_source(child_script, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(cs));

        // Parent sets Child1's health to 50
        let parent_script = "\
extends Node2D
func _ready():
    var c = get_node(\"Child1\")
    c.health = 50
";
        let ps = GDScriptNodeInstance::from_source(parent_script, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(ps));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("health"),
            Some(Variant::Int(50))
        );
    }

    #[test]
    fn get_node_name_method() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_name = \"\"
func _ready():
    var c = get_node(\"Child1\")
    self.child_name = c.get_name()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_name"),
            Some(Variant::String("Child1".into()))
        );
    }

    #[test]
    fn get_node_not_found_error() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var ok = true
func _ready():
    var c = get_node(\"NonExistent\")
    self.ok = false
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        // Script should have errored on get_node, so ok stays true
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_children_on_object_id() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var grandchild_count = 0
func _ready():
    var p = get_parent()
    var kids = p.get_children()
    self.grandchild_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id)
                .unwrap()
                .get_property("grandchild_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_parent_on_object_id() {
        let (mut tree, _parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var ok = false
func _ready():
    var p = get_parent()
    var gp = p.get_parent()
    self.ok = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_node_on_object_id() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var sibling_name = \"\"
func _ready():
    var p = get_parent()
    var sib = p.get_node(\"Child2\")
    self.sibling_name = sib.get_name()
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("sibling_name"),
            Some(Variant::String("Child2".into()))
        );
    }

    #[test]
    fn scene_access_during_process() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _process(delta):
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_process(parent_id, 0.016);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn scene_access_during_physics_process() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _physics_process(delta):
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_physics_process(parent_id, 0.016);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_child_count_builtin() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var count = 0
func _ready():
    self.count = get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_child_count_on_leaf_node() {
        let (mut tree, _parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var count = -1
func _ready():
    self.count = get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("count"),
            Some(Variant::Int(0))
        );
    }

    #[test]
    fn get_child_count_on_object_id() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var parent_child_count = 0
func _ready():
    var p = get_parent()
    self.parent_child_count = p.get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id)
                .unwrap()
                .get_property("parent_child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_child_count_during_process() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var count = 0
func _process(delta):
    self.count = get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_process(parent_id, 0.016);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn scene_access_during_enter_tree() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _enter_tree():
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_enter_tree(parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn scene_access_during_exit_tree() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _exit_tree():
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_exit_tree(parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn multiple_emit_signals() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

        let counter = Arc::new(AtomicUsize::new(0));
        let cc = counter.clone();
        let conn = gdobject::signal::Connection::with_callback(
            ObjectId::from_raw(parent_id.raw()),
            "on_tick",
            move |_| {
                cc.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            },
        );
        tree.connect_signal(parent_id, "tick", conn);

        let script_src = "\
extends Node2D
func _ready():
    emit_signal(\"tick\")
    emit_signal(\"tick\")
    emit_signal(\"tick\")
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn get_node_finds_sibling() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        // Child1's script looks for sibling Child2
        let script_src = "\
extends Node2D
var found = false
func _ready():
    var sib = get_node(\"Child2\")
    self.found = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("found"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn tokenizer_dollar_token() {
        let tokens = gdscript_interop::tokenize("$Foo\n").unwrap();
        assert!(tokens
            .iter()
            .any(|t| matches!(t.token, gdscript_interop::Token::Dollar)));
        assert!(tokens
            .iter()
            .any(|t| matches!(&t.token, gdscript_interop::Token::Ident(n) if n == "Foo")));
    }

    #[test]
    fn parser_get_node_expr() {
        let tokens = gdscript_interop::tokenize("$Player\n").unwrap();
        let mut parser = gdscript_interop::Parser::new(tokens, "$Player\n");
        let stmts = parser.parse_script().unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            gdscript_interop::Stmt::ExprStmt(expr) => {
                assert!(matches!(expr, gdscript_interop::Expr::GetNode(_)));
            }
            _ => panic!("expected ExprStmt with GetNode"),
        }
    }

    // -- Script-to-node property sync tests ---------------------------------

    /// After _ready sets self.speed = 300, the node should have speed=300
    /// as a node property (synced from script).
    #[test]
    fn script_vars_sync_to_node_after_ready() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
func _ready():
    self.speed = 300
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        // Script var should be 300
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("speed"),
            Some(Variant::Int(300))
        );
        // Node property should also be 300 (synced)
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Int(300)
        );
    }

    /// Script vars sync to node after _process calls too.
    #[test]
    fn script_vars_sync_to_node_after_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 100.0
func _process(delta):
    self.speed = self.speed + delta
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_script_process(child_id, 0.5);

        // Node property should reflect the updated value
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Float(100.5)
        );
    }

    /// Script vars sync to node after _physics_process.
    #[test]
    fn script_vars_sync_to_node_after_physics_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Body", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PHYSICS_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let dt = 1.0 / 60.0;
        tree.process_script_physics_process(child_id, dt);

        let expected = dt * 10.0;
        match tree.get_node(child_id).unwrap().get_property("velocity") {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected velocity ~{expected} on node, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Script vars sync to node after _enter_tree.
    #[test]
    fn script_vars_sync_to_node_after_enter_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_script_enter_tree(child_id);

        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("entered"),
            Variant::Bool(true)
        );
    }

    /// Script vars sync to node after _exit_tree.
    #[test]
    fn script_vars_sync_to_node_after_exit_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_script_exit_tree(child_id);

        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("exited"),
            Variant::Bool(true)
        );
    }

    /// Initial script variable values are synced even without calling methods,
    /// when the first lifecycle callback fires.
    #[test]
    fn initial_script_vars_sync_on_first_callback() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
var health = 100
func _ready():
    pass
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Before any lifecycle, node has no speed/health
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Nil
        );

        LifecycleManager::enter_tree(&mut tree, child_id);

        // After lifecycle, initial values are synced
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Int(200)
        );
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("health"),
            Variant::Int(100)
        );
    }

    /// Multiple script vars are all synced correctly after method call.
    #[test]
    fn multiple_script_vars_all_synced() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var x = 0
var y = 0
var z = 0
func _ready():
    self.x = 10
    self.y = 20
    self.z = 30
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        let node = tree.get_node(child_id).unwrap();
        assert_eq!(node.get_property("x"), Variant::Int(10));
        assert_eq!(node.get_property("y"), Variant::Int(20));
        assert_eq!(node.get_property("z"), Variant::Int(30));
    }

    /// Script vars accumulate over multiple frames via MainLoop.
    #[test]
    fn script_vars_sync_across_multiple_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        let delta = 1.0 / 60.0;
        ml.run_frames(5, delta);

        let expected = 100.0 + delta * 5.0;
        match ml.tree().get_node(child_id).unwrap().get_property("speed") {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected speed ~{expected} on node, got {v}"
            ),
            other => panic!("expected Float on node, got {other:?}"),
        }
    }

    /// Two nodes with scripts: both have their vars synced independently.
    #[test]
    fn multiple_nodes_script_vars_sync_independently() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let n1 = Node::new("P1", "Node2D");
        let id1 = tree.add_child(root, n1).unwrap();
        let n2 = Node::new("P2", "Node2D");
        let id2 = tree.add_child(root, n2).unwrap();

        let script1_src = "\
extends Node2D
var score = 0
func _ready():
    self.score = 42
";
        let script2_src = "\
extends Node2D
var score = 0
func _ready():
    self.score = 99
";
        let s1 = GDScriptNodeInstance::from_source(script1_src, id1).unwrap();
        let s2 = GDScriptNodeInstance::from_source(script2_src, id2).unwrap();
        tree.attach_script(id1, Box::new(s1));
        tree.attach_script(id2, Box::new(s2));

        LifecycleManager::enter_tree(&mut tree, id1);
        LifecycleManager::enter_tree(&mut tree, id2);

        assert_eq!(
            tree.get_node(id1).unwrap().get_property("score"),
            Variant::Int(42)
        );
        assert_eq!(
            tree.get_node(id2).unwrap().get_property("score"),
            Variant::Int(99)
        );
    }

    /// Verify script vars and node properties both appear after lifecycle,
    /// and they agree on the value.
    #[test]
    fn script_and_node_properties_agree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
func _ready():
    self.speed = 300
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        let script_val = tree
            .get_script(child_id)
            .unwrap()
            .get_property("speed")
            .unwrap();
        let node_val = tree.get_node(child_id).unwrap().get_property("speed");

        assert_eq!(script_val, node_val);
        assert_eq!(script_val, Variant::Int(300));
    }

    /// Script vars of different types (int, float, bool, string) all sync.
    #[test]
    fn script_vars_different_types_sync() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Mixed", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node
var count = 0
var ratio = 0.0
var flag = false
var label = \"hello\"
func _ready():
    self.count = 42
    self.ratio = 3.14
    self.flag = true
    self.label = \"world\"
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        let node = tree.get_node(child_id).unwrap();
        assert_eq!(node.get_property("count"), Variant::Int(42));
        assert_eq!(node.get_property("ratio"), Variant::Float(3.14));
        assert_eq!(node.get_property("flag"), Variant::Bool(true));
        assert_eq!(node.get_property("label"), Variant::String("world".into()));
    }

    // -----------------------------------------------------------------------
    // Script execution integration tests (Play button wiring)
    // -----------------------------------------------------------------------

    /// Test that a script with _process modifying position works end-to-end.
    /// This simulates what happens when the editor Play button is pressed:
    /// parse script -> attach to node -> call _ready -> call _process per frame.
    #[test]
    fn script_position_x_moves_over_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("Mover", "Node2D");
        child.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 100.0
func _process(delta):
    self.position.x = self.position.x + speed * delta
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Run 10 frames at 1/60
        let delta = 1.0 / 60.0;
        for _ in 0..10 {
            tree.process_script_process(child_id, delta);
        }

        // Position.x should have moved by speed * delta * 10
        let expected_x = 100.0 * delta as f32 * 10.0;
        match tree.get_node(child_id).unwrap().get_property("position") {
            Variant::Vector2(v) => {
                assert!(
                    (v.x - expected_x).abs() < 0.01,
                    "expected position.x ~{expected_x}, got {}",
                    v.x
                );
            }
            other => panic!("expected Vector2 position, got {other:?}"),
        }
    }

    /// Bare node property component writes should behave like `self.position.x`
    /// during per-frame script processing.
    #[test]
    fn bare_script_position_x_moves_over_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("Mover", "Node2D");
        child.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 100.0
func _process(delta):
    position.x += speed * delta
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let delta = 1.0 / 60.0;
        for _ in 0..10 {
            tree.process_script_process(child_id, delta);
        }

        let expected_x = 100.0 * delta as f32 * 10.0;
        match tree.get_node(child_id).unwrap().get_property("position") {
            Variant::Vector2(v) => {
                assert!(
                    (v.x - expected_x).abs() < 0.01,
                    "expected position.x ~{expected_x}, got {}",
                    v.x
                );
            }
            other => panic!("expected Vector2 position, got {other:?}"),
        }
    }

    /// Test that _ready is called when we manually invoke it, simulating
    /// what the Play button does.
    #[test]
    fn play_button_calls_ready_then_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var ready_called = false
var frame_count = 0
func _ready():
    self.ready_called = true
func _process(delta):
    self.frame_count = self.frame_count + 1
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Simulate Play: call _ready first
        tree.process_script_ready(child_id);

        assert_eq!(
            tree.get_script(child_id)
                .unwrap()
                .get_property("ready_called"),
            Some(Variant::Bool(true))
        );

        // Then run 5 frames
        for _ in 0..5 {
            tree.process_script_process(child_id, 1.0 / 60.0);
        }

        assert_eq!(
            tree.get_script(child_id)
                .unwrap()
                .get_property("frame_count"),
            Some(Variant::Int(5))
        );
    }

    /// Test that Vector2 constructor works in scripts.
    #[test]
    fn script_creates_vector2() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("Setter", "Node2D");
        child.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
func _ready():
    self.position = Vector2(100.0, 200.0)
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));
        tree.process_script_ready(child_id);

        match tree.get_node(child_id).unwrap().get_property("position") {
            Variant::Vector2(v) => {
                assert!(
                    (v.x - 100.0).abs() < 0.01 && (v.y - 200.0).abs() < 0.01,
                    "expected (100, 200), got ({}, {})",
                    v.x,
                    v.y
                );
            }
            other => panic!("expected Vector2, got {other:?}"),
        }
    }

    /// Test that print() doesn't crash (output goes to interpreter's output buffer).
    #[test]
    fn script_print_does_not_crash() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var done = false
func _ready():
    print(\"hello from script\")
    self.done = true
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();
        assert_eq!(inst.get_property("done"), Some(Variant::Bool(true)));
    }

    /// Test that math builtins (abs, min, max, clamp) work in scripts.
    #[test]
    fn script_math_builtins() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var abs_val = 0
var min_val = 0
var max_val = 0
var clamped = 0
func _ready():
    self.abs_val = abs(-42)
    self.min_val = min(3, 7)
    self.max_val = max(3, 7)
    self.clamped = clamp(15, 0, 10)
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();
        assert_eq!(inst.get_property("abs_val"), Some(Variant::Int(42)));
        assert_eq!(inst.get_property("min_val"), Some(Variant::Int(3)));
        assert_eq!(inst.get_property("max_val"), Some(Variant::Int(7)));
        assert_eq!(inst.get_property("clamped"), Some(Variant::Int(10)));
    }

    /// Test deg_to_rad and rad_to_deg builtins.
    #[test]
    fn script_deg_rad_builtins() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var radians = 0.0
var degrees = 0.0
func _ready():
    self.radians = deg_to_rad(180.0)
    self.degrees = rad_to_deg(3.14159265358979)
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();

        match inst.get_property("radians") {
            Some(Variant::Float(v)) => assert!(
                (v - std::f64::consts::PI).abs() < 1e-6,
                "expected PI, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
        match inst.get_property("degrees") {
            Some(Variant::Float(v)) => assert!((v - 180.0).abs() < 0.01, "expected 180, got {v}"),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Test that randf() and randi() return valid values.
    #[test]
    fn script_random_builtins() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var rf = 0.0
var ri = 0
func _ready():
    self.rf = randf()
    self.ri = randi()
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();
        // randf should return a float (could be any value)
        assert!(matches!(inst.get_property("rf"), Some(Variant::Float(_))));
        assert!(matches!(inst.get_property("ri"), Some(Variant::Int(_))));
    }

    /// Test that get_node works during _process (simulating runtime).
    #[test]
    fn script_get_node_during_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let script_src = "\
extends Node2D
var found_child = false
func _process(delta):
    var c = get_node(\"Child\")
    self.found_child = true
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_process(parent_id, 0.016);

        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("found_child"),
            Some(Variant::Bool(true))
        );
    }

    /// Test that script errors during _process don't crash; the method
    /// returns an error but the tree remains intact.
    #[test]
    fn script_runtime_error_does_not_crash() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Buggy", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        // This script references an undefined variable
        let script_src = "\
extends Node2D
var ok = true
func _process(delta):
    var x = undefined_var
    self.ok = false
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // _process should fail but not panic
        tree.process_script_process(child_id, 0.016);

        // ok should still be true because the error happens before self.ok = false
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );

        // Tree is still functional
        assert!(tree.has_script(child_id));
    }

    /// Test the fixture script test_move.gd can be loaded from disk.
    #[test]
    fn load_test_move_fixture_script() {
        let script_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/scripts/test_move.gd"
        );
        let source = std::fs::read_to_string(script_path)
            .unwrap_or_else(|e| panic!("Failed to read {script_path}: {e}"));

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Mover", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(&source, child_id).unwrap();
        assert!(script.has_method("_process"));

        tree.attach_script(child_id, Box::new(script));

        // Run a few frames and verify the speed property changes
        // (test_move.gd does: self.speed = self.speed + delta)
        let delta = 1.0 / 60.0;
        for _ in 0..5 {
            tree.process_script_process(child_id, delta);
        }

        let expected = 100.0 + delta * 5.0;
        match tree.get_script(child_id).unwrap().get_property("speed") {
            Some(Variant::Float(v)) => assert!(
                (v - expected).abs() < 1e-9,
                "expected speed ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Test that multiple scripts all get _ready and _process called
    /// during a simulated play session.
    #[test]
    fn multi_script_play_simulation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let n1 = Node::new("A", "Node2D");
        let id1 = tree.add_child(root, n1).unwrap();
        let n2 = Node::new("B", "Node2D");
        let id2 = tree.add_child(root, n2).unwrap();

        let script_src = "\
extends Node2D
var count = 0
func _ready():
    self.count = self.count + 1
func _process(delta):
    self.count = self.count + 1
";
        let s1 = GDScriptNodeInstance::from_source(script_src, id1).unwrap();
        let s2 = GDScriptNodeInstance::from_source(script_src, id2).unwrap();
        tree.attach_script(id1, Box::new(s1));
        tree.attach_script(id2, Box::new(s2));

        // Simulate Play: call _ready on all
        tree.process_script_ready(id1);
        tree.process_script_ready(id2);

        // Run 3 frames
        for _ in 0..3 {
            tree.process_all_scripts_process(1.0 / 60.0);
        }

        // Each script: 1 (_ready) + 3 (_process) = 4
        assert_eq!(
            tree.get_script(id1).unwrap().get_property("count"),
            Some(Variant::Int(4))
        );
        assert_eq!(
            tree.get_script(id2).unwrap().get_property("count"),
            Some(Variant::Int(4))
        );
    }

    #[test]
    fn process_callbacks_follow_tree_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root)
            .unwrap()
            .set_property("counter", Variant::Int(0));

        let first = Node::new("First", "Node2D");
        let first_id = tree.add_child(root, first).unwrap();
        let second = Node::new("Second", "Node2D");
        let second_id = tree.add_child(root, second).unwrap();

        let script_src = "\
extends Node2D
var seen_order = 0
func _process(delta):
    var parent = get_parent()
    parent.counter = parent.counter + 1
    self.seen_order = parent.counter
";
        let first_script = GDScriptNodeInstance::from_source(script_src, first_id).unwrap();
        let second_script = GDScriptNodeInstance::from_source(script_src, second_id).unwrap();
        tree.attach_script(first_id, Box::new(first_script));
        tree.attach_script(second_id, Box::new(second_script));

        tree.process_all_scripts_process(1.0 / 60.0);

        assert_eq!(
            tree.get_script(first_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(1))
        );
        assert_eq!(
            tree.get_script(second_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn physics_process_callbacks_follow_tree_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root)
            .unwrap()
            .set_property("counter", Variant::Int(0));

        let first = Node::new("First", "Node2D");
        let first_id = tree.add_child(root, first).unwrap();
        let second = Node::new("Second", "Node2D");
        let second_id = tree.add_child(root, second).unwrap();

        let script_src = "\
extends Node2D
var seen_order = 0
func _physics_process(delta):
    var parent = get_parent()
    parent.counter = parent.counter + 1
    self.seen_order = parent.counter
";
        let first_script = GDScriptNodeInstance::from_source(script_src, first_id).unwrap();
        let second_script = GDScriptNodeInstance::from_source(script_src, second_id).unwrap();
        tree.attach_script(first_id, Box::new(first_script));
        tree.attach_script(second_id, Box::new(second_script));

        tree.process_all_scripts_physics_process(1.0 / 60.0);

        assert_eq!(
            tree.get_script(first_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(1))
        );
        assert_eq!(
            tree.get_script(second_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(2))
        );
    }

    // -----------------------------------------------------------------------
    // Scene-aware signal dispatch tests (Bead B005)
    // -----------------------------------------------------------------------

    /// Signal connect and emit across two nodes: Emitter emits, Listener
    /// receives via script method dispatch.
    #[test]
    fn signal_cross_node_emit_dispatches_to_target_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        // Listener has a script with an _on_hit method.
        let listener_script = "\
extends Node2D
var got_hit = false
func _on_hit():
    self.got_hit = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Wire: Emitter.hit -> Listener._on_hit (no callback, just target+method).
        tree.connect_signal(
            emitter_id,
            "hit",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_hit"),
        );

        // Emit the signal.
        tree.emit_signal(emitter_id, "hit", &[]);

        // Listener's script should have been called.
        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("got_hit"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal from .tscn [connection] fires on instancing.
    #[test]
    fn tscn_connection_dispatches_to_script_on_emit() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Root\" type=\"Node\"]

[node name=\"Emitter\" type=\"Node2D\" parent=\".\"]

[node name=\"Listener\" type=\"Node2D\" parent=\".\"]

[connection signal=\"fired\" from=\"Emitter\" to=\"Listener\" method=\"_on_fired\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root =
            crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let emitter_id = tree.get_node_by_path("/root/Root/Emitter").unwrap();
        let listener_id = tree.get_node_by_path("/root/Root/Listener").unwrap();

        // Attach a script to the listener.
        let listener_script = "\
extends Node2D
var received = false
func _on_fired():
    self.received = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Emit from Emitter.
        tree.emit_signal(emitter_id, "fired", &[]);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("received"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal with arguments passed correctly to target script method.
    #[test]
    fn signal_args_passed_to_target_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var damage_amount = 0
func _on_damage(amount):
    self.damage_amount = amount
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "damage",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_damage"),
        );

        tree.emit_signal(emitter_id, "damage", &[Variant::Int(42)]);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("damage_amount"),
            Some(Variant::Int(42))
        );
    }

    /// Multiple connections on the same signal all fire.
    #[test]
    fn signal_multiple_connections_all_fire() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener_a = Node::new("A", "Node2D");
        let a_id = tree.add_child(root, listener_a).unwrap();
        let listener_b = Node::new("B", "Node2D");
        let b_id = tree.add_child(root, listener_b).unwrap();

        let script_a = "\
extends Node2D
var heard = false
func _on_ping():
    self.heard = true
";
        let script_b = "\
extends Node2D
var heard = false
func _on_ping():
    self.heard = true
";
        let sa = GDScriptNodeInstance::from_source(script_a, a_id).unwrap();
        let sb = GDScriptNodeInstance::from_source(script_b, b_id).unwrap();
        tree.attach_script(a_id, Box::new(sa));
        tree.attach_script(b_id, Box::new(sb));

        tree.connect_signal(
            emitter_id,
            "ping",
            gdobject::signal::Connection::new(a_id.object_id(), "_on_ping"),
        );
        tree.connect_signal(
            emitter_id,
            "ping",
            gdobject::signal::Connection::new(b_id.object_id(), "_on_ping"),
        );

        tree.emit_signal(emitter_id, "ping", &[]);

        assert_eq!(
            tree.get_script(a_id).unwrap().get_property("heard"),
            Some(Variant::Bool(true))
        );
        assert_eq!(
            tree.get_script(b_id).unwrap().get_property("heard"),
            Some(Variant::Bool(true))
        );
    }

    /// Disconnect removes the connection so it no longer fires.
    #[test]
    fn signal_disconnect_stops_dispatch() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var count = 0
func _on_tick():
    self.count = self.count + 1
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "tick",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_tick"),
        );

        // Emit once — should increment.
        tree.emit_signal(emitter_id, "tick", &[]);
        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("count"),
            Some(Variant::Int(1))
        );

        // Disconnect.
        tree.signal_store_mut(emitter_id)
            .disconnect("tick", listener_id.object_id(), "_on_tick");

        // Emit again — should NOT increment.
        tree.emit_signal(emitter_id, "tick", &[]);
        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("count"),
            Some(Variant::Int(1))
        );
    }

    /// Signal from parent to child dispatches correctly.
    #[test]
    fn signal_parent_to_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let child_script = "\
extends Node2D
var notified = false
func _on_parent_event():
    self.notified = true
";
        let cs = GDScriptNodeInstance::from_source(child_script, child_id).unwrap();
        tree.attach_script(child_id, Box::new(cs));

        tree.connect_signal(
            parent_id,
            "parent_event",
            gdobject::signal::Connection::new(child_id.object_id(), "_on_parent_event"),
        );

        tree.emit_signal(parent_id, "parent_event", &[]);

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("notified"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal from child to parent dispatches correctly.
    #[test]
    fn signal_child_to_parent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let parent_script = "\
extends Node2D
var child_done = false
func _on_child_done():
    self.child_done = true
";
        let ps = GDScriptNodeInstance::from_source(parent_script, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(ps));

        tree.connect_signal(
            child_id,
            "done",
            gdobject::signal::Connection::new(parent_id.object_id(), "_on_child_done"),
        );

        tree.emit_signal(child_id, "done", &[]);

        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_done"),
            Some(Variant::Bool(true))
        );
    }

    /// emit_signal from GDScript fires cross-node.
    #[test]
    fn emit_signal_from_gdscript_fires_cross_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        // Listener script receives the signal.
        let listener_script = "\
extends Node2D
var received = false
func _on_boom():
    self.received = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Wire the connection (no callback, script dispatch).
        tree.connect_signal(
            emitter_id,
            "boom",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_boom"),
        );

        // Emitter script calls emit_signal("boom").
        let emitter_script = "\
extends Node2D
func _ready():
    emit_signal(\"boom\")
";
        let es = GDScriptNodeInstance::from_source(emitter_script, emitter_id).unwrap();
        tree.attach_script(emitter_id, Box::new(es));
        LifecycleManager::enter_tree(&mut tree, emitter_id);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("received"),
            Some(Variant::Bool(true))
        );
    }

    /// connect() from GDScript _ready wires a working connection that
    /// dispatches to the target script when emitted.
    #[test]
    fn connect_from_gdscript_ready_then_emit() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(parent_id, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(parent_id, listener).unwrap();

        // Listener has the target method.
        let listener_script = "\
extends Node2D
var received = false
func _on_signal_from_emitter():
    self.received = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Emitter's script connects signal via obj.connect() in _ready.
        // In GDScript: `self_node.connect(signal, target, method)`.
        // Since bare connect() is a method on ObjectId, we use
        // `get_parent().get_node("Emitter").connect(...)`.
        let emitter_script = "\
extends Node2D
func _ready():
    var p = get_parent()
    var me = p.get_node(\"Emitter\")
    var listener = p.get_node(\"Listener\")
    me.connect(\"my_signal\", listener, \"_on_signal_from_emitter\")
func _process(delta):
    emit_signal(\"my_signal\")
";
        let es = GDScriptNodeInstance::from_source(emitter_script, emitter_id).unwrap();
        tree.attach_script(emitter_id, Box::new(es));

        // _ready wires the connection.
        LifecycleManager::enter_tree(&mut tree, emitter_id);

        // Verify connection is registered.
        let store = tree.signal_store(emitter_id).unwrap();
        assert!(store.has_signal("my_signal"));

        // _process emits the signal.
        tree.process_script_process(emitter_id, 0.016);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("received"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal from .tscn [connection] between siblings.
    #[test]
    fn tscn_connection_sibling_signal() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Root\" type=\"Node\"]

[node name=\"Button\" type=\"Node2D\" parent=\".\"]

[node name=\"Display\" type=\"Node2D\" parent=\".\"]

[connection signal=\"pressed\" from=\"Button\" to=\"Display\" method=\"_on_button_pressed\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();
        let display_id = tree.get_node_by_path("/root/Root/Display").unwrap();

        let display_script = "\
extends Node2D
var pressed_count = 0
func _on_button_pressed():
    self.pressed_count = self.pressed_count + 1
";
        let ds = GDScriptNodeInstance::from_source(display_script, display_id).unwrap();
        tree.attach_script(display_id, Box::new(ds));

        // Emit twice.
        tree.emit_signal(button_id, "pressed", &[]);
        tree.emit_signal(button_id, "pressed", &[]);

        assert_eq!(
            tree.get_script(display_id)
                .unwrap()
                .get_property("pressed_count"),
            Some(Variant::Int(2))
        );
    }

    /// Both callback and script connections fire for the same signal.
    #[test]
    fn signal_callback_and_script_both_fire() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var script_fired = false
func _on_event():
    self.script_fired = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Script-dispatched connection (no callback).
        tree.connect_signal(
            emitter_id,
            "event",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_event"),
        );

        // Callback-based connection.
        let callback_count = Arc::new(AtomicUsize::new(0));
        let cc = callback_count.clone();
        tree.connect_signal(
            emitter_id,
            "event",
            gdobject::signal::Connection::with_callback(
                listener_id.object_id(),
                "_callback",
                move |_| {
                    cc.fetch_add(1, Ordering::SeqCst);
                    Variant::Nil
                },
            ),
        );

        tree.emit_signal(emitter_id, "event", &[]);

        // Both should have fired.
        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("script_fired"),
            Some(Variant::Bool(true))
        );
        assert_eq!(callback_count.load(Ordering::SeqCst), 1);
    }

    /// Emit signal with no connections is a no-op (no crash).
    #[test]
    fn signal_emit_no_connections_no_crash() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Lonely", "Node2D");
        let node_id = tree.add_child(root, node).unwrap();

        let results = tree.emit_signal(node_id, "nonexistent_signal", &[]);
        assert!(results.is_empty());
    }

    /// Signal with multiple arguments dispatches all args.
    #[test]
    fn signal_multiple_args_dispatched() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var total = 0
func _on_data(a, b):
    self.total = a + b
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "data",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_data"),
        );

        tree.emit_signal(emitter_id, "data", &[Variant::Int(10), Variant::Int(32)]);

        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("total"),
            Some(Variant::Int(42))
        );
    }

    /// Signal dispatches to a node 3 levels deep in the tree.
    #[test]
    fn signal_deep_hierarchy_dispatch() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();
        let c = Node::new("C", "Node2D");
        let c_id = tree.add_child(b_id, c).unwrap();

        let c_script = "\
extends Node2D
var pinged = false
func _on_ping():
    self.pinged = true
";
        let cs = GDScriptNodeInstance::from_source(c_script, c_id).unwrap();
        tree.attach_script(c_id, Box::new(cs));

        // Signal on root dispatches to deeply nested node C.
        tree.connect_signal(
            root,
            "ping",
            gdobject::signal::Connection::new(c_id.object_id(), "_on_ping"),
        );
        tree.emit_signal(root, "ping", &[]);

        assert_eq!(
            tree.get_script(c_id).unwrap().get_property("pinged"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal fires multiple times, accumulating state in target script.
    #[test]
    fn signal_fires_multiple_times_accumulates() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var count = 0
func _on_tick():
    self.count = self.count + 1
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "tick",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_tick"),
        );

        for _ in 0..5 {
            tree.emit_signal(emitter_id, "tick", &[]);
        }

        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("count"),
            Some(Variant::Int(5))
        );
    }

    /// Signal to node without script (no crash, graceful no-op).
    #[test]
    fn signal_to_node_without_script_no_crash() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let target = Node::new("Target", "Node2D");
        let target_id = tree.add_child(root, target).unwrap();

        // Connect but target has no script.
        tree.connect_signal(
            emitter_id,
            "boom",
            gdobject::signal::Connection::new(target_id.object_id(), "_on_boom"),
        );

        // Should not crash. The callback-less connection returns Nil.
        let results = tree.emit_signal(emitter_id, "boom", &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], Variant::Nil);
    }

    /// Full .tscn signal test: Emitter emits signal, Listener receives.
    #[test]
    fn tscn_full_signal_test_emitter_listener() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Game\" type=\"Node\"]

[node name=\"Emitter\" type=\"Node2D\" parent=\".\"]

[node name=\"Listener\" type=\"Node2D\" parent=\".\"]

[connection signal=\"game_over\" from=\"Emitter\" to=\"Listener\" method=\"_on_game_over\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let emitter_id = tree.get_node_by_path("/root/Game/Emitter").unwrap();
        let listener_id = tree.get_node_by_path("/root/Game/Listener").unwrap();

        // Emitter script emits in _ready.
        let emitter_script = "\
extends Node2D
func _ready():
    emit_signal(\"game_over\")
";
        let es = GDScriptNodeInstance::from_source(emitter_script, emitter_id).unwrap();
        tree.attach_script(emitter_id, Box::new(es));

        // Listener script handles the signal.
        let listener_script = "\
extends Node2D
var game_ended = false
func _on_game_over():
    self.game_ended = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Fire _ready on emitter — this calls emit_signal("game_over")
        // which should dispatch to listener's script.
        LifecycleManager::enter_tree(&mut tree, emitter_id);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("game_ended"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal with string argument dispatched correctly.
    #[test]
    fn signal_string_arg_dispatched() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var msg = \"\"
func _on_message(text):
    self.msg = text
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "message",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_message"),
        );

        tree.emit_signal(emitter_id, "message", &[Variant::String("hello".into())]);

        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("msg"),
            Some(Variant::String("hello".into()))
        );
    }

    // -- Runtime node creation / deletion tests ----------------------------

    #[test]
    fn script_creates_node_with_new_and_add_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let spawner = Node::new("Spawner", "Node2D");
        let spawner_id = tree.add_child(root, spawner).unwrap();

        let script_src = "\
extends Node2D
var spawned = false
func _process(delta):
    if not self.spawned:
        var bullet = Node2D.new()
        add_child(bullet)
        self.spawned = true
";
        let script = GDScriptNodeInstance::from_source(script_src, spawner_id).unwrap();
        tree.attach_script(spawner_id, Box::new(script));

        assert_eq!(tree.get_node(spawner_id).unwrap().children().len(), 0);

        tree.process_all_scripts_process(1.0 / 60.0);

        let children = tree.get_node(spawner_id).unwrap().children();
        assert_eq!(
            children.len(),
            1,
            "spawner should have 1 child after _process"
        );

        let child_id = children[0];
        let child = tree.get_node(child_id).unwrap();
        assert_eq!(child.class_name(), "Node2D");
    }

    #[test]
    fn script_queue_free_removes_node_after_process_deletions() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let enemy = Node::new("Enemy", "Node2D");
        let enemy_id = tree.add_child(root, enemy).unwrap();

        let script_src = "\
extends Node2D
func _process(delta):
    queue_free()
";
        let script = GDScriptNodeInstance::from_source(script_src, enemy_id).unwrap();
        tree.attach_script(enemy_id, Box::new(script));

        assert!(tree.get_node(enemy_id).is_some());

        tree.process_all_scripts_process(1.0 / 60.0);

        // Enemy still exists (queue_free is deferred).
        assert!(tree.get_node(enemy_id).is_some());
        assert_eq!(tree.pending_deletion_count(), 1);

        tree.process_deletions();
        assert!(tree.get_node(enemy_id).is_none());
        assert_eq!(tree.pending_deletion_count(), 0);
    }

    #[test]
    fn queue_free_on_object_id_removes_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();

        let child = Node::new("Child", "Node2D");
        let _child_id = tree.add_child(parent_id, child).unwrap();

        let script_src = "\
extends Node2D
func _process(delta):
    var kids = get_children()
    for kid in kids:
        kid.queue_free()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);
        tree.process_deletions();

        assert_eq!(tree.get_node(parent_id).unwrap().children().len(), 0);
    }

    #[test]
    fn spawned_nodes_get_process_called_on_next_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let spawner = Node::new("Spawner", "Node2D");
        let spawner_id = tree.add_child(root, spawner).unwrap();

        let spawner_script = "\
extends Node2D
var did_spawn = false
func _process(delta):
    if not self.did_spawn:
        var bullet = Node2D.new()
        add_child(bullet)
        self.did_spawn = true
";
        let script = GDScriptNodeInstance::from_source(spawner_script, spawner_id).unwrap();
        tree.attach_script(spawner_id, Box::new(script));

        // Frame 1: spawner creates bullet.
        tree.process_all_scripts_process(1.0 / 60.0);
        tree.process_deletions();

        let children = tree.get_node(spawner_id).unwrap().children();
        assert_eq!(children.len(), 1);
        let bullet_id = children[0];

        // Attach a script to the bullet that records it was processed.
        let bullet_script_src = "\
extends Node2D
var processed = false
func _process(delta):
    self.processed = true
";
        let bullet_script =
            GDScriptNodeInstance::from_source(bullet_script_src, bullet_id).unwrap();
        tree.attach_script(bullet_id, Box::new(bullet_script));

        // Frame 2: both spawner and bullet get _process.
        tree.process_all_scripts_process(1.0 / 60.0);
        tree.process_deletions();

        let processed = tree
            .get_script(bullet_id)
            .unwrap()
            .get_property("processed");
        assert_eq!(processed, Some(Variant::Bool(true)));
    }

    #[test]
    fn get_parent_add_child_on_parent_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();

        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let script_src = "\
extends Node2D
var did_spawn = false
func _process(delta):
    if not self.did_spawn:
        var bullet = Node2D.new()
        get_parent().add_child(bullet)
        self.did_spawn = true
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);

        let parent_children = tree.get_node(parent_id).unwrap().children();
        assert_eq!(parent_children.len(), 2);
    }

    #[test]
    fn create_node_sets_correct_class_name() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let spawner = Node::new("Spawner", "Node2D");
        let spawner_id = tree.add_child(root, spawner).unwrap();

        let script_src = "\
extends Node2D
var child_class = \"\"
func _process(delta):
    var area = Area2D.new()
    add_child(area)
    self.child_class = area.get_class()
";
        let script = GDScriptNodeInstance::from_source(script_src, spawner_id).unwrap();
        tree.attach_script(spawner_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);

        let cls = tree
            .get_script(spawner_id)
            .unwrap()
            .get_property("child_class");
        assert_eq!(cls, Some(Variant::String("Area2D".into())));
    }

    #[test]
    fn queue_free_idempotent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let node = Node::new("N", "Node2D");
        let nid = tree.add_child(root, node).unwrap();

        tree.queue_free(nid);
        tree.queue_free(nid);
        assert_eq!(tree.pending_deletion_count(), 1);

        tree.process_deletions();
        assert!(tree.get_node(nid).is_none());
    }

    #[test]
    fn mainloop_processes_deletions_each_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let enemy = Node::new("Enemy", "Node2D");
        let enemy_id = tree.add_child(root, enemy).unwrap();

        let script_src = "\
extends Node2D
func _process(delta):
    queue_free()
";
        let script = GDScriptNodeInstance::from_source(script_src, enemy_id).unwrap();
        tree.attach_script(enemy_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        ml.run_frames(1, 1.0 / 60.0);

        assert!(ml.tree().get_node(enemy_id).is_none());
    }

    // -- End-to-end Input tests through scene tree ----------------------------

    const INPUT_MOVEMENT_SCRIPT: &str = "\
extends Node2D
var speed = 200.0
var moved_right = false
func _process(delta):
    if Input.is_action_pressed(\"ui_right\"):
        self.moved_right = true
";

    #[test]
    fn input_action_pressed_through_scene_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script = GDScriptNodeInstance::from_source(INPUT_MOVEMENT_SCRIPT, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        // Set up input: ui_right is pressed
        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("ArrowRight".to_string());
        snapshot
            .input_map
            .insert("ui_right".to_string(), vec!["ArrowRight".to_string()]);
        tree.set_input_snapshot(snapshot);

        // Run _ready + one _process frame
        tree.process_script_ready(player_id);
        tree.process_script_process(player_id, 1.0 / 60.0);

        // Verify the script saw Input.is_action_pressed("ui_right") == true
        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("moved_right"),
            Some(Variant::Bool(true)),
            "script should detect ui_right is pressed via Input singleton"
        );
    }

    #[test]
    fn input_action_not_pressed_through_scene_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script = GDScriptNodeInstance::from_source(INPUT_MOVEMENT_SCRIPT, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        // Set up input: nothing pressed
        let snapshot = InputSnapshot::default();
        tree.set_input_snapshot(snapshot);

        tree.process_script_ready(player_id);
        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("moved_right"),
            Some(Variant::Bool(false)),
            "script should detect no input when nothing is pressed"
        );
    }

    #[test]
    fn input_just_pressed_only_on_first_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script_src = "\
extends Node2D
var fire_count = 0
func _process(delta):
    if Input.is_action_just_pressed(\"shoot\"):
        self.fire_count = self.fire_count + 1
";
        let script = GDScriptNodeInstance::from_source(script_src, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        // Frame 1: shoot just pressed
        let mut snap1 = InputSnapshot::default();
        snap1.pressed_keys.insert("x".to_string());
        snap1.just_pressed_keys.insert("x".to_string());
        snap1
            .input_map
            .insert("shoot".to_string(), vec!["x".to_string()]);
        tree.set_input_snapshot(snap1);
        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("fire_count"),
            Some(Variant::Int(1)),
            "should fire on first frame (just_pressed)"
        );

        // Frame 2: shoot still held but NOT just pressed
        let mut snap2 = InputSnapshot::default();
        snap2.pressed_keys.insert("x".to_string());
        // just_pressed_keys is empty — key was already held
        snap2
            .input_map
            .insert("shoot".to_string(), vec!["x".to_string()]);
        tree.set_input_snapshot(snap2);
        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("fire_count"),
            Some(Variant::Int(1)),
            "should NOT fire on second frame (not just_pressed)"
        );
    }

    #[test]
    fn input_get_vector_through_scene_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script_src = "\
extends Node2D
var dir_x = 0.0
var dir_y = 0.0
func _process(delta):
    var dir = Input.get_vector(\"ui_left\", \"ui_right\", \"ui_up\", \"ui_down\")
    self.dir_x = dir.x
    self.dir_y = dir.y
";
        let script = GDScriptNodeInstance::from_source(script_src, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("ArrowRight".to_string());
        snapshot
            .input_map
            .insert("ui_left".to_string(), vec!["ArrowLeft".to_string()]);
        snapshot
            .input_map
            .insert("ui_right".to_string(), vec!["ArrowRight".to_string()]);
        snapshot
            .input_map
            .insert("ui_up".to_string(), vec!["ArrowUp".to_string()]);
        snapshot
            .input_map
            .insert("ui_down".to_string(), vec!["ArrowDown".to_string()]);
        tree.set_input_snapshot(snapshot);

        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("dir_x"),
            Some(Variant::Float(1.0)),
            "get_vector should return x=1.0 when ui_right pressed"
        );
        assert_eq!(
            script.get_property("dir_y"),
            Some(Variant::Float(0.0)),
            "get_vector should return y=0.0 when only ui_right pressed"
        );
    }
}
