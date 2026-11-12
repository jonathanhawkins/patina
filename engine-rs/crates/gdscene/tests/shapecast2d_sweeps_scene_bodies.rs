//! pat-s3rrv.87 — `ShapeCast2D` sweeps real scene bodies and reports to GDScript.
//!
//! ClassDB registered the class with properties and nothing else. There was no
//! driver anywhere — no `shapecast2d.rs`, no mention in any query phase — so a
//! ShapeCast2D in a scene published nothing and every getter read `Nil`.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/shape_cast_2d.cpp`,
//! which updates under `NOTIFICATION_INTERNAL_PHYSICS_PROCESS` — i.e. the sweep
//! is already published by the time any `_physics_process` runs, which is what
//! makes "after one step" reachable at all.
//!
//! ## Geometry
//!
//! `Cast` is a 20×20 box at the origin sweeping `target_position = (0, 100)`.
//! `Body` is a 200×20 box centred at `(0, 80)`, so it spans `y ∈ [70, 90]`. The
//! cast's leading edge (centre + 10) first touches it at centre `y = 60`, i.e.
//! at fraction **0.6** of the sweep — strictly inside `(0, 1)`, which is what
//! the safe/unsafe fractions must bracket. At the sweep's start the cast spans
//! `y ∈ [-10, 10]` and is well clear, so the first substep is genuinely clean.

use gdcore::math::Vector2;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// The scene, parameterised by the ShapeCast2D's `enabled` flag so the disabled
/// case differs by exactly one property.
fn tscn(enabled: bool) -> String {
    format!(
        r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="CastShape_1"]
size = Vector2(20, 20)

[sub_resource type="RectangleShape2D" id="BodyShape_1"]
size = Vector2(200, 20)

[node name="Main" type="Node2D"]

[node name="Cast" type="ShapeCast2D" parent="."]
position = Vector2(0, 0)
enabled = {enabled}
target_position = Vector2(0, 100)
collision_mask = 1
shape = SubResource("CastShape_1")

[node name="Body" type="StaticBody2D" parent="."]
position = Vector2(0, 80)
collision_layer = 1

[node name="BodyShape" type="CollisionShape2D" parent="Body"]
shape = SubResource("BodyShape_1")
"#
    )
}

/// Reads a script variable off the node's script instance.
fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Finds a node by name anywhere in the tree — `add_packed_scene_to_tree`
/// returns the scene's own root, so paths depend on where it was attached.
fn by_name(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| tree.get_node(*id).is_some_and(|n| n.name() == name))
        .unwrap_or_else(|| panic!("the scene must contain a node named {name}"))
}

const READER_GD: &str = "\
extends ShapeCast2D
var hit = false
var count = 0
var collider = null
var safe = 0.0
var unsafe_f = 0.0
var ticks = 0
func _physics_process(delta):
    self.ticks = self.ticks + 1
    self.hit = is_colliding()
    self.count = get_collision_count()
    self.collider = get_collider()
    self.safe = get_closest_collision_safe_fraction()
    self.unsafe_f = get_closest_collision_unsafe_fraction()
";

/// Loads the scene, attaches the reader script to the cast, and registers the
/// bodies. Returns the loop plus the cast and body ids.
fn scene(enabled: bool) -> (MainLoop, NodeId, NodeId) {
    // ClassDB is empty in every nextest process; without it the class chain does
    // not resolve and the physics registration walk finds nothing.
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(&tscn(enabled)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let cast = by_name(&tree, "Cast");
    let body = by_name(&tree, "Body");
    tree.attach_script(
        cast,
        Box::new(GDScriptNodeInstance::from_source(READER_GD, cast).unwrap()),
    );

    // `MainLoop::step` never populates `node_to_body`; without this the world
    // holds no bodies and every sweep trivially misses.
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, cast, body)
}

