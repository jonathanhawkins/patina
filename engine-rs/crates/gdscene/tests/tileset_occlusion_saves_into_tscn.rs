//! pat-s3rrv.65: an authored tile occlusion layer survives a save and reload.
//!
//! The runtime consumer landed first, deliberately — `collect_occluders_2d` had
//! no tile path at all, so persisting occlusion before that existed would have
//! written data nothing could ever read. This is the other half.
//!
//! ⚠️ Formats were taken from the vendored C++ before any code was written,
//! after the navigation pass shipped the wrong shape three times:
//!   * the tile key is `occlusion_layer_<i>/polygon_<j>/polygon` — THREE
//!     components. The two-component form parses but is "Kept for
//!     compatibility" (`tile_set.cpp:6715`); Godot writes the indexed one.
//!   * its value is a `Ref<OccluderPolygon2D>` (`tile_set.cpp:6717`), so a
//!     SubResource reference — a raw array becomes a null Ref and is dropped
//!     without an error.
//!   * `OccluderPolygon2D` serialises `polygon` / `closed` / `cull_mode`
//!     (`scene/2d/light_occluder_2d.cpp:128-134`).

use gdcore::math::{Vector2, Vector2i};
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::tilemap::{
    encode_tile_map_data, tile_set_source_key, TileData, TILE_OCCLUDER_POLYGON,
    TILE_OCCLUDER_SOURCES, TILE_SET_SOURCE_IDS, TILE_SET_TILE_SIZE,
};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::collections::HashMap;

const TILE: i32 = 16;

fn authored_layer(occluder_polygon: Vec<Vector2>) -> (SceneTree, gdscene::NodeId) {
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
    node.set_property(TILE_OCCLUDER_SOURCES, Variant::PackedInt32Array(vec![0]));
    node.set_property(
        TILE_OCCLUDER_POLYGON,
        Variant::PackedVector2Array(occluder_polygon),
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

/// **The acceptance case.** Godot's real occlusion shape reaches the file.
#[test]
fn tile_occlusion_reaches_the_saved_tileset() {
    let (tree, root) = authored_layer(Vec::new());
    let saved = TscnSaver::save_tree(&tree, root);

    assert!(
        saved.contains("[sub_resource type=\"OccluderPolygon2D\""),
        "the geometry must live in its own OccluderPolygon2D sub-resource:\n{saved}"
    );
    assert!(
        saved.contains("closed = true"),
        "a tile is solid, so its occluder is a closed polygon:\n{saved}"
    );
    // ⚠️ THREE components, and a SubResource reference — the two ways the
    // navigation pass got this wrong.
    assert!(
        saved.contains("0:0/0/occlusion_layer_0/polygon_0/polygon = SubResource("),
        "the tile key is the indexed form Godot writes:\n{saved}"
    );
    assert!(
        saved.contains("occlusion_layer_0/light_mask = 1"),
        "Godot reads a tile's occluder only when the layer exists on the \
         TileSet:\n{saved}"
    );
    assert!(
        !saved.contains("__tile_occluder_"),
        "the internal companions must stay out of the file:\n{saved}"
    );
}

/// The polygon is re-expressed around the tile centre on the way out and back
/// to top-left on the way in.
///
/// ⚠️ Asserting only the round trip would pass with BOTH conversions inverted,
/// so the file's own literal is pinned too.
#[test]
fn the_polygon_is_rebased_in_both_directions() {
    let quadrant = vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(8.0, 0.0),
        Vector2::new(8.0, 8.0),
        Vector2::new(0.0, 8.0),
    ];
    let (tree, root) = authored_layer(quadrant.clone());
    let saved = TscnSaver::save_tree(&tree, root);

    assert!(
        saved.contains("polygon = PackedVector2Array(-8, -8, 0, -8, 0, 0, -8, 0)"),
        "centre-relative in the file; an unconverted polygon would read \
         `(0, 0, 8, 0, 8, 8, 0, 8)`:\n{saved}"
    );

    let (reloaded, layer) = reload(&saved);
    let node = reloaded.get_node(layer).expect("layer node");
    assert_eq!(
        node.get_property(TILE_OCCLUDER_SOURCES),
        Variant::PackedInt32Array(vec![0]),
        "the occlusion source list must be rebuilt from the file"
    );
    assert_eq!(
        node.get_property(TILE_OCCLUDER_POLYGON),
        Variant::PackedVector2Array(quadrant),
        "and the polygon must come back top-left relative"
    );
}

/// The whole-tile default stays implicit, so an untouched tile does not look
/// edited on the next save.
#[test]
fn the_default_polygon_stays_implicit() {
    let (tree, root) = authored_layer(Vec::new());
    let saved = TscnSaver::save_tree(&tree, root);
    let (reloaded, layer) = reload(&saved);
    let node = reloaded.get_node(layer).expect("layer node");

    assert_eq!(
        node.get_property(TILE_OCCLUDER_SOURCES),
        Variant::PackedInt32Array(vec![0]),
        "occlusion is still enabled"
    );
    assert_eq!(node.get_property(TILE_OCCLUDER_POLYGON), Variant::Nil);
}

/// A layer with no occlusion sources writes no occlusion keys at all.
#[test]
fn a_layer_without_occlusion_declares_none() {
    let (mut tree, root) = authored_layer(Vec::new());
    let layer = tree.find_child(root, "Ground", true).expect("layer exists");
    tree.get_node_mut(layer)
        .unwrap()
        .set_property(TILE_OCCLUDER_SOURCES, Variant::PackedInt32Array(Vec::new()));

    let saved = TscnSaver::save_tree(&tree, root);
    assert!(
        !saved.contains("occlusion_layer_0"),
        "no occlusion source means no occlusion keys:\n{saved}"
    );
}

/// ⚠️ The bug this file's first run caught, plus the latent one beside it.
///
/// Occlusion keys were written only inside the loop over PHYSICS tiles, so a
/// tile that occludes without being solid — the ordinary case — was never
/// declared and its keys never written. The reload then reported no occlusion
/// sources at all.
///
/// The fix walks the UNION of the navigation and occlusion tiles, which also
/// closes a latent bug on the navigation side: a tile that is both walkable and
/// occluding would have had its `<ax>:<ay>/0 = 0` declaration written twice.
#[test]
fn a_tile_with_both_layers_is_declared_exactly_once() {
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
    // Both layers on the same tile, and no collision anywhere.
    node.set_property(TILE_OCCLUDER_SOURCES, Variant::PackedInt32Array(vec![0]));
    node.set_property(
        gdscene::tilemap::TILE_NAV_SOURCES,
        Variant::PackedInt32Array(vec![0]),
    );

    let saved = TscnSaver::save_tree(&tree, root);

    assert_eq!(
        saved.matches("0:0/0 = 0\n").count(),
        1,
        "the tile must be declared exactly once, not once per layer:\n{saved}"
    );
    assert!(
        saved.contains("0:0/0/occlusion_layer_0/polygon_0/polygon = SubResource("),
        "the occluding tile needs its key even with no collision:\n{saved}"
    );
    assert!(
        saved.contains("0:0/0/navigation_layer_0/polygon = SubResource("),
        "and the navigation key alongside it:\n{saved}"
    );
}
