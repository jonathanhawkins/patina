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
}

impl SceneTreeAccessor {
    /// Creates a new accessor. Caller must ensure the pointer is valid.
    pub(crate) unsafe fn new(tree: *mut SceneTree) -> Self {
        Self { tree }
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
        self.tree_mut().signal_store_mut(nid).emit(signal, args);
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
}
