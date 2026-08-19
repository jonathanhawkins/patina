//! pat-s3rrv.106: RayCast3D updates INSIDE the physics tick, before scripts.
//!
//! `process_queries_3d` drives RayCast3D / ShapeCast3D / SpringArm3D, and its
//! only caller was the per-FRAME idle block — after the fixed-step physics loop
//! and after the script phase. So a `_physics_process` handler read
//! `is_colliding()` from the PREVIOUS frame, and false for the whole first one.
//!
//! Godot updates these under `NOTIFICATION_INTERNAL_PHYSICS_PROCESS`
//! (`scene/3d/ray_cast_3d.cpp`), i.e. before any script runs. The 2D twins were
//! moved into the tick for exactly this reason by pat-0q2kx.32; this is the 3D
//! half of the same fix.
//!
//! 🔑 The assertion that matters is FIRST-FRAME. A test that steps several times
//! and then checks `is_colliding()` passes under the old idle placement too —
//! the cast is correct by frame 2. Only "what did the script see on frame 1"
//! distinguishes the ordering.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

/// `Root -> {Floor (StaticBody3D box at the origin), Ray (RayCast3D above it,
/// pointing down, with a script that records what it saw on the FIRST tick)}`.
fn scene() -> (MainLoop, NodeId) {
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let floor = tree
        .add_child(root, Node::new("Floor", "StaticBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, floor, Vector3::ZERO);
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    shape.set_property("size", Variant::Vector3(Vector3::new(4.0, 2.0, 4.0)));
    tree.add_child(floor, shape).unwrap();

    let ray = tree.add_child(root, Node::new("Ray", "RayCast3D")).unwrap();
    node3d::set_position(&mut tree, ray, Vector3::new(0.0, 3.0, 0.0));
    // Straight down, far enough to reach the box's top face at y = 1.
    if let Some(n) = tree.get_node_mut(ray) {
        n.set_property("enabled", Variant::Bool(true));
        n.set_property(
            "target_position",
            Variant::Vector3(Vector3::new(0.0, -5.0, 0.0)),
        );
    }
    tree.attach_script(
        ray,
        Box::new(
            GDScriptNodeInstance::from_source(
                "\
extends RayCast3D
var ticks = 0
var saw_on_first_tick = false

func _physics_process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        self.saw_on_first_tick = is_colliding()
",
                ray,
            )
            .unwrap(),
        ),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, ray)
}

/// **The acceptance.** `is_colliding()` is already true on the first tick.
#[test]
fn a_raycast3d_is_updated_before_the_first_physics_process() {
    let (mut ml, ray) = scene();

    ml.step(1.0 / 60.0);

    // CONTROL: the handler ran at all. Without this a scene that never reached
    // `_physics_process` would report `saw_on_first_tick = false` and read as
    // an ordering bug rather than a fixture that never stepped.
    assert_eq!(
        script_var(&ml, ray, "ticks"),
        Variant::Int(1),
        "`_physics_process` must have run exactly once for a single 1/60 step"
    );

    assert_eq!(
        script_var(&ml, ray, "saw_on_first_tick"),
        Variant::Bool(true),
        "the RayCast3D must already be updated when the first `_physics_process` \
         runs. False here is the bug this bead is about: the cast used to be \
         driven from the per-frame idle block AFTER the script phase, so a \
         script saw the previous frame's result — and nothing at all on frame 1"
    );
}
