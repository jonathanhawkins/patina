//! pat-s3rrv.189 — a `CollisionPolygon3D` child contributes a real extruded
//! prism collider to its body.
//!
//! Godot ground truth: `CollisionPolygon3D : Node3D` carries its outline inline
//! as a `polygon` PackedVector2Array in the node's local XY plane, extruded
//! along Z by `depth` (default 1.0). The extrusion is CENTRED — each outline
//! point is emitted twice, at `+depth * 0.5` and `-depth * 0.5`
//! (`scene/3d/physics/collision_polygon_3d.cpp:66-67`).
//!
//! ⚠️ THE FAILURE THIS PINS IS SILENT. The prism geometry already existed, but
//! `shape_from_node_3d` rejected every class except `CollisionShape3D`, and the
//! body scan falls back to `Shape3D::Sphere { radius: 0.5 }` when no child
//! yields a shape. So a body whose only collider was a polygon did not lose its
//! collision — it silently got a half-metre ball instead of the authored
//! geometry. A test asking "does the body collide?" passes either way.
//!
//! ⚠️ WHY THESE ASSERT THE REGISTERED SHAPE RATHER THAN A DROPPED BODY COMING
//! TO REST. An earlier version of this file dropped a RigidBody3D onto the
//! prism and asserted its resting height. That measures the SOLVER, not this
//! bead: `animatable_body_carries_riders.rs:277` already notes that a full 3D
//! landing "would additionally depend on 3D resting behaviour this bead does
//! not touch", and in practice the body fell straight through to y = -88.
//! Whether 3D resting contact works is a separate question from whether the
//! authored polygon became a collider — which is what .189 is about, and what
//! is asserted here, by naming the exact shape the world registered.

use gdcore::math::{Vector2, Vector3};
use gdobject::class_db::{
    class_exists, get_property_getter, is_parent_class, property_get_revert, register_3d_classes,
};
use gdphysics3d::shape::Shape3D;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const STEP: f64 = 1.0 / 60.0;

/// A square outline from `-half` to `+half` in the node's local XY plane.
fn square(half: f32) -> Variant {
    Variant::PackedVector2Array(vec![
        Vector2::new(-half, -half),
        Vector2::new(half, -half),
        Vector2::new(half, half),
        Vector2::new(-half, half),
    ])
}

