//! pat-s3rrv.65: a custom data layer's declared TYPE survives a round trip.
//!
//! # What was wrong
//!
//! `custom_data_layer_<i>/name` round-tripped; `custom_data_layer_<i>/type` did
//! not. The loader read only the name, so the saver could only write the name
//! back — a `.tscn` authored in real Godot came out of Patina with every custom
//! data layer's type declaration stripped, and Godot reopening it saw untyped
//! layers.
//!
//! # ⚠️ Why the fix PRESERVES rather than infers
//!
//! The saver's existing comment is right and is deliberately kept: a **guessed**
//! type is worse than an absent one, because it silently coerces the value on
//! load whereas an absent type leaves the Variant as authored. So nothing here
//! derives a type from a value. The type is carried through only when the file
//! itself declared one, and an editor-authored TileSet still emits no `/type`
//! at all — which `an_untyped_layer_still_emits_no_type` pins, because that is
//! the property a "just write the value's type" implementation would break.
//!
//! Godot's values are `Variant::Type` ordinals (`tile_set.cpp:4211`): `2` is
//! INT and `4` is STRING, matching the two values the fixture authors.

use gdcore::math::Vector2i;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::tilemap::{
    encode_tile_map_data, tile_custom_data_key, tile_set_source_key, TileData,
    TILE_CUSTOM_DATA_LAYERS, TILE_CUSTOM_DATA_TYPES, TILE_SET_SOURCE_IDS, TILE_SET_TILE_SIZE,
};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::collections::HashMap;

const TILE: i32 = 16;
/// `Variant::INT` / `Variant::STRING` ordinals, as Godot writes them.
const TYPE_INT: i64 = 2;
const TYPE_STRING: i64 = 4;

/// Two layers with one painted tile, exactly as the editor authors them —
/// **no type declarations**, because the editor has none to give.
///
/// Two layers, not one: a single layer passes even if the index is hardcoded to
/// 0, and the whole question here is whether index, name and type stay paired.
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

/// A `.tscn` as **real Godot** writes it: the same scene, plus the `/type`
/// declarations Patina has never emitted.
///
/// ⚠️ Built by injecting into the saver's own output rather than hand-authoring
/// a whole `.tscn`. Hand-authoring risks the fixture failing to parse for some
/// unrelated reason, which would look exactly like the feature being broken;
/// this way everything around the injected lines is known-good text.
fn godot_authored_tscn(types: [Option<i64>; 2]) -> String {
    let (tree, root) = authored_layer();
    let mut saved = TscnSaver::save_tree(&tree, root);

    for (index, name, ty) in [(0usize, "damage", types[0]), (1usize, "surface", types[1])] {
        let Some(ty) = ty else { continue };
        let decl = format!("custom_data_layer_{index}/name = \"{name}\"");
        assert!(
            saved.contains(&decl),
            "fixture precondition: the saver must declare `{decl}` before this \
             test can inject a type beside it:\n{saved}"
        );
        saved = saved.replace(
            &decl,
            &format!("{decl}\ncustom_data_layer_{index}/type = {ty}"),
        );
    }
    saved
}

/// Returns the tree, the instantiated scene ROOT and the layer.
///
/// The root is handed back rather than searched for by name: re-saving needs
/// it, and `add_packed_scene_to_tree` already knows exactly which node it is.
fn reload(saved: &str) -> (SceneTree, gdscene::NodeId, gdscene::NodeId) {
    let scene = PackedScene::from_tscn(saved).expect("the saved scene must parse");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let new_root = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");
    let layer = tree
        .find_child(new_root, "Ground", true)
        .expect("the layer must survive");
    (tree, new_root, layer)
}

/// Loads a Godot-authored scene and re-saves it, which is the full path the bug
/// lived on.
fn round_trip(types: [Option<i64>; 2]) -> String {
    let (reloaded, root, _layer) = reload(&godot_authored_tscn(types));
    TscnSaver::save_tree(&reloaded, root)
}

