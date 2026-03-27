//! TileMap and TileSet system for 2D grid-based levels.
//!
//! Mirrors Godot's TileMap/TileSet workflow: a [`TileSet`] defines tile
//! definitions (texture, collision, custom data), and a [`TileMapStore`] node
//! stores per-layer cell data referencing those definitions.

use std::collections::HashMap;

use gdcore::math::{Rect2, Vector2, Vector2i};
use gdvariant::Variant;

use crate::node::NodeId;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ColorTile {
    pub id: i32,
    pub name: String,
    pub color: gdcore::math::Color,
    pub collision: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorTileSet {
    pub cell_size: Vector2,
    pub tiles: HashMap<i32, ColorTile>,
}

impl ColorTileSet {
    pub fn new(cs: Vector2) -> Self {
        Self {
            cell_size: cs,
            tiles: HashMap::new(),
        }
    }
    pub fn add_tile(&mut self, t: ColorTile) {
        self.tiles.insert(t.id, t);
    }
    pub fn get_tile(&self, id: i32) -> Option<&ColorTile> {
        self.tiles.get(&id)
    }
    pub fn tile_ids_sorted(&self) -> Vec<i32> {
        let mut v: Vec<i32> = self.tiles.keys().copied().collect();
        v.sort();
        v
    }
}

pub fn default_color_tileset() -> ColorTileSet {
    let mut ts = ColorTileSet::new(Vector2::new(16.0, 16.0));
    ts.add_tile(ColorTile {
        id: 1,
        name: "Ground".into(),
        color: gdcore::math::Color::new(0.545, 0.271, 0.075, 1.0),
        collision: true,
    });
    ts.add_tile(ColorTile {
        id: 2,
        name: "Wall".into(),
        color: gdcore::math::Color::new(0.412, 0.412, 0.412, 1.0),
        collision: true,
    });
    ts.add_tile(ColorTile {
        id: 3,
        name: "Water".into(),
        color: gdcore::math::Color::new(0.255, 0.412, 0.882, 1.0),
        collision: false,
    });
    ts.add_tile(ColorTile {
        id: 4,
        name: "Grass".into(),
        color: gdcore::math::Color::new(0.133, 0.545, 0.133, 1.0),
        collision: false,
    });
    ts.add_tile(ColorTile {
        id: 5,
        name: "Lava".into(),
        color: gdcore::math::Color::new(1.0, 0.271, 0.0, 1.0),
        collision: false,
    });
    ts.add_tile(ColorTile {
        id: 6,
        name: "Ice".into(),
        color: gdcore::math::Color::new(0.529, 0.808, 0.922, 1.0),
        collision: false,
    });
    ts.add_tile(ColorTile {
        id: 7,
        name: "Sand".into(),
        color: gdcore::math::Color::new(0.824, 0.706, 0.549, 1.0),
        collision: false,
    });
    ts.add_tile(ColorTile {
        id: 8,
        name: "Stone".into(),
        color: gdcore::math::Color::new(0.184, 0.310, 0.310, 1.0),
        collision: true,
    });
    ts
}

#[derive(Debug, Clone, PartialEq)]
pub struct TileGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<i32>,
}
impl TileGrid {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            width: w,
            height: h,
            cells: vec![0; w * h],
        }
    }
    pub fn get(&self, x: i32, y: i32) -> Option<i32> {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return None;
        }
        Some(self.cells[y as usize * self.width + x as usize])
    }
    pub fn set(&mut self, x: i32, y: i32, t: i32) -> bool {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return false;
        }
        self.cells[y as usize * self.width + x as usize] = t;
        true
    }
    pub fn fill_rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, t: i32) -> usize {
        let (ax, bx) = (
            x1.min(x2).max(0) as usize,
            (x1.max(x2) as usize).min(self.width.saturating_sub(1)),
        );
        let (ay, by) = (
            y1.min(y2).max(0) as usize,
            (y1.max(y2) as usize).min(self.height.saturating_sub(1)),
        );
        let mut n = 0;
        for r in ay..=by {
            for c in ax..=bx {
                self.cells[r * self.width + c] = t;
                n += 1;
            }
        }
        n
    }
    pub fn resize(&mut self, nw: usize, nh: usize) {
        let mut nc = vec![0i32; nw * nh];
        for r in 0..self.height.min(nh) {
            for c in 0..self.width.min(nw) {
                nc[r * nw + c] = self.cells[r * self.width + c];
            }
        }
        self.width = nw;
        self.height = nh;
        self.cells = nc;
    }
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "width": self.width, "height": self.height, "cells": self.cells })
    }
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        let w = v.get("width")?.as_u64()? as usize;
        let h = v.get("height")?.as_u64()? as usize;
        let a = v.get("cells")?.as_array()?;
        if a.len() != w * h {
            return None;
        }
        let c: Vec<i32> = a
            .iter()
            .filter_map(|x: &serde_json::Value| x.as_i64().map(|n| n as i32))
            .collect();
        if c.len() != w * h {
            return None;
        }
        Some(Self {
            width: w,
            height: h,
            cells: c,
        })
    }
}

