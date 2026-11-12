//! pat-s3rrv.102 clause (c): the four shape-indexed `Area3D` signals.
//!
//! Godot pairs every `body_entered` / `area_entered` with a shape-indexed
//! variant carrying the collider's RID, the node, and TWO shape indices:
//! `body_shape_entered(body_rid, body, body_shape_index, local_shape_index)`
//! and likewise `body_shape_exited` / `area_shape_entered` /
//! `area_shape_exited` (`doc/classes/Area3D.xml`).
//!
//! ⚠️ None of the four existed anywhere in the workspace — a repo-wide grep for
//! `body_shape_entered` returned zero hits in `src`. A script connecting the
//! shape-indexed form, which is the one you need in order to tell WHICH
//! collider touched you, received nothing at all and no error.
//!
//! 🔑 Both indices are 0, and that is CORRECT rather than a stub: 3D bodies and
//! areas are single-shape in Patina today (`gdphysics3d` carries no
//! `extra_shapes`, unlike its 2D twin), so shape 0 is the only shape there is.
//! The signal's ARITY and argument ORDER are Godot's, so a script written
//! against this keeps working when real indices arrive.
//!
//! ⚠️ Every observed value starts at an out-of-band SENTINEL, never 0. A handler
//! that never ran and a handler that ran and reported index 0 are different
//! outcomes, and 0 would conflate exactly the two the assertions care about.

use gdcore::math::Vector3;
use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance};
use gdvariant::Variant;

/// Records the four arguments of whichever shape-indexed signal fired.
const WATCHER_GD: &str = "\
extends Node3D
var fired = 0
var rid = -999
var body_idx = -999
var local_idx = -999
var got_node = false

func _on_shape_entered(collider_rid, collider, collider_shape_index, local_shape_index):
    self.fired = self.fired + 1
    self.rid = collider_rid
    self.body_idx = collider_shape_index
    self.local_idx = local_shape_index
    self.got_node = collider != null
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn add_shape(tree: &mut SceneTree, parent: NodeId, class: &str, dim: (&str, Variant)) {
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String(class.to_string()));
    shape.set_property(dim.0, dim.1);
    tree.add_child(parent, shape).unwrap();
}

/// `Root -> {Zone (Area3D, 4×4×4 box at the origin), Ball (RigidBody3D sphere)}`
/// with the ball starting high enough to fall through the zone.
///
/// `signal_name` is connected to the watcher script on Root, exactly as a
/// `.tscn` `[connection]` reaches a handler — a method-name connection rather
/// than a Callable.
fn falling_ball_scene(signal_name: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let zone = tree.add_child(root, Node::new("Zone", "Area3D")).unwrap();
    node3d::set_position(&mut tree, zone, Vector3::ZERO);
    add_shape(
        &mut tree,
        zone,
        "BoxShape3D",
        ("size", Variant::Vector3(Vector3::new(4.0, 4.0, 4.0))),
    );

    let ball = tree
        .add_child(root, Node::new("Ball", "RigidBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, ball, Vector3::new(0.0, 6.0, 0.0));
    add_shape(
        &mut tree,
        ball,
        "SphereShape3D",
        ("radius", Variant::Float(0.5)),
    );

    tree.connect_signal(
        zone,
        signal_name,
        Connection::new(root.object_id(), "_on_shape_entered"),
    );
    tree.attach_script(
        root,
        Box::new(GDScriptNodeInstance::from_source(WATCHER_GD, root).unwrap()),
    );

    (MainLoop::new(tree), root)
}

/// **The acceptance clause.** A body falling into an Area3D fires
/// `body_shape_entered` with four arguments, including a `local_shape_index`
/// of 0.
#[test]
fn body_shape_entered_delivers_four_arguments() {
    let (mut ml, root) = falling_ball_scene("body_shape_entered");

    // Nothing before the first frame — proves the assertions below are about
    // the signal and not about a handler that had already run.
    assert_eq!(script_var(&ml, root, "fired"), Some(Variant::Int(0)));

    for _ in 0..240 {
        ml.step(1.0 / 60.0);
        if script_var(&ml, root, "fired") != Some(Variant::Int(0)) {
            break;
        }
    }

    assert_eq!(
        script_var(&ml, root, "fired"),
        Some(Variant::Int(1)),
        "the ball falling into the zone must fire `body_shape_entered` exactly \
         once; 0 means the signal is still not emitted at all"
    );
    assert_eq!(
        script_var(&ml, root, "local_idx"),
        Some(Variant::Int(0)),
        "`local_shape_index` must arrive as the 4th argument and be 0. A -999 \
         here means the handler ran with FEWER than four arguments, so the \
         signal's shape does not match Godot's"
    );
    assert_eq!(
        script_var(&ml, root, "body_idx"),
        Some(Variant::Int(0)),
        "`body_shape_index` must arrive as the 3rd argument and be 0"
    );
    assert_eq!(
        script_var(&ml, root, "got_node"),
        Some(Variant::Bool(true)),
        "the 2nd argument must be the colliding NODE, not null"
    );
    // The RID slot is an integer handle in Patina (the Variant has no RID
    // type). Asserting only that it moved off the sentinel keeps this from
    // pinning an allocation order that is not part of the contract.
    assert_ne!(
        script_var(&ml, root, "rid"),
        Some(Variant::Int(-999)),
        "the 1st argument must be the collider's RID handle, not absent"
    );
}

/// The area-vs-area half of the same clause.
#[test]
fn area_shape_entered_delivers_four_arguments() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let zone_a = tree.add_child(root, Node::new("ZoneA", "Area3D")).unwrap();
    node3d::set_position(&mut tree, zone_a, Vector3::ZERO);
    add_shape(
        &mut tree,
        zone_a,
        "BoxShape3D",
        ("size", Variant::Vector3(Vector3::new(4.0, 4.0, 4.0))),
    );

    let zone_b = tree.add_child(root, Node::new("ZoneB", "Area3D")).unwrap();
    node3d::set_position(&mut tree, zone_b, Vector3::new(1.0, 0.0, 0.0));
    add_shape(
        &mut tree,
        zone_b,
        "BoxShape3D",
        ("size", Variant::Vector3(Vector3::new(4.0, 4.0, 4.0))),
    );

    tree.connect_signal(
        zone_a,
        "area_shape_entered",
        Connection::new(root.object_id(), "_on_shape_entered"),
    );
    tree.attach_script(
        root,
        Box::new(GDScriptNodeInstance::from_source(WATCHER_GD, root).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, root, "fired"),
        Some(Variant::Int(1)),
        "two overlapping Area3Ds must fire `area_shape_entered` once"
    );
    assert_eq!(
        script_var(&ml, root, "local_idx"),
        Some(Variant::Int(0)),
        "`local_shape_index` must arrive as the 4th argument and be 0; -999 \
         means fewer than four arguments were delivered"
    );
}
