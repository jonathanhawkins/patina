//! pat-0q2kx.117: painted tiles become static collision bodies.
//!
//! Godot's `TileMapLayer::_physics_update`
//! (upstream/godot/scene/2d/tile_map_layer.cpp) builds a static body per painted
//! cell out of the tile's physics layers, and frees it when the cell is erased.
//! That is what makes a character walk on painted ground.
//!
//! Patina had the conversion — `gdphysics2d::tilemap_collision::tilemap_to_physics`,
//! unit-tested — but **nothing called it**: its only reference outside its own
//! module was the `lib.rs` re-export, and `PhysicsServer::register_bodies`
//! handled no `TileMap`/`TileMapLayer` class at all. A `.tscn` tilemap therefore
//! contributed zero collision and characters fell straight through the floor.
//!
//! These drive the real `MainLoop` physics phase and the real `PackedScene`
//! loader. The cells are encoded with the shipping `encode_tile_map_data` codec,
//! so the node carries exactly the bytes a loaded scene would.

use std::collections::HashMap;

use gdcore::math::{Vector2, Vector2i};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::tilemap::{
    encode_tile_map_data, tile_physics_one_way_key, tile_physics_points_key, TileData,
    TILE_PHYSICS_SOURCES,
};
use gdscene::SceneTree;
use gdvariant::Variant;

const TILE: i32 = 16;
/// The tile source carrying collision; `BARE_SOURCE` is painted but collides
/// with nothing, so "a body exists per painted cell" cannot pass by accident.
const SOLID_SOURCE: i32 = 1;
const BARE_SOURCE: i32 = 2;
/// Ground row: cells at y = 2 span world y in [32, 48], so their top edge is 32.
const GROUND_ROW: i32 = 2;
const GROUND_TOP: f32 = (GROUND_ROW * TILE) as f32;
/// The character is 16x16, so resting on the row puts its centre 8px above it.
const RESTING_Y: f32 = GROUND_TOP - 8.0;

/// Encodes a row of cells at [`GROUND_ROW`], each from `source`.
fn ground_row(xs: &[i32], source: i32) -> Vec<u8> {
    let mut painted: HashMap<Vector2i, TileData> = HashMap::new();
    for x in xs {
        painted.insert(Vector2i::new(*x, GROUND_ROW), TileData::new(source));
    }
    encode_tile_map_data(&painted)
}

/// `root -> Layer (TileMapLayer) + Walker (CharacterBody2D falling onto it)`.
fn tilemap_scene(cells: Vec<u8>, physics_sources: Vec<i32>) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut layer = Node::new("Layer", "TileMapLayer");
    layer.set_property("tile_map_data", Variant::PackedByteArray(cells));
    layer.set_property("tile_size", Variant::Vector2i(Vector2i::new(TILE, TILE)));
    layer.set_property(
        TILE_PHYSICS_SOURCES,
        Variant::PackedInt32Array(physics_sources),
    );
    // A full-cell collision polygon, authored the way Godot writes one: in
    // tile-local pixels about the tile centre.
    layer.set_property(
        &tile_physics_points_key(SOLID_SOURCE, Vector2i::ZERO),
        Variant::PackedVector2Array(vec![
            Vector2::new(-8.0, -8.0),
            Vector2::new(8.0, -8.0),
            Vector2::new(8.0, 8.0),
            Vector2::new(-8.0, 8.0),
        ]),
    );
    layer.set_property(
        &tile_physics_one_way_key(SOLID_SOURCE, Vector2i::ZERO),
        Variant::Bool(false),
    );
    let layer_id = tree.add_child(root, layer).unwrap();

    // Starts above the middle of cell (1, GROUND_ROW) and walks downward.
    let mut walker = Node::new("Walker", "CharacterBody2D");
    walker.set_property("position", Variant::Vector2(Vector2::new(24.0, 8.0)));
    walker.set_property("velocity", Variant::Vector2(Vector2::new(0.0, 200.0)));
    let walker_id = tree.add_child(root, walker).unwrap();

    let mut shape = Node::new("WalkerShape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_owned()));
    shape.set_property("size", Variant::Vector2(Vector2::new(16.0, 16.0)));
    tree.add_child(walker_id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, layer_id, walker_id)
}

fn walker_y(ml: &MainLoop, walker: NodeId) -> f32 {
    gdscene::node2d::get_position(ml.tree(), walker).y
}

fn run(ml: &mut MainLoop, frames: usize) {
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
    }
}

