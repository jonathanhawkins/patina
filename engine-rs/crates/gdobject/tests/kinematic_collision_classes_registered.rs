//! pat-s3rrv.95 — `KinematicCollision2D` / `KinematicCollision3D` are
//! ClassDB-registered with Godot's getter surface.
//!
//! Godot ground truth: `doc/classes/KinematicCollision2D.xml` and
//! `KinematicCollision3D.xml`.
//!
//! These are the objects `get_slide_collision(i)` and `move_and_collide()` hand
//! back. The 2D collision data was already being computed every physics tick
//! and thrown away — `CharacterBody2D` fills a `Vec<KinematicCollision2D>`
//! during its slide loop and nothing ever turned it into anything a script
//! could read — so registration is the first half of making
//! `get_slide_collision(0).get_collider()` reachable at all.

use gdobject::class_db::{class_has_method, get_method_list, register_all_classes};
use gdobject::{class_exists, is_parent_class};

/// Arity matters as much as presence: a method registered with the wrong
/// argument count still answers `class_has_method` and then mis-dispatches.
fn arity_of(class_name: &str, method: &str) -> Option<usize> {
    get_method_list(class_name)
        .into_iter()
        .find(|m| m.name == method)
        .map(|m| m.argument_count)
}

#[test]
fn both_kinematic_collision_classes_exist_under_refcounted() {
    register_all_classes();
    for class in ["KinematicCollision2D", "KinematicCollision3D"] {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} inherits RefCounted in Godot; a wrong parent breaks the \
             reference-counting contract scripts rely on"
        );
    }
}

#[test]
fn kinematic_collision_2d_exposes_godots_getters() {
    register_all_classes();
    for method in [
        "get_position",
        "get_normal",
        "get_travel",
        "get_remainder",
        "get_angle",
        "get_depth",
        "get_local_shape",
        "get_collider",
        "get_collider_id",
        "get_collider_rid",
        "get_collider_shape",
        "get_collider_shape_index",
        "get_collider_velocity",
    ] {
        assert!(
            class_has_method("KinematicCollision2D", method),
            "KinematicCollision2D.{method} is in the class XML and must be \
             registered — `class_has_method` is the dispatch gate, so an absent \
             entry makes the call unreachable from GDScript"
        );
    }
}

/// ⚠️ **THE TWO CLASSES ARE NOT SYMMETRIC.** This is the assertion that stops
/// someone "tidying up" by mirroring one registration onto the other.
///
/// A `KinematicCollision3D` reports SEVERAL contacts, so nine of its getters
/// take a `collision_index` and it carries an extra `get_collision_count()`.
/// Every 2D getter is 0-arg. `get_angle` differs in both directions: 1 argument
/// in 2D, 2 in 3D. Mirroring would leave nine 3D methods with an arity that
/// silently mis-dispatches.
#[test]
fn the_3d_class_takes_a_collision_index_where_the_2d_class_does_not() {
    register_all_classes();

    // Same NAME, different ARITY — the trap in one line.
    assert_eq!(arity_of("KinematicCollision2D", "get_normal"), Some(0));
    assert_eq!(arity_of("KinematicCollision3D", "get_normal"), Some(1));
    assert_eq!(arity_of("KinematicCollision2D", "get_angle"), Some(1));
    assert_eq!(arity_of("KinematicCollision3D", "get_angle"), Some(2));

    // Indexed in 3D only.
    for method in [
        "get_collider",
        "get_collider_id",
        "get_collider_rid",
        "get_collider_shape",
        "get_collider_shape_index",
        "get_collider_velocity",
        "get_local_shape",
        "get_position",
    ] {
        assert_eq!(
            arity_of("KinematicCollision3D", method),
            Some(1),
            "KinematicCollision3D.{method} selects a contact by index"
        );
        assert_eq!(
            arity_of("KinematicCollision2D", method),
            Some(0),
            "KinematicCollision2D.{method} has no index — it holds one contact"
        );
    }

    // Whole-collision values stay 0-arg in 3D, and the count exists only there.
    for method in ["get_depth", "get_remainder", "get_travel"] {
        assert_eq!(arity_of("KinematicCollision3D", method), Some(0));
    }
    assert_eq!(
        arity_of("KinematicCollision3D", "get_collision_count"),
        Some(0)
    );
    assert!(
        !class_has_method("KinematicCollision2D", "get_collision_count"),
        "get_collision_count is 3D-only; registering it on the 2D class would \
         advertise an API Godot does not have"
    );
}
