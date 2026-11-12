//! pat-s3rrv.103 clause (c) — `CharacterBody3D.get_slide_collision(i)`.
//!
//! The 3D solver resolved contacts and threw them away: `grep slide_collision
//! crates/gdphysics3d/src/character.rs` returned nothing, so
//! `get_slide_collision_count()` answered 0 for every 3D body however hard it
//! was pressed against a wall.
//!
//! 🔑 No new script arms were needed. The `KinematicCollision` surface in
//! `scripting.rs` reads the node property `_slide_collisions` and hands each
//! field straight back (`rec.get(k).cloned()`), so publishing the same array
//! with `Vector3` values from the 3D bridge makes the existing
//! `get_slide_collision_count` / `get_slide_collision(i)` / `get_normal` /
//! `get_collider` arms serve 3D unchanged — the same trick that let
//! `_body_contacts` serve both dimensions.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A character driven into a wall standing at x = +2.
fn wall_scene() -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut wall = Node::new("Wall", "StaticBody3D");
    wall.set_property("position", Variant::Vector3(Vector3::new(2.0, 0.0, 0.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut ws = Node::new("WallShape", "CollisionShape3D");
    ws.set_property("shape", Variant::String("BoxShape3D".to_string()));
    ws.set_property("size", Variant::Vector3(Vector3::new(1.0, 40.0, 40.0)));
    tree.add_child(wall_id, ws).unwrap();

    let player = tree
        .add_child(root, Node::new("Player", "CharacterBody3D"))
        .unwrap();
    let mut ps = Node::new("Shape", "CollisionShape3D");
    ps.set_property("shape", Variant::String("SphereShape3D".to_string()));
    ps.set_property("radius", Variant::Float(0.5));
    tree.add_child(player, ps).unwrap();

    // Reads the contact back through the SAME script surface a Godot game uses.
    let script = "\
extends CharacterBody3D
var ticks = 0
var count = -1
var normal = Vector3(-9, -9, -9)
var collider = null
func _physics_process(delta):
    velocity = Vector3(8, 0, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.count = get_slide_collision_count()
    if self.count > 0:
        var c = get_slide_collision(0)
        self.normal = c.get_normal()
        self.collider = c.get_collider()
";
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(script, player).unwrap()),
    );
    (MainLoop::new(tree), player, wall_id)
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(node)
        .expect("player node")
        .get_property(name)
}

/// **Acceptance clause (c).** A body pressed against a wall reports the contact,
/// its normal, and what it hit.
#[test]
fn a_character_body_3d_reports_its_slide_collisions() {
    let (mut ml, player, wall) = wall_scene();
    for _ in 0..90 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        matches!(prop(&ml, player, "ticks"), Variant::Int(n) if n > 0),
        "`_physics_process` never ran, so every value below is an initialiser"
    );
    // ⚠️ `-1` is the initialiser, so this separates "reported zero contacts"
    // from "the call raised and the assignment never happened".
    assert!(
        !matches!(prop(&ml, player, "count"), Variant::Int(-1)),
        "get_slide_collision_count() must have been called successfully"
    );
    match prop(&ml, player, "count") {
        Variant::Int(n) => assert!(
            n >= 1,
            "a body driven into a wall for 90 ticks must report at least one \
             slide collision, got {n}"
        ),
        other => panic!("`count` must be an int, got {other:?}"),
    }

    // 🔑 The normal points AWAY from the wall surface, back toward the body —
    // the wall is at +x, so the body is pushed in -x.
    match prop(&ml, player, "normal") {
        Variant::Vector3(v) => {
            assert!(
                v.x < -0.5,
                "the contact normal must point back off the +x wall (x well \
                 below zero), got {v:?}; a normal pointing INTO the wall means \
                 the sign was taken from the solver's A->B convention without \
                 flipping it"
            );
            assert!(
                v.y.abs() < 0.5 && v.z.abs() < 0.5,
                "and it must be essentially axis-aligned for a flat wall, got {v:?}"
            );
        }
        other => panic!(
            "`normal` must be a Vector3 — a Vector2 here means the record was \
             published with 2D values, got {other:?}"
        ),
    }

    // 🔑 The collider resolves back to the WALL NODE. The solver records only a
    // `BodyId3D`; if that id -> node mapping were wrong or missing this would be
    // Nil while every other assertion above still passed.
    assert_eq!(
        prop(&ml, player, "collider"),
        Variant::ObjectId(wall.object_id()),
        "get_collider() must hand back the Wall node itself"
    );
}
