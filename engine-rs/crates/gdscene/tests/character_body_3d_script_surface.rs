//! pat-s3rrv.103 — `CharacterBody3D`'s velocity getters answer in **3D**.
//!
//! `get_real_velocity()` and `get_platform_velocity()` live in the script table
//! shared by `CharacterBody2D` and `CharacterBody3D`, and that table was 2D
//! only: both getters wrapped their result in `Variant::Vector2`.
//!
//! ⚠️ The failure was worse than a wrong type. The reader they used matched
//! `Variant::Vector2` and fell through to `ZERO` for anything else — and the 3D
//! solver publishes `Vector3`. So a 3D script did not get a type error, it got
//! **zero**: a body riding a moving platform read `(0, 0)`, which reads as "the
//! platform is not moving" rather than as a bug. A wrong type announces itself;
//! a wrong value that is a plausible resting state does not.
//!
//! Two independent defects met here, and the tests keep them distinguishable:
//!
//! 1. `platform_velocity` **was** published as `Vector3` and thrown away by the
//!    `Vector2`-only reader (a getter bug).
//! 2. `_real_velocity` was never published by the 3D solver at all — only
//!    `physics_server.rs` (2D) wrote it — so there was nothing to read even
//!    with a fixed getter (a producer bug).

use gdcore::math::{Vector2, Vector3};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// `TYPE_VECTOR3` / `TYPE_VECTOR2` as GDScript's `typeof()` reports them.
const TYPE_VECTOR2: i64 = 5;
const TYPE_VECTOR3: i64 = 9;

