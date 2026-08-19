//! pat-s3rrv.65: per-tile custom data survives a save and reload.
//!
//! # What was wrong
//!
//! Custom data was **write-only**. The editor authors `__tile_custom_data_*`
//! companions, `is_internal_prop` correctly keeps those caches out of the saved
//! file — and, unlike the physics, navigation and occlusion families, nothing
//! regenerated them as real Godot keys. `grep custom_data` returned **0** in
//! both `packed_scene.rs` and `scene_saver.rs`.
//!
//! So a user set per-tile custom data in the editor, saved, reloaded, and it was
//! gone; and a `.tscn` authored in real Godot lost it on load. Silently, in both
//! directions.
//!
//! # Formats, taken from the vendored C++ before writing any code
//!
//! ⚠️ The two key shapes are NOT the same prefix, which is the trap:
//!   * the TileSet declares `custom_data_layer_<i>/name` — a `/` component
//!     (`tile_set.cpp:4211`, and `ADD_ARRAY("custom_data_layers",
//!     "custom_data_layer_")` at `:4378`);
//!   * a tile stores `<x>:<y>/<alt>/custom_data_<i>` — ONE component, no
//!     `layer` word (`tile_set.cpp:6848`).
//!
//! Patina's companion is keyed by layer NAME, Godot's file by layer INDEX, so
//! the declaration order is the mapping between them.
//!
//! ⚠️ The file's own literal text is pinned below, not just the round trip. A
//! round trip proves the writer and reader AGREE — it stays green with both
//! sides inverted, which is exactly how the navigation pass shipped the wrong
//! shape three times.

use gdcore::math::Vector2i;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::tilemap::{
    encode_tile_map_data, tile_custom_data_key, tile_set_source_key, TileData,
    TILE_CUSTOM_DATA_LAYERS, TILE_SET_SOURCE_IDS, TILE_SET_TILE_SIZE,
};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::collections::HashMap;

const TILE: i32 = 16;

/// A layer with one painted tile carrying two custom data values.
///
/// Two layers, not one: a single layer passes even if the index is hardcoded to
/// 0, and the whole mapping question is whether index and name stay paired.
fn authored_layer() -> (SceneTree, gdscene::NodeId) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Main", "Node2D"))
        .unwrap();
    let layer = tree
        .add_child(root, Node::new("Ground", "TileMapLayer"))
        .unwrap();

    let mut cells: HashMap<Vector2i, TileData> = HashMap::new();
    cells.insert(
        Vector2i::new(0, 0),
        TileData::with_atlas(0, Vector2i::new(0, 0), 0),
    );

    let node = tree.get_node_mut(layer).unwrap();
    node.set_property(TILE_SET_SOURCE_IDS, Variant::PackedInt32Array(vec![0]));
    node.set_property(
        TILE_SET_TILE_SIZE,
        Variant::Vector2i(Vector2i::new(TILE, TILE)),
    );
    node.set_property(
        &tile_set_source_key(0, "texture"),
        Variant::String("res://atlas.png".to_string()),
    );
    node.set_property(
        "tile_map_data",
        Variant::PackedByteArray(encode_tile_map_data(&cells)),
    );
    node.set_property(
        TILE_CUSTOM_DATA_LAYERS,
        Variant::PackedStringArray(vec!["damage".to_string(), "surface".to_string()]),
    );
    node.set_property(
        &tile_custom_data_key(0, Vector2i::new(0, 0), "damage"),
        Variant::Int(7),
    );
    node.set_property(
        &tile_custom_data_key(0, Vector2i::new(0, 0), "surface"),
        Variant::String("ice".to_string()),
    );

    (tree, root)
}

fn reload(saved: &str) -> (SceneTree, gdscene::NodeId) {
    let scene = PackedScene::from_tscn(saved).expect("the saved scene must parse");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let new_root = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");
    let layer = tree
        .find_child(new_root, "Ground", true)
        .expect("the layer must survive");
    (tree, layer)
}

/// **The file shape.** Godot's real keys reach the `.tscn`.
#[test]
fn custom_data_reaches_the_saved_tileset() {
    let (tree, root) = authored_layer();
    let saved = TscnSaver::save_tree(&tree, root);

    assert!(
        saved.contains("custom_data_layer_0/name = \"damage\""),
        "the TileSet must declare the layer, or Godot has nothing for a \
         `custom_data_0` key to belong to:\n{saved}"
    );
    assert!(
        saved.contains("custom_data_layer_1/name = \"surface\""),
        "both layers must be declared, in order:\n{saved}"
    );
    // ⚠️ ONE component — `custom_data_0`, not `custom_data_layer_0/...`. The
    // tile-level and TileSet-level spellings differ.
    assert!(
        saved.contains("0:0/0/custom_data_0 = 7"),
        "the tile's value is keyed by layer INDEX with a single \
         component:\n{saved}"
    );
    assert!(
        saved.contains("0:0/0/custom_data_1 = \"ice\""),
        "and the second layer's value maps to index 1:\n{saved}"
    );
    assert!(
        !saved.contains("__tile_custom_data_"),
        "the internal companions must stay out of the file:\n{saved}"
    );
}

/// **The round trip.** What the editor authored comes back after a reload.
#[test]
fn custom_data_survives_the_round_trip() {
    let (tree, root) = authored_layer();
    let saved = TscnSaver::save_tree(&tree, root);
    let (reloaded, layer) = reload(&saved);
    let node = reloaded.get_node(layer).expect("layer node");

    assert_eq!(
        node.get_property(TILE_CUSTOM_DATA_LAYERS),
        Variant::PackedStringArray(vec!["damage".to_string(), "surface".to_string()]),
        "the layer names must be rebuilt from custom_data_layer_<i>/name"
    );
    assert_eq!(
        node.get_property(&tile_custom_data_key(0, Vector2i::new(0, 0), "damage")),
        Variant::Int(7),
        "the tile's value must come back under its layer NAME — this is the \
         data that was silently discarded on every save"
    );
    assert_eq!(
        node.get_property(&tile_custom_data_key(0, Vector2i::new(0, 0), "surface")),
        Variant::String("ice".to_string()),
        "and the second layer must not be confused with the first"
    );
}

/// ⚠️ The mapping guard. Both values are asserted on the SECOND layer's name,
/// so an implementation that hardcodes index 0, or that pairs index with the
/// wrong name, fails here while passing a single-layer fixture.
#[test]
fn the_layer_index_and_name_stay_paired() {
    let (tree, root) = authored_layer();
    let saved = TscnSaver::save_tree(&tree, root);
    let (reloaded, layer) = reload(&saved);
    let node = reloaded.get_node(layer).expect("layer node");

    assert_ne!(
        node.get_property(&tile_custom_data_key(0, Vector2i::new(0, 0), "surface")),
        Variant::Int(7),
        "`surface` must not carry `damage`'s value — that is index 0 leaking \
         into every layer"
    );
    assert_eq!(
        node.get_property(&tile_custom_data_key(0, Vector2i::new(0, 0), "missing")),
        Variant::Nil,
        "a layer that was never declared has no value at all"
    );
}
