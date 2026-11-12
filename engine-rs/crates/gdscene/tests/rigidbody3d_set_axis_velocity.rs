//! pat-s3rrv.105 — `RigidBody3D.set_axis_velocity()` takes a **Vector3**.
//!
//! # The gap
//!
//! The script arm wrote `Variant::Vector2(vec2_arg(args, 0))` into
//! `_pending_axis_velocity` unconditionally. That did not merely drop the z
//! component: the 3D server reads the property as a `Vector3`, so a 3D call
//! landed as a `Vector2` that matched nothing and the entire request vanished.
//! Nothing raised — the body simply kept moving as though the call had never
//! happened.
//!
//! # What this pins
//!
//! ⚠️ Both halves, because either alone is a plausible wrong answer:
//! the axis component must be APPLIED (catching the old silent drop), and the
//! perpendicular component must be RETAINED (catching a naive implementation
//! that overwrites the whole velocity). Godot:
//! `axis = p_axis.normalized(); vel -= axis * axis.dot(vel); vel += p_axis`
//! (`rigid_body_3d.cpp:463`).

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A free-floating body already moving along +Z, which calls
/// `set_axis_velocity` once to set its +X motion.
fn drifting_body_scene() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut body = Node::new("Drifter", "RigidBody3D");
    body.set_property("position", Variant::Vector3(Vector3::ZERO));
    // Already moving along Z — this is the component that must SURVIVE.
    body.set_property(
        "linear_velocity",
        Variant::Vector3(Vector3::new(0.0, 0.0, 10.0)),
    );
    // No gravity and no damping, so the only thing that can change the velocity
    // is the call under test.
    body.set_property("gravity_scale", Variant::Float(0.0));
    let body_id = tree.add_child(root, body).unwrap();
    let mut bs = Node::new("Shape", "CollisionShape3D");
    bs.set_property("shape", Variant::String("SphereShape3D".to_string()));
    bs.set_property("radius", Variant::Float(0.5));
    tree.add_child(body_id, bs).unwrap();

    // Called ONCE: Godot's set_axis_velocity is a one-shot request, not a
    // per-frame mode.
    let script = "\
extends RigidBody3D
var ticks = 0
var applied = false
var vel = Vector3(-9, -9, -9)
func _physics_process(delta):
    self.ticks = self.ticks + 1
    if not self.applied:
        self.applied = true
        set_axis_velocity(Vector3(5, 0, 0))
    self.vel = linear_velocity
";
    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(script, body_id).unwrap()),
    );
    (MainLoop::new(tree), body_id)
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(node)
        .expect("body node")
        .get_property(name)
}

/// **The acceptance.** The axis is applied and the perpendicular motion is kept.
#[test]
fn set_axis_velocity_applies_a_vector3_and_retains_the_other_axes() {
    let (mut ml, body) = drifting_body_scene();
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        matches!(prop(&ml, body, "ticks"), Variant::Int(n) if n > 0),
        "`_physics_process` never ran"
    );
    assert_eq!(
        prop(&ml, body, "applied"),
        Variant::Bool(true),
        "the script must have reached the set_axis_velocity call"
    );

    match prop(&ml, body, "vel") {
        Variant::Vector3(v) => {
            // Catches the old bug: a Vector2 written into a Vector3 property
            // matched nothing, so x stayed 0 and the call vanished silently.
            assert!(
                (v.x - 5.0).abs() < 0.5,
                "the requested axis velocity must reach the body: expected x ~5, \
                 got {v:?}. x = 0 means the call was written as a Vector2 and \
                 the 3D server never saw it"
            );
            // 🔑 The half a naive implementation gets wrong: Godot replaces only
            // the component ALONG the axis. Overwriting the whole velocity
            // gives (5, 0, 0) and passes the assertion above.
            assert!(
                (v.z - 10.0).abs() < 0.5,
                "motion perpendicular to the axis must be RETAINED: expected \
                 z ~10, got {v:?}"
            );
        }
        other => panic!(
            "`vel` must be a Vector3 — a Vector2 here would mean the arm is \
             still coercing, got {other:?}"
        ),
    }
}

/// ⚠️ One-shot, not a mode. The request is cleared after it is consumed, so it
/// must not keep re-pinning the axis on later ticks.
///
/// Without the clear, a body that later changed its own velocity would find the
/// old axis override reasserted every single tick, which reads as "my velocity
/// writes are being ignored".
#[test]
fn set_axis_velocity_is_consumed_once() {
    let (mut ml, body) = drifting_body_scene();
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        prop(&ml, body, "_pending_axis_velocity"),
        Variant::Nil,
        "the pending request must be cleared after it is applied; a lingering \
         value re-applies the override on every later tick"
    );
}