#[derive(Debug, Default)]
pub struct TileGridStore {
    grids: HashMap<NodeId, TileGrid>,
    pub tileset: Option<ColorTileSet>,
}
impl TileGridStore {
    pub fn new_with_defaults() -> Self {
        Self {
            grids: HashMap::new(),
            tileset: Some(default_color_tileset()),
        }
    }
    pub fn insert(&mut self, id: NodeId, g: TileGrid) {
        self.grids.insert(id, g);
    }
    pub fn get(&self, id: NodeId) -> Option<&TileGrid> {
        self.grids.get(&id)
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut TileGrid> {
        self.grids.get_mut(&id)
    }
    pub fn remove(&mut self, id: NodeId) -> Option<TileGrid> {
        self.grids.remove(&id)
    }
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.grids.keys().copied().collect()
    }
}

// ---------------------------------------------------------------------------
// Tile painting / editor tools
// ---------------------------------------------------------------------------

/// The active painting mode for the tilemap editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilePaintMode {
    /// Paint single cells on click/drag.
    Single,
    /// Paint a straight line between two points.
    Line,
    /// Paint a filled rectangle between two corners.
    Rect,
    /// Flood-fill connected cells that match the target value.
    FloodFill,
    /// Erase cells (paint with tile_id = 0 or remove from store).
    Erase,
}

/// Editor brush state for tile painting.
#[derive(Debug, Clone)]
pub struct TileBrush {
    /// The tile source ID to paint with.
    pub tile_id: i32,
    /// Current painting mode.
    pub mode: TilePaintMode,
    /// Target layer index.
    pub layer: usize,
}

impl TileBrush {
    /// Creates a new brush painting tile_id 1 in single mode on layer 0.
    pub fn new() -> Self {
        Self {
            tile_id: 1,
            mode: TilePaintMode::Single,
            layer: 0,
        }
    }

    /// Creates a brush with the given tile ID.
    pub fn with_tile(tile_id: i32) -> Self {
        Self {
            tile_id,
            mode: TilePaintMode::Single,
            layer: 0,
        }
    }
}

impl Default for TileBrush {
    fn default() -> Self {
        Self::new()
    }
}

/// Paints a line of tiles between two grid coordinates using Bresenham's algorithm.
///
/// Returns the list of cells that were painted.
pub fn paint_line(grid: &mut TileGrid, from: Vector2i, to: Vector2i, tile_id: i32) -> Vec<Vector2i> {
    let mut painted = Vec::new();
    let mut x0 = from.x;
    let mut y0 = from.y;
    let x1 = to.x;
    let y1 = to.y;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if grid.set(x0, y0, tile_id) {
            painted.push(Vector2i::new(x0, y0));
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    painted
}

/// Paints a filled rectangle of tiles between two corner coordinates.
///
/// Returns the list of cells that were painted.
pub fn paint_rect(grid: &mut TileGrid, a: Vector2i, b: Vector2i, tile_id: i32) -> Vec<Vector2i> {
    let x_min = a.x.min(b.x);
    let x_max = a.x.max(b.x);
    let y_min = a.y.min(b.y);
    let y_max = a.y.max(b.y);

    let mut painted = Vec::new();
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            if grid.set(x, y, tile_id) {
                painted.push(Vector2i::new(x, y));
            }
        }
    }
    painted
}

/// Erases a line of tiles (sets to 0) between two grid coordinates.
///
/// Returns the list of cells that were erased.
pub fn erase_line(grid: &mut TileGrid, from: Vector2i, to: Vector2i) -> Vec<Vector2i> {
    paint_line(grid, from, to, 0)
}

/// Erases a filled rectangle of tiles (sets to 0) between two corners.
///
/// Returns the list of cells that were erased.
pub fn erase_rect(grid: &mut TileGrid, a: Vector2i, b: Vector2i) -> Vec<Vector2i> {
    paint_rect(grid, a, b, 0)
}

/// Flood-fills connected cells starting from `origin` that match the tile at `origin`.
///
/// Uses 4-directional connectivity (up/right/down/left). Replaces matching cells
/// with `tile_id`. Returns the list of cells that were filled. If `origin` already
/// contains `tile_id`, no fill is performed (returns empty).
pub fn flood_fill(grid: &mut TileGrid, origin: Vector2i, tile_id: i32) -> Vec<Vector2i> {
    let target = match grid.get(origin.x, origin.y) {
        Some(t) => t,
        None => return Vec::new(),
    };

    // Don't fill if target already matches paint tile.
    if target == tile_id {
        return Vec::new();
    }

    let mut filled = Vec::new();
    let mut stack = vec![origin];

    while let Some(pos) = stack.pop() {
        match grid.get(pos.x, pos.y) {
            Some(t) if t == target => {}
            _ => continue,
        }

        grid.set(pos.x, pos.y, tile_id);
        filled.push(pos);

        stack.push(Vector2i::new(pos.x + 1, pos.y));
        stack.push(Vector2i::new(pos.x - 1, pos.y));
        stack.push(Vector2i::new(pos.x, pos.y + 1));
        stack.push(Vector2i::new(pos.x, pos.y - 1));
    }

    filled
}

