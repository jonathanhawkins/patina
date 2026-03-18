//! TileMap and TileSet system for 2D grid-based levels.
//!
//! Mirrors Godot's TileMap/TileSet workflow: a [`TileSet`] defines tile
//! definitions (texture, collision, custom data), and a [`TileMap`] node
//! stores per-layer cell data referencing those definitions.

use std::collections::HashMap;

use gdcore::math::{Rect2, Vector2, Vector2i};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// TileDefinition & TileSet
// ---------------------------------------------------------------------------

/// A single tile definition within a [`TileSet`].
#[derive(Debug, Clone, PartialEq)]
pub struct TileDefinition {
    /// Path to the texture resource (e.g. `"res://tileset.png"`).
    pub texture_path: String,
    /// Region within the texture atlas for this tile.
    pub texture_region: Rect2,
    /// Collision shapes associated with this tile.
    pub collision_shapes: Vec<Rect2>,
    /// Custom data keys this tile supports.
    pub custom_data_keys: Vec<String>,
}

/// A set of tile definitions, matching Godot's `TileSet` resource.
#[derive(Debug, Clone, PartialEq)]
pub struct TileSet {
    /// Size of each tile in pixels.
    pub tile_size: Vector2i,
    /// Tile definitions keyed by source ID.
    pub tiles: HashMap<i32, TileDefinition>,
}

impl TileSet {
    /// Creates a new tile set with the given tile size.
    pub fn new(tile_size: Vector2i) -> Self {
        Self {
            tile_size,
            tiles: HashMap::new(),
        }
    }

    /// Adds a tile definition and returns the previous definition if any.
    pub fn add_tile(&mut self, source_id: i32, def: TileDefinition) -> Option<TileDefinition> {
        self.tiles.insert(source_id, def)
    }

    /// Returns a tile definition by source ID.
    pub fn get_tile(&self, source_id: i32) -> Option<&TileDefinition> {
        self.tiles.get(&source_id)
    }
}

// ---------------------------------------------------------------------------
// TileData
// ---------------------------------------------------------------------------

/// Data for a single cell in a TileMap layer.
#[derive(Debug, Clone, PartialEq)]
pub struct TileData {
    /// Which tile source this cell uses.
    pub source_id: i32,
    /// Atlas coordinates within the tile source.
    pub atlas_coords: Vector2i,
    /// Alternative tile ID (0 = default).
    pub alternative_id: i32,
    /// Custom data attached to this cell instance.
    pub custom_data: HashMap<String, Variant>,
}

impl TileData {
    /// Creates tile data referencing a source with default atlas coords.
    pub fn new(source_id: i32) -> Self {
        Self {
            source_id,
            atlas_coords: Vector2i::ZERO,
            alternative_id: 0,
            custom_data: HashMap::new(),
        }
    }