/// `Root -> Floor (StaticBody3D)`, optionally with a CollisionPolygon3D child.
///
/// Returns the loop and the floor's node id. One step is run so the server's
/// lazy registration happens.
fn floor_scene(child: Option<(f32, bool, Option<f32>)>) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let floor = tree
        .add_child(root, Node::new("Floor", "StaticBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, floor, Vector3::ZERO);

    if let Some((half, disabled, depth)) = child {
        let mut poly = Node::new("Poly", "CollisionPolygon3D");
        poly.set_property("polygon", square(half));
        if let Some(d) = depth {
            poly.set_property("depth", Variant::Float(d as f64));
        }
        if disabled {
            poly.set_property("disabled", Variant::Bool(true));
        }
        tree.add_child(floor, poly).unwrap();
    }

    let mut ml = MainLoop::new(tree);
    // The 3D server registers lazily inside `sync_to_physics`, so the body does
    // not exist until a tick has run.
    ml.step(STEP);
    (ml, floor)
}

/// The shape the physics world actually registered for `node`.
fn registered_shape(ml: &MainLoop, node: NodeId) -> Shape3D {
    let server = ml.physics_server_3d();
    let id = server
        .body_for_node(node)
        .expect("the StaticBody3D should be registered with the 3D world");
    server
        .world()
        .get_body(id)
        .expect("the registered body should exist")
        .shape
        .clone()
}

/// **The acceptance.** A body whose only collision child is a
/// CollisionPolygon3D registers the extruded prism — not the fallback sphere.
#[test]
fn collision_polygon_3d_builds_collider() {
    let (ml, floor) = floor_scene(Some((5.0, false, Some(2.0))));

    let Shape3D::ConvexPolygonShape { points } = registered_shape(&ml, floor) else {
        panic!(
            "expected an extruded prism; got {:?} — the fallback sphere means \
             the polygon child was never collected",
            registered_shape(&ml, floor)
        );
    };

    // Four outline points, each emitted at both caps.
    assert_eq!(points.len(), 8, "one vertex per outline point per cap");

    // depth = 2 extrudes to ±1, CENTRED. A 0..depth extrusion would put every
    // z at 0 or 2 and sink the prism half its thickness below the author's
    // intent.
    let mut zs: Vec<f32> = points.iter().map(|p| p.z).collect();
    zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(zs.first(), Some(&-1.0), "near cap at -depth/2");
    assert_eq!(zs.last(), Some(&1.0), "far cap at +depth/2");

    // The outline survives into XY.
    let max_x = points.iter().fold(f32::MIN, |m, p| m.max(p.x));
    let max_y = points.iter().fold(f32::MIN, |m, p| m.max(p.y));
    assert_eq!(max_x, 5.0, "outline half-width");
    assert_eq!(max_y, 5.0, "outline half-height");
}

/// `depth` defaults to 1.0, not 0 — a missing property must not collapse the
/// prism into a zero-thickness sheet.
#[test]
fn depth_defaults_to_one() {
    let (ml, floor) = floor_scene(Some((2.0, false, None)));

    let Shape3D::ConvexPolygonShape { points } = registered_shape(&ml, floor) else {
        panic!("expected a prism");
    };
    let max_z = points.iter().fold(f32::MIN, |m, p| m.max(p.z));
    assert_eq!(max_z, 0.5, "default depth 1.0 extrudes to ±0.5");
}

/// The control that gives the acceptance its meaning: with NO collision child
/// the body scan falls back to a 0.5-radius sphere.
///
/// This is exactly what the polygon case produced before the fix — collision
/// that exists but is not the authored shape. Without naming it, "the body has
/// a shape" would pass on the unwired engine.
#[test]
fn an_unwired_body_gets_the_fallback_sphere() {
    let (ml, floor) = floor_scene(None);
    assert!(
        matches!(registered_shape(&ml, floor), Shape3D::Sphere { radius } if radius == 0.5),
        "a body with no collision child should fall back to the default sphere, \
         got {:?}",
        registered_shape(&ml, floor)
    );
}

/// `disabled` is honoured: a disabled polygon contributes nothing, so the body
/// falls back exactly as if the child were absent.
#[test]
fn a_disabled_polygon_contributes_no_geometry() {
    let (ml, floor) = floor_scene(Some((5.0, true, Some(2.0))));
    assert!(
        matches!(registered_shape(&ml, floor), Shape3D::Sphere { radius } if radius == 0.5),
        "a disabled CollisionPolygon3D must not build a prism, got {:?}",
        registered_shape(&ml, floor)
    );
}

/// `CollisionPolygon3D` is registered in ClassDB with Godot's documented
/// members (pat-s3rrv.189).
///
/// The collider works without this — the body scan matches on `class_name()`
/// and never consults ClassDB — but reflection (`has_method`, the inspector,
/// `get_property_list`) reported the class as unknown.
///
/// ⚠️ The accessors are asserted BY NAME, not merely for existence. Two of them
/// do not follow the `get_<name>` pattern a guess would produce, and a
/// duplicate registration SHADOWS silently (last declaration wins), so an
/// accessor name is the assertion that actually catches a wrong or duplicated
/// entry.
#[test]
fn collision_polygon_3d_is_registered_in_classdb() {
    register_3d_classes();

    assert!(class_exists("CollisionPolygon3D"));
    assert!(
        is_parent_class("CollisionPolygon3D", "Node3D"),
        "Godot parents it to Node3D, not to a CollisionObject"
    );

    assert_eq!(
        get_property_getter("CollisionPolygon3D", "disabled"),
        "is_disabled",
        "doc/classes/CollisionPolygon3D.xml gives `is_disabled`, not `get_disabled`"
    );
    assert_eq!(
        get_property_getter("CollisionPolygon3D", "debug_fill"),
        "get_enable_debug_fill",
        "the debug_fill accessors carry an `enable_` infix"
    );

    // depth defaults to 1.0, not 0 — a zero would collapse the prism.
    assert_eq!(
        property_get_revert("CollisionPolygon3D", "depth"),
        Some(Variant::Float(1.0))
    );
    assert_eq!(
        property_get_revert("CollisionPolygon3D", "margin"),
        Some(Variant::Float(0.04))
    );
}