/// Applies a [`TileBrush`] action to a [`TileGrid`].
///
/// `from` and `to` are grid coordinates. For `Single` and `FloodFill` modes,
/// only `from` is used. For `Line` and `Rect`, both endpoints matter.
/// Returns the list of cells modified.
pub fn apply_brush(
    grid: &mut TileGrid,
    brush: &TileBrush,
    from: Vector2i,
    to: Vector2i,
) -> Vec<Vector2i> {
    match brush.mode {
        TilePaintMode::Single => {
            if grid.set(from.x, from.y, brush.tile_id) {
                vec![from]
            } else {
                Vec::new()
            }
        }
        TilePaintMode::Line => paint_line(grid, from, to, brush.tile_id),
        TilePaintMode::Rect => paint_rect(grid, from, to, brush.tile_id),
        TilePaintMode::FloodFill => flood_fill(grid, from, brush.tile_id),
        TilePaintMode::Erase => {
            // Erase uses the same shape as the mode but paints 0.
            if from == to {
                if grid.set(from.x, from.y, 0) {
                    vec![from]
                } else {
                    Vec::new()
                }
            } else {
                paint_line(grid, from, to, 0)
            }
        }
    }
}

/// Paints a line of tiles into a TileMapStore layer using Bresenham's algorithm.
///
/// Returns the list of cells painted.
pub fn paint_line_store(
    store: &mut TileMapStore,
    node_id: NodeId,
    layer: usize,
    from: Vector2i,
    to: Vector2i,
    tile_data: TileData,
) -> Vec<Vector2i> {
    let mut painted = Vec::new();
    let mut x0 = from.x;
    let mut y0 = from.y;
    let x1 = to.x;
    let y1 = to.y;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        let coords = Vector2i::new(x0, y0);
        if set_cell(store, node_id, layer, coords, tile_data.clone()) {
            painted.push(coords);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    painted
}

/// Paints a filled rectangle into a TileMapStore layer.
///
/// Returns the list of cells painted.
pub fn paint_rect_store(
    store: &mut TileMapStore,
    node_id: NodeId,
    layer: usize,
    a: Vector2i,
    b: Vector2i,
    tile_data: TileData,
) -> Vec<Vector2i> {
    let x_min = a.x.min(b.x);
    let x_max = a.x.max(b.x);
    let y_min = a.y.min(b.y);
    let y_max = a.y.max(b.y);

    let mut painted = Vec::new();
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let coords = Vector2i::new(x, y);
            if set_cell(store, node_id, layer, coords, tile_data.clone()) {
                painted.push(coords);
            }
        }
    }
    painted
}

/// Erases a line of cells from a TileMapStore layer.
///
/// Returns the list of cells that had data removed.
pub fn erase_line_store(
    store: &mut TileMapStore,
    node_id: NodeId,
    layer: usize,
    from: Vector2i,
    to: Vector2i,
) -> Vec<Vector2i> {
    let mut erased = Vec::new();
    let mut x0 = from.x;
    let mut y0 = from.y;
    let x1 = to.x;
    let y1 = to.y;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        let coords = Vector2i::new(x0, y0);
        if erase_cell(store, node_id, layer, coords).is_some() {
            erased.push(coords);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    erased
}

/// Erases a filled rectangle of cells from a TileMapStore layer.
///
/// Returns the list of cells that had data removed.
pub fn erase_rect_store(
    store: &mut TileMapStore,
    node_id: NodeId,
    layer: usize,
    a: Vector2i,
    b: Vector2i,
) -> Vec<Vector2i> {
    let x_min = a.x.min(b.x);
    let x_max = a.x.max(b.x);
    let y_min = a.y.min(b.y);
    let y_max = a.y.max(b.y);

    let mut erased = Vec::new();
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let coords = Vector2i::new(x, y);
            if erase_cell(store, node_id, layer, coords).is_some() {
                erased.push(coords);
            }
        }
    }
    erased
}

// ---------------------------------------------------------------------------
// Terrain & Autotile
// ---------------------------------------------------------------------------

/// Terrain matching mode, mirroring Godot's `TileSet.TerrainMode`.
///
/// Controls which neighbor cells are examined when computing the bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerrainMode {
    /// Match all 8 neighbors (corners + edges) — 8-bit bitmask.
    #[default]
    MatchCornersAndSides,
    /// Match 4 cardinal neighbors only — 4-bit bitmask.
    MatchSides,
    /// Match 4 corner neighbors only — 4-bit bitmask.
    MatchCorners,
}

/// Neighbor directions for bitmask computation.
///
/// Bit positions follow Godot's convention (clockwise from top-left):
/// ```text
///  0(TL)  1(T)  2(TR)
///  3(L)   X     4(R)
///  5(BL)  6(B)  7(BR)
/// ```
const NEIGHBOR_OFFSETS_8: [(i32, i32); 8] = [
    (-1, -1), // 0: top-left
    (0, -1),  // 1: top
    (1, -1),  // 2: top-right
    (-1, 0),  // 3: left
    (1, 0),   // 4: right
    (-1, 1),  // 5: bottom-left
    (0, 1),   // 6: bottom
    (1, 1),   // 7: bottom-right
];

