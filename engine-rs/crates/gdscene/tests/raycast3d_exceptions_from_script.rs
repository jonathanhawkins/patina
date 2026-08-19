//! pat-s3rrv.106: `add_exception` / `clear_exceptions` reach RayCast3D from script.
//!
//! `gdscene/src/raycast3d.rs` and `shapecast3d.rs` have carried
//! `add_exception` / `remove_exception` / `clear_exceptions` since they landed,
//! and no script could reach any of them — the dispatch covered only
//! `is_colliding`, the collision getters and `force_raycast_update`. Excluding
//! a body is how a character stops its own cast hitting itself, so its absence
//! is felt immediately in a real scene.
//!
//! 🔑 The assertions are DIFFERENTIAL across ticks: the same cast must report a
//! hit BEFORE the exception and a miss AFTER it. Asserting only "misses after"
//! would pass against a cast that never hit anything at all — including one
//! broken by the tick-ordering change in this same bead.

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

fn scene(script: &str) -> (MainLoop, NodeId) {
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
    if let Some(n) = tree.get_node_mut(ray) {
        n.set_property("enabled", Variant::Bool(true));
        n.set_property(
            "target_position",
            Variant::Vector3(Vector3::new(0.0, -5.0, 0.0)),
        );
    }
    tree.attach_script(
        ray,
        Box::new(GDScriptNodeInstance::from_source(script, ray).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, ray)
}

/// **The acceptance.** An excluded body stops being reported.
#[test]
fn add_exception_removes_a_body_from_a_raycast3d() {
    let (mut ml, ray) = scene(
        "\
extends RayCast3D
var ticks = 0
var before = false
var after = true

func _physics_process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        self.before = is_colliding()
        add_exception(get_node(\"../Floor\"))
    if self.ticks == 2:
        self.after = is_colliding()
",
    );

    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, ray, "ticks"),
        Variant::Int(2),
        "fixture: two steps must produce two physics ticks"
    );
    // BEFORE: the cast genuinely hits, so the assertion below is about the
    // exception rather than about a cast that never worked.
    assert_eq!(
        script_var(&ml, ray, "before"),
        Variant::Bool(true),
        "the ray must hit the floor before any exception is added"
    );
    assert_eq!(
        script_var(&ml, ray, "after"),
        Variant::Bool(false),
        "after `add_exception(floor)` the same cast must miss — a true here \
         means the call did not reach `raycast3d::add_exception`, which is the \
         gap this covers"
    );
}

/// `clear_exceptions` puts the body back.
///
/// Without this, an `add_exception` that permanently disabled the cast — rather
/// than excluding one body — would pass the test above.
#[test]
fn clear_exceptions_restores_the_hit() {
    let (mut ml, ray) = scene(
        "\
extends RayCast3D
var ticks = 0
var excluded = true
var restored = false

func _physics_process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        add_exception(get_node(\"../Floor\"))
    if self.ticks == 2:
        self.excluded = is_colliding()
        clear_exceptions()
    if self.ticks == 3:
        self.restored = is_colliding()
",
    );

    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, ray, "excluded"),
        Variant::Bool(false),
        "the exception must take effect on the following tick"
    );
    assert_eq!(
        script_var(&ml, ray, "restored"),
        Variant::Bool(true),
        "`clear_exceptions()` must restore the hit — false here means the \
         exception list is never emptied, so a script could exclude a body once \
         and never get it back"
    );
}
