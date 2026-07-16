//! pat-nzgdu.167: SceneState introspection over PackedScene.
//!
//! Godot's `PackedScene.get_state()` returns a `SceneState` exposing read-only
//! introspection of the packed node table and signal connections, and
//! `SceneState` is a ClassDB-registered RefCounted resource. This guards both:
//! the class is registered, and a parsed `.tscn` reports its nodes/connections.

use gdobject::class_db;
use gdscene::packed_scene::PackedScene;

#[test]
fn scene_state_introspection_test() {
    // SceneState is registered under RefCounted so scripts/introspection reach it.
    class_db::register_editor_classes();
    assert!(
        class_db::class_exists("SceneState"),
        "SceneState must be registered in ClassDB"
    );

    // A .tscn with two nodes (Root, Button) and one signal connection.
    let tscn = "[gd_scene format=3 uid=\"uid://scenestatetest\"]\n\n\
        [node name=\"Root\" type=\"Node2D\"]\n\n\
        [node name=\"Button\" type=\"Button\" parent=\".\"]\n\n\
        [connection signal=\"pressed\" from=\"Button\" to=\".\" method=\"_on_pressed\"]\n";

    let scene = PackedScene::from_tscn(tscn).expect("the .tscn should parse");
    let state = scene.get_state();

    // Node table introspection.
    assert_eq!(state.get_node_count(), 2, "two nodes in the scene");
    assert_eq!(state.get_node_name(0), Some("Root"));
    assert_eq!(state.get_node_type(0), Some("Node2D"));
    assert_eq!(state.get_node_name(1), Some("Button"));
    assert_eq!(state.get_node_type(1), Some("Button"));

    // Connection introspection: source / signal / target / method.
    assert_eq!(state.get_connection_count(), 1, "one connection in the scene");
    assert_eq!(state.get_connection_signal(0), Some("pressed"));
    assert_eq!(state.get_connection_source(0), Some("Button"));
    assert_eq!(state.get_connection_target(0), Some("."));
    assert_eq!(state.get_connection_method(0), Some("_on_pressed"));
}