/// Cardinal-only neighbor offsets (T, R, B, L) — bits 0..3.
const NEIGHBOR_OFFSETS_4_SIDES: [(i32, i32); 4] = [
    (0, -1),  // 0: top
    (1, 0),   // 1: right
    (0, 1),   // 2: bottom
    (-1, 0),  // 3: left
];

/// Corner-only neighbor offsets (TL, TR, BR, BL) — bits 0..3.
const NEIGHBOR_OFFSETS_4_CORNERS: [(i32, i32); 4] = [
    (-1, -1), // 0: top-left
    (1, -1),  // 1: top-right
    (1, 1),   // 2: bottom-right
    (-1, 1),  // 3: bottom-left
];

/// A single terrain type definition.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainType {
    /// Human-readable name (e.g., "Grass", "Water", "Dirt").
    pub name: String,
    /// Terrain type ID (unique within a TerrainSet).
    pub id: i32,
    /// Bitmask-to-tile mapping: maps a neighbor bitmask to (source_id, atlas_coords).
    ///
    /// The bitmask encodes which neighboring cells share this terrain type.
    /// Missing entries fall back to the best partial match.
    pub bitmask_tiles: HashMap<u8, (i32, Vector2i)>,
}

impl TerrainType {
    /// Creates a new terrain type with no tile mappings.
    pub fn new(id: i32, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id,
            bitmask_tiles: HashMap::new(),
        }
    }

    /// Adds a bitmask → tile mapping.
    pub fn add_tile(&mut self, bitmask: u8, source_id: i32, atlas_coords: Vector2i) {
        self.bitmask_tiles.insert(bitmask, (source_id, atlas_coords));
    }
}

/// A set of terrain types with matching mode configuration.
///
/// Mirrors Godot's terrain system within a TileSet: each terrain type
/// defines which tile should be used based on the surrounding terrain pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainSet {
    /// How neighbors are matched.
    pub mode: TerrainMode,
    /// Terrain types in this set, keyed by terrain ID.
    pub terrains: HashMap<i32, TerrainType>,
}

impl TerrainSet {
    /// Creates a new terrain set with the given matching mode.
    pub fn new(mode: TerrainMode) -> Self {
        Self {
            mode,
            terrains: HashMap::new(),
        }
    }

    /// Adds a terrain type.
    pub fn add_terrain(&mut self, terrain: TerrainType) {
        self.terrains.insert(terrain.id, terrain);
    }

    /// Returns a terrain type by ID.
    pub fn get_terrain(&self, id: i32) -> Option<&TerrainType> {
        self.terrains.get(&id)
    }
}

/// Per-cell terrain assignment in a layer.
///
/// Maps cell coordinates to the terrain type ID painted there.
/// This is separate from the visual tile data — the terrain brush
/// first writes to the terrain map, then resolves each cell's visual
/// tile from the bitmask.
pub type TerrainMap = HashMap<Vector2i, i32>;

/// Computes the bitmask for a cell based on which neighbors share its terrain.
///
/// Returns `None` if the cell has no terrain assigned.
pub fn compute_bitmask(
    terrain_map: &TerrainMap,
    cell: Vector2i,
    mode: TerrainMode,
) -> Option<u8> {
    let terrain_id = *terrain_map.get(&cell)?;
    let offsets: &[(i32, i32)] = match mode {
        TerrainMode::MatchCornersAndSides => &NEIGHBOR_OFFSETS_8,
        TerrainMode::MatchSides => &NEIGHBOR_OFFSETS_4_SIDES,
        TerrainMode::MatchCorners => &NEIGHBOR_OFFSETS_4_CORNERS,
    };
    let mut mask: u8 = 0;
    for (bit, &(dx, dy)) in offsets.iter().enumerate() {
        let neighbor = Vector2i::new(cell.x + dx, cell.y + dy);
        if terrain_map.get(&neighbor) == Some(&terrain_id) {
            mask |= 1 << bit;
        }
    }
    Some(mask)
}

/// Resolves the best tile for a cell given its bitmask and terrain type.
///
/// Exact match is preferred. If no exact match exists, finds the entry whose
/// bitmask is a subset of the target (most bits in common wins). Returns `None`
/// if no mapping exists at all.
pub fn resolve_terrain_tile(
    terrain: &TerrainType,
    bitmask: u8,
) -> Option<(i32, Vector2i)> {
    // Exact match
    if let Some(&tile) = terrain.bitmask_tiles.get(&bitmask) {
        return Some(tile);
    }
    // Best partial match: find the bitmask whose bits are all within our mask,
    // with the most bits set.
    let mut best: Option<(u8, (i32, Vector2i))> = None;
    for (&candidate_mask, &tile) in &terrain.bitmask_tiles {
        if candidate_mask & bitmask == candidate_mask {
            let bits = candidate_mask.count_ones();
            if best.map_or(true, |(_, _)| bits > best.unwrap().0.count_ones()) {
                best = Some((candidate_mask, tile));
            }
        }
    }
    best.map(|(_, tile)| tile)
}

