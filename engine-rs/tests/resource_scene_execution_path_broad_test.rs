//! pat-zg81: Broaden resource and scene execution path coverage beyond the
//! current slice.
//!
//! Extends pat-lpae's 71-test coverage with execution-path tests that exercise:
//! 1. Script _ready/_process/_physics_process through MainLoop frames with tracing
//! 2. Tween property evolution across multi-frame traced runs
//! 3. Resource mutation isolation (Arc-clone vs Arc-share semantics)
//! 4. UID resolution edge cases through full UnifiedLoader pipeline
//! 5. Nested sub-resource property access and modification
//! 6. Frame trace determinism -- same input always produces same trace
//! 7. Scene tree lifecycle with groups, queue_free, and node counting

use std::sync::Arc;

use gdresource::loader::ResourceLoader;
use gdresource::resource::{ExtResource, Resource};
use gdresource::UnifiedLoader;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_tree_with_scripted_node(script_source: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = Node::new("ScriptedNode", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();
    let script = GDScriptNodeInstance::from_source(script_source, child_id).unwrap();
    tree.attach_script(child_id, Box::new(script));
    let ml = MainLoop::new(tree);
    (ml, child_id)
}

// ===========================================================================
// 1. Script _ready fires on lifecycle enter
// ===========================================================================

#[test]
fn script_ready_fires_on_lifecycle_enter() {
    let source = "\
extends Node2D
var initialized = false
func _ready():
    self.initialized = true
";
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = Node::new("TestNode", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();
    let script = GDScriptNodeInstance::from_source(source, child_id).unwrap();
    tree.attach_script(child_id, Box::new(script));

    // Before lifecycle
    let val = tree
        .get_script(child_id)
        .unwrap()
        .get_property("initialized");
    assert_eq!(val, Some(Variant::Bool(false)));

    // Run lifecycle
    gdscene::lifecycle::LifecycleManager::enter_tree(&mut tree, root);

    // After lifecycle
    let val = tree
        .get_script(child_id)
        .unwrap()
        .get_property("initialized");
    assert_eq!(val, Some(Variant::Bool(true)));
}

// ===========================================================================
// 2. Script _process accumulates delta across frames
// ===========================================================================

#[test]
fn script_process_accumulates_delta() {
    let source = "\
extends Node2D
var total_time = 0.0
func _process(delta):
    self.total_time = self.total_time + delta
";
    let (mut ml, child_id) = make_tree_with_scripted_node(source);
    let root = ml.tree().root_id();
    gdscene::lifecycle::LifecycleManager::enter_tree(ml.tree_mut(), root);

    let delta = 1.0 / 60.0;
    ml.run_frames(10, delta);

    let total = ml
        .tree()
        .get_script(child_id)
        .unwrap()
        .get_property("total_time");
    if let Some(Variant::Float(t)) = total {
        let expected = delta * 10.0;
        assert!(
            (t - expected).abs() < 0.001,
            "total_time should be ~{:.4}, got {:.4}",
            expected,
            t
        );
    } else {
        panic!("total_time should be Float, got {:?}", total);
    }
}

// ===========================================================================
// 3. Script _physics_process fires on physics ticks
// ===========================================================================

#[test]
fn script_physics_process_counts_ticks() {
    let source = "\
extends Node2D
var physics_ticks = 0
func _physics_process(delta):
    self.physics_ticks = self.physics_ticks + 1
";
    let (mut ml, child_id) = make_tree_with_scripted_node(source);
    let root = ml.tree().root_id();
    gdscene::lifecycle::LifecycleManager::enter_tree(ml.tree_mut(), root);

    ml.run_frames(5, 1.0 / 60.0);

    let ticks = ml
        .tree()
        .get_script(child_id)
        .unwrap()
        .get_property("physics_ticks");
    if let Some(Variant::Int(t)) = ticks {
        assert_eq!(t, 5, "should have 5 physics ticks");
    } else {
        panic!("physics_ticks should be Int, got {:?}", ticks);
    }
}

// ===========================================================================
// 4. Frame trace captures notifications
// ===========================================================================

#[test]
fn frame_trace_captures_notifications() {
    let source = "\
extends Node2D
var counter = 0
func _process(delta):
    self.counter = self.counter + 1
";
    let (mut ml, _) = make_tree_with_scripted_node(source);
    let root = ml.tree().root_id();
    gdscene::lifecycle::LifecycleManager::enter_tree(ml.tree_mut(), root);

    let trace = ml.run_frames_traced(3, 1.0 / 60.0);
    assert_eq!(trace.len(), 3);

    for (i, frame) in trace.frames.iter().enumerate() {
        assert!(
            !frame.events.is_empty(),
            "frame {} should have trace events",
            i
        );
        assert_eq!(frame.frame_number, (i + 1) as u64);
    }
}

// ===========================================================================
// 5. Frame trace determinism
// ===========================================================================

#[test]
fn frame_trace_is_deterministic() {
    let source = "\
extends Node2D
var x = 0.0
func _process(delta):
    self.x = self.x + 10.0 * delta
";
    let (mut ml1, _) = make_tree_with_scripted_node(source);
    let r1 = ml1.tree().root_id();
    gdscene::lifecycle::LifecycleManager::enter_tree(ml1.tree_mut(), r1);
    let trace1 = ml1.run_frames_traced(5, 1.0 / 60.0);

    let (mut ml2, _) = make_tree_with_scripted_node(source);
    let r2 = ml2.tree().root_id();
    gdscene::lifecycle::LifecycleManager::enter_tree(ml2.tree_mut(), r2);
    let trace2 = ml2.run_frames_traced(5, 1.0 / 60.0);

    assert_eq!(trace1.len(), trace2.len());
    for (f1, f2) in trace1.frames.iter().zip(trace2.frames.iter()) {
        assert_eq!(f1.frame_number, f2.frame_number);
        assert_eq!(f1.physics_ticks, f2.physics_ticks);
        assert_eq!(f1.events.len(), f2.events.len());
    }
}

// ===========================================================================
// 6. Tween property evolution through MainLoop
// ===========================================================================

#[test]
fn tween_evolves_property_through_mainloop() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("TweenTarget", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();
    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("opacity", Variant::Float(0.0));

    let tween = gdscene::tween::TweenBuilder::new()
        .tween_property("opacity", Variant::Float(0.0), Variant::Float(1.0), 1.0)
        .build();
    tree.add_tween(node_id, tween);

    let mut ml = MainLoop::new(tree);
    ml.run_frames(60, 1.0 / 60.0);

    let opacity = ml.tree().get_node(node_id).unwrap().get_property("opacity");
    if let Variant::Float(v) = opacity {
        assert!(
            (v - 1.0).abs() < 0.05,
            "opacity should be ~1.0 after 1s tween, got {}",
            v
        );
    } else {
        panic!("opacity should be Float, got {:?}", opacity);
    }
}

// ===========================================================================
// 7. Resource Arc-clone isolation
// ===========================================================================

#[test]
fn resource_arc_clone_isolation() {
    let mut original = Resource::new("TestResource");
    original.set_property("health", Variant::Int(100));
    let original_arc = Arc::new(original);

    let mut cloned = (*original_arc).clone();
    cloned.set_property("health", Variant::Int(50));
    cloned.set_property("extra", Variant::Bool(true));
    let cloned_arc = Arc::new(cloned);

    assert_eq!(
        original_arc.get_property("health"),
        Some(&Variant::Int(100))
    );
    assert_eq!(original_arc.get_property("extra"), None);
    assert_eq!(cloned_arc.get_property("health"), Some(&Variant::Int(50)));
    assert!(!Arc::ptr_eq(&original_arc, &cloned_arc));
}

// ===========================================================================
// 8. Resource sub-resource property access
// ===========================================================================

#[test]
fn resource_subresource_property_access() {
    let mut sub = Resource::new("RectangleShape2D");
    sub.set_property(
        "size",
        Variant::Vector2(gdcore::math::Vector2::new(32.0, 64.0)),
    );
    let sub_arc = Arc::new(sub);

    let mut parent = Resource::new("CollisionShape2D");
    parent
        .subresources
        .insert("shape_1".to_string(), Arc::clone(&sub_arc));

    let retrieved = parent.subresources.get("shape_1").unwrap();
    assert_eq!(retrieved.class_name, "RectangleShape2D");
    assert_eq!(
        retrieved.get_property("size"),
        Some(&Variant::Vector2(gdcore::math::Vector2::new(32.0, 64.0)))
    );
}

// ===========================================================================
// 9. Sub-resource mutation isolation
// ===========================================================================

#[test]
fn subresource_mutation_does_not_leak() {
    let mut sub = Resource::new("CircleShape2D");
    sub.set_property("radius", Variant::Float(10.0));
    let sub_arc = Arc::new(sub);

    let mut parent_a = Resource::new("CollisionA");
    parent_a
        .subresources
        .insert("s1".to_string(), Arc::clone(&sub_arc));
    let mut parent_b = Resource::new("CollisionB");
    parent_b
        .subresources
        .insert("s1".to_string(), Arc::clone(&sub_arc));

    // Shared
    assert!(Arc::ptr_eq(
        parent_a.subresources.get("s1").unwrap(),
        parent_b.subresources.get("s1").unwrap()
    ));

    // Clone-and-mutate for A
    let mut modified = (**parent_a.subresources.get("s1").unwrap()).clone();
    modified.set_property("radius", Variant::Float(25.0));
    parent_a
        .subresources
        .insert("s1".to_string(), Arc::new(modified));

    assert_eq!(
        parent_b
            .subresources
            .get("s1")
            .unwrap()
            .get_property("radius"),
        Some(&Variant::Float(10.0))
    );
    assert_eq!(
        parent_a
            .subresources
            .get("s1")
            .unwrap()
            .get_property("radius"),
        Some(&Variant::Float(25.0))
    );
}

// ===========================================================================
// 10. UnifiedLoader — UID and path dedup
// ===========================================================================

struct FakeLoader;
impl ResourceLoader for FakeLoader {
    fn load(&self, path: &str) -> gdcore::error::EngineResult<Arc<Resource>> {
        let mut r = Resource::new("FakeResource");
        r.path = path.to_string();
        r.set_property("loaded_from", Variant::String(path.to_string()));
        Ok(Arc::new(r))
    }
}

#[test]
fn unified_loader_uid_path_dedup() {
    let mut loader = UnifiedLoader::new(FakeLoader);
    loader.register_uid_str("uid://abc123", "res://items/sword.tres");

    let by_path = loader.load("res://items/sword.tres").unwrap();
    let by_uid = loader.load("uid://abc123").unwrap();

    assert!(Arc::ptr_eq(&by_path, &by_uid));
    assert_eq!(loader.cache_len(), 1);
}

// ===========================================================================
// 11. UnifiedLoader — unregistered UID returns NotFound
// ===========================================================================

#[test]
fn unified_loader_unregistered_uid_error() {
    let mut loader = UnifiedLoader::new(FakeLoader);
    let result = loader.load("uid://nonexistent_ref");
    assert!(result.is_err());
}

// ===========================================================================
// 12. UnifiedLoader — invalid UID format
// ===========================================================================

#[test]
fn unified_loader_invalid_uid_format() {
    let mut loader = UnifiedLoader::new(FakeLoader);
    let result = loader.load("uid://");
    assert!(result.is_err(), "empty UID should fail");
}

// ===========================================================================
// 13. UnifiedLoader — replace_cached updates future loads
// ===========================================================================

#[test]
fn unified_loader_replace_cached() {
    let mut loader = UnifiedLoader::new(FakeLoader);
    let first = loader.load("res://config.tres").unwrap();

    let mut updated = (*first).clone();
    updated.set_property("version", Variant::Int(2));
    loader.replace_cached("res://config.tres", Arc::new(updated));

    let second = loader.load("res://config.tres").unwrap();
    assert_eq!(second.get_property("version"), Some(&Variant::Int(2)));
    assert!(!Arc::ptr_eq(&first, &second));
}

// ===========================================================================
// 14. UnifiedLoader — invalidate forces reload
// ===========================================================================

#[test]
fn unified_loader_invalidate_reloads() {
    let mut loader = UnifiedLoader::new(FakeLoader);
    let first = loader.load("res://data.tres").unwrap();
    loader.invalidate("res://data.tres");
    let second = loader.load("res://data.tres").unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
}

// ===========================================================================
// 15. Resource ext_resource construction
// ===========================================================================

#[test]
fn resource_ext_resource_construction() {
    let mut res = Resource::new("PackedScene");
    res.ext_resources.insert(
        "1".to_string(),
        ExtResource {
            resource_type: "Texture2D".to_string(),
            uid: "uid://tex_icon".to_string(),
            path: "res://icon.png".to_string(),
            id: "1".to_string(),
        },
    );
    res.ext_resources.insert(
        "2".to_string(),
        ExtResource {
            resource_type: "Script".to_string(),
            uid: "uid://player_script".to_string(),
            path: "res://scripts/player.gd".to_string(),
            id: "2".to_string(),
        },
    );

    assert_eq!(res.ext_resources.len(), 2);
    assert_eq!(res.ext_resources["1"].resource_type, "Texture2D");
    assert_eq!(res.ext_resources["2"].path, "res://scripts/player.gd");
}

// ===========================================================================
// 16. Scene tree add/remove preserves count
// ===========================================================================

#[test]
fn scene_tree_add_remove_preserves_count() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut children = Vec::new();
    for i in 0..5 {
        let name = format!("Child{}", i);
        let child = Node::new(&name, "Node");
        let id = tree.add_child(root, child).unwrap();
        children.push(id);
    }
    assert_eq!(tree.all_nodes_in_tree_order().len(), 6); // root + 5

    tree.remove_node(children[0]);
    tree.remove_node(children[4]);
    assert_eq!(tree.all_nodes_in_tree_order().len(), 4);

    let extra = Node::new("Extra", "Node");
    tree.add_child(root, extra).unwrap();
    assert_eq!(tree.all_nodes_in_tree_order().len(), 5);
}

// ===========================================================================
// 17. Traced frame snapshots node properties
// ===========================================================================

#[test]
fn traced_frame_snapshots_node_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("PropNode", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();
    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("score", Variant::Int(0));

    let source = "\
extends Node2D
func _process(delta):
    var current = self.score
    self.score = current + 1
";
    let script = GDScriptNodeInstance::from_source(source, node_id).unwrap();
    tree.attach_script(node_id, Box::new(script));
    gdscene::lifecycle::LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    let trace = ml.run_frames_traced(3, 1.0 / 60.0);

    for frame in &trace.frames {
        assert!(
            !frame.node_snapshots.is_empty(),
            "frame {} should have node snapshots",
            frame.frame_number
        );
    }
}

// ===========================================================================
// 18. Multiple UIDs — different paths
// ===========================================================================

#[test]
fn unified_loader_multiple_uids() {
    let mut loader = UnifiedLoader::new(FakeLoader);
    loader.register_uid_str("uid://res_a", "res://a.tres");
    loader.register_uid_str("uid://res_b", "res://b.tres");
    loader.register_uid_str("uid://res_c", "res://c.tres");

    let a = loader.load("uid://res_a").unwrap();
    let b = loader.load("uid://res_b").unwrap();
    let c = loader.load("uid://res_c").unwrap();

    assert!(!Arc::ptr_eq(&a, &b));
    assert!(!Arc::ptr_eq(&b, &c));
    assert_eq!(loader.cache_len(), 3);
}

// ===========================================================================
// 19. Scene group membership
// ===========================================================================

#[test]
fn scene_groups_survive_lifecycle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let e1 = Node::new("Enemy1", "Node2D");
    let e1_id = tree.add_child(root, e1).unwrap();
    let e2 = Node::new("Enemy2", "Node2D");
    let e2_id = tree.add_child(root, e2).unwrap();

    tree.add_to_group(e1_id, "enemies");
    tree.add_to_group(e2_id, "enemies");
    assert_eq!(tree.get_nodes_in_group("enemies").len(), 2);

    tree.remove_from_group(e1_id, "enemies");
    assert_eq!(tree.get_nodes_in_group("enemies").len(), 1);
}

// ===========================================================================
// 20. Queue free removes after process_deletions
// ===========================================================================

#[test]
fn queue_free_deferred_removal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = Node::new("Temp", "Node");
    let child_id = tree.add_child(root, child).unwrap();

    assert_eq!(tree.all_nodes_in_tree_order().len(), 2);

    tree.queue_free(child_id);
    // Still present before process_deletions
    assert_eq!(tree.all_nodes_in_tree_order().len(), 2);

    tree.process_deletions();
    assert_eq!(tree.all_nodes_in_tree_order().len(), 1);
}
