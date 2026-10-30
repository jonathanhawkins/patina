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
// Atlas TileSet — the Godot-shaped resource model (pat-0q2kx.108)
// ---------------------------------------------------------------------------

/// One `TileSetAtlasSource`: a texture carved into a uniform grid of tiles.
///
/// Mirrors `scene/resources/2d/tile_set.cpp` `TileSetAtlasSource`, whose region
/// for atlas coordinates `(cx, cy)` is
/// `margins + (cx, cy) * (texture_region_size + separation)`, sized
/// `texture_region_size`.
///
/// [`TileDefinition`] above models a *single* region per source; a real Godot
/// atlas addresses many tiles in one texture by atlas coordinate, which is what
/// `tile_map_data` cells actually reference.
#[derive(Debug, Clone, PartialEq)]
pub struct TileSetAtlasSource {
    /// Path to the atlas texture (`res://…`).
    pub texture_path: String,
    /// Size of one tile's region in the atlas, in pixels.
    pub texture_region_size: Vector2i,
    /// Offset of the first tile's top-left corner within the texture.
    pub margins: Vector2i,
    /// Gap between adjacent tile regions.
    pub separation: Vector2i,
}

impl TileSetAtlasSource {
    /// Creates an atlas source with Godot's default margins and separation.
    pub fn new(texture_path: impl Into<String>, texture_region_size: Vector2i) -> Self {
        Self {
            texture_path: texture_path.into(),
            texture_region_size,
            margins: Vector2i::ZERO,
            separation: Vector2i::ZERO,
        }
    }

    /// The texture sub-rectangle for the tile at `atlas_coords`.
    ///
    /// Port of `TileSetAtlasSource::get_tile_texture_region`.
    pub fn tile_region(&self, atlas_coords: Vector2i) -> Rect2 {
        let step_x = self.texture_region_size.x + self.separation.x;
        let step_y = self.texture_region_size.y + self.separation.y;
        Rect2::new(
            Vector2::new(
                (self.margins.x + atlas_coords.x * step_x) as f32,
                (self.margins.y + atlas_coords.y * step_y) as f32,
            ),
            Vector2::new(
                self.texture_region_size.x as f32,
                self.texture_region_size.y as f32,
            ),
        )
    }
}

/// A Godot-shaped `TileSet`: a cell size plus atlas sources keyed by source id.
///
/// This is what a `.tscn`-authored `tile_set = SubResource(...)` resolves to.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasTileSet {
    /// `TileSet.tile_size` — the size of one map cell in pixels.
    pub tile_size: Vector2i,
    /// Atlas sources by `source_id` (the key `tile_map_data` cells reference).
    pub sources: HashMap<i32, TileSetAtlasSource>,
}

impl AtlasTileSet {
    /// Creates an empty tile set with the given cell size.
    pub fn new(tile_size: Vector2i) -> Self {
        Self {
            tile_size,
            sources: HashMap::new(),
        }
    }

    /// The texture path and sub-rectangle a cell's [`TileData`] resolves to, or
    /// `None` when its `source_id` names no atlas source.
    pub fn tile_texture_region(&self, data: &TileData) -> Option<(&str, Rect2)> {
        let source = self.sources.get(&data.source_id)?;
        Some((
            source.texture_path.as_str(),
            source.tile_region(data.atlas_coords),
        ))
    }
}

/// Property prefix under which `packed_scene` flattens a resolved `tile_set`
/// sub-resource onto its `TileMapLayer` node, mirroring the `__shape__*` and
/// `__curve__*` companions used for other sub-resources.
pub const TILE_SET_PREFIX: &str = "__tile_set__";

/// Reads the flattened `__tile_set__*` companion properties off a node back
/// into an [`AtlasTileSet`].
///
/// Returns `None` when the node carries no tile set at all, so a caller can
/// tell "no tile set authored" from "an empty one".
pub fn tile_set_from_properties(
    get: impl Fn(&str) -> Variant,
    source_ids: &[i32],
) -> Option<AtlasTileSet> {
    let vec2i = |key: &str| -> Option<Vector2i> {
        match get(key) {
            Variant::Vector2i(v) => Some(v),
            Variant::Vector2(v) => Some(Vector2i::new(v.x as i32, v.y as i32)),
            _ => None,
        }
    };
    let tile_size = vec2i(TILE_SET_TILE_SIZE)?;
    let mut set = AtlasTileSet::new(tile_size);
    for &id in source_ids {
        // Keys come from `tile_set_source_key` rather than a local `format!`,
        // so the reader here and the TileSet editor that writes them cannot
        // drift apart (pat-s3rrv.65).
        let Variant::String(texture_path) = get(&tile_set_source_key(id, "texture")) else {
            continue;
        };
        // Godot's atlas default is 16x16; a source that omits the property
        // still addresses tiles on that grid.
        let region = vec2i(&tile_set_source_key(id, "texture_region_size"))
            .unwrap_or_else(|| Vector2i::new(16, 16));
        set.sources.insert(
            id,
            TileSetAtlasSource {
                texture_path,
                texture_region_size: region,
                margins: vec2i(&tile_set_source_key(id, "margins")).unwrap_or(Vector2i::ZERO),
                separation: vec2i(&tile_set_source_key(id, "separation")).unwrap_or(Vector2i::ZERO),
            },
        );
    }
    Some(set)
}

