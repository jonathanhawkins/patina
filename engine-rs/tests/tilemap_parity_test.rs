//! Parity test for the TileMap runtime: layer rendering output and
//! physics-integration hand-off via `gdphysics2d::tilemap_collision`.
//!
//! Disabled until `gdscene::tilemap` gains `render_layer`, `render_tilemap`,
//! and `collect_tilemap_cells`. Re-enable by removing the cfg gate below
//! once those functions land. Tracked by the editor-parity TileMap lane.
#![cfg(any())]

use gdcore::math::{Rect2, Vector2, Vector2i};
use gdphysics2d::tilemap_collision::{collect_collision_tiles, tilemap_to_physics};
use gdphysics2d::world::PhysicsWorld2D;
use gdscene::node::NodeId;
use gdscene::tilemap::{
    collect_tilemap_cells, map_to_local, render_layer, render_tilemap, set_cell, TileData,
    TileDefinition, TileMapData, TileMapLayer, TileMapStore, TileSet,
};

fn tile_size() -> Vector2i {
    Vector2i::new(16, 16)
}

fn grass_def() -> TileDefinition {
    TileDefinition {
        texture_path: "res://tileset.png".to_string(),
        texture_region: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
        collision_shapes: vec![Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0))],
        custom_data_keys: vec![],
    }
}

fn water_def() -> TileDefinition {
    TileDefinition {
        texture_path: "res://tileset.png".to_string(),
        texture_region: Rect2::new(Vector2::new(16.0, 0.0), Vector2::new(16.0, 16.0)),
        collision_shapes: vec![],
        custom_data_keys: vec![],
    }
}

fn build_store() -> (TileMapStore, NodeId) {
    let mut tile_set = TileSet::new(tile_size());
    tile_set.add_tile(1, grass_def());
    tile_set.add_tile(2, water_def());

    let mut data = TileMapData::new(tile_set);
    let ground = TileMapLayer::new("ground");
    let ground_idx = data.add_layer(ground);
    let mut overlay = TileMapLayer::new("overlay");
    overlay.z_index = 5;
    let overlay_idx = data.add_layer(overlay);
    let mut hidden = TileMapLayer::new("hidden");
    hidden.enabled = false;
    let hidden_idx = data.add_layer(hidden);

    let mut store = TileMapStore::new();
    let node = NodeId::next();
    store.insert(node, data);

    set_cell(&mut store, node, ground_idx, Vector2i::new(0, 0), TileData::new(1));
    set_cell(&mut store, node, ground_idx, Vector2i::new(1, 0), TileData::new(2));
    set_cell(&mut store, node, ground_idx, Vector2i::new(0, 1), TileData::new(1));
    // Unknown source_id must be dropped by the renderer.
    set_cell(&mut store, node, ground_idx, Vector2i::new(2, 2), TileData::new(99));

    set_cell(&mut store, node, overlay_idx, Vector2i::new(0, 0), TileData::new(2));

    set_cell(&mut store, node, hidden_idx, Vector2i::new(5, 5), TileData::new(1));

    (store, node)
}

#[test]
fn tilemap_layer_render() {
    let (store, node) = build_store();

    // --- render_layer: ground layer --------------------------------------
    let ground_items = render_layer(&store, node, 0, Vector2::ZERO);
    assert_eq!(
        ground_items.len(),
        3,
        "ground should drop the cell with unknown source_id"
    );
    // Sorted by (y, x): (0,0), (1,0), (0,1).
    assert_eq!(ground_items[0].cell, Vector2i::new(0, 0));
    assert_eq!(ground_items[1].cell, Vector2i::new(1, 0));
    assert_eq!(ground_items[2].cell, Vector2i::new(0, 1));

    // world_position matches map_to_local for every item.
    for item in &ground_items {
        let expected = map_to_local(tile_size(), item.cell);
        assert!((item.world_position.x - expected.x).abs() < 1e-5);
        assert!((item.world_position.y - expected.y).abs() < 1e-5);
        assert_eq!(item.z_index, 0);
        assert_eq!(item.layer_index, 0);
    }
    // Texture region comes from TileDefinition (cell (1,0) is water).
    assert_eq!(ground_items[1].texture_region, water_def().texture_region);
    assert_eq!(ground_items[0].texture_region, grass_def().texture_region);

    // --- render_layer: disabled layer yields nothing ---------------------
    let hidden_items = render_layer(&store, node, 2, Vector2::ZERO);
    assert!(
        hidden_items.is_empty(),
        "disabled layers must produce no render items"
    );

    // --- render_tilemap: global sort by z_index then layer_index ---------
    let offset = Vector2::new(100.0, 200.0);
    let all = render_tilemap(&store, node, offset);
    assert_eq!(all.len(), 4, "three ground + one overlay");
    // z_index 0 items first, then z_index 5.
    for item in &all[0..3] {
        assert_eq!(item.z_index, 0);
        assert_eq!(item.layer_index, 0);
    }
    assert_eq!(all[3].z_index, 5);
    assert_eq!(all[3].layer_index, 1);
    assert_eq!(all[3].cell, Vector2i::new(0, 0));
    // Offset is applied.
    let expected = map_to_local(tile_size(), Vector2i::new(0, 0)) + offset;
    assert!((all[3].world_position.x - expected.x).abs() < 1e-5);
    assert!((all[3].world_position.y - expected.y).abs() < 1e-5);

    // --- physics integration --------------------------------------------
    let cells = collect_tilemap_cells(&store, node);
    // Hidden layer skipped; unknown-source cell still included (filtering
    // happens in the renderer, not the physics collector).
    assert!(
        cells.iter().all(|(cell, _)| *cell != Vector2i::new(5, 5)),
        "disabled layer must not contribute to physics cells"
    );

    let tile_set_copy = store
        .get(node)
        .map(|d| d.tile_set.clone())
        .expect("tilemap data exists");
    let collision_tiles = collect_collision_tiles(&cells, |source_id| {
        tile_set_copy
            .get_tile(source_id)
            .and_then(|def| def.collision_shapes.first().copied())
    });
    // Only grass (source 1) has collision; two grass cells live in ground.
    assert_eq!(collision_tiles.len(), 2);

    let mut world = PhysicsWorld2D::new();
    let result = tilemap_to_physics(&mut world, &collision_tiles, tile_size(), Vector2::ZERO);
    assert_eq!(result.body_ids.len(), 2);
    assert_eq!(world.body_count(), 2);

    // Bodies sit at tile centers: (0,0)->(8,8), (0,1)->(8,24).
    let body0 = world.get_body(result.body_ids[0]).unwrap();
    let body1 = world.get_body(result.body_ids[1]).unwrap();
    let mut positions = [
        (body0.position.x, body0.position.y),
        (body1.position.x, body1.position.y),
    ];
    positions.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    assert!((positions[0].0 - 8.0).abs() < 1e-5);
    assert!((positions[0].1 - 8.0).abs() < 1e-5);
    assert!((positions[1].0 - 8.0).abs() < 1e-5);
    assert!((positions[1].1 - 24.0).abs() < 1e-5);
}
