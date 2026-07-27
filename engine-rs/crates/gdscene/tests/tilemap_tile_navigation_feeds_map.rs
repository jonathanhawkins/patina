//! pat-0q2kx.99: TileSet-authored navigation reaches the runtime navigation map.
//!
//! Godot's `TileMapLayer::_navigation_update` (upstream/godot/scene/2d/tile_map_layer.cpp)
//! creates one navigation region per painted cell whose tile carries a
//! navigation polygon, positioned at that cell's world transform, and frees the
//! region when the cell is erased. Agents then path across the tiles because
//! neighbouring regions share edges.
//!
//! Patina's `tilemap.rs` had **zero** navigation references — a case-insensitive
//! grep for `navigation|navmesh|nav_` returned nothing — so tile-authored
//! navigation, a standard 2D Godot pattern, produced nothing at runtime.
//!
//! These drive `MainLoop::process_navigation`, the real per-frame gather, and
//! assert on the agent's computed path. The cells are encoded with the shipping
//! `encode_tile_map_data` codec, so the node carries exactly the bytes a loaded
//! scene would carry rather than a hand-built side structure.

use gdcore::math::{Vector2, Vector2i};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::tilemap::{encode_tile_map_data, TileData, TILE_NAV_POLYGON, TILE_NAV_SOURCES};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::collections::HashMap;

const TILE: i32 = 16;
/// The walkable tile source; `NON_NAV_SOURCE` is painted but not navigable.
const NAV_SOURCE: i32 = 1;
const NON_NAV_SOURCE: i32 = 2;

/// Builds `root -> Layer (TileMapLayer) + Agent (NavigationAgent2D)`.
///
/// `cells` lists `(x, source_id)` painted along row 0 — a horizontal corridor.
/// `target` is the agent's destination in world pixels.
fn corridor_scene(cells: &[(i32, i32)], target: Vector2) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut painted: HashMap<Vector2i, TileData> = HashMap::new();
    for (x, source) in cells {
        painted.insert(Vector2i::new(*x, 0), TileData::new(*source));
    }

    let mut layer = Node::new("Layer", "TileMapLayer");
    layer.set_property(
        "tile_map_data",
        Variant::PackedByteArray(encode_tile_map_data(&painted)),
    );
    layer.set_property("tile_size", Variant::Vector2i(Vector2i::new(TILE, TILE)));
    // Only NAV_SOURCE tiles are walkable — the TileSet's navigation layer.
    layer.set_property(
        TILE_NAV_SOURCES,
        Variant::PackedInt32Array(vec![NAV_SOURCE]),
    );
    tree.add_child(root, layer).unwrap();

    // Start in the middle of the first cell, on the tile surface.
    let mut agent = Node::new("Agent", "NavigationAgent2D");
    agent.set_property(
        "position",
        Variant::Vector2(Vector2::new(TILE as f32 * 0.5, TILE as f32 * 0.5)),
    );
    agent.set_property("target_position", Variant::Vector2(target));
    let agent_id = tree.add_child(root, agent).unwrap();

    (tree, agent_id)
}

/// Centre of cell `x` in row 0, in world pixels.
fn cell_centre(x: i32) -> Vector2 {
    Vector2::new((x as f32 + 0.5) * TILE as f32, TILE as f32 * 0.5)
}

/// **The acceptance test.** A 6-tile corridor is traversable; a gap is not.
#[test]
fn tilemap_tile_navigation_feeds_map() {
    // -- (1) an unbroken 6-tile corridor yields a multi-waypoint path --------
    let corridor: Vec<(i32, i32)> = (0..6).map(|x| (x, NAV_SOURCE)).collect();
    let (tree, agent) = corridor_scene(&corridor, cell_centre(5));
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);

    let len = ml.nav_agent_path_len_2d(agent);
    assert!(
        len >= 2,
        "an agent must path across the painted tiles from one end of the \
         corridor to the other; got {len} waypoint(s). A single waypoint means \
         the tiles produced no regions and the agent is heading straight at \
         the target."
    );

    // -- (2) a gap in the corridor breaks the path ---------------------------
    //
    // Cell 3 is left unpainted, so the run splits into two disjoint groups with
    // no shared edge. Without this the first assertion would also pass on an
    // implementation that emitted one giant region covering everything.
    let gapped: Vec<(i32, i32)> = (0..6)
        .filter(|x| *x != 3)
        .map(|x| (x, NAV_SOURCE))
        .collect();
    let (tree, agent) = corridor_scene(&gapped, cell_centre(5));
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);

    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "a gap in the painted tiles must leave the far end unreachable"
    );
}