/// Paints terrain at the given cells and updates all affected neighbors.
///
/// 1. Assigns `terrain_id` to each cell in `cells`.
/// 2. For each painted cell and its neighbors, recomputes the bitmask and
///    resolves the visual tile via `resolve_terrain_tile`.
/// 3. Returns a list of `(cell, source_id, atlas_coords)` for all cells
///    whose visual tile was resolved (for the caller to apply to the TileMap).
pub fn paint_terrain(
    terrain_map: &mut TerrainMap,
    terrain_set: &TerrainSet,
    terrain_id: i32,
    cells: &[Vector2i],
) -> Vec<(Vector2i, i32, Vector2i)> {
    // Step 1: assign terrain
    for &cell in cells {
        terrain_map.insert(cell, terrain_id);
    }

    // Step 2: collect all cells that need visual update (painted + neighbors)
    let mut dirty = std::collections::HashSet::new();
    let offsets: &[(i32, i32)] = match terrain_set.mode {
        TerrainMode::MatchCornersAndSides => &NEIGHBOR_OFFSETS_8,
        TerrainMode::MatchSides => &NEIGHBOR_OFFSETS_4_SIDES,
        TerrainMode::MatchCorners => &NEIGHBOR_OFFSETS_4_CORNERS,
    };
    for &cell in cells {
        dirty.insert(cell);
        for &(dx, dy) in offsets {
            let neighbor = Vector2i::new(cell.x + dx, cell.y + dy);
            if terrain_map.contains_key(&neighbor) {
                dirty.insert(neighbor);
            }
        }
    }

    // Step 3: resolve tiles
    let mut results = Vec::new();
    for cell in dirty {
        if let Some(bitmask) = compute_bitmask(terrain_map, cell, terrain_set.mode) {
            let cell_terrain = terrain_map[&cell];
            if let Some(terrain) = terrain_set.get_terrain(cell_terrain) {
                if let Some((source_id, atlas_coords)) = resolve_terrain_tile(terrain, bitmask) {
                    results.push((cell, source_id, atlas_coords));
                }
            }
        }
    }
    results
}

/// Erases terrain at the given cells and updates all affected neighbors.
///
/// Returns the list of cells that need visual tile updates (neighbors whose
/// bitmask changed), along with their new resolved tiles.
pub fn erase_terrain(
    terrain_map: &mut TerrainMap,
    terrain_set: &TerrainSet,
    cells: &[Vector2i],
) -> Vec<(Vector2i, i32, Vector2i)> {
    // Collect neighbors before erasing
    let offsets: &[(i32, i32)] = match terrain_set.mode {
        TerrainMode::MatchCornersAndSides => &NEIGHBOR_OFFSETS_8,
        TerrainMode::MatchSides => &NEIGHBOR_OFFSETS_4_SIDES,
        TerrainMode::MatchCorners => &NEIGHBOR_OFFSETS_4_CORNERS,
    };
    let mut dirty = std::collections::HashSet::new();
    for &cell in cells {
        for &(dx, dy) in offsets {
            let neighbor = Vector2i::new(cell.x + dx, cell.y + dy);
            if terrain_map.contains_key(&neighbor) {
                dirty.insert(neighbor);
            }
        }
    }

    // Erase
    for &cell in cells {
        terrain_map.remove(&cell);
        dirty.remove(&cell);
    }

    // Re-resolve dirty neighbors
    let mut results = Vec::new();
    for cell in dirty {
        if let Some(bitmask) = compute_bitmask(terrain_map, cell, terrain_set.mode) {
            let cell_terrain = terrain_map[&cell];
            if let Some(terrain) = terrain_set.get_terrain(cell_terrain) {
                if let Some((source_id, atlas_coords)) = resolve_terrain_tile(terrain, bitmask) {
                    results.push((cell, source_id, atlas_coords));
                }
            }
        }
    }
    results
}

/// Terrain brush mode for the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerrainBrushMode {
    /// Paint terrain on single cells.
    #[default]
    Single,
    /// Paint terrain along a line.
    Line,
    /// Paint a filled rectangle of terrain.
    Rect,
}

/// Editor brush for terrain painting.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainBrush {
    /// Which terrain type to paint.
    pub terrain_id: i32,
    /// Painting mode.
    pub mode: TerrainBrushMode,
    /// Target layer index.
    pub layer: usize,
}

impl TerrainBrush {
    /// Creates a new terrain brush for the given terrain type.
    pub fn new(terrain_id: i32) -> Self {
        Self {
            terrain_id,
            mode: TerrainBrushMode::Single,
            layer: 0,
        }
    }
}

