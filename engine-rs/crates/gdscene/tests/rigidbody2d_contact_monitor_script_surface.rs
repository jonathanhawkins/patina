//! pat-s3rrv.100 — `RigidBody2D.get_contact_count()` and
//! `get_colliding_bodies()` are reachable from GDScript.
//!
//! The contact monitor itself already worked: `PhysicsServer::publish_body_contacts`
//! leaves a `_body_contacts` list on every monitoring body each tick, and
//! `body_entered`/`body_exited` fire from it. But the two methods Godot's own
//! docs point you at — *"Use [method get_contact_count] to retrieve the count or
//! [method get_colliding_bodies] to retrieve bodies that have been collided
//! with"* — had no script arm at all, so the data was published and unreadable.
//!
//! ⚠️ Both are GATED. Godot returns 0 / empty unless the body opted in with
//! `contact_monitor = true` AND `max_contacts_reported > 0`, and the publisher
//! applies that gate. The default-off control below is therefore asserting
//! parity, not absence of a feature.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const FLOOR_Y: f32 = 300.0;
const BOX: f32 = 10.0;

/// A box that falls onto a wide static floor, optionally monitoring contacts.
fn resting_box_scene(monitor: bool, script: &str) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, FLOOR_Y)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("FloorShape", "CollisionShape2D");
    fs.set_property("size", Variant::Vector2(Vector2::new(400.0, BOX * 2.0)));
    tree.add_child(floor_id, fs).unwrap();

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    // The gate, authored exactly as a .tscn would.
    body.set_property("contact_monitor", Variant::Bool(monitor));
    body.set_property(
        "max_contacts_reported",
        Variant::Int(if monitor { 8 } else { 0 }),
    );
    let body_id = tree.add_child(root, body).unwrap();
    let mut bs = Node::new("Shape", "CollisionShape2D");
    bs.set_property("size", Variant::Vector2(Vector2::new(BOX * 2.0, BOX * 2.0)));
    tree.add_child(body_id, bs).unwrap();
    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(script, body_id).unwrap()),
    );

    (MainLoop::new(tree), body_id, floor_id)
}

/// Reports both methods every tick, so the values read are the settled ones.
const REPORTER: &str = "\
extends RigidBody2D
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

/// **The acceptance.** A monitoring body reports its contacts and its collider.
#[test]
fn a_monitoring_body_reports_contacts_and_colliding_bodies() {
    let (mut ml, body, floor) = resting_box_scene(true, REPORTER);
    for _ in 0..180 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        int_prop(&ml, body, "ticks") > 0,
        "`_physics_process` never ran, so every value below is an initialiser"
    );
    assert!(
        int_prop(&ml, body, "count") >= 1,
        "a box resting on the floor must report at least one contact; 0 means \
         the arm is reading a `_body_contacts` list nothing published, and -1 \
         means the call raised"
    );
    // 🔑 The dedup guard. `get_contact_count` counts CONTACT POINTS and
    // `get_colliding_bodies` counts BODIES — Godot is explicit that a flat rest
    // yields several contacts against the one floor. Whenever the solver
    // reports more than one contact here, an implementation that forgot to
    // deduplicate returns the floor twice and this fails.
    assert_eq!(
        int_prop(&ml, body, "bodies"),
        1,
        "exactly one BODY is being touched (the floor), however many contact \
         points it has; got {} against a contact count of {}",
        int_prop(&ml, body, "bodies"),
        int_prop(&ml, body, "count")
    );
    assert!(
        int_prop(&ml, body, "bodies") <= int_prop(&ml, body, "count"),
        "bodies can never exceed contacts — that ordering is what makes the two \
         methods different, and inverting it means one of them is reading the \
         other's list"
    );
    // The entry must be the floor NODE, not an internal body id.
    assert_eq!(
        prop(&ml, body, "first"),
        Variant::ObjectId(floor.object_id()),
        "get_colliding_bodies() must hand back the Floor node itself, the way a \
         script expects to call methods on it"
    );
}

/// ⚠️ The control: without the opt-in Godot reports nothing, and so must Patina.
///
/// Without this, an implementation that ignored the gate entirely — reporting
/// contacts for every body in the scene — would pass the acceptance above
/// unchanged. It is also the assertion that proves the numbers there came from
/// the monitor rather than from something that always answers.
#[test]
fn a_body_that_did_not_opt_in_reports_nothing() {
    let (mut ml, body, _floor) = resting_box_scene(false, REPORTER);
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
        "`contact_monitor` is false, so Godot reports 0 contacts even while the \
         body is visibly resting on the floor"
    );
    assert_eq!(int_prop(&ml, body, "bodies"), 0, "and no colliding bodies");
    assert_eq!(
        prop(&ml, body, "first"),
        Variant::Nil,
        "the list must be empty rather than absent — a script may iterate it \
         without a guard"
    );
}