    /// Creates tile data with full atlas coordinates.
    pub fn with_atlas(source_id: i32, atlas_coords: Vector2i, alternative_id: i32) -> Self {
        Self {
            source_id,
            atlas_coords,
            alternative_id,
            custom_data: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// TileMapLayer
// ---------------------------------------------------------------------------

/// A single layer within a TileMap node.
#[derive(Debug, Clone, PartialEq)]
pub struct TileMapLayer {
    /// Human-readable name for the layer.
    pub name: String,
    /// Whether this layer is rendered/processed.
    pub enabled: bool,
    /// Draw order relative to other layers.
    pub z_index: i32,
    /// Cell storage: grid coordinate -> tile data.
    pub cells: HashMap<Vector2i, TileData>,
}

impl TileMapLayer {
    /// Creates a new empty layer with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            enabled: true,
            z_index: 0,
            cells: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// TileMapData — stored alongside nodes
// ---------------------------------------------------------------------------

/// All tile map data for a single TileMap node.
#[derive(Debug, Clone, PartialEq)]
pub struct TileMapData {
    /// The tile set used by this tile map.
    pub tile_set: TileSet,
    /// Ordered layers.
    pub layers: Vec<TileMapLayer>,
}

impl TileMapData {
    /// Creates tile map data with the given tile set and no layers.
    pub fn new(tile_set: TileSet) -> Self {
        Self {
            tile_set,
            layers: Vec::new(),
        }
    }

    /// Adds a layer and returns its index.
    pub fn add_layer(&mut self, layer: TileMapLayer) -> usize {
        let idx = self.layers.len();
        self.layers.push(layer);
        idx
    }

    /// Returns a reference to a layer by index.
    pub fn get_layer(&self, index: usize) -> Option<&TileMapLayer> {
        self.layers.get(index)
    }

    /// Returns a mutable reference to a layer by index.
    pub fn get_layer_mut(&mut self, index: usize) -> Option<&mut TileMapLayer> {
        self.layers.get_mut(index)
    }
}

// ---------------------------------------------------------------------------
// TileMap store — maps NodeId -> TileMapData
// ---------------------------------------------------------------------------

/// Storage for TileMap data, keyed by the node that owns it.
#[derive(Debug, Default)]
pub struct TileMapStore {
    data: HashMap<NodeId, TileMapData>,
}

impl TileMapStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Associates tile map data with a node.
    pub fn insert(&mut self, node_id: NodeId, data: TileMapData) {
        self.data.insert(node_id, data);
    }

    /// Returns a reference to the tile map data for a node.
    pub fn get(&self, node_id: NodeId) -> Option<&TileMapData> {
        self.data.get(&node_id)
    }

    /// Returns a mutable reference to the tile map data for a node.
    pub fn get_mut(&mut self, node_id: NodeId) -> Option<&mut TileMapData> {
        self.data.get_mut(&node_id)
    }

    /// Removes tile map data for a node.
    pub fn remove(&mut self, node_id: NodeId) -> Option<TileMapData> {
        self.data.remove(&node_id)
    }
}

// ---------------------------------------------------------------------------
// TileMap helper functions
// ---------------------------------------------------------------------------

/// Sets a cell in the specified layer of a TileMap node.
pub fn set_cell(
    store: &mut TileMapStore,
    node_id: NodeId,
    layer: usize,
    coords: Vector2i,
    tile_data: TileData,
) -> bool {
    if let Some(map_data) = store.get_mut(node_id) {
        if let Some(map_layer) = map_data.layers.get_mut(layer) {
            map_layer.cells.insert(coords, tile_data);
            return true;
        }
    }
    false
}

/// Gets the tile data at a cell in the specified layer.
pub fn get_cell(
    store: &TileMapStore,
    node_id: NodeId,
    layer: usize,
    coords: Vector2i,
) -> Option<TileData> {
    store
        .get(node_id)
        .and_then(|d| d.layers.get(layer))
        .and_then(|l| l.cells.get(&coords))
        .cloned()
}

/// Erases a cell in the specified layer. Returns the removed tile data.
pub fn erase_cell(
    store: &mut TileMapStore,
    node_id: NodeId,
    layer: usize,
    coords: Vector2i,
) -> Option<TileData> {
    store
        .get_mut(node_id)
        .and_then(|d| d.layers.get_mut(layer))
        .and_then(|l| l.cells.remove(&coords))
}

/// Returns all used cell coordinates in a layer.
pub fn get_used_cells(store: &TileMapStore, node_id: NodeId, layer: usize) -> Vec<Vector2i> {
    store
        .get(node_id)
        .and_then(|d| d.layers.get(layer))
        .map(|l| l.cells.keys().copied().collect())
        .unwrap_or_default()
}

/// Converts a grid cell coordinate to local pixel position (center of cell).
///
/// Matches Godot's `TileMap.map_to_local()`.
pub fn map_to_local(tile_size: Vector2i, cell: Vector2i) -> Vector2 {
    Vector2::new(
        (cell.x as f32 + 0.5) * tile_size.x as f32,
        (cell.y as f32 + 0.5) * tile_size.y as f32,
    )
}

/// Converts a local pixel position to grid cell coordinates.
///
/// Matches Godot's `TileMap.local_to_map()`.
pub fn local_to_map(tile_size: Vector2i, local: Vector2) -> Vector2i {
    Vector2i::new(
        (local.x / tile_size.x as f32).floor() as i32,
        (local.y / tile_size.y as f32).floor() as i32,
    )
}

/// Returns the 4 orthogonal neighbors of a cell (up, right, down, left).
pub fn get_surrounding_cells(cell: Vector2i) -> Vec<Vector2i> {
    vec![
        cell + Vector2i::UP,
        cell + Vector2i::RIGHT,
        cell + Vector2i::DOWN,
        cell + Vector2i::LEFT,
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    const EPSILON: f32 = 1e-6;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn make_store_with_tilemap() -> (TileMapStore, NodeId) {
        let mut store = TileMapStore::new();
        let node = Node::new("TileMap", "TileMap");
        let node_id = node.id();

        let tile_set = TileSet::new(Vector2i::new(16, 16));
        let mut map_data = TileMapData::new(tile_set);
        map_data.add_layer(TileMapLayer::new("Ground"));
        map_data.add_layer(TileMapLayer::new("Objects"));

        store.insert(node_id, map_data);
        (store, node_id)
    }

    // -- TileSet --

    #[test]
    fn tileset_new_has_correct_size() {
        let ts = TileSet::new(Vector2i::new(16, 16));
        assert_eq!(ts.tile_size, Vector2i::new(16, 16));
        assert!(ts.tiles.is_empty());
    }

    #[test]
    fn tileset_add_and_get_tile() {
        let mut ts = TileSet::new(Vector2i::new(16, 16));
        let def = TileDefinition {
            texture_path: "res://tiles.png".into(),
            texture_region: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
            collision_shapes: vec![],
            custom_data_keys: vec![],
        };
        assert!(ts.add_tile(0, def.clone()).is_none());
        assert_eq!(ts.get_tile(0), Some(&def));
        assert_eq!(ts.get_tile(1), None);
    }

    #[test]
    fn tileset_replace_tile() {
        let mut ts = TileSet::new(Vector2i::new(16, 16));
        let def1 = TileDefinition {
            texture_path: "res://a.png".into(),
            texture_region: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
            collision_shapes: vec![],
            custom_data_keys: vec![],
        };
        let def2 = TileDefinition {
            texture_path: "res://b.png".into(),
            texture_region: Rect2::new(Vector2::ZERO, Vector2::new(32.0, 32.0)),
            collision_shapes: vec![],
            custom_data_keys: vec![],
        };
        ts.add_tile(0, def1.clone());
        let old = ts.add_tile(0, def2.clone());
        assert_eq!(old, Some(def1));
        assert_eq!(ts.get_tile(0).unwrap().texture_path, "res://b.png");
    }

    // -- TileData --

    #[test]
    fn tile_data_new_defaults() {
        let td = TileData::new(5);
        assert_eq!(td.source_id, 5);
        assert_eq!(td.atlas_coords, Vector2i::ZERO);
        assert_eq!(td.alternative_id, 0);
        assert!(td.custom_data.is_empty());
    }

    #[test]
    fn tile_data_with_atlas() {
        let td = TileData::with_atlas(1, Vector2i::new(3, 4), 2);
        assert_eq!(td.source_id, 1);
        assert_eq!(td.atlas_coords, Vector2i::new(3, 4));
        assert_eq!(td.alternative_id, 2);
    }

    #[test]
    fn tile_data_custom_data() {
        let mut td = TileData::new(0);
        td.custom_data.insert("speed".into(), Variant::Float(1.5));
        assert_eq!(td.custom_data.get("speed"), Some(&Variant::Float(1.5)));
    }

    // -- TileMapLayer --

    #[test]
    fn layer_defaults() {
        let layer = TileMapLayer::new("Ground");
        assert_eq!(layer.name, "Ground");
        assert!(layer.enabled);
        assert_eq!(layer.z_index, 0);
        assert!(layer.cells.is_empty());
    }

    // -- TileMapData --

    #[test]
    fn tilemap_data_add_layers() {
        let ts = TileSet::new(Vector2i::new(16, 16));
        let mut data = TileMapData::new(ts);
        assert_eq!(data.add_layer(TileMapLayer::new("Ground")), 0);
        assert_eq!(data.add_layer(TileMapLayer::new("Objects")), 1);
        assert_eq!(data.layers.len(), 2);
        assert_eq!(data.get_layer(0).unwrap().name, "Ground");
        assert_eq!(data.get_layer(1).unwrap().name, "Objects");
        assert!(data.get_layer(2).is_none());
    }

    // -- TileMapStore --

    #[test]
    fn store_insert_get_remove() {
        let mut store = TileMapStore::new();
        let node = Node::new("TM", "TileMap");
        let nid = node.id();
        let data = TileMapData::new(TileSet::new(Vector2i::new(16, 16)));
        store.insert(nid, data);
        assert!(store.get(nid).is_some());
        let removed = store.remove(nid);
        assert!(removed.is_some());
        assert!(store.get(nid).is_none());
    }

    // -- set_cell / get_cell / erase_cell --

    #[test]
    fn set_and_get_cell() {
        let (mut store, nid) = make_store_with_tilemap();
        let td = TileData::new(1);
        let coords = Vector2i::new(5, 3);

        assert!(set_cell(&mut store, nid, 0, coords, td.clone()));
        let got = get_cell(&store, nid, 0, coords);
        assert_eq!(got, Some(td));
    }

    #[test]
    fn get_cell_empty_returns_none() {
        let (store, nid) = make_store_with_tilemap();
        assert_eq!(get_cell(&store, nid, 0, Vector2i::new(99, 99)), None);
    }

    #[test]
    fn set_cell_invalid_layer_returns_false() {
        let (mut store, nid) = make_store_with_tilemap();
        assert!(!set_cell(
            &mut store,
            nid,
            10,
            Vector2i::ZERO,
            TileData::new(0)
        ));
    }

    #[test]
    fn erase_cell_removes_and_returns() {
        let (mut store, nid) = make_store_with_tilemap();
        let coords = Vector2i::new(2, 2);
        set_cell(&mut store, nid, 0, coords, TileData::new(1));
        let erased = erase_cell(&mut store, nid, 0, coords);
        assert!(erased.is_some());
        assert_eq!(get_cell(&store, nid, 0, coords), None);
    }

    #[test]
    fn erase_cell_nonexistent_returns_none() {
        let (mut store, nid) = make_store_with_tilemap();
        assert_eq!(erase_cell(&mut store, nid, 0, Vector2i::new(99, 99)), None);
    }

    // -- get_used_cells --

    #[test]
    fn get_used_cells_returns_all_coords() {
        let (mut store, nid) = make_store_with_tilemap();
        set_cell(&mut store, nid, 0, Vector2i::new(0, 0), TileData::new(1));
        set_cell(&mut store, nid, 0, Vector2i::new(1, 0), TileData::new(1));
        set_cell(&mut store, nid, 0, Vector2i::new(2, 3), TileData::new(2));

        let mut cells = get_used_cells(&store, nid, 0);
        cells.sort_by_key(|c| (c.x, c.y));
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0], Vector2i::new(0, 0));
        assert_eq!(cells[1], Vector2i::new(1, 0));
        assert_eq!(cells[2], Vector2i::new(2, 3));
    }

    #[test]
    fn get_used_cells_empty_layer() {
        let (store, nid) = make_store_with_tilemap();
        assert!(get_used_cells(&store, nid, 0).is_empty());
    }

    #[test]
    fn get_used_cells_invalid_layer() {
        let (store, nid) = make_store_with_tilemap();
        assert!(get_used_cells(&store, nid, 99).is_empty());
    }

    // -- map_to_local / local_to_map --

    #[test]
    fn map_to_local_center_of_cell() {
        let size = Vector2i::new(16, 16);
        let pos = map_to_local(size, Vector2i::new(0, 0));
        assert!(approx_eq(pos.x, 8.0));
        assert!(approx_eq(pos.y, 8.0));
    }

    #[test]
    fn map_to_local_offset_cell() {
        let size = Vector2i::new(16, 16);
        let pos = map_to_local(size, Vector2i::new(3, 2));
        assert!(approx_eq(pos.x, 56.0)); // (3+0.5)*16
        assert!(approx_eq(pos.y, 40.0)); // (2+0.5)*16
    }

    #[test]
    fn local_to_map_basic() {
        let size = Vector2i::new(16, 16);
        let cell = local_to_map(size, Vector2::new(8.0, 8.0));
        assert_eq!(cell, Vector2i::new(0, 0));
    }

    #[test]
    fn local_to_map_exact_boundary() {
        let size = Vector2i::new(16, 16);
        let cell = local_to_map(size, Vector2::new(16.0, 16.0));
        assert_eq!(cell, Vector2i::new(1, 1));
    }

    #[test]
    fn map_to_local_roundtrip() {
        let size = Vector2i::new(32, 32);
        let original = Vector2i::new(5, -3);
        let local = map_to_local(size, original);
        let back = local_to_map(size, local);
        assert_eq!(back, original);
    }

    #[test]
    fn local_to_map_negative_coords() {
        let size = Vector2i::new(16, 16);
        let cell = local_to_map(size, Vector2::new(-8.0, -8.0));
        assert_eq!(cell, Vector2i::new(-1, -1));
    }

    // -- get_surrounding_cells --

    #[test]
    fn surrounding_cells_at_origin() {
        let neighbors = get_surrounding_cells(Vector2i::ZERO);
        assert_eq!(neighbors.len(), 4);
        assert!(neighbors.contains(&Vector2i::new(0, -1))); // up
        assert!(neighbors.contains(&Vector2i::new(1, 0))); // right
        assert!(neighbors.contains(&Vector2i::new(0, 1))); // down
        assert!(neighbors.contains(&Vector2i::new(-1, 0))); // left
    }

    #[test]
    fn surrounding_cells_at_offset() {
        let neighbors = get_surrounding_cells(Vector2i::new(5, 5));
        assert_eq!(neighbors.len(), 4);
        assert!(neighbors.contains(&Vector2i::new(5, 4)));
        assert!(neighbors.contains(&Vector2i::new(6, 5)));
        assert!(neighbors.contains(&Vector2i::new(5, 6)));
        assert!(neighbors.contains(&Vector2i::new(4, 5)));
    }

    // -- Multi-layer isolation --

    #[test]
    fn layers_are_independent() {
        let (mut store, nid) = make_store_with_tilemap();
        let coords = Vector2i::new(1, 1);
        set_cell(&mut store, nid, 0, coords, TileData::new(10));
        set_cell(&mut store, nid, 1, coords, TileData::new(20));

        assert_eq!(get_cell(&store, nid, 0, coords).unwrap().source_id, 10);
        assert_eq!(get_cell(&store, nid, 1, coords).unwrap().source_id, 20);
    }

    // -- Overwrite cell --

    #[test]
    fn overwrite_cell_replaces_data() {
        let (mut store, nid) = make_store_with_tilemap();
        let coords = Vector2i::new(0, 0);
        set_cell(&mut store, nid, 0, coords, TileData::new(1));
        set_cell(&mut store, nid, 0, coords, TileData::new(2));
        assert_eq!(get_cell(&store, nid, 0, coords).unwrap().source_id, 2);
    }
}