/// The source ids a node's flattened tile set declares, as written by
/// `packed_scene` into `__tile_set__source_ids`.
pub fn tile_set_source_ids(value: &Variant) -> Vec<i32> {
    match value {
        Variant::PackedInt32Array(ids) => ids.clone(),
        Variant::Array(items) => items
            .to_vec()
            .iter()
            .filter_map(|v| match v {
                Variant::Int(i) => Some(*i as i32),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

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

/// Decodes a Godot `TileMapLayer.tile_map_data` PackedByteArray into its
/// painted cells (pat-nzgdu.52).
///
/// Binary layout, matching upstream `TileMapLayer::set_tile_map_data_from_array`
/// (scene/2d/tile_map_layer.cpp): a little-endian `u16` format-version header
/// followed by fixed 12-byte cell records until the end of the buffer —
/// `[i16 x][i16 y][u16 source_id][u16 atlas_x][u16 atlas_y][u16 alternative]`.
/// Cell coordinates are signed; the tile identifiers are unsigned. The cell
/// layout does not vary by format version, so the header is skipped. A buffer
/// shorter than the 2-byte header (or with a trailing partial record) yields no
/// further cells rather than panicking.
pub fn decode_tile_map_data(bytes: &[u8]) -> HashMap<Vector2i, TileData> {
    let mut cells = HashMap::new();
    if bytes.len() < 2 {
        return cells;
    }
    // bytes[0..2] = format version (currently always 0).
    let mut index = 2usize;
    while index + 12 <= bytes.len() {
        let c = &bytes[index..index + 12];
        let x = i16::from_le_bytes([c[0], c[1]]) as i32;
        let y = i16::from_le_bytes([c[2], c[3]]) as i32;
        let source_id = u16::from_le_bytes([c[4], c[5]]) as i32;
        let atlas_x = u16::from_le_bytes([c[6], c[7]]) as i32;
        let atlas_y = u16::from_le_bytes([c[8], c[9]]) as i32;
        let alternative = u16::from_le_bytes([c[10], c[11]]) as i32;
        cells.insert(
            Vector2i::new(x, y),
            TileData::with_atlas(source_id, Vector2i::new(atlas_x, atlas_y), alternative),
        );
        index += 12;
    }
    cells
}

/// Encodes cells into Godot's `tile_map_data` PackedByteArray.
///
/// The exact inverse of [`decode_tile_map_data`]: a little-endian `u16` format
/// header (0) followed by 12-byte `[i16 x][i16 y][u16 source_id][u16 atlas_x]
/// [u16 atlas_y][u16 alternative]` records. Records are emitted in sorted cell
/// order so that re-saving an unchanged layer produces an unchanged file rather
/// than reshuffling with `HashMap` iteration order.
pub fn encode_tile_map_data(cells: &HashMap<Vector2i, TileData>) -> Vec<u8> {
    let mut keys: Vec<&Vector2i> = cells.keys().collect();
    keys.sort_by_key(|c| (c.y, c.x));

    let mut out = Vec::with_capacity(2 + keys.len() * 12);
    out.extend_from_slice(&0u16.to_le_bytes());
    for key in keys {
        let data = &cells[key];
        out.extend_from_slice(&(key.x as i16).to_le_bytes());
        out.extend_from_slice(&(key.y as i16).to_le_bytes());
        out.extend_from_slice(&(data.source_id as u16).to_le_bytes());
        out.extend_from_slice(&(data.atlas_coords.x as u16).to_le_bytes());
        out.extend_from_slice(&(data.atlas_coords.y as u16).to_le_bytes());
        out.extend_from_slice(&(data.alternative_id as u16).to_le_bytes());
    }
    out
}

impl TileMapLayer {
    /// Builds a layer named `name` whose cells are decoded from a Godot
    /// `tile_map_data` PackedByteArray (see [`decode_tile_map_data`]). This is
    /// the hook a scene loader uses to populate a TileMapLayer node's cells
    /// from its `.tscn` `tile_map_data` property.
    pub fn from_tile_map_data(name: &str, bytes: &[u8]) -> Self {
        let mut layer = Self::new(name);
        layer.cells = decode_tile_map_data(bytes);
        layer
    }
}

/// The `(width, height)` a dense grid needs to hold every cell in `bytes`.
///
/// Negative coordinates are ignored — the editor's `TileGrid` is anchored at
/// (0, 0) and has no room for them — so a layer painted into negative space
/// round-trips only its non-negative quadrant.
pub fn tile_map_data_extent(bytes: &[u8]) -> (usize, usize) {
    let mut w = 0i32;
    let mut h = 0i32;
    for cell in decode_tile_map_data(bytes).keys() {
        if cell.x >= 0 {
            w = w.max(cell.x + 1);
        }
        if cell.y >= 0 {
            h = h.max(cell.y + 1);
        }
    }
    (w as usize, h as usize)
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

// ---------------------------------------------------------------------------
// Per-tile navigation polygons (pat-0q2kx.99)
// ---------------------------------------------------------------------------

/// Node property listing the tile source ids whose tiles are walkable.
///
/// Godot stores navigation polygons per `TileData` inside the `TileSet`
/// resource (`tile_map_layer.cpp` `_navigation_update`). Patina's `PackedScene`
/// flattens sub-resources onto the node, so the TileSet's navigation layer
/// arrives as these two flat properties — the same shape
/// `NavigationRegion2D` uses for `__navmesh_vertices__`.
pub const TILE_NAV_SOURCES: &str = "__tile_nav_sources__";

/// Node property holding the **tile-local** navigation polygon, in pixels
/// relative to the tile's top-left corner. Optional: a navigation-enabled tile
/// with no explicit polygon is walkable across its whole rect, which is the
/// common authoring case and what Godot's default tile navigation polygon is.
pub const TILE_NAV_POLYGON: &str = "__tile_nav_polygon__";

/// Builds one world-space navigation polygon per painted, navigation-enabled
/// cell (pat-0q2kx.99).
///
/// Mirrors Godot's `TileMapLayer::_navigation_update`: every painted cell whose
/// tile carries navigation data contributes a region positioned at that cell's
/// world transform, and erasing the cell removes it. `origin` is the layer
/// node's global position.
///
/// ⚠️ The polygons are emitted **per cell, untouched** rather than merged: the
/// navigation mesh derives connectivity from shared edges, so two orthogonally
/// adjacent tiles stitch automatically because their rects share an edge. Union
/// them here and that edge information — the thing that makes a corridor
/// traversable — is destroyed.
pub fn tile_navigation_polygons(
    cells: &HashMap<Vector2i, TileData>,
    tile_size: Vector2i,
    nav_sources: &[i32],
    tile_local_polygon: &[Vector2],
    origin: Vector2,
) -> Vec<Vec<Vector2>> {
    if nav_sources.is_empty() || tile_size.x <= 0 || tile_size.y <= 0 {
        return Vec::new();
    }
    // Deterministic order so a path is reproducible run to run; a HashMap walk
    // is not.
    let mut painted: Vec<(&Vector2i, &TileData)> = cells.iter().collect();
    painted.sort_by_key(|(cell, _)| (cell.y, cell.x));

    let mut out = Vec::new();
    for (cell, data) in painted {
        if !nav_sources.contains(&data.source_id) {
            continue;
        }
        // Top-left corner of the cell in layer-local pixels. `map_to_local`
        // returns the cell CENTRE, so the half-tile offset must come back off
        // — using the centre directly would shift every region by half a tile
        // and open phantom gaps between neighbours.
        let corner = Vector2::new(
            cell.x as f32 * tile_size.x as f32,
            cell.y as f32 * tile_size.y as f32,
        ) + origin;

        let poly: Vec<Vector2> = if tile_local_polygon.len() >= 3 {
            tile_local_polygon.iter().map(|v| *v + corner).collect()
        } else {
            let w = tile_size.x as f32;
            let h = tile_size.y as f32;
            vec![
                corner,
                corner + Vector2::new(w, 0.0),
                corner + Vector2::new(w, h),
                corner + Vector2::new(0.0, h),
            ]
        };
        out.push(poly);
    }
    out
}

// ---------------------------------------------------------------------------
// Per-tile light occlusion (pat-s3rrv.65)
// ---------------------------------------------------------------------------

/// Node property listing the tile source ids whose tiles cast 2D shadows.
///
/// Godot stores an occlusion polygon per `TileData` under
/// `occlusion_layer_<n>/polygon`; Patina's `PackedScene` flattens sub-resources
/// onto the node, so it arrives as this pair — the same shape
/// [`TILE_NAV_SOURCES`] uses for the navigation layer.
pub const TILE_OCCLUDER_SOURCES: &str = "__tile_occluder_sources__";

/// Node property holding the **tile-local** occluder polygon, in pixels
/// relative to the tile's top-left corner. Empty means the whole tile occludes,
/// which is Godot's default shape for an occlusion-enabled tile.
pub const TILE_OCCLUDER_POLYGON: &str = "__tile_occluder_polygon__";

/// Builds one occluder polygon per painted, occlusion-enabled cell
/// (pat-s3rrv.65).
///
/// The occlusion twin of [`tile_navigation_polygons`], and deliberately the
/// same shape: same top-left-relative tile-local polygon, same per-cell
/// emission, same whole-tile default.
///
/// ⚠️ Returned in LAYER-LOCAL space, NOT world space — unlike
/// `collect_occluders_2d`, whose caller applies the node's global transform.
/// A `TileMapLayer` under a moving parent has to cast its shadows where the
/// parent puts it, and the transform is the caller's to apply because only it
/// knows the node.
pub fn tile_occluder_polygons(
    cells: &HashMap<Vector2i, TileData>,
    tile_size: Vector2i,
    occluder_sources: &[i32],
    tile_local_polygon: &[Vector2],
) -> Vec<Vec<Vector2>> {
    if occluder_sources.is_empty() || tile_size.x <= 0 || tile_size.y <= 0 {
        return Vec::new();
    }
    // Deterministic order so a shadow pass is reproducible run to run.
    let mut painted: Vec<(&Vector2i, &TileData)> = cells.iter().collect();
    painted.sort_by_key(|(cell, _)| (cell.y, cell.x));

    let mut out = Vec::new();
    for (cell, data) in painted {
        if !occluder_sources.contains(&data.source_id) {
            continue;
        }
        let corner = Vector2::new(
            cell.x as f32 * tile_size.x as f32,
            cell.y as f32 * tile_size.y as f32,
        );
        let poly: Vec<Vector2> = if tile_local_polygon.len() >= 3 {
            tile_local_polygon.iter().map(|v| *v + corner).collect()
        } else {
            let w = tile_size.x as f32;
            let h = tile_size.y as f32;
            vec![
                corner,
                corner + Vector2::new(w, 0.0),
                corner + Vector2::new(w, h),
                corner + Vector2::new(0.0, h),
            ]
        };
        out.push(poly);
    }
    out
}

// ---------------------------------------------------------------------------
// Per-tile physics polygons (pat-0q2kx.117)
// ---------------------------------------------------------------------------

/// Node property listing the tile source ids whose tiles carry collision.
///
/// Godot stores collision polygons per `TileData` inside the `TileSet`, keyed by
/// atlas coordinate (`<ax>:<ay>/<alt>/physics_layer_<n>/polygon_<m>/points` on
/// the `TileSetAtlasSource`). `PackedScene` flattens those onto the node, the
/// same way `__tile_nav_sources__` carries the navigation layer.
pub const TILE_PHYSICS_SOURCES: &str = "__tile_physics_sources__";

/// Prefix of the flattened per-tile collision companions. The full keys are
/// `__tile_physics_<source>_<ax>_<ay>_points` (a `PackedVector2Array` in
/// **tile-local pixels, relative to the tile centre**, which is Godot's own
/// convention) and `__tile_physics_<source>_<ax>_<ay>_one_way` (a bool).
pub const TILE_PHYSICS_PREFIX: &str = "__tile_physics_";

/// Builds the flattened property key holding a tile's collision polygon.
pub fn tile_physics_points_key(source_id: i32, atlas: Vector2i) -> String {
    format!(
        "{TILE_PHYSICS_PREFIX}{source_id}_{}_{}_points",
        atlas.x, atlas.y
    )
}

/// Builds the flattened property key holding a tile's `one_way_collision` flag.
pub fn tile_physics_one_way_key(source_id: i32, atlas: Vector2i) -> String {
    format!(
        "{TILE_PHYSICS_PREFIX}{source_id}_{}_{}_one_way",
        atlas.x, atlas.y
    )
}

/// Builds one of the flattened atlas-source companion keys, e.g.
/// `__tile_set__source_3_texture`.
///
/// The TileSet editor writes these and [`tile_set_from_properties`] reads them
/// back. Going through one builder is the point: the two used to be independent
/// `format!` calls, and a companion the editor writes under a name the loader
/// does not read is invisible rather than broken — the value survives the save
/// and simply never reaches the runtime (pat-s3rrv.65).
pub fn tile_set_source_key(source_id: i32, suffix: &str) -> String {
    format!("{TILE_SET_PREFIX}source_{source_id}_{suffix}")
}

/// Node property listing the atlas source ids the flattened tile set declares.
pub const TILE_SET_SOURCE_IDS: &str = "__tile_set__source_ids";

/// Node property holding the tile set's cell size (`TileSet.tile_size`).
pub const TILE_SET_TILE_SIZE: &str = "__tile_set__tile_size";

/// Node property listing the tile set's custom data layer names, in index
/// order — Godot's `TileSet.custom_data_layer_*` (`tile_set.cpp`).
///
/// The names matter, not just the count: `TileData.get_custom_data(name)` is
/// the API a game actually calls, so a layer without a name is unreachable
/// from script.
pub const TILE_CUSTOM_DATA_LAYERS: &str = "__tile_custom_data_layers__";

/// Node property listing each custom data layer's Godot Variant TYPE, parallel
/// to [`TILE_CUSTOM_DATA_LAYERS`] and in the same index order.
///
/// ⚠️ This exists to PRESERVE a type the file already declares, never to invent
/// one. A guessed type is worse than an absent one — it silently coerces the
/// value on load, whereas an absent type leaves the Variant as authored — so
/// layers authored in the editor carry no entry here and the saver emits no
/// `custom_data_layer_<i>/type` for them, exactly as before.
///
/// `-1` marks "the file declared this layer without a type", which keeps the
/// array parallel to the names rather than collapsing the index mapping.
/// Godot's own values are `Variant::Type` ordinals (`tile_set.cpp:4211`).
pub const TILE_CUSTOM_DATA_TYPES: &str = "__tile_custom_data_types__";

/// Prefix of the flattened per-tile custom data companions. Full keys are
/// `__tile_custom_data_<source>_<ax>_<ay>_<layer name>`.
///
/// ⚠️ Also covers [`TILE_CUSTOM_DATA_TYPES`] for `is_internal_prop` filtering,
/// which is intentional: both are companions and neither belongs in a `.tscn`
/// as a literal node property. Nothing SCANS this prefix — the per-tile keys
/// are always built by [`tile_custom_data_key`] — so there is no parser for the
/// types key to confuse.
pub const TILE_CUSTOM_DATA_PREFIX: &str = "__tile_custom_data_";

/// Builds the flattened property key holding one tile's value for one custom
/// data layer.
pub fn tile_custom_data_key(source_id: i32, atlas: Vector2i, layer: &str) -> String {
    format!(
        "{TILE_CUSTOM_DATA_PREFIX}{source_id}_{}_{}_{layer}",
        atlas.x, atlas.y
    )
}

/// Parses a `__tile_physics_<source>_<ax>_<ay>_points` key back into its atlas
/// coordinate, for the given source only.
///
/// The inverse of [`tile_physics_points_key`]. The TileSet editor needs it to
/// enumerate which tiles of a source have authored collision: the companions
/// are flat node properties, so the atlas coordinate exists only inside the
/// key's own text.
///
/// Returns `None` for any key that is not this source's points companion —
/// including the `_one_way` sibling, which shares the prefix.
pub fn tile_physics_points_atlas(key: &str, source_id: i32) -> Option<Vector2i> {
    let rest = key.strip_prefix(TILE_PHYSICS_PREFIX)?;
    let rest = rest.strip_suffix("_points")?;
    // `<source>_<ax>_<ay>`; ax/ay may be negative, so split from the right and
    // keep whatever remains as the source id.
    let (head, ay) = rest.rsplit_once('_')?;
    let (src, ax) = head.rsplit_once('_')?;
    if src.parse::<i32>().ok()? != source_id {
        return None;
    }
    Some(Vector2i::new(ax.parse().ok()?, ay.parse().ok()?))
}

/// Converts an authored collision rect into the 4-point polygon the runtime
/// consumes.
///
/// ⚠️ Tile collision polygons are in tile-local pixels **relative to the tile
/// centre**, which is Godot's convention and what
/// `gdphysics2d::tilemap_collision` expects — a full-cell box on a 16×16 tile
/// is `(-8,-8)…(8,8)`, not `(0,0)…(16,16)`. An editor that wrote corner-origin
/// points would place every collider a half-tile down and right, which still
/// collides and so looks almost right.
///
/// `rect` is given in the same centred space, so a whole-cell rect is
/// `Rect2::new((-w/2, -h/2), (w, h))`.
pub fn collision_rect_polygon(rect: Rect2) -> Vec<Vector2> {
    let min = rect.position;
    let max = Vector2::new(rect.position.x + rect.size.x, rect.position.y + rect.size.y);
    vec![
        Vector2::new(min.x, min.y),
        Vector2::new(max.x, min.y),
        Vector2::new(max.x, max.y),
        Vector2::new(min.x, max.y),
    ]
}

/// The whole-cell collision polygon for a tile of `tile_size`, centred on the
/// tile — the shape Godot's "add a full-tile collision box" button produces.
pub fn full_cell_collision_polygon(tile_size: Vector2i) -> Vec<Vector2> {
    let (w, h) = (tile_size.x as f32, tile_size.y as f32);
    collision_rect_polygon(Rect2::new(
        Vector2::new(-w / 2.0, -h / 2.0),
        Vector2::new(w, h),
    ))
}

/// Builds one [`TileCollisionInfo`] per painted cell whose tile carries
/// collision (pat-0q2kx.117).
///
/// Mirrors Godot's `TileMapLayer::_physics_update`, which creates a static body
/// per painted cell from the tile's physics layers. Only cells whose
/// `source_id` appears in `physics_sources` contribute, so erasing a cell — or
/// painting one from a source with no collision — leaves no body behind.
///
/// `get_polygon` resolves a `(source_id, atlas_coords)` pair to that tile's
/// tile-local polygon and its one-way flag. A collision-enabled tile with no
/// explicit polygon collides across its whole cell rect, which is Godot's
/// default tile collision shape and the common authoring case.
///
/// The returned cells are ordered deterministically (row-major), so the bodies
/// a map produces are reproducible run to run — a `HashMap` walk is not.
pub fn tile_collision_infos(
    cells: &HashMap<Vector2i, TileData>,
    tile_size: Vector2i,
    physics_sources: &[i32],
    get_polygon: impl Fn(i32, Vector2i) -> (Vec<Vector2>, bool),
) -> Vec<gdphysics2d::tilemap_collision::TileCollisionInfo> {
    use gdphysics2d::tilemap_collision::TileCollisionInfo;

    if physics_sources.is_empty() || tile_size.x <= 0 || tile_size.y <= 0 {
        return Vec::new();
    }

    let mut painted: Vec<(&Vector2i, &TileData)> = cells.iter().collect();
    painted.sort_by_key(|(cell, _)| (cell.y, cell.x));

    let mut out = Vec::new();
    for (cell, data) in painted {
        if !physics_sources.contains(&data.source_id) {
            continue;
        }
        let (polygon, one_way) = get_polygon(data.source_id, data.atlas_coords);
        if polygon.len() >= 3 {
            out.push(TileCollisionInfo::polygon(*cell, polygon, one_way));
        } else {
            // No authored polygon: collide across the whole cell. `rect` is
            // two-sided, so carry the one-way flag over explicitly rather than
            // silently dropping it.
            let mut info = TileCollisionInfo::rect(
                *cell,
                Rect2::new(
                    Vector2::ZERO,
                    Vector2::new(tile_size.x as f32, tile_size.y as f32),
                ),
            );
            info.one_way = one_way;
            out.push(info);
        }
    }
    out
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
    /// Encodes the grid as Godot's `tile_map_data` PackedByteArray.
    ///
    /// The editor's dense grid and Godot's sparse cell map are different models,
    /// so the bridge is explicit: a non-zero grid value is one cell whose
    /// `source_id` IS that tile id, at atlas (0, 0) with alternative 0; a zero
    /// is "no cell" and is simply absent. Empty cells are omitted rather than
    /// written as source 0, because Godot treats a present record as a placed
    /// tile — writing them would resurrect every erased cell on reload.
    ///
    /// This is what makes painted tiles survive a save: the bytes land on the
    /// node's `tile_map_data` property, which the `.tscn` writer already knows
    /// how to emit (pat-0q2kx.75).
    pub fn to_tile_map_data(&self) -> Vec<u8> {
        let mut cells: HashMap<Vector2i, TileData> = HashMap::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let t = self.cells[y * self.width + x];
                if t != 0 {
                    cells.insert(Vector2i::new(x as i32, y as i32), TileData::new(t));
                }
            }
        }
        encode_tile_map_data(&cells)
    }

    /// Rebuilds a `width` x `height` grid from Godot `tile_map_data` bytes.
    ///
    /// Cells outside the requested extent are dropped rather than growing the
    /// grid, so a caller that knows the layer's authored size gets that size
    /// back. Use [`tile_map_data_extent`] to derive a size from the data itself.
    pub fn from_tile_map_data(bytes: &[u8], width: usize, height: usize) -> Self {
        let mut grid = Self::new(width, height);
        for (cell, data) in decode_tile_map_data(bytes) {
            grid.set(cell.x, cell.y, data.source_id);
        }
        grid
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
pub fn paint_line(
    grid: &mut TileGrid,
    from: Vector2i,
    to: Vector2i,
    tile_id: i32,
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
    (0, -1), // 0: top
    (1, 0),  // 1: right
    (0, 1),  // 2: bottom
    (-1, 0), // 3: left
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
        self.bitmask_tiles
            .insert(bitmask, (source_id, atlas_coords));
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
pub fn compute_bitmask(terrain_map: &TerrainMap, cell: Vector2i, mode: TerrainMode) -> Option<u8> {
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
pub fn resolve_terrain_tile(terrain: &TerrainType, bitmask: u8) -> Option<(i32, Vector2i)> {
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

    /// pat-nzgdu.52: decode a Godot `tile_map_data` PackedByteArray (built with
    /// the exact upstream `get_tile_map_data_as_array` layout) into painted
    /// cells with correct coordinates and atlas identifiers.
    #[test]
    fn decode_tile_map_data_reads_painted_cells() {
        // A 12-byte cell record: [i16 x][i16 y][u16 src][u16 ax][u16 ay][u16 alt].
        fn push_cell(buf: &mut Vec<u8>, x: i16, y: i16, src: u16, ax: u16, ay: u16, alt: u16) {
            buf.extend_from_slice(&(x as u16).to_le_bytes());
            buf.extend_from_slice(&(y as u16).to_le_bytes());
            buf.extend_from_slice(&src.to_le_bytes());
            buf.extend_from_slice(&ax.to_le_bytes());
            buf.extend_from_slice(&ay.to_le_bytes());
            buf.extend_from_slice(&alt.to_le_bytes());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_le_bytes()); // format version header
        push_cell(&mut bytes, 0, 0, 1, 2, 3, 0);
        push_cell(&mut bytes, 1, 0, 1, 4, 0, 0);
        push_cell(&mut bytes, -2, 5, 7, 0, 0, 1); // negative coords exercise i16

        let cells = decode_tile_map_data(&bytes);
        assert_eq!(
            cells.len(),
            3,
            "three painted cells decoded, got {}",
            cells.len()
        );

        let c0 = cells.get(&Vector2i::new(0, 0)).expect("cell (0,0)");
        assert_eq!(c0.source_id, 1);
        assert_eq!(c0.atlas_coords, Vector2i::new(2, 3));
        assert_eq!(c0.alternative_id, 0);

        let c1 = cells.get(&Vector2i::new(1, 0)).expect("cell (1,0)");
        assert_eq!(c1.atlas_coords, Vector2i::new(4, 0));

        let c2 = cells
            .get(&Vector2i::new(-2, 5))
            .expect("negative-coord cell (-2,5)");
        assert_eq!(c2.source_id, 7);
        assert_eq!(c2.alternative_id, 1);

        // The convenience constructor exposes the same cells on a layer.
        let layer = TileMapLayer::from_tile_map_data("Ground", &bytes);
        assert_eq!(layer.cells.len(), 3);

        // Robustness: empty / header-only / trailing-partial buffers yield no
        // cells rather than panicking.
        assert!(decode_tile_map_data(&[]).is_empty());
        assert!(decode_tile_map_data(&0u16.to_le_bytes()).is_empty());
        let mut partial = 0u16.to_le_bytes().to_vec();
        partial.extend_from_slice(&[1, 0, 2, 0]); // 4 trailing bytes < 12 → ignored
        assert!(decode_tile_map_data(&partial).is_empty());
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
        assert_eq!(
            compute_bitmask(&map, Vector2i::ZERO, TerrainMode::MatchSides),
            None
        );
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
        assert!(
            updated_cells.contains(&Vector2i::new(5, 5)),
            "Middle cell should be updated when neighbors change"
        );
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
        assert!(
            !updated_cells.is_empty(),
            "Neighbors should be re-resolved after erase"
        );
    }

    #[test]
    fn terrain_brush_single_mode() {
        let mut map = TerrainMap::new();
        let ts = make_terrain_set_4side();
        let brush = TerrainBrush::new(0);
        let results = apply_terrain_brush(
            &mut map,
            &ts,
            &brush,
            Vector2i::new(3, 3),
            Vector2i::new(3, 3),
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
            &mut map,
            &ts,
            &brush,
            Vector2i::new(0, 0),
            Vector2i::new(4, 0),
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
            &mut map,
            &ts,
            &brush,
            Vector2i::new(0, 0),
            Vector2i::new(2, 2),
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

    // -- pat-s3rrv.65: TileSet editor companion keys ------------------------

    #[test]
    fn collision_polygon_is_centred_on_the_tile() {
        // ⚠️ The convention the whole feature hangs on. Godot's tile collision
        // polygons are tile-local and CENTRED, so a full-cell box on a 16×16
        // tile runs -8..8. Corner-origin points (0..16) would still produce a
        // collider — offset by half a tile down and right — which reads as
        // "collision is slightly wrong" rather than as a bug in the editor.
        let poly = full_cell_collision_polygon(Vector2i::new(16, 16));
        assert_eq!(
            poly,
            vec![
                Vector2::new(-8.0, -8.0),
                Vector2::new(8.0, -8.0),
                Vector2::new(8.0, 8.0),
                Vector2::new(-8.0, 8.0),
            ],
            "a full-cell polygon must be centred; 0..16 here means the editor \
             would write corner-origin points and every collider would sit a \
             half-tile off",
        );
        // Four points is what `tile_collision_infos` needs to take the polygon
        // branch at all — it falls back to a whole-cell rect below 3.
        assert!(poly.len() >= 3);
    }

    #[test]
    fn a_partial_collision_rect_keeps_its_own_bounds() {
        // Half-height box sitting on the tile's lower half — the ordinary
        // "floor surface" authoring case, and asymmetric so a transposed or
        // recentred implementation cannot pass by coincidence.
        let poly =
            collision_rect_polygon(Rect2::new(Vector2::new(-8.0, 0.0), Vector2::new(16.0, 8.0)));
        assert_eq!(poly[0], Vector2::new(-8.0, 0.0), "top-left");
        assert_eq!(poly[2], Vector2::new(8.0, 8.0), "bottom-right");
    }

    #[test]
    fn companion_keys_match_what_the_reader_looks_up() {
        // The editor writes these and `tile_set_from_properties` /
        // `physics_server` read them. A key written under a name nothing reads
        // is INVISIBLE, not broken: it survives the save and never reaches the
        // runtime, so only an exact-string test catches it.
        assert_eq!(
            tile_set_source_key(3, "texture"),
            "__tile_set__source_3_texture"
        );
        assert_eq!(
            tile_physics_points_key(3, Vector2i::new(1, 2)),
            "__tile_physics_3_1_2_points"
        );
        assert_eq!(
            tile_physics_one_way_key(3, Vector2i::new(1, 2)),
            "__tile_physics_3_1_2_one_way"
        );
        assert_eq!(
            tile_custom_data_key(3, Vector2i::new(1, 2), "damage"),
            "__tile_custom_data_3_1_2_damage"
        );
    }

    #[test]
    fn a_points_key_round_trips_back_to_its_atlas_coord() {
        let key = tile_physics_points_key(4, Vector2i::new(2, 3));
        assert_eq!(
            tile_physics_points_atlas(&key, 4),
            Some(Vector2i::new(2, 3))
        );
        // A different source must not claim it.
        assert_eq!(tile_physics_points_atlas(&key, 5), None);
        // ⚠️ The `_one_way` sibling shares the whole prefix AND the source and
        // coordinate fields; only the suffix separates them. Matching it here
        // would report a phantom tile for every one-way flag.
        let one_way = tile_physics_one_way_key(4, Vector2i::new(2, 3));
        assert_eq!(tile_physics_points_atlas(&one_way, 4), None);
        // Negative atlas coordinates are legal in Godot and must survive the
        // rsplit — a left-to-right split would read `-1` as the source id.
        let neg = tile_physics_points_key(4, Vector2i::new(-1, -2));
        assert_eq!(
            tile_physics_points_atlas(&neg, 4),
            Some(Vector2i::new(-1, -2))
        );
    }

    #[test]
    fn a_source_written_with_the_builder_reads_back() {
        // Round-trips the editor's write path through the loader's read path,
        // which is the only thing that proves the two agree.
        let mut props: HashMap<String, Variant> = HashMap::new();
        props.insert(
            TILE_SET_TILE_SIZE.to_string(),
            Variant::Vector2i(Vector2i::new(16, 16)),
        );
        props.insert(
            tile_set_source_key(7, "texture"),
            Variant::String("res://atlas.png".into()),
        );
        props.insert(
            tile_set_source_key(7, "texture_region_size"),
            Variant::Vector2i(Vector2i::new(32, 32)),
        );
        props.insert(
            tile_set_source_key(7, "margins"),
            Variant::Vector2i(Vector2i::new(2, 3)),
        );

        let set = tile_set_from_properties(|k| props.get(k).cloned().unwrap_or(Variant::Nil), &[7])
            .expect("a tile set with a tile_size must parse");
        let src = set.sources.get(&7).expect("source 7 must round-trip");
        assert_eq!(src.texture_path, "res://atlas.png");
        assert_eq!(src.texture_region_size, Vector2i::new(32, 32));
        assert_eq!(src.margins, Vector2i::new(2, 3));
        // Unwritten separation falls back to Godot's default rather than
        // dropping the whole source.
        assert_eq!(src.separation, Vector2i::ZERO);

        // The region maths the atlas grid depends on: source 7's tile (1, 0)
        // starts after the margin plus one 32px step.
        let region = src.tile_region(Vector2i::new(1, 0));
        assert_eq!(region.position, Vector2::new(34.0, 3.0));
        assert_eq!(region.size, Vector2::new(32.0, 32.0));
    }
}

// ---------------------------------------------------------------------------
// GDScript node method surface (pat-s3rrv.88)
// ---------------------------------------------------------------------------
//
// Every cell operation above takes a [`TileMapStore`], which is an EDITOR-side
// structure: nothing in the running engine ever builds one (`rg TileMapStore`
// finds no constructor outside this module). At runtime the source of truth is
// the node's own `tile_map_data` property — both the render path
// (`viewport_2d_render::tilemap_quads`) and the tile-physics pass
// (`physics_server`) decode it directly. So the script bridge below works on
// that property: decode, mutate, re-encode. That is also exactly what makes a
// scripted edit show up in pixels and in collision on the next frame, with no
// invalidation step of its own.
//
// Godot ground truth: `upstream/godot/scene/2d/tile_map_layer.cpp`.

/// Godot's `TileSet::INVALID_SOURCE` — the "no tile here" source id.
pub const INVALID_SOURCE: i32 = -1;
/// Godot's `TileSetSource::INVALID_ATLAS_COORDS`.
pub const INVALID_ATLAS_COORDS: Vector2i = Vector2i::new(-1, -1);
/// Godot's `TileSetSource::INVALID_TILE_ALTERNATIVE`.
pub const INVALID_TILE_ALTERNATIVE: i32 = -1;
/// Godot's `TileSet.tile_size` default, used when a layer has no TileSet yet.
const DEFAULT_TILE_SIZE: Vector2i = Vector2i::new(16, 16);

/// Decodes the cells a TileMapLayer node currently holds.
fn node_cells(tree: &crate::scene_tree::SceneTree, node_id: NodeId) -> HashMap<Vector2i, TileData> {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property("tile_map_data"))
    {
        Some(Variant::PackedByteArray(bytes)) => decode_tile_map_data(&bytes),
        _ => HashMap::new(),
    }
}

/// Re-encodes cells onto the node, which is what the render and physics passes
/// read on the next frame.
fn write_node_cells(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: NodeId,
    cells: &HashMap<Vector2i, TileData>,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            "tile_map_data",
            Variant::PackedByteArray(encode_tile_map_data(cells)),
        );
    }
}

/// The layer's cell size in pixels, from the flattened TileSet companion.
fn node_tile_size(tree: &crate::scene_tree::SceneTree, node_id: NodeId) -> Vector2i {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property(TILE_SET_TILE_SIZE))
    {
        Some(Variant::Vector2i(v)) if v.x != 0 && v.y != 0 => v,
        _ => DEFAULT_TILE_SIZE,
    }
}

/// Applies Godot's `set_cell` to a decoded cell map.
///
/// Godot normalises a PARTIALLY invalid triple to fully invalid — passing an
/// invalid source id, atlas coord OR alternative while the others are valid
/// clears all three (`tile_map_layer.cpp`), so the cell is erased rather than
/// stored half-specified.
fn apply_set_cell(
    cells: &mut HashMap<Vector2i, TileData>,
    coords: Vector2i,
    source_id: i32,
    atlas_coords: Vector2i,
    alternative: i32,
) {
    let any_invalid = source_id == INVALID_SOURCE
        || atlas_coords == INVALID_ATLAS_COORDS
        || alternative == INVALID_TILE_ALTERNATIVE;
    if any_invalid {
        cells.remove(&coords);
        return;
    }
    let entry = cells.entry(coords).or_insert_with(|| TileData {
        source_id,
        atlas_coords,
        alternative_id: alternative,
        custom_data: HashMap::new(),
    });
    entry.source_id = source_id;
    entry.atlas_coords = atlas_coords;
    entry.alternative_id = alternative;
}

/// Answers `TileMapLayer`'s script method surface, or `None` for any other node.
///
/// Runs ahead of the generic node-method table in `scripting::call_node_method`
/// and declines every non-TileMapLayer node, the contract
/// `navigation_agents::call_agent_method` follows. `TileMap` (the deprecated
/// multi-layer node) is accepted too so a `.tscn` authored against either class
/// reaches the same implementation.
pub fn call_tilemap_method(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let class = tree.get_node(node_id)?.class_name().to_string();
    if class != "TileMapLayer" && class != "TileMap" {
        return None;
    }

    let cell_arg = |i: usize| -> Vector2i {
        match args.get(i) {
            Some(Variant::Vector2i(v)) => *v,
            // A script may hand a float Vector2 to a coordinate parameter;
            // Godot coerces rather than failing.
            Some(Variant::Vector2(v)) => Vector2i::new(v.x as i32, v.y as i32),
            _ => Vector2i::ZERO,
        }
    };
    let int_arg = |i: usize, default: i32| -> i32 {
        match args.get(i) {
            Some(Variant::Int(v)) => *v as i32,
            Some(Variant::Float(f)) => *f as i32,
            _ => default,
        }
    };

    Some(match method {
        "set_cell" => {
            let coords = cell_arg(0);
            // Godot's defaults: an omitted source id means "erase".
            let source_id = int_arg(1, INVALID_SOURCE);
            let atlas = match args.get(2) {
                Some(Variant::Vector2i(v)) => *v,
                Some(Variant::Vector2(v)) => Vector2i::new(v.x as i32, v.y as i32),
                _ => Vector2i::ZERO,
            };
            let alternative = int_arg(3, 0);
            let mut cells = node_cells(tree, node_id);
            apply_set_cell(&mut cells, coords, source_id, atlas, alternative);
            write_node_cells(tree, node_id, &cells);
            Variant::Nil
        }
        "erase_cell" => {
            let coords = cell_arg(0);
            let mut cells = node_cells(tree, node_id);
            cells.remove(&coords);
            write_node_cells(tree, node_id, &cells);
            Variant::Nil
        }
        "clear" => {
            write_node_cells(tree, node_id, &HashMap::new());
            Variant::Nil
        }
        "get_cell_source_id" => {
            let coords = cell_arg(0);
            Variant::Int(
                node_cells(tree, node_id)
                    .get(&coords)
                    .map(|t| t.source_id)
                    .unwrap_or(INVALID_SOURCE) as i64,
            )
        }
        "get_cell_atlas_coords" => {
            let coords = cell_arg(0);
            Variant::Vector2i(
                node_cells(tree, node_id)
                    .get(&coords)
                    .map(|t| t.atlas_coords)
                    .unwrap_or(INVALID_ATLAS_COORDS),
            )
        }
        "get_cell_alternative_tile" => {
            let coords = cell_arg(0);
            Variant::Int(
                node_cells(tree, node_id)
                    .get(&coords)
                    .map(|t| t.alternative_id)
                    .unwrap_or(INVALID_TILE_ALTERNATIVE) as i64,
            )
        }
        "get_used_cells" => {
            // Sorted so a script iterating the result gets a stable order rather
            // than `HashMap` iteration order, which would differ run to run.
            let mut coords: Vec<Vector2i> = node_cells(tree, node_id).into_keys().collect();
            coords.sort_by_key(|c| (c.y, c.x));
            Variant::Array(coords.into_iter().map(Variant::Vector2i).collect())
        }
        "get_used_rect" => {
            let cells = node_cells(tree, node_id);
            if cells.is_empty() {
                // Godot returns an empty rect for an empty layer.
                Variant::Rect2i(gdcore::math::Rect2i::new(Vector2i::ZERO, Vector2i::ZERO))
            } else {
                let (mut min, mut max) = (
                    Vector2i::new(i32::MAX, i32::MAX),
                    Vector2i::new(i32::MIN, i32::MIN),
                );
                for c in cells.keys() {
                    min = Vector2i::new(min.x.min(c.x), min.y.min(c.y));
                    max = Vector2i::new(max.x.max(c.x), max.y.max(c.y));
                }
                // The rect spans through the last cell, hence the +1.
                Variant::Rect2i(gdcore::math::Rect2i::new(
                    min,
                    Vector2i::new(max.x - min.x + 1, max.y - min.y + 1),
                ))
            }
        }
        "map_to_local" => {
            let tile_size = node_tile_size(tree, node_id);
            Variant::Vector2(map_to_local(tile_size, cell_arg(0)))
        }
        "local_to_map" => {
            let tile_size = node_tile_size(tree, node_id);
            let local = match args.first() {
                Some(Variant::Vector2(v)) => *v,
                Some(Variant::Vector2i(v)) => Vector2::new(v.x as f32, v.y as f32),
                _ => Vector2::ZERO,
            };
            Variant::Vector2i(local_to_map(tile_size, local))
        }
        "get_surrounding_cells" => Variant::Array(
            get_surrounding_cells(cell_arg(0))
                .into_iter()
                .map(Variant::Vector2i)
                .collect(),
        ),
        // Not a tilemap method: let the generic table answer it.
        _ => return None,
    })
}