/// **The regression.** A declared type survives load → save.
#[test]
fn a_declared_layer_type_survives_the_round_trip() {
    let out = round_trip([Some(TYPE_INT), Some(TYPE_STRING)]);

    assert!(
        out.contains(&format!("custom_data_layer_0/type = {TYPE_INT}")),
        "the first layer's declared type was dropped on the round trip — the \
         loader read only `/name`, so the saver could only write `/name` \
         back:\n{out}"
    );
    assert!(
        out.contains(&format!("custom_data_layer_1/type = {TYPE_STRING}")),
        "the second layer's type must survive too, with its own value:\n{out}"
    );
    // The names must still be there — a "fix" that replaced them would pass
    // every type assertion above.
    assert!(
        out.contains("custom_data_layer_0/name = \"damage\"")
            && out.contains("custom_data_layer_1/name = \"surface\""),
        "the names must still be declared alongside the types:\n{out}"
    );
}

/// The companion the loader builds, asserted directly so a failure says which
/// half of the round trip broke.
#[test]
fn the_loader_records_each_declared_type_in_index_order() {
    let (reloaded, _root, layer) =
        reload(&godot_authored_tscn([Some(TYPE_INT), Some(TYPE_STRING)]));
    let node = reloaded.get_node(layer).expect("layer node");

    assert_eq!(
        node.get_property(TILE_CUSTOM_DATA_TYPES),
        Variant::PackedInt32Array(vec![TYPE_INT as i32, TYPE_STRING as i32]),
        "the types must come back parallel to the names and in index order"
    );
    assert_eq!(
        node.get_property(TILE_CUSTOM_DATA_LAYERS),
        Variant::PackedStringArray(vec!["damage".to_string(), "surface".to_string()]),
        "the names must be unaffected by carrying the types"
    );
}

/// ⚠️ **The no-guess guard.** An editor-authored TileSet has no declared type,
/// and must still emit none — even though its values are obviously an Int and a
/// String. This is the assertion that fails on a "just write the value's type"
/// implementation, which would silently coerce on load.
#[test]
fn an_untyped_layer_still_emits_no_type() {
    let (tree, root) = authored_layer();
    let saved = TscnSaver::save_tree(&tree, root);

    assert!(
        saved.contains("custom_data_layer_0/name = \"damage\""),
        "precondition: the layer must be declared at all:\n{saved}"
    );
    assert!(
        !saved.contains("custom_data_layer_0/type"),
        "no type was ever declared for this layer, so none may be written — a \
         guessed type silently coerces the value on load, while an absent one \
         leaves the Variant as authored:\n{saved}"
    );
    assert!(
        !saved.contains("__tile_custom_data_types__"),
        "the types companion must stay out of the file like every other \
         companion:\n{saved}"
    );
}

/// ⚠️ Partial typing. Godot writes both keys together, but a hand-edited file
/// may not — and an untyped layer sitting BEFORE a typed one is what shakes out
/// an implementation that walks the two arrays with independent counters.
#[test]
fn an_untyped_layer_does_not_shift_a_later_layers_type() {
    let (reloaded, _root, layer) = reload(&godot_authored_tscn([None, Some(TYPE_STRING)]));
    let node = reloaded.get_node(layer).expect("layer node");

    assert_eq!(
        node.get_property(TILE_CUSTOM_DATA_TYPES),
        Variant::PackedInt32Array(vec![-1, TYPE_STRING as i32]),
        "the untyped layer must hold the -1 placeholder rather than collapsing \
         the array and shifting index 1's type onto index 0"
    );

    let out = round_trip([None, Some(TYPE_STRING)]);
    assert!(
        !out.contains("custom_data_layer_0/type"),
        "the untyped layer must not acquire a type:\n{out}"
    );
    assert!(
        out.contains(&format!("custom_data_layer_1/type = {TYPE_STRING}")),
        "the typed layer must keep its own type at its own index:\n{out}"
    );
}
