//! pat-s3rrv.65: an authored tile navigation layer survives a save.
//!
//! Stage 1 of the TileSet editor rebuilt the `TileSet` sub-resources for
//! collision and custom data, but navigation was explicitly not delivered: the
//! `__tile_nav_*` companions are filtered out of the node section (they are
//! property names Godot has never heard of) and nothing wrote them back as
//! TileSet keys — so enabling navigation on a tile survived in memory and was
//! dropped the moment the scene was saved.
//!
//! ⚠️ This is NOT "the collision path with a different key name". The two
//! polygon families use different origins inside Patina — see
//! `nav_polygon_is_converted_to_godots_tile_centre_origin` below.

use gdcore::math::{Vector2, Vector2i};
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::tilemap::{
    encode_tile_map_data, tile_set_source_key, TileData, TILE_NAV_POLYGON, TILE_NAV_SOURCES,
    TILE_SET_SOURCE_IDS, TILE_SET_TILE_SIZE,
};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::collections::HashMap;

const TILE: i32 = 16;

/// A TileMapLayer with one atlas source, two painted cells, and navigation
/// enabled on that source — the shape the editor's TileSet panel produces.
///
/// `nav_polygon` is the tile-local polygon; empty means "the whole tile", which
/// is Godot's default and the common authoring case.
fn authored_layer(nav_polygon: Vec<Vector2>) -> (SceneTree, gdscene::NodeId) {
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
    cells.insert(
        Vector2i::new(1, 0),
        TileData::with_atlas(0, Vector2i::new(2, 1), 0),
    );

    let node = tree.get_node_mut(layer).unwrap();
    node.set_property(TILE_SET_SOURCE_IDS, Variant::PackedInt32Array(vec![0]));
    node.set_property(
        TILE_SET_TILE_SIZE,
        Variant::Vector2i(Vector2i::new(TILE, TILE)),
    );
    // ⚠️ Built with the helper rather than spelled out: the key is
    // `__tile_set__source_<id>_<suffix>`, and a hand-written
    // `__tile_set__0_texture` is silently ignored by the saver — the source
    // would be skipped and every assertion below would fail against an empty
    // TileSet.
    node.set_property(
        &tile_set_source_key(0, "texture"),
        Variant::String("res://atlas.png".to_string()),
    );
    node.set_property(
        &tile_set_source_key(0, "texture_region_size"),
        Variant::Vector2i(Vector2i::new(TILE, TILE)),
    );
    node.set_property(
        "tile_map_data",
        Variant::PackedByteArray(encode_tile_map_data(&cells)),
    );
    node.set_property(TILE_NAV_SOURCES, Variant::PackedInt32Array(vec![0]));
    node.set_property(TILE_NAV_POLYGON, Variant::PackedVector2Array(nav_polygon));

    (tree, root)
}

/// **The acceptance case.** The saved file carries Godot's real navigation keys.
#[test]
fn tile_navigation_reaches_the_saved_tileset() {
    let (tree, root) = authored_layer(Vec::new());
    let saved = TscnSaver::save_tree(&tree, root);

    // ⚠️ Godot reads a tile's `navigation_layer_0/polygon` only when the layer
    // EXISTS on the TileSet. Without this line the polygons are written and
    // then ignored on load — wired but inert, in the file format.
    assert!(
        saved.contains("navigation_layer_0/layers = 1"),
        "the TileSet must declare the navigation layer:\n{saved}"
    );

    // ⚠️ A SubResource REFERENCE, not an inline array. Godot's `TileData::_set`
    // does `Ref<NavigationPolygon> polygon = p_value` (tile_set.cpp:6824): a
    // PackedVector2Array converts to a NULL Ref, so an inline array loads
    // without error and drops the geometry. This engine's own round trip cannot
    // catch that — our loader read back whatever our saver wrote — so the shape
    // is pinned here explicitly.
    assert!(
        saved.contains("[sub_resource type=\"NavigationPolygon\""),
        "the geometry must live in its own NavigationPolygon sub-resource:\n{saved}"
    );
    assert!(
        saved.contains("vertices = PackedVector2Array("),
        "the sub-resource carries the points as `vertices`:\n{saved}"
    );
    assert!(
        saved.contains("polygons = [PackedInt32Array("),
        "a NavigationPolygon without `polygons` has vertices and no surface:\n{saved}"
    );

    // Both painted tiles, not just the first.
    for coord in ["0:0", "2:1"] {
        assert!(
            saved.contains(&format!(
                "{coord}/0/navigation_layer_0/polygon = SubResource("
            )),
            "tile {coord} is painted with a navigation-enabled source, so it \
             must carry a polygon:\n{saved}"
        );
        assert!(
            saved.contains(&format!("{coord}/0 = 0")),
            "the tile itself must be declared, or its `{coord}/0/...` keys are \
             orphaned:\n{saved}"
        );
    }

    // The companions themselves must NOT appear: they are not property names
    // Godot can parse.
    assert!(
        !saved.contains("__tile_nav_"),
        "the internal companions must stay out of the file:\n{saved}"
    );
}