/// A scripted `CharacterBody3D` standing on a wide static floor.
///
/// ⚠️ The `CollisionShape3D` child is not decoration: `process_character_movement`
/// resolves the body's shape with a `find_map` over its children and `continue`s
/// when there is none, so a body without one is silently skipped and every
/// assertion below would read an initialiser.
fn walker_scene(script: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut floor = Node::new("Floor", "StaticBody3D");
    // ⚠️ Placed at -2.0, not -1.0. A 1.0-tall box at -1.0 has its top face at
    // exactly -0.5, which is exactly where the player sphere's bottom sits — a
    // tangency, the coincidence point where "resting on" and "not touching"
    // agree. The floor is not load-bearing for anything asserted here (the
    // script holds velocity.y at 0, so the body never falls); it is scene
    // furniture, and it should not sit on a degenerate contact.
    floor.set_property("position", Variant::Vector3(Vector3::new(0.0, -2.0, 0.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut floor_shape = Node::new("FloorShape", "CollisionShape3D");
    floor_shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    floor_shape.set_property("size", Variant::Vector3(Vector3::new(200.0, 1.0, 200.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    let player = tree
        .add_child(root, Node::new("Player", "CharacterBody3D"))
        .unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(player, shape).unwrap();
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(script, player).unwrap()),
    );

    (MainLoop::new(tree), player)
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(node)
        .expect("player node")
        .get_property(name)
}

fn int_prop(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match prop(ml, node, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` must be an int, got {other:?}"),
    }
}

/// **The acceptance.** Both getters answer `Vector3` on a `CharacterBody3D`.
#[test]
fn character_body_3d_velocity_getters_are_vector3() {
    // `velocity` is assigned bare rather than via `self.`, the way an exported
    // Godot scene drives a body — the interpreter routes it to the node
    // property because `CharacterBody3D.velocity` has a non-Nil ClassDB default.
    let script = "\
extends CharacterBody3D
var ticks = 0
var rv = Vector3(-1, -1, -1)
var pv = Vector3(-1, -1, -1)
var rv_type = -1
var pv_type = -1
func _physics_process(delta):
    velocity = Vector3(4, 0, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.rv = get_real_velocity()
    self.pv = get_platform_velocity()
    self.rv_type = typeof(get_real_velocity())
    self.pv_type = typeof(get_platform_velocity())
";
    let (mut ml, player) = walker_scene(script);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }

    // Asserted first: with no ticks, every value below is still its
    // initialiser, and `rv_type == -1` would read as "not a Vector3" for the
    // wrong reason entirely.
    assert!(
        int_prop(&ml, player, "ticks") > 0,
        "`_physics_process` never ran, so nothing below is evidence of anything"
    );

    // 🔑 The acceptance line: `typeof()` as the script sees it.
    assert_eq!(
        int_prop(&ml, player, "rv_type"),
        TYPE_VECTOR3,
        "get_real_velocity() must be a Vector3 on a CharacterBody3D; {TYPE_VECTOR2} \
         (TYPE_VECTOR2) means the shared script table is still answering in 2D"
    );
    assert_eq!(
        int_prop(&ml, player, "pv_type"),
        TYPE_VECTOR3,
        "get_platform_velocity() must be a Vector3 on a CharacterBody3D"
    );

    // ⚠️ And the VALUE, not just the type. The old reader returned
    // `Vector2::ZERO`, so a test that only checked "is it 2D?" would still pass
    // against a getter that answered `Vector3::ZERO` for every body forever.
    // The walker is driven at 4 units/s along +x and nothing obstructs it, so
    // the motion it actually achieved must show up on x.
    match prop(&ml, player, "rv") {
        Variant::Vector3(v) => assert!(
            (v.x - 4.0).abs() < 0.5,
            "get_real_velocity().x must reflect the ~4/s the body actually \
             travelled, got {v:?} — a zero here means the getter answers the \
             right TYPE with a dead value"
        ),
        other => panic!("`rv` must be a Vector3, got {other:?}"),
    }

    // Positive control on the producer: the body really did move, so a
    // `_real_velocity` of zero would be the solver's silence, not a true rest.
    let end = node3d::get_position(ml.tree(), player);
    assert!(
        end.x > 1.0,
        "the walker must have travelled along +x for the velocity above to mean \
         anything, ended at {end:?}"
    );
}

/// 🔑 The 2D body must keep answering in 2D.
///
/// ⚠️ The obvious fix — make the getters return `Vector3` — passes every 3D
/// assertion above and breaks every 2D script in the engine. The getters are
/// one shared table, so widening it is only correct if the 2D half is pinned.
#[test]
fn character_body_2d_velocity_getters_stay_vector2() {
    let script = "\
extends CharacterBody2D
var ticks = 0
var rv_type = -1
var pv_type = -1
func _physics_process(delta):
    velocity = Vector2(4, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.rv_type = typeof(get_real_velocity())
    self.pv_type = typeof(get_platform_velocity())
";
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    let body = tree
        .add_child(root, Node::new("Body", "CharacterBody2D"))
        .unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(Vector2::new(10.0, 10.0)));
    tree.add_child(body, shape).unwrap();
    tree.attach_script(
        body,
        Box::new(GDScriptNodeInstance::from_source(script, body).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        int_prop(&ml, body, "ticks") > 0,
        "the 2D `_physics_process` never ran"
    );
    assert_eq!(
        int_prop(&ml, body, "rv_type"),
        TYPE_VECTOR2,
        "get_real_velocity() must stay a Vector2 on a CharacterBody2D"
    );
    assert_eq!(
        int_prop(&ml, body, "pv_type"),
        TYPE_VECTOR2,
        "get_platform_velocity() must stay a Vector2 on a CharacterBody2D"
    );
}

/// Before the first physics tick nothing has been published, and Godot still
/// answers a zero vector rather than null so a script may chain `.length()`.
///
/// ⚠️ It has to be the zero of the RIGHT dimension. A single shared
/// `Vector2::ZERO` fallback makes the very first read of a 3D body a `Vector2`
/// again — the same bug, surviving in the one frame most likely to be read by
/// an `_ready`-adjacent script.
#[test]
fn unpublished_velocity_reads_as_zero_in_the_bodys_own_dimension() {
    let script = "\
extends CharacterBody3D
var ran = false
var rv_type = -1
var rv_len = -1.0
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    self.rv_type = typeof(get_real_velocity())
    self.rv_len = get_real_velocity().length()
";
    // Note: no `velocity` is ever assigned, so the solver skips this body
    // entirely and never publishes `_real_velocity`.
    let (mut ml, player) = walker_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        prop(&ml, player, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run"
    );
    assert_eq!(
        int_prop(&ml, player, "rv_type"),
        TYPE_VECTOR3,
        "an unpublished get_real_velocity() must still be a Vector3 on a 3D body"
    );
    assert_eq!(
        prop(&ml, player, "rv_len"),
        Variant::Float(0.0),
        "and it must be the ZERO vector, chainable with .length() the way Godot \
         guarantees"
    );
}
