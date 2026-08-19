//! pat-s3rrv.105 — `_integrate_forces(state)` sees CONTACTS on a RigidBody3D.
//!
//! `DirectBodyState3D` already had a `contacts` field and the interpreter's 3D
//! state table already implemented `get_contact_count` /
//! `get_contact_local_normal` / `get_contact_local_position`. But `build_state_3d`
//! filled the struct with `..Default::default()`, so the field was silently
//! empty and every one of those getters answered zero — a whole implemented
//! surface reachable only through a value nothing populated.
//!
//! 🔑 This is also the first test that can observe the contact NORMAL, and so
//! the first that pins its SIGN. `ContactPoint3D.normal` runs from the body
//! toward the other body; Godot's `get_contact_local_normal` points away from
//! the surface — up, for a body on a floor. The publisher negates it, and
//! nothing until now could tell whether it had.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A monitoring `RigidBody3D` resting on a floor, with `custom_integrator` on so
/// `_integrate_forces` is dispatched.
///
/// ⚠️ It STARTS in contact rather than falling into it. `custom_integrator`
/// tells the engine the script owns integration, so a script that only reads
/// state may never apply gravity — a fixture that relied on falling could
/// report zero contacts for a reason that has nothing to do with what is being
/// tested.
fn resting_body_scene(monitor: bool) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    // Box centred at -1 with half-height 1 => top face at y = 0.
    let mut floor = Node::new("Floor", "StaticBody3D");
    floor.set_property("position", Variant::Vector3(Vector3::new(0.0, -1.0, 0.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("FloorShape", "CollisionShape3D");
    fs.set_property("shape", Variant::String("BoxShape3D".to_string()));
    fs.set_property("size", Variant::Vector3(Vector3::new(100.0, 2.0, 100.0)));
    tree.add_child(floor_id, fs).unwrap();

    // Sphere radius 0.5 at y = 0.45 => bottom at -0.05, overlapping the floor.
    let mut body = Node::new("Rester", "RigidBody3D");
    body.set_property("position", Variant::Vector3(Vector3::new(0.0, 0.45, 0.0)));
    body.set_property("custom_integrator", Variant::Bool(true));
    body.set_property("contact_monitor", Variant::Bool(monitor));
    body.set_property(
        "max_contacts_reported",
        Variant::Int(if monitor { 8 } else { 0 }),
    );
    let body_id = tree.add_child(root, body).unwrap();
    let mut bs = Node::new("Shape", "CollisionShape3D");
    bs.set_property("shape", Variant::String("SphereShape3D".to_string()));
    bs.set_property("radius", Variant::Float(0.5));
    tree.add_child(body_id, bs).unwrap();

    let script = "\
extends RigidBody3D
var ticks = 0
var count = -1
var normal = Vector3(-9, -9, -9)
func _integrate_forces(state):
    self.ticks = self.ticks + 1
    self.count = state.get_contact_count()
    if self.count > 0:
        self.normal = state.get_contact_local_normal(0)
";
    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(script, body_id).unwrap()),
    );
    (MainLoop::new(tree), body_id, floor_id)
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(node)
        .expect("body node")
        .get_property(name)
}

/// **The acceptance.** The state carries contacts, and the normal points away
/// from the floor.
#[test]
fn integrate_forces_3d_sees_contacts_and_their_normal() {
    let (mut ml, body, _floor) = resting_body_scene(true);
    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }

    // Asserted first: `_integrate_forces` is gated on `custom_integrator`, so a
    // zero count below could otherwise mean the callback never ran at all.
    assert!(
        matches!(prop(&ml, body, "ticks"), Variant::Int(n) if n > 0),
        "`_integrate_forces` never ran — the callback is gated on \
         custom_integrator, so nothing below would be evidence about contacts"
    );
    match prop(&ml, body, "count") {
        Variant::Int(n) => assert!(
            n >= 1,
            "a body resting on the floor must report at least one contact to \
             _integrate_forces, got {n}; 0 means DirectBodyState3D.contacts is \
             still falling through to Default"
        ),
        other => panic!("`count` must be an int, got {other:?}"),
    }

    // 🔑 The sign. Godot's get_contact_local_normal points AWAY from the surface
    // the body rests on, so a body on a floor sees +Y. The solver's own normal
    // runs the other way; publishing it unflipped would report every floor
    // contact as pointing DOWN and silently invert the standard floor test.
    match prop(&ml, body, "normal") {
        Variant::Vector3(v) => {
            assert!(
                v.y > 0.5,
                "the contact normal must point UP away from the floor, got \
                 {v:?} — a negative y means the publisher's flip was lost"
            );
            assert!(
                v.x.abs() < 0.5 && v.z.abs() < 0.5,
                "and it must be essentially vertical for a flat floor, got {v:?}"
            );
        }
        other => panic!("`normal` must be a Vector3, got {other:?}"),
    }
}

/// ⚠️ The control: without `contact_monitor` the state reports nothing, even
/// though the body is in exactly the same place touching exactly the same floor.
#[test]
fn integrate_forces_3d_reports_no_contacts_without_the_opt_in() {
    let (mut ml, body, _floor) = resting_body_scene(false);
    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        matches!(prop(&ml, body, "ticks"), Variant::Int(n) if n > 0),
        "`_integrate_forces` never ran"
    );
    assert_eq!(
        prop(&ml, body, "count"),
        Variant::Int(0),
        "contact_monitor is false, so Godot reports no contacts even for a body \
         demonstrably in contact — this is the gate, not a missing read"
    );
}
