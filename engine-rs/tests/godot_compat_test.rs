//! Integration tests for real Godot 4.3 scene format compatibility.
//!
//! Loads hand-written `.tscn` fixture files that match the format exported
//! by Godot 4.3 and verifies the parsers handle all constructs correctly.

use gdscene::packed_scene::PackedScene;
use gdvariant::Variant;

/// Helper: reads a fixture file relative to the repo root.
fn load_fixture(name: &str) -> String {
    // CARGO_MANIFEST_DIR is engine-rs/, fixtures are at the repo root.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest}/../fixtures/real_godot/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

// ---------------------------------------------------------------------------
// simple_2d.tscn
// ---------------------------------------------------------------------------

#[test]
fn simple_2d_parses_successfully() {
    let source = load_fixture("simple_2d.tscn");
    let scene = PackedScene::from_tscn(&source).expect("should parse simple_2d.tscn");
    assert!(scene.node_count() > 0);
}

#[test]
fn simple_2d_has_correct_node_count() {
    let source = load_fixture("simple_2d.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    // Level, Player, Sprite, CollisionShape, Camera = 5 nodes
    assert_eq!(scene.node_count(), 5);
}

#[test]
fn simple_2d_uid_parsed() {
    let source = load_fixture("simple_2d.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    assert_eq!(scene.uid.as_deref(), Some("uid://bx2m7kcr1f4qp"));
}

#[test]
fn simple_2d_instances_with_properties() {
    let source = load_fixture("simple_2d.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    // Find Player node
    let player = nodes.iter().find(|n| n.name() == "Player").unwrap();
    assert_eq!(player.class_name(), "CharacterBody2D");

    // Check position property
    let pos = player.get_property("position");
    match pos {
        Variant::Vector2(v) => {
            assert_eq!(v.x, 100.0);
            assert_eq!(v.y, 200.0);
        }
        other => panic!("expected Vector2, got {other:?}"),
    }
}

#[test]
fn simple_2d_script_ext_resource_property() {
    let source = load_fixture("simple_2d.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let player = nodes.iter().find(|n| n.name() == "Player").unwrap();
    // script = ExtResource("1_abc12") is stored as the raw reference string.
    let script = player.get_property("script");
    assert_eq!(script, Variant::String("ExtResource(\"1_abc12\")".into()));
    // The resolved script path is stored in the _script_path property.
    let script_path = player.get_property("_script_path");
    assert_eq!(
        script_path,
        Variant::String("res://scripts/player_controller.gd".into())
    );
}

#[test]
fn simple_2d_metadata_properties() {
    let source = load_fixture("simple_2d.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let player = nodes.iter().find(|n| n.name() == "Player").unwrap();
    // metadata/move_speed = 300.0
    let speed = player.get_property("metadata/move_speed");
    assert_eq!(speed, Variant::Float(300.0));
}

// ---------------------------------------------------------------------------
// animated_character.tscn
// ---------------------------------------------------------------------------

#[test]
fn animated_character_parses_successfully() {
    let source = load_fixture("animated_character.tscn");
    let scene = PackedScene::from_tscn(&source).expect("should parse animated_character.tscn");
    assert_eq!(scene.node_count(), 4);
}

#[test]
fn animated_character_sprite_properties() {
    let source = load_fixture("animated_character.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let sprite = nodes.iter().find(|n| n.name() == "AnimatedSprite").unwrap();
    assert_eq!(sprite.class_name(), "AnimatedSprite2D");
    assert_eq!(
        sprite.get_property("animation"),
        Variant::String("idle".into())
    );
    assert_eq!(
        sprite.get_property("autoplay"),
        Variant::String("idle".into())
    );
}

#[test]
fn animated_character_dictionary_property() {
    let source = load_fixture("animated_character.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let anim_player = nodes
        .iter()
        .find(|n| n.name() == "AnimationPlayer")
        .unwrap();
    // libraries = {"": SubResource("AnimationLibrary_anim1")}
    let libs = anim_player.get_property("libraries");
    match libs {
        Variant::Dictionary(map) => {
            assert_eq!(map.len(), 1);
            assert!(map.contains_key(""));
        }
        other => panic!("expected Dictionary, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// ui_screen.tscn
// ---------------------------------------------------------------------------

#[test]
fn ui_screen_parses_successfully() {
    let source = load_fixture("ui_screen.tscn");
    let scene = PackedScene::from_tscn(&source).expect("should parse ui_screen.tscn");
    // UIRoot, Background, VBoxContainer, Logo, Title, StartButton, OptionsButton, QuitButton = 8
    assert_eq!(scene.node_count(), 8);
}

#[test]
fn ui_screen_anchor_properties() {
    let source = load_fixture("ui_screen.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let root = &nodes[0];
    assert_eq!(root.class_name(), "Control");
    assert_eq!(root.get_property("anchor_right"), Variant::Float(1.0));
    assert_eq!(root.get_property("anchor_bottom"), Variant::Float(1.0));
}

#[test]
fn ui_screen_color_rect() {
    let source = load_fixture("ui_screen.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let bg = nodes.iter().find(|n| n.name() == "Background").unwrap();
    let color = bg.get_property("color");
    match color {
        Variant::Color(c) => {
            assert!((c.r - 0.12).abs() < 0.01);
            assert!((c.g - 0.12).abs() < 0.01);
            assert!((c.b - 0.18).abs() < 0.01);
            assert!((c.a - 1.0).abs() < 0.01);
        }
        other => panic!("expected Color, got {other:?}"),
    }
}

#[test]
fn ui_screen_connections_parsed() {
    let source = load_fixture("ui_screen.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    assert_eq!(scene.connection_count(), 3);

    let conns = scene.connections();
    assert_eq!(conns[0].signal_name, "pressed");
    assert_eq!(conns[0].from_path, "VBoxContainer/StartButton");
    assert_eq!(conns[0].to_path, ".");
    assert_eq!(conns[0].method_name, "_on_start_pressed");
}

#[test]
fn ui_screen_negative_offsets() {
    let source = load_fixture("ui_screen.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let vbox = nodes.iter().find(|n| n.name() == "VBoxContainer").unwrap();
    assert_eq!(vbox.get_property("offset_left"), Variant::Float(-200.0));
    assert_eq!(vbox.get_property("offset_top"), Variant::Float(-150.0));
}

// ---------------------------------------------------------------------------
// tilemap_level.tscn — Packed*Array types
// ---------------------------------------------------------------------------

#[test]
fn tilemap_level_parses_successfully() {
    let source = load_fixture("tilemap_level.tscn");
    let scene = PackedScene::from_tscn(&source).expect("should parse tilemap_level.tscn");
    assert!(scene.node_count() >= 8);
}

#[test]
fn tilemap_packed_byte_array_empty() {
    let source = load_fixture("tilemap_level.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let tilemap = nodes.iter().find(|n| n.name() == "TileMapLayer").unwrap();
    let data = tilemap.get_property("tile_map_data");
    assert_eq!(data, Variant::Array(vec![]));
}

#[test]
fn tilemap_packed_vector2_array() {
    let source = load_fixture("tilemap_level.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let spawns = nodes.iter().find(|n| n.name() == "SpawnPoints").unwrap();
    let positions = spawns.get_property("metadata/spawn_positions");
    match positions {
        Variant::Array(items) => {
            assert_eq!(items.len(), 3);
            // PackedVector2Array(64, 320, 512, 320, 256, 64) -> 3 Vector2s
            match &items[0] {
                Variant::Vector2(v) => {
                    assert_eq!(v.x, 64.0);
                    assert_eq!(v.y, 320.0);
                }
                other => panic!("expected Vector2, got {other:?}"),
            }
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn tilemap_packed_string_array() {
    let source = load_fixture("tilemap_level.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let spawns = nodes.iter().find(|n| n.name() == "SpawnPoints").unwrap();
    let types = spawns.get_property("metadata/enemy_types");
    match types {
        Variant::Array(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], Variant::String("slime".into()));
            assert_eq!(items[1], Variant::String("bat".into()));
            assert_eq!(items[2], Variant::String("skeleton".into()));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn tilemap_packed_float32_array() {
    let source = load_fixture("tilemap_level.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let bounds = nodes.iter().find(|n| n.name() == "Boundaries").unwrap();
    let level_bounds = bounds.get_property("metadata/level_bounds");
    match level_bounds {
        Variant::Array(items) => {
            assert_eq!(items.len(), 4);
            assert_eq!(items[0], Variant::Float(0.0));
            assert_eq!(items[3], Variant::Float(640.0));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn tilemap_packed_int32_array() {
    let source = load_fixture("tilemap_level.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let bounds = nodes.iter().find(|n| n.name() == "Boundaries").unwrap();
    let indices = bounds.get_property("metadata/tile_indices");
    match indices {
        Variant::Array(items) => {
            assert_eq!(items.len(), 10);
            assert_eq!(items[0], Variant::Int(0));
            assert_eq!(items[9], Variant::Int(9));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// physics_scene.tscn
// ---------------------------------------------------------------------------

#[test]
fn physics_scene_parses_successfully() {
    let source = load_fixture("physics_scene.tscn");
    let scene = PackedScene::from_tscn(&source).expect("should parse physics_scene.tscn");
    assert!(scene.node_count() >= 9);
}

#[test]
fn physics_scene_collision_layers() {
    let source = load_fixture("physics_scene.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let crate_node = nodes.iter().find(|n| n.name() == "DynamicCrate").unwrap();
    assert_eq!(crate_node.class_name(), "RigidBody2D");
    assert_eq!(crate_node.get_property("collision_layer"), Variant::Int(1));
    assert_eq!(crate_node.get_property("collision_mask"), Variant::Int(3));
    assert_eq!(crate_node.get_property("mass"), Variant::Float(5.0));
}

#[test]
fn physics_scene_null_property() {
    let source = load_fixture("physics_scene.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let ball = nodes.iter().find(|n| n.name() == "BouncyBall").unwrap();
    assert_eq!(ball.get_property("physics_material_override"), Variant::Nil);
}

#[test]
fn physics_scene_connections() {
    let source = load_fixture("physics_scene.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    assert_eq!(scene.connection_count(), 1);

    let conn = &scene.connections()[0];
    assert_eq!(conn.signal_name, "body_entered");
    assert_eq!(conn.from_path, "KillZone");
    assert_eq!(conn.method_name, "_on_kill_zone_body_entered");
}

#[test]
fn physics_scene_sub_resource_references() {
    let source = load_fixture("physics_scene.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    // Find first CollisionShape under DynamicCrate (index-based since multiple share the name)
    // DynamicCrate is at index 1, its CollisionShape child is at index 3
    let crate_idx = nodes
        .iter()
        .position(|n| n.name() == "DynamicCrate")
        .unwrap();
    let crate_id = nodes[crate_idx].id();
    let crate_collision = nodes
        .iter()
        .find(|n| n.name() == "CollisionShape" && n.parent() == Some(crate_id))
        .unwrap();
    let shape = crate_collision.get_property("shape");
    assert_eq!(
        shape,
        Variant::String("SubResource:RectangleShape2D_rect1".into())
    );
}

#[test]
fn physics_scene_bool_property() {
    let source = load_fixture("physics_scene.tscn");
    let scene = PackedScene::from_tscn(&source).unwrap();
    let nodes = scene.instance().unwrap();

    let kill_zone = nodes.iter().find(|n| n.name() == "KillZone").unwrap();
    assert_eq!(kill_zone.get_property("monitorable"), Variant::Bool(false));
}