/// A painted tile whose source is not in the TileSet's navigation layer is not
/// walkable. Without this, "every painted cell is walkable" would pass the
/// acceptance test while ignoring the navigation layer entirely.
#[test]
fn only_navigation_enabled_sources_contribute() {
    // The corridor is continuous, but cell 3 uses a non-navigable source — the
    // same topology as the gap case, expressed through the navigation layer
    // rather than through absence.
    let mixed: Vec<(i32, i32)> = (0..6)
        .map(|x| (x, if x == 3 { NON_NAV_SOURCE } else { NAV_SOURCE }))
        .collect();
    let (tree, agent) = corridor_scene(&mixed, cell_centre(5));
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);

    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "a painted tile outside the navigation layer must not be walkable"
    );
}

/// A layer with no navigation sources at all contributes nothing — a plain
/// decorative tile layer must not silently become a floor.
#[test]
fn layer_without_navigation_sources_contributes_nothing() {
    let corridor: Vec<(i32, i32)> = (0..6).map(|x| (x, NAV_SOURCE)).collect();
    let (mut tree, agent) = corridor_scene(&corridor, cell_centre(5));
    let layer = tree.get_node_by_path("/root/Layer").expect("layer node");
    if let Some(node) = tree.get_node_mut(layer) {
        node.set_property(TILE_NAV_SOURCES, Variant::PackedInt32Array(Vec::new()));
    }
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);

    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "a tile layer with an empty navigation layer must contribute no regions"
    );
}

/// The authored strip's height. Three quarters of the tile, so the polygon is
/// clearly NOT the default full-cell rect, yet still strictly contains the
/// agent's row at `TILE / 2`.
///
/// It must not be `TILE / 2`: `corridor_scene` puts the agent and the target at
/// exactly `TILE * 0.5`, so a half-height strip ends precisely ON the agent, and
/// `NavPolygon::contains_point` is a strict ray-cast that excludes boundary
/// points. `find_path` then fails to locate a start polygon and returns an empty
/// path — the strip never had a hole in it, the query point was just sitting on
/// its edge.
const STRIP_H: f32 = TILE as f32 * 0.75;

/// Paints the corridor and authors `STRIP_H`-tall navigation strips on the node,
/// the way a `PackedScene` property would carry them.
fn authored_strip_scene(target: Vector2) -> (MainLoop, NodeId) {
    let corridor: Vec<(i32, i32)> = (0..6).map(|x| (x, NAV_SOURCE)).collect();
    let (mut tree, agent) = corridor_scene(&corridor, target);
    let layer = tree.get_node_by_path("/root/Layer").expect("layer node");
    if let Some(node) = tree.get_node_mut(layer) {
        node.set_property(
            TILE_NAV_POLYGON,
            Variant::PackedVector2Array(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(TILE as f32, 0.0),
                Vector2::new(TILE as f32, STRIP_H),
                Vector2::new(0.0, STRIP_H),
            ]),
        );
    }
    (MainLoop::new(tree), agent)
}

/// An authored tile polygon set on the node drives the runtime gather, not just
/// the helper — this is the property `PackedScene` would carry.
///
/// The strips span the full cell width, so neighbours share their vertical edge
/// and the corridor still stitches.
#[test]
fn authored_tile_polygon_property_reaches_the_runtime_gather() {
    let (mut ml, agent) = authored_strip_scene(cell_centre(5));
    ml.step(0.1);

    assert!(
        ml.nav_agent_path_len_2d(agent) >= 2,
        "an authored per-tile polygon must reach the gather and still stitch \
         across neighbouring cells"
    );
}

/// …and the authored polygon is genuinely the one being used.
///
/// Without this, the test above passes either way: the *default* for a
/// navigation-enabled tile is the whole cell rect, which also contains the agent
/// and also stitches, so ignoring `TILE_NAV_POLYGON` entirely would still look
/// green. Here the target sits below the authored strip but still inside that
/// default rect, so a path can only exist if the property was dropped.
#[test]
fn a_target_outside_the_authored_strip_is_unreachable() {
    let below_strip = Vector2::new(
        cell_centre(5).x,
        // Between the strip's lower edge and the cell's, so it is outside the
        // authored polygon and inside the default one.
        (STRIP_H + TILE as f32) * 0.5,
    );
    let (mut ml, agent) = authored_strip_scene(below_strip);
    ml.step(0.1);

    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "a target below the authored strip must be unreachable — reaching it \
         would mean the gather fell back to the default whole-cell polygon and \
         ignored TILE_NAV_POLYGON"
    );
}