/// Applies the terrain brush and returns cells to update visually.
///
/// Uses Bresenham-style line rasterization for `Line` mode to collect cells
/// between `from` and `to`. For `Single` mode, only `from` is used.
pub fn apply_terrain_brush(
    terrain_map: &mut TerrainMap,
    terrain_set: &TerrainSet,
    brush: &TerrainBrush,
    from: Vector2i,
    to: Vector2i,
) -> Vec<(Vector2i, i32, Vector2i)> {
    let cells = match brush.mode {
        TerrainBrushMode::Single => vec![from],
        TerrainBrushMode::Line => bresenham_cells(from, to),
        TerrainBrushMode::Rect => {
            let x_min = from.x.min(to.x);
            let x_max = from.x.max(to.x);
            let y_min = from.y.min(to.y);
            let y_max = from.y.max(to.y);
            let mut cells = Vec::new();
            for y in y_min..=y_max {
                for x in x_min..=x_max {
                    cells.push(Vector2i::new(x, y));
                }
            }
            cells
        }
    };
    paint_terrain(terrain_map, terrain_set, brush.terrain_id, &cells)
}

/// Bresenham line rasterization between two grid cells.
fn bresenham_cells(from: Vector2i, to: Vector2i) -> Vec<Vector2i> {
    let mut cells = Vec::new();
    let dx = (to.x - from.x).abs();
    let dy = -(to.y - from.y).abs();
    let sx = if from.x < to.x { 1 } else { -1 };
    let sy = if from.y < to.y { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = from.x;
    let mut y = from.y;
    loop {
        cells.push(Vector2i::new(x, y));
        if x == to.x && y == to.y {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    cells
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

    #[test]
    fn default_tileset_8() {
        assert_eq!(default_color_tileset().tiles.len(), 8);
    }
    #[test]
    fn grid_get_set() {
        let mut g = TileGrid::new(5, 5);
        g.set(2, 3, 1);
        assert_eq!(g.get(2, 3), Some(1));
    }
    #[test]
    fn grid_fill() {
        let mut g = TileGrid::new(10, 10);
        assert_eq!(g.fill_rect(2, 2, 4, 4, 3), 9);
    }
    #[test]
    fn grid_resize() {
        let mut g = TileGrid::new(5, 5);
        g.set(1, 1, 7);
        g.resize(10, 10);
        assert_eq!(g.get(1, 1), Some(7));
    }
    #[test]
    fn grid_json_rt() {
        let mut g = TileGrid::new(4, 3);
        g.set(0, 0, 1);
        assert_eq!(TileGrid::from_json(&g.to_json()).unwrap(), g);
    }
    #[test]
    fn grid_store() {
        let mut s = TileGridStore::new_with_defaults();
        let n = NodeId::next();
        s.insert(n, TileGrid::new(8, 8));
        assert!(s.get(n).is_some());
    }

    // -- Terrain & Autotile ------------------------------------------------

    fn make_terrain_set_4side() -> TerrainSet {
        let mut ts = TerrainSet::new(TerrainMode::MatchSides);
        let mut grass = TerrainType::new(0, "Grass");
        // 0b0000 = isolated, 0b1111 = all sides, etc.
        grass.add_tile(0b0000, 1, Vector2i::new(0, 0)); // isolated
        grass.add_tile(0b1111, 1, Vector2i::new(1, 1)); // center (all sides)
        grass.add_tile(0b0110, 1, Vector2i::new(1, 0)); // right + bottom
        grass.add_tile(0b1010, 1, Vector2i::new(0, 1)); // top + bottom
        grass.add_tile(0b0101, 1, Vector2i::new(2, 0)); // right + left (horizontal)
        ts.add_terrain(grass);
        ts
    }

    fn make_terrain_set_8() -> TerrainSet {
        let mut ts = TerrainSet::new(TerrainMode::MatchCornersAndSides);
        let mut water = TerrainType::new(1, "Water");
        water.add_tile(0b00000000, 2, Vector2i::new(0, 0)); // isolated
        water.add_tile(0b11111111, 2, Vector2i::new(3, 3)); // fully surrounded
        water.add_tile(0b01000010, 2, Vector2i::new(1, 0)); // top + bottom edges
        ts.add_terrain(water);
        ts
    }

    #[test]
    fn terrain_type_add_and_lookup() {
        let mut t = TerrainType::new(0, "Grass");
        t.add_tile(0b0000, 1, Vector2i::new(0, 0));
        t.add_tile(0b1111, 1, Vector2i::new(1, 1));
        assert_eq!(t.bitmask_tiles.len(), 2);
        assert_eq!(t.bitmask_tiles[&0b0000], (1, Vector2i::new(0, 0)));
    }

    #[test]
    fn terrain_set_add_and_get() {
        let ts = make_terrain_set_4side();
        assert!(ts.get_terrain(0).is_some());
        assert!(ts.get_terrain(99).is_none());
        assert_eq!(ts.get_terrain(0).unwrap().name, "Grass");
    }

    #[test]
    fn compute_bitmask_isolated_cell() {
        let mut map = TerrainMap::new();
        map.insert(Vector2i::new(5, 5), 0);
        let mask = compute_bitmask(&map, Vector2i::new(5, 5), TerrainMode::MatchSides);
        assert_eq!(mask, Some(0b0000));
    }

    #[test]
    fn compute_bitmask_no_terrain() {
        let map = TerrainMap::new();
        assert_eq!(compute_bitmask(&map, Vector2i::ZERO, TerrainMode::MatchSides), None);
    }

    #[test]
    fn compute_bitmask_all_sides() {
        let mut map = TerrainMap::new();
        let center = Vector2i::new(5, 5);
        map.insert(center, 0);
        // Add all 4 cardinal neighbors
        map.insert(Vector2i::new(5, 4), 0); // top
        map.insert(Vector2i::new(6, 5), 0); // right
        map.insert(Vector2i::new(5, 6), 0); // bottom
        map.insert(Vector2i::new(4, 5), 0); // left
        let mask = compute_bitmask(&map, center, TerrainMode::MatchSides).unwrap();
        assert_eq!(mask, 0b1111);
    }

    #[test]
    fn compute_bitmask_partial() {
        let mut map = TerrainMap::new();
        let center = Vector2i::new(5, 5);
        map.insert(center, 0);
        map.insert(Vector2i::new(5, 4), 0); // top (bit 0)
        map.insert(Vector2i::new(5, 6), 0); // bottom (bit 2)
        let mask = compute_bitmask(&map, center, TerrainMode::MatchSides).unwrap();
        assert_eq!(mask, 0b0101); // top + bottom
    }

    #[test]
    fn compute_bitmask_different_terrain_ignored() {
        let mut map = TerrainMap::new();
        let center = Vector2i::new(5, 5);
        map.insert(center, 0);
        map.insert(Vector2i::new(5, 4), 1); // different terrain type
        let mask = compute_bitmask(&map, center, TerrainMode::MatchSides).unwrap();
        assert_eq!(mask, 0b0000); // neighbor not counted
    }

    #[test]
    fn compute_bitmask_8way() {
        let mut map = TerrainMap::new();
        let center = Vector2i::new(5, 5);
        map.insert(center, 1);
        map.insert(Vector2i::new(5, 4), 1); // top (bit 1)
        map.insert(Vector2i::new(5, 6), 1); // bottom (bit 6)
        let mask = compute_bitmask(&map, center, TerrainMode::MatchCornersAndSides).unwrap();
        assert_eq!(mask, 0b01000010); // bit 1 (top) + bit 6 (bottom)
    }

    #[test]
    fn resolve_terrain_tile_exact() {
        let ts = make_terrain_set_4side();
        let grass = ts.get_terrain(0).unwrap();
        let tile = resolve_terrain_tile(grass, 0b1111);
        assert_eq!(tile, Some((1, Vector2i::new(1, 1))));
    }

    #[test]
    fn resolve_terrain_tile_partial_fallback() {
        let ts = make_terrain_set_4side();
        let grass = ts.get_terrain(0).unwrap();
        // 0b1110 = top+right+bottom but not left — no exact match
        // Should fallback to best subset: 0b0110 (right+bottom, 2 bits) or 0b1010 (top+bottom, 2 bits)
        let tile = resolve_terrain_tile(grass, 0b1110);
        assert!(tile.is_some());
        let (source_id, _) = tile.unwrap();
        assert_eq!(source_id, 1);
    }

    #[test]
    fn resolve_terrain_tile_no_match() {
        let terrain = TerrainType::new(0, "Empty");
        // No bitmask_tiles at all
        assert_eq!(resolve_terrain_tile(&terrain, 0b1111), None);
    }

    #[test]
    fn paint_terrain_single_cell() {
        let mut map = TerrainMap::new();
        let ts = make_terrain_set_4side();
        let results = paint_terrain(&mut map, &ts, 0, &[Vector2i::new(5, 5)]);
        assert!(!results.is_empty());
        // Cell should be in the terrain map
        assert_eq!(map.get(&Vector2i::new(5, 5)), Some(&0));
    }

    #[test]
    fn paint_terrain_updates_neighbors() {
        let mut map = TerrainMap::new();
        let ts = make_terrain_set_4side();
        // Paint a 3-cell horizontal line
        paint_terrain(&mut map, &ts, 0, &[Vector2i::new(4, 5)]);
        paint_terrain(&mut map, &ts, 0, &[Vector2i::new(5, 5)]);
        let results = paint_terrain(&mut map, &ts, 0, &[Vector2i::new(6, 5)]);
        // Should include updates for the middle cell (now has left+right neighbors)
        let updated_cells: Vec<Vector2i> = results.iter().map(|(c, _, _)| *c).collect();
        assert!(updated_cells.contains(&Vector2i::new(5, 5)),
            "Middle cell should be updated when neighbors change");
    }

    #[test]
    fn erase_terrain_removes_and_updates_neighbors() {
        let mut map = TerrainMap::new();
        let ts = make_terrain_set_4side();
        // Paint a cross
        let cells = vec![
            Vector2i::new(5, 5),
            Vector2i::new(5, 4),
            Vector2i::new(5, 6),
            Vector2i::new(4, 5),
            Vector2i::new(6, 5),
        ];
        paint_terrain(&mut map, &ts, 0, &cells);
        assert_eq!(map.len(), 5);

        // Erase the center
        let results = erase_terrain(&mut map, &ts, &[Vector2i::new(5, 5)]);
        assert!(!map.contains_key(&Vector2i::new(5, 5)));
        assert_eq!(map.len(), 4);
        // Neighbors should be updated
        let updated_cells: Vec<Vector2i> = results.iter().map(|(c, _, _)| *c).collect();
        assert!(!updated_cells.is_empty(), "Neighbors should be re-resolved after erase");
    }

    #[test]
    fn terrain_brush_single_mode() {
        let mut map = TerrainMap::new();
        let ts = make_terrain_set_4side();
        let brush = TerrainBrush::new(0);
        let results = apply_terrain_brush(
            &mut map, &ts, &brush,
            Vector2i::new(3, 3), Vector2i::new(3, 3),
        );
        assert!(!results.is_empty());
        assert_eq!(map.get(&Vector2i::new(3, 3)), Some(&0));
    }

    #[test]
    fn terrain_brush_line_mode() {
        let mut map = TerrainMap::new();
        let ts = make_terrain_set_4side();
        let brush = TerrainBrush {
            terrain_id: 0,
            mode: TerrainBrushMode::Line,
            layer: 0,
        };
        let results = apply_terrain_brush(
            &mut map, &ts, &brush,
            Vector2i::new(0, 0), Vector2i::new(4, 0),
        );
        // Should have painted 5 cells in a horizontal line
        assert_eq!(map.len(), 5);
        for x in 0..5 {
            assert!(map.contains_key(&Vector2i::new(x, 0)));
        }
        assert!(!results.is_empty());
    }

    #[test]
    fn terrain_brush_rect_mode() {
        let mut map = TerrainMap::new();
        let ts = make_terrain_set_4side();
        let brush = TerrainBrush {
            terrain_id: 0,
            mode: TerrainBrushMode::Rect,
            layer: 0,
        };
        let results = apply_terrain_brush(
            &mut map, &ts, &brush,
            Vector2i::new(0, 0), Vector2i::new(2, 2),
        );
        // 3x3 rect = 9 cells
        assert_eq!(map.len(), 9);
        assert!(!results.is_empty());
    }

    #[test]
    fn bresenham_horizontal() {
        let cells = bresenham_cells(Vector2i::new(0, 0), Vector2i::new(3, 0));
        assert_eq!(cells.len(), 4);
        for (i, c) in cells.iter().enumerate() {
            assert_eq!(c.x, i as i32);
            assert_eq!(c.y, 0);
        }
    }

    #[test]
    fn bresenham_vertical() {
        let cells = bresenham_cells(Vector2i::new(0, 0), Vector2i::new(0, 3));
        assert_eq!(cells.len(), 4);
        for (i, c) in cells.iter().enumerate() {
            assert_eq!(c.x, 0);
            assert_eq!(c.y, i as i32);
        }
    }

    #[test]
    fn bresenham_diagonal() {
        let cells = bresenham_cells(Vector2i::new(0, 0), Vector2i::new(3, 3));
        assert_eq!(cells.len(), 4);
        for (i, c) in cells.iter().enumerate() {
            assert_eq!(c.x, i as i32);
            assert_eq!(c.y, i as i32);
        }
    }

    #[test]
    fn bresenham_single_cell() {
        let cells = bresenham_cells(Vector2i::new(5, 5), Vector2i::new(5, 5));
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0], Vector2i::new(5, 5));
    }

    #[test]
    fn terrain_mode_corners_only() {
        let mut ts = TerrainSet::new(TerrainMode::MatchCorners);
        let mut t = TerrainType::new(0, "Test");
        t.add_tile(0b0000, 1, Vector2i::ZERO);
        t.add_tile(0b1111, 1, Vector2i::new(1, 1));
        ts.add_terrain(t);

        let mut map = TerrainMap::new();
        let center = Vector2i::new(5, 5);
        map.insert(center, 0);
        // Add all 4 corner neighbors
        map.insert(Vector2i::new(4, 4), 0); // TL
        map.insert(Vector2i::new(6, 4), 0); // TR
        map.insert(Vector2i::new(6, 6), 0); // BR
        map.insert(Vector2i::new(4, 6), 0); // BL
        let mask = compute_bitmask(&map, center, TerrainMode::MatchCorners).unwrap();
        assert_eq!(mask, 0b1111);
    }

    #[test]
    fn terrain_8way_full_surround() {
        let ts = make_terrain_set_8();
        let mut map = TerrainMap::new();
        let center = Vector2i::new(5, 5);
        // Fill all 8 neighbors + center
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                map.insert(Vector2i::new(5 + dx, 5 + dy), 1);
            }
        }
        let mask = compute_bitmask(&map, center, TerrainMode::MatchCornersAndSides).unwrap();
        assert_eq!(mask, 0b11111111);
        let tile = resolve_terrain_tile(ts.get_terrain(1).unwrap(), mask);
        assert_eq!(tile, Some((2, Vector2i::new(3, 3))));
    }
}
