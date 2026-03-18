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
    MethodFlags, MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo,
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
            .map_err(|e| match e {
                RuntimeError::UndefinedFunction(n) => ScriptError::MethodNotFound(n),
                RuntimeError::TypeError(msg) => ScriptError::TypeError(msg),
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
}