/// `enabled = false` switches the whole layer's navigation off, matching how
/// the NavigationRegion nodes behave.
#[test]
fn disabled_layer_contributes_nothing() {
    let corridor: Vec<(i32, i32)> = (0..6).map(|x| (x, NAV_SOURCE)).collect();
    let (mut tree, agent) = corridor_scene(&corridor, cell_centre(5));
    let layer = tree.get_node_by_path("/root/Layer").expect("layer node");
    if let Some(node) = tree.get_node_mut(layer) {
        node.set_property("enabled", Variant::Bool(false));
    }
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);

    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "a disabled tile layer must contribute no navigation regions"
    );
}

/// An explicit tile-local polygon is honoured and placed at the cell's corner,
/// not its centre. A half-tile misplacement would open phantom gaps between
/// neighbours, so this pins the offset.
#[test]
fn explicit_tile_polygon_is_placed_at_the_cell_corner() {
    use gdscene::tilemap::tile_navigation_polygons;

    let mut painted: HashMap<Vector2i, TileData> = HashMap::new();
    painted.insert(Vector2i::new(2, 0), TileData::new(NAV_SOURCE));

    // A full-tile square expressed explicitly.
    let square = vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(TILE as f32, 0.0),
        Vector2::new(TILE as f32, TILE as f32),
        Vector2::new(0.0, TILE as f32),
    ];
    let polys = tile_navigation_polygons(
        &painted,
        Vector2i::new(TILE, TILE),
        &[NAV_SOURCE],
        &square,
        Vector2::ZERO,
    );

    assert_eq!(polys.len(), 1, "one painted navigable cell -> one polygon");
    assert_eq!(
        polys[0][0],
        Vector2::new(32.0, 0.0),
        "cell (2,0) with a 16px tile starts at x=32 — the CORNER. The centre \
         would be (40, 8), which would shift every region by half a tile."
    );

    // And the implicit form covers the same rect, so a tile with no authored
    // polygon is walkable across its whole cell.
    let implicit = tile_navigation_polygons(
        &painted,
        Vector2i::new(TILE, TILE),
        &[NAV_SOURCE],
        &[],
        Vector2::ZERO,
    );
    assert_eq!(
        implicit, polys,
        "a navigation-enabled tile with no authored polygon must default to \
         its full rect"
    );
}

/// The layer node's own position offsets every region it contributes.
#[test]
fn layer_origin_offsets_the_regions() {
    use gdscene::tilemap::tile_navigation_polygons;

    let mut painted: HashMap<Vector2i, TileData> = HashMap::new();
    painted.insert(Vector2i::new(0, 0), TileData::new(NAV_SOURCE));

    let polys = tile_navigation_polygons(
        &painted,
        Vector2i::new(TILE, TILE),
        &[NAV_SOURCE],
        &[],
        Vector2::new(100.0, 50.0),
    );
    assert_eq!(
        polys[0][0],
        Vector2::new(100.0, 50.0),
        "moving the layer node must move its navigation regions with it"
    );
}

/// Emitted polygons are ordered deterministically, so a path is reproducible
/// across runs rather than varying with HashMap iteration order.
#[test]
fn polygon_order_is_deterministic() {
    use gdscene::tilemap::tile_navigation_polygons;

    let mut painted: HashMap<Vector2i, TileData> = HashMap::new();
    for (x, y) in [(2, 1), (0, 0), (1, 1), (1, 0)] {
        painted.insert(Vector2i::new(x, y), TileData::new(NAV_SOURCE));
    }
    let first = tile_navigation_polygons(
        &painted,
        Vector2i::new(TILE, TILE),
        &[NAV_SOURCE],
        &[],
        Vector2::ZERO,
    );
    for _ in 0..5 {
        let again = tile_navigation_polygons(
            &painted,
            Vector2i::new(TILE, TILE),
            &[NAV_SOURCE],
            &[],
            Vector2::ZERO,
        );
        assert_eq!(again, first, "polygon order must not vary between calls");
    }
    // Row-major: (0,0), (1,0), (1,1), (2,1).
    assert_eq!(first[0][0], Vector2::new(0.0, 0.0));
    assert_eq!(first[1][0], Vector2::new(16.0, 0.0));
}
