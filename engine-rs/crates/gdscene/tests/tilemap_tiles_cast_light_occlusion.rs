//! pat-s3rrv.65: painted tiles cast 2D shadows.
//!
//! # Why this had to come before any authoring surface
//!
//! `collect_occluders_2d` only ever walked nodes whose class is literally
//! `LightOccluder2D`. There was **no tile occlusion consumer anywhere in the
//! engine** — so building the occlusion half of the TileSet editor the way
//! navigation was built (companion → saver → loader → route) would have produced
//! data that round-trips through the `.tscn` perfectly and lights nothing. Every
//! one of those pieces would have looked delivered and been inert.
//!
//! So the runtime path is first, and the authoring surface can follow knowing it
//! drives something real.

use gdcore::math::{Vector2, Vector2i};
use gdscene::light2d_scene::collect_occluders_2d;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::tilemap::{
    encode_tile_map_data, TileData, TILE_OCCLUDER_POLYGON, TILE_OCCLUDER_SOURCES,
    TILE_SET_TILE_SIZE,
};
use gdvariant::Variant;
use std::collections::HashMap;

const TILE: i32 = 16;
const SOURCE: i32 = 0;

/// A layer with two painted cells, occlusion enabled on their source.
fn occluding_layer(local_polygon: Vec<Vector2>, parent_position: Option<Vector2>) -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let holder = if let Some(pos) = parent_position {
        let mut n = Node::new("Holder", "Node2D");
        n.set_property("position", Variant::Vector2(pos));
        tree.add_child(root, n).unwrap()
    } else {
        root
    };

    let mut cells: HashMap<Vector2i, TileData> = HashMap::new();
    cells.insert(Vector2i::new(0, 0), TileData::new(SOURCE));
    cells.insert(Vector2i::new(1, 0), TileData::new(SOURCE));

    let mut layer = Node::new("Ground", "TileMapLayer");
    layer.set_property(
        "tile_map_data",
        Variant::PackedByteArray(encode_tile_map_data(&cells)),
    );
    layer.set_property(
        TILE_SET_TILE_SIZE,
        Variant::Vector2i(Vector2i::new(TILE, TILE)),
    );
    layer.set_property(
        TILE_OCCLUDER_SOURCES,
        Variant::PackedInt32Array(vec![SOURCE]),
    );
    layer.set_property(
        TILE_OCCLUDER_POLYGON,
        Variant::PackedVector2Array(local_polygon),
    );
    tree.add_child(holder, layer).unwrap();
    tree
}

/// **The acceptance case.** One occluder per painted cell, covering the tile.
#[test]
fn painted_tiles_produce_occluders() {
    let tree = occluding_layer(Vec::new(), None);
    let occluders = collect_occluders_2d(&tree);

    assert_eq!(
        occluders.len(),
        2,
        "one occluder per painted, occlusion-enabled cell"
    );
    // Cell (0,0) covers the tile's own rect: an empty polygon means "the whole
    // tile", which is Godot's default shape.
    assert_eq!(occluders[0].points[0], Vector2::new(0.0, 0.0));
    assert_eq!(occluders[0].points[2], Vector2::new(16.0, 16.0));
    // Cell (1,0) is one tile to the right, NOT stacked on the first.
    assert_eq!(occluders[1].points[0], Vector2::new(16.0, 0.0));
    assert!(
        occluders[0].closed,
        "a tile is solid, so its occluder is closed — an open polyline leaves \
         the far edge lit through, which reads as a shadow with a missing side"
    );
}

/// A layer with no occlusion sources contributes nothing — the discriminating
/// case, since "two occluders" would also be satisfied by code that ignored the
/// source list entirely.
#[test]
fn a_layer_without_occlusion_contributes_nothing() {
    let mut tree = occluding_layer(Vec::new(), None);
    let layer = tree.find_child(tree.root_id(), "Ground", true).unwrap();
    tree.get_node_mut(layer)
        .unwrap()
        .set_property(TILE_OCCLUDER_SOURCES, Variant::PackedInt32Array(Vec::new()));

    assert!(
        collect_occluders_2d(&tree).is_empty(),
        "occlusion keys off the source list; an empty list means no shadows"
    );
}

/// ⚠️ World space, via the node's global transform — not layer-local.
///
/// A tilemap parented under a moving `Node2D` has to cast its shadows where the
/// parent put it. Emitting layer-local points would light correctly only while
/// the parent sat at the origin, which is exactly the bug
/// `light2d_occluder_subresource_shades_scene` records for the
/// `LightOccluder2D` path.
#[test]
fn occluders_are_positioned_by_the_parents_transform() {
    let tree = occluding_layer(Vec::new(), Some(Vector2::new(100.0, 40.0)));
    let occluders = collect_occluders_2d(&tree);

    assert_eq!(occluders.len(), 2);
    assert_eq!(
        occluders[0].points[0],
        Vector2::new(100.0, 40.0),
        "the first tile's corner must be offset by the parent's position"
    );
}

/// A custom tile-local polygon is top-left relative, the same convention the
/// navigation polygon uses — and the opposite of the tile COLLISION polygon,
/// which is centred.
#[test]
fn a_custom_polygon_is_tile_local_from_the_top_left() {
    let left_half = vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(8.0, 0.0),
        Vector2::new(8.0, 16.0),
        Vector2::new(0.0, 16.0),
    ];
    let tree = occluding_layer(left_half, None);
    let occluders = collect_occluders_2d(&tree);

    assert_eq!(occluders[0].points[1], Vector2::new(8.0, 0.0));
    // The second cell's copy is offset by exactly one tile, so the polygon is
    // applied per cell rather than once globally.
    assert_eq!(occluders[1].points[1], Vector2::new(24.0, 0.0));
}