#[test]
fn shapecast2d_sweeps_scene_bodies() {
    let (mut ml, cast, body) = scene(true);
    ml.step(1.0 / 60.0);

    // The script must actually have run: every assertion below reads a variable
    // it writes, so a script that never ran would report the initialisers and
    // look like a clean miss.
    assert_eq!(
        script_var(&ml, cast, "ticks"),
        Some(Variant::Int(1)),
        "_physics_process must have run exactly once"
    );

    assert_eq!(
        script_var(&ml, cast, "hit"),
        Some(Variant::Bool(true)),
        "the sweep crosses the body, so is_colliding() must be true after one step"
    );
    match script_var(&ml, cast, "count") {
        Some(Variant::Int(n)) => assert!(n >= 1, "get_collision_count() must be >= 1, got {n}"),
        other => panic!("get_collision_count() must return an Int, got {other:?}"),
    }
    assert_eq!(
        script_var(&ml, cast, "collider"),
        Some(Variant::ObjectId(body.object_id())),
        "get_collider() must resolve to the StaticBody2D"
    );

    // The fractions bracket the real contact at 0.6 of the sweep. Both must be
    // strictly inside (0, 1): a 1.0 would mean "never blocked" and a 0.0 would
    // mean "already overlapping at the start", and each is what a driver that
    // did not actually sweep would report.
    let safe = match script_var(&ml, cast, "safe") {
        Some(Variant::Float(f)) => f as f32,
        other => panic!("safe fraction must be a Float, got {other:?}"),
    };
    let unsafe_f = match script_var(&ml, cast, "unsafe_f") {
        Some(Variant::Float(f)) => f as f32,
        other => panic!("unsafe fraction must be a Float, got {other:?}"),
    };
    assert!(
        safe > 0.0 && safe < 1.0,
        "get_closest_collision_safe_fraction() must be strictly between 0 and 1, got {safe}"
    );
    assert!(
        unsafe_f > 0.0 && unsafe_f < 1.0,
        "the unsafe fraction must also be strictly between 0 and 1, got {unsafe_f}"
    );
    assert!(
        safe < unsafe_f,
        "the safe fraction is the last CLEAN step and must precede the blocked one: \
         safe={safe} unsafe={unsafe_f}"
    );
    // The contact is at 0.6 of the sweep; substepping quantises to 1/32, so the
    // pair must straddle it rather than land anywhere in range.
    assert!(
        safe <= 0.6 && unsafe_f >= 0.6,
        "the fractions must straddle the true contact at 0.6, got safe={safe} unsafe={unsafe_f}"
    );

    // -- moving the body out of the sweep clears the hit on the NEXT frame ---
    ml.tree_mut()
        .get_node_mut(body)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(500.0, 80.0)));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, cast, "hit"),
        Some(Variant::Bool(false)),
        "with the body moved aside the next frame must report no collision"
    );
    assert_eq!(
        script_var(&ml, cast, "collider"),
        Some(Variant::Nil),
        "a miss must clear the collider, not leave a stale one"
    );
    assert_eq!(
        script_var(&ml, cast, "ticks"),
        Some(Variant::Int(2)),
        "the second frame must have run — otherwise 'false' is just a stale read"
    );
}

/// `enabled = false` reports no collision even though the geometry overlaps —
/// the control that proves the hit above comes from the sweep and not from the
/// mere presence of a body.
#[test]
fn a_disabled_shapecast2d_reports_nothing() {
    let (mut ml, cast, _body) = scene(false);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, cast, "ticks"),
        Some(Variant::Int(1)),
        "_physics_process must still run on a disabled cast"
    );
    assert_eq!(
        script_var(&ml, cast, "hit"),
        Some(Variant::Bool(false)),
        "a disabled ShapeCast2D must report false regardless of the geometry"
    );
    assert_eq!(
        script_var(&ml, cast, "count"),
        Some(Variant::Int(0)),
        "a disabled cast must report no results"
    );
}