/// **The acceptance test.** A character falls onto painted tiles and rests on
/// the row; erasing the cells drops it through.
#[test]
fn tilemap_tiles_provide_static_collision() {
    let (mut ml, layer, walker) =
        tilemap_scene(ground_row(&[0, 1, 2], SOLID_SOURCE), vec![SOLID_SOURCE]);

    run(&mut ml, 60);
    let landed = walker_y(&ml, walker);

    assert!(
        landed > 8.0,
        "the walker must actually descend from its start, got {landed}",
    );
    assert!(
        landed < GROUND_TOP,
        "the walker must stop at the painted row, not pass through it: y {landed} \
         is at or below the tile top {GROUND_TOP}",
    );
    assert!(
        (landed - RESTING_Y).abs() < TILE as f32,
        "the walker must rest within one cell of the row top ({RESTING_Y}), got {landed}",
    );

    // Erasing the cells frees their bodies, exactly as Godot frees the physics
    // quadrant — the walker now has nothing to stand on.
    ml.tree_mut()
        .get_node_mut(layer)
        .expect("layer still in tree")
        .set_property("tile_map_data", Variant::PackedByteArray(Vec::new()));
    ml.register_physics_bodies();

    // Re-arm the walker's velocity, which is what a `_physics_process` does on
    // every frame of a real game.
    //
    // This is NOT a workaround for a Patina bug: a `CharacterBody2D` gets no
    // automatic gravity in Godot either — `character_body_2d.cpp` mentions
    // gravity exactly once, in a comment, and CharacterBody2D.xml not at all.
    // The script owns `velocity += get_gravity() * delta`, and `move_and_slide`
    // zeroes it on floor contact (mirrored here at gdphysics2d
    // `character.rs:466/:485`). Landing therefore left this unscripted walker at
    // zero velocity, and the auto-move pass skips idle bodies — so without this
    // line the test would be demanding that the engine own gravity, which would
    // be a deliberate parity REGRESSION if someone "fixed" it in the engine.
    //
    // The assertion below keeps all of its teeth: if erasing the cells had not
    // actually freed the tile bodies, a re-armed walker would still be stopped
    // at the row and `fell` would not clear `GROUND_TOP + TILE`.
    ml.tree_mut()
        .get_node_mut(walker)
        .expect("walker still in tree")
        .set_property("velocity", Variant::Vector2(Vector2::new(0.0, 200.0)));

    run(&mut ml, 60);
    let fell = walker_y(&ml, walker);
    assert!(
        fell > GROUND_TOP + TILE as f32,
        "after erasing the cells the walker must fall past the old row, got {fell}",
    );
}

/// Painting a source with no collision must produce no bodies — otherwise the
/// test above would pass for any painted cell whatsoever.
#[test]
fn tiles_without_collision_do_not_block() {
    let (mut ml, _layer, walker) =
        tilemap_scene(ground_row(&[0, 1, 2], BARE_SOURCE), vec![SOLID_SOURCE]);

    run(&mut ml, 60);
    let y = walker_y(&ml, walker);
    assert!(
        y > GROUND_TOP + TILE as f32,
        "cells painted from a source with no collision must not block the walker, \
         but it stopped at {y}",
    );
}

/// A `.tscn`-authored `TileSet` physics layer reaches the node.
///
/// Godot writes tile collision on the `TileSetAtlasSource`, one key per tile per
/// physics layer (`<ax>:<ay>/<alt>/physics_layer_0/polygon_0/points`). Without
/// flattening those onto the node the physics server has no geometry to build
/// from and every authored tile collider is silently lost.
const TILESET_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_1"]
texture_region_size = Vector2i(16, 16)
0:0/0 = 0
0:0/0/physics_layer_0/polygon_0/points = PackedVector2Array(-8, -8, 8, -8, 8, 8, -8, 8)
0:0/0/physics_layer_0/polygon_0/one_way = true

[sub_resource type="TileSet" id="TileSet_1"]
tile_size = Vector2i(16, 16)
physics_layer_0/collision_layer = 1
sources/1 = SubResource("TileSetAtlasSource_1")

[node name="Main" type="Node2D"]

[node name="Layer" type="TileMapLayer" parent="."]
tile_set = SubResource("TileSet_1")
"#;

#[test]
fn tscn_tileset_physics_layer_flattens_onto_the_node() {
    let scene = PackedScene::from_tscn(TILESET_TSCN).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let layer = tree
        .get_node_or_null(scene_root, "Layer")
        .expect("Layer instanced");
    let node = tree.get_node(layer).expect("layer node");

    assert_eq!(
        node.get_property(TILE_PHYSICS_SOURCES),
        Variant::PackedInt32Array(vec![SOLID_SOURCE]),
        "the atlas source carrying a physics polygon must be listed as a collision source",
    );

    let points = node.get_property(&tile_physics_points_key(SOLID_SOURCE, Vector2i::ZERO));
    assert_eq!(
        points,
        Variant::PackedVector2Array(vec![
            Vector2::new(-8.0, -8.0),
            Vector2::new(8.0, -8.0),
            Vector2::new(8.0, 8.0),
            Vector2::new(-8.0, 8.0),
        ]),
        "the tile's authored collision polygon must reach the node verbatim",
    );

    assert_eq!(
        node.get_property(&tile_physics_one_way_key(SOLID_SOURCE, Vector2i::ZERO)),
        Variant::Bool(true),
        "the tile's one_way_collision flag must reach the node",
    );
}