/// ⚠️ **The trap this bead's "same shape as the collision work" framing hides.**
///
/// The two polygon families do not share an origin inside Patina:
///
/// * `__tile_physics_*` points are **tile-centre** relative — the static body is
///   placed at the tile centre (`gdphysics2d/tilemap_collision.rs:83`) and the
///   points are used as they are.
/// * `__tile_nav_polygon__` is **top-left** relative — documented so at
///   `tilemap.rs:526`, and `tile_navigation_polygons` adds the cell's top-left
///   corner to every point.
///
/// Godot stores both centred on the tile. Emitting the navigation polygon
/// unconverted writes one displaced by half a tile, which still loads and still
/// looks like a navigation mesh — it just covers the wrong quarter of the map.
#[test]
fn nav_polygon_is_converted_to_godots_tile_centre_origin() {
    // The top-left QUADRANT of a 16x16 tile, in tile-local pixels.
    let quadrant = vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(8.0, 0.0),
        Vector2::new(8.0, 8.0),
        Vector2::new(0.0, 8.0),
    ];
    let (tree, root) = authored_layer(quadrant);
    let saved = TscnSaver::save_tree(&tree, root);

    // Centre-relative, so every coordinate moves by -8: the quadrant spans
    // (-8,-8) to (0,0), NOT (0,0) to (8,8).
    assert!(
        saved.contains("vertices = PackedVector2Array(-8, -8, 0, -8, 0, 0, -8, 0)"),
        "the polygon must be re-expressed around the tile centre; an unconverted \
         one would read `PackedVector2Array(0, 0, 8, 0, 8, 8, 0, 8)`:\n{saved}"
    );
}

/// The default polygon is the whole tile — which for a 16x16 tile is the
/// centred rect, not a rect starting at the origin.
#[test]
fn an_unset_polygon_covers_the_whole_tile() {
    let (tree, root) = authored_layer(Vec::new());
    let saved = TscnSaver::save_tree(&tree, root);

    assert!(
        saved.contains("vertices = PackedVector2Array(-8, -8, 8, -8, 8, 8, -8, 8)"),
        "an empty `__tile_nav_polygon__` means the whole tile is walkable:\n{saved}"
    );
}

/// A layer with no navigation must not gain a navigation layer.
#[test]
fn a_layer_without_navigation_declares_none() {
    let (mut tree, root) = authored_layer(Vec::new());
    // Drop the navigation companions back off.
    let layer = tree.find_child(root, "Ground", true).expect("layer exists");
    tree.get_node_mut(layer)
        .unwrap()
        .set_property(TILE_NAV_SOURCES, Variant::PackedInt32Array(Vec::new()));

    let saved = TscnSaver::save_tree(&tree, root);
    assert!(
        !saved.contains("navigation_layer_0"),
        "no navigation source means no navigation keys at all:\n{saved}"
    );
}

/// Parses `saved` and instantiates it, returning the tree and the TileMapLayer.
///
/// ⚠️ Goes through `add_packed_scene_to_tree` rather than reading
/// `PackedScene`'s templates: that field is private, and a live node is the
/// better assertion target anyway — it proves the companions reach an
/// instantiated node, not merely a parsed template.
fn reload(saved: &str) -> (SceneTree, gdscene::NodeId) {
    let scene = PackedScene::from_tscn(saved).expect("the saved scene must parse");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let new_root = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");
    let layer = tree
        .find_child(new_root, "Ground", true)
        .expect("the layer must survive the round trip");
    (tree, layer)
}

/// **The round trip.** The file the saver writes must load back into the same
/// companions — the half that was missing: nothing parsed
/// `navigation_layer_0/polygon`, so a scene carrying navigation (including one
/// this engine had just written) lost it the moment it was reopened.
///
/// ⚠️ Asserting save→load→save produces identical text would NOT be enough on
/// its own: two inverse bugs cancel, and a polygon rebased the wrong way on both
/// sides round-trips perfectly while being wrong in the file. So the middle
/// state is asserted — the loaded companion must be back in TOP-LEFT
/// coordinates.
#[test]
fn navigation_survives_a_save_and_reload() {
    let quadrant = vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(8.0, 0.0),
        Vector2::new(8.0, 8.0),
        Vector2::new(0.0, 8.0),
    ];
    let (tree, root) = authored_layer(quadrant.clone());
    let saved = TscnSaver::save_tree(&tree, root);

    let (reloaded, layer) = reload(&saved);
    let node = reloaded.get_node(layer).expect("the layer node exists");

    assert_eq!(
        node.get_property(TILE_NAV_SOURCES),
        Variant::PackedInt32Array(vec![0]),
        "the navigation source list must be rebuilt from the file"
    );
    assert_eq!(
        node.get_property(TILE_NAV_POLYGON),
        Variant::PackedVector2Array(quadrant),
        "the polygon must come back top-left relative; leaving it centred would \
         still round-trip through the saver and still be wrong"
    );
}

/// An unedited tile — the whole-tile default — must NOT come back as an explicit
/// polygon. It would behave identically but would make an untouched tile look
/// edited on the next save, churning the file.
#[test]
fn the_default_polygon_does_not_become_explicit_on_load() {
    let (tree, root) = authored_layer(Vec::new());
    let saved = TscnSaver::save_tree(&tree, root);

    let (reloaded, layer) = reload(&saved);
    let node = reloaded.get_node(layer).expect("the layer node exists");

    assert_eq!(
        node.get_property(TILE_NAV_SOURCES),
        Variant::PackedInt32Array(vec![0]),
        "navigation is still enabled"
    );
    assert_eq!(
        node.get_property(TILE_NAV_POLYGON),
        Variant::Nil,
        "the whole-tile default stays implicit"
    );
}
