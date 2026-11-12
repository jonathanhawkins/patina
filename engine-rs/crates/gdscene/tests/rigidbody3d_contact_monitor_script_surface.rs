//! pat-s3rrv.105 — `RigidBody3D.get_contact_count()` / `get_colliding_bodies()`.
//!
//! 🔑 **The 3D gap was a missing publisher, not missing script arms.** The
//! `KinematicCollision`/contact arms in `scripting.rs` read the node property
//! `_body_contacts` and hand each field straight back, so they are
//! dimension-agnostic. The 2D server published that array; the 3D one published
//! only `_overlapping_bodies`, so both methods answered 0 for every
//! `RigidBody3D` however hard it was pressed against something — while the
//! solver had the contacts in hand the whole time.
//!
//! This is the same shape as the 2D pass (msg 81582): publishing onto the node
//! made the existing arms serve 3D with no new plumbing and no `unsafe`.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A monitoring `RigidBody3D` dropped onto a wide static floor.
fn falling_body_scene(monitor: bool) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut floor = Node::new("Floor", "StaticBody3D");
    floor.set_property("position", Variant::Vector3(Vector3::new(0.0, -2.0, 0.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("FloorShape", "CollisionShape3D");
    fs.set_property("shape", Variant::String("BoxShape3D".to_string()));
    fs.set_property("size", Variant::Vector3(Vector3::new(100.0, 2.0, 100.0)));
    tree.add_child(floor_id, fs).unwrap();

    let mut body = Node::new("Faller", "RigidBody3D");
    body.set_property("position", Variant::Vector3(Vector3::new(0.0, 1.0, 0.0)));
    // Godot's gate: BOTH must be set before any contact is reported.
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
var bodies = -1
var first = null
func _physics_process(delta):
    self.ticks = self.ticks + 1
    self.count = get_contact_count()
    var cb = get_colliding_bodies()
    self.bodies = cb.size()
    if cb.size() > 0:
        self.first = cb[0]
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

fn int_prop(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match prop(ml, node, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` must be an int, got {other:?}"),
    }
}

/// **The acceptance.** A monitoring 3D body reports its contacts and collider.
#[test]
fn a_monitoring_rigidbody3d_reports_contacts_and_colliding_bodies() {
    let (mut ml, body, floor) = falling_body_scene(true);
    for _ in 0..180 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        int_prop(&ml, body, "ticks") > 0,
        "`_physics_process` never ran, so every value below is an initialiser"
    );
    // -1 is the initialiser: this separates "reported zero" from "the call raised".
    assert!(
        int_prop(&ml, body, "count") >= 0,
        "get_contact_count() must have been called successfully"
    );
    assert!(
        int_prop(&ml, body, "count") >= 1,
        "a body resting on the floor must report at least one contact; 0 means \
         the 3D bridge is still publishing nothing for the arms to read"
    );
    // 🔑 Contacts are POINTS, colliding bodies are BODIES — the arm deduplicates
    // by collider, so one floor stays one entry however many points touch it.
    assert_eq!(
        int_prop(&ml, body, "bodies"),
        1,
        "exactly one BODY is being touched (the floor); got {} against a contact \
         count of {}",
        int_prop(&ml, body, "bodies"),
        int_prop(&ml, body, "count")
    );
    // 🔑 The entry is the floor NODE. The solver records only a `BodyId3D`, so a
    // Nil here means the id -> node mapping failed while everything else passed.
    assert_eq!(
        prop(&ml, body, "first"),
        Variant::ObjectId(floor.object_id()),
        "get_colliding_bodies() must hand back the Floor node itself"
    );
}

/// ⚠️ The control: without the opt-in, Godot reports nothing — and so must
/// Patina, even though the body is visibly resting on the floor.
///
/// Without this, a publisher that ignored `contact_monitor` entirely would
/// satisfy the acceptance above unchanged.
#[test]
fn a_rigidbody3d_that_did_not_opt_in_reports_nothing() {
    let (mut ml, body, _floor) = falling_body_scene(false);
    for _ in 0..180 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        int_prop(&ml, body, "ticks") > 0,
        "`_physics_process` never ran"
    );
    assert_eq!(
        int_prop(&ml, body, "count"),
        0,
        "contact_monitor is false, so Godot reports 0 contacts even while the \
         body rests on the floor"
    );
    assert_eq!(int_prop(&ml, body, "bodies"), 0, "and no colliding bodies");
    assert_eq!(
        prop(&ml, body, "first"),
        Variant::Nil,
        "the list must be empty rather than absent"
    );
}
