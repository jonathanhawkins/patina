//! Tilemap editor with tile painting and erasing.
//!
//! Provides the editor-side state and interaction model for Godot 4's TileMap
//! editor, including:
//!
//! - **Tool modes**: Paint, erase, bucket fill, rectangle paint, line paint, picker.
//! - **Brush**: Current tile selection (source ID, atlas coords) with multi-tile support.
//! - **Painting**: Stamp tiles onto the grid, erase cells, flood fill.
//! - **Layer management**: Select active layer for painting.
//! - **Grid coordinate conversion**: Screen-to-grid and grid-to-screen mapping.
//! - **Undo support**: Stroke recording for batch undo.

use std::collections::{HashMap, HashSet, VecDeque};

use gdcore::math::{Vector2, Vector2i};

// ---------------------------------------------------------------------------
// Tool mode
// ---------------------------------------------------------------------------

/// Active tilemap editor tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileToolMode {
    /// Paint tiles with the current brush.
    Paint,
    /// Erase tiles under the cursor.
    Erase,
    /// Bucket fill a contiguous region with the current brush.
    BucketFill,
    /// Paint a filled rectangle between two corners.
    RectPaint,
    /// Paint a line of tiles between two points.
    LinePaint,
    /// Pick a tile from the map (eyedropper).
    Picker,
}

impl Default for TileToolMode {
    fn default() -> Self {
        Self::Paint
    }
}

// ---------------------------------------------------------------------------
// TileBrush
// ---------------------------------------------------------------------------

/// The current painting brush — which tile(s) to stamp.
#[derive(Debug, Clone, PartialEq)]
pub struct TileBrush {
    /// Source ID in the TileSet.
    pub source_id: i32,
    /// Atlas coordinates for single-tile brush.
    pub atlas_coords: Vector2i,
    /// Alternative tile ID (0 = default).
    pub alternative_id: i32,
    /// Multi-tile pattern: offsets from the origin cell.
    /// Empty means single-tile brush.
    pub pattern: Vec<(Vector2i, i32, Vector2i)>, // (offset, source_id, atlas_coords)
}

impl Default for TileBrush {
    fn default() -> Self {
        Self {
            source_id: 0,
            atlas_coords: Vector2i::ZERO,
            alternative_id: 0,
            pattern: Vec::new(),
        }
    }
}

impl TileBrush {
    /// Creates a single-tile brush.
    pub fn single(source_id: i32, atlas_coords: Vector2i) -> Self {
        Self {
            source_id,
            atlas_coords,
            alternative_id: 0,
            pattern: Vec::new(),
        }
    }

    /// Returns true if this is a single-tile brush (no pattern).
    pub fn is_single(&self) -> bool {
        self.pattern.is_empty()
    }

    /// Returns the cells this brush would paint at the given grid position.
    /// Each entry is (grid_pos, source_id, atlas_coords).
    pub fn cells_at(&self, origin: Vector2i) -> Vec<(Vector2i, i32, Vector2i)> {
        if self.pattern.is_empty() {
            vec![(origin, self.source_id, self.atlas_coords)]
        } else {
            self.pattern
                .iter()
                .map(|(offset, src, atlas)| {
                    (
                        Vector2i::new(origin.x + offset.x, origin.y + offset.y),
                        *src,
                        *atlas,
                    )
                })
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// StrokeRecord — for undo
// ---------------------------------------------------------------------------

/// A single cell change within a paint stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct CellChange {
    /// Grid coordinate that was changed.
    pub coord: Vector2i,
    /// Previous tile at this cell (None if it was empty).
    pub old_source_id: Option<i32>,
    /// New tile at this cell (None if erased).
    pub new_source_id: Option<i32>,
}

/// A complete paint stroke for undo/redo.
#[derive(Debug, Clone)]
pub struct StrokeRecord {
    /// Layer index this stroke applied to.
    pub layer_index: usize,
    /// All cell changes in this stroke.
    pub changes: Vec<CellChange>,
}

impl StrokeRecord {
    pub fn new(layer_index: usize) -> Self {
        Self {
            layer_index,
            changes: Vec::new(),
        }
    }

    /// Records a cell change.
    pub fn record(&mut self, coord: Vector2i, old: Option<i32>, new: Option<i32>) {
        self.changes.push(CellChange {
            coord,
            old_source_id: old,
            new_source_id: new,
        });
    }

    /// Returns the number of changes.
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Returns true if no changes were recorded.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

// ---------------------------------------------------------------------------
// GridCoordConverter — screen to grid mapping
// ---------------------------------------------------------------------------

/// Converts between screen/world coordinates and grid cell coordinates.
#[derive(Debug, Clone)]
pub struct GridCoordConverter {
    /// Tile size in pixels.
    pub tile_size: Vector2i,
    /// World-space origin of the tilemap (top-left of cell (0,0)).
    pub origin: Vector2,
}

impl GridCoordConverter {
    pub fn new(tile_size: Vector2i, origin: Vector2) -> Self {
        Self { tile_size, origin }
    }

    /// Converts a world-space point to a grid cell coordinate.
    pub fn world_to_grid(&self, world: Vector2) -> Vector2i {
        if self.tile_size.x == 0 || self.tile_size.y == 0 {
            return Vector2i::ZERO;
        }
        let local_x = world.x - self.origin.x;
        let local_y = world.y - self.origin.y;
        Vector2i::new(
            (local_x / self.tile_size.x as f32).floor() as i32,
            (local_y / self.tile_size.y as f32).floor() as i32,
        )
    }

    /// Returns the world-space center of the given grid cell.
    pub fn grid_to_world_center(&self, cell: Vector2i) -> Vector2 {
        Vector2::new(
            self.origin.x + (cell.x as f32 + 0.5) * self.tile_size.x as f32,
            self.origin.y + (cell.y as f32 + 0.5) * self.tile_size.y as f32,
        )
    }

    /// Returns the world-space top-left corner of the given grid cell.
    pub fn grid_to_world(&self, cell: Vector2i) -> Vector2 {
        Vector2::new(
            self.origin.x + cell.x as f32 * self.tile_size.x as f32,
            self.origin.y + cell.y as f32 * self.tile_size.y as f32,
        )
    }
}

// ---------------------------------------------------------------------------
// TilemapEditor
// ---------------------------------------------------------------------------

/// Top-level tilemap editor state.
#[derive(Debug, Clone)]
pub struct TilemapEditor {
    /// Active tool mode.
    pub tool_mode: TileToolMode,
    /// Current brush.
    pub brush: TileBrush,
    /// Active layer index for painting.
    pub active_layer: usize,
    /// Grid coordinate converter.
    pub grid: GridCoordConverter,
    /// Simple in-memory tile storage: layer -> (coord -> source_id).
    /// Used for editor preview; the real data lives in the scene tree.
    layers: Vec<HashMap<Vector2i, i32>>,
    /// Active stroke being recorded (None when not painting).
    active_stroke: Option<StrokeRecord>,
    /// Undo history.
    undo_stack: Vec<StrokeRecord>,
    /// Redo history.
    redo_stack: Vec<StrokeRecord>,
}

impl TilemapEditor {
    pub fn new(tile_size: Vector2i, layer_count: usize) -> Self {
        Self {
            tool_mode: TileToolMode::default(),
            brush: TileBrush::default(),
            active_layer: 0,
            grid: GridCoordConverter::new(tile_size, Vector2::ZERO),
            layers: (0..layer_count.max(1)).map(|_| HashMap::new()).collect(),
            active_stroke: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Returns the number of layers.
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Adds a new empty layer and returns its index.
    pub fn add_layer(&mut self) -> usize {
        let idx = self.layers.len();
        self.layers.push(HashMap::new());
        idx
    }

    /// Sets the active tool mode.
    pub fn set_tool_mode(&mut self, mode: TileToolMode) {
        self.tool_mode = mode;
    }

    /// Returns the tile at the given cell on the active layer.
    pub fn get_tile(&self, coord: Vector2i) -> Option<i32> {
        self.layers
            .get(self.active_layer)
            .and_then(|layer| layer.get(&coord).copied())
    }

    /// Returns the tile at the given cell on a specific layer.
    pub fn get_tile_on_layer(&self, layer: usize, coord: Vector2i) -> Option<i32> {
        self.layers.get(layer).and_then(|l| l.get(&coord).copied())
    }

    // -- Painting -----------------------------------------------------------

    /// Begins a new paint stroke.
    pub fn begin_stroke(&mut self) {
        self.active_stroke = Some(StrokeRecord::new(self.active_layer));
    }

    /// Paints the current brush at the given grid coordinate.
    /// Must be called between begin_stroke and end_stroke.
    pub fn paint_at(&mut self, coord: Vector2i) {
        if self.active_layer >= self.layers.len() {
            return;
        }
        let cells = self.brush.cells_at(coord);
        for (cell, source_id, _atlas) in cells {
            let old = self.layers[self.active_layer].get(&cell).copied();
            if old == Some(source_id) {
                continue; // no change
            }
            self.layers[self.active_layer].insert(cell, source_id);
            if let Some(ref mut stroke) = self.active_stroke {
                stroke.record(cell, old, Some(source_id));
            }
        }
    }

    /// Erases the tile at the given grid coordinate.
    pub fn erase_at(&mut self, coord: Vector2i) {
        if self.active_layer >= self.layers.len() {
            return;
        }
        let old = self.layers[self.active_layer].remove(&coord);
        if old.is_some() {
            if let Some(ref mut stroke) = self.active_stroke {
                stroke.record(coord, old, None);
            }
        }
    }

    /// Ends the current stroke, pushing it to the undo stack.
    /// Returns the stroke record if any changes were made.
    pub fn end_stroke(&mut self) -> Option<StrokeRecord> {
        let stroke = self.active_stroke.take()?;
        if stroke.is_empty() {
            return None;
        }
        self.redo_stack.clear();
        self.undo_stack.push(stroke.clone());
        Some(stroke)
    }

    /// Returns true if a stroke is in progress.
    pub fn is_painting(&self) -> bool {
        self.active_stroke.is_some()
    }

    // -- Bucket fill --------------------------------------------------------

    /// Flood-fills from the given coordinate with the current brush.
    /// Fills contiguous cells that match the target tile (or are empty if target is empty).
    /// Must be called between begin_stroke and end_stroke.
    pub fn bucket_fill(&mut self, start: Vector2i) {
        if self.active_layer >= self.layers.len() {
            return;
        }
        let target = self.layers[self.active_layer].get(&start).copied();
        let fill_id = self.brush.source_id;

        // Don't fill if target already matches brush.
        if target == Some(fill_id) {
            return;
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        visited.insert(start);

        // Limit fill to prevent runaway on empty maps.
        let max_cells = 10000;
        let mut count = 0;

        while let Some(coord) = queue.pop_front() {
            if count >= max_cells {
                break;
            }
            let current = self.layers[self.active_layer].get(&coord).copied();
            if current != target {
                continue;
            }

            // Paint this cell.
            let old = self.layers[self.active_layer].get(&coord).copied();
            self.layers[self.active_layer].insert(coord, fill_id);
            if let Some(ref mut stroke) = self.active_stroke {
                stroke.record(coord, old, Some(fill_id));
            }
            count += 1;

            // Enqueue 4-connected neighbors.
            for &(dx, dy) in &[(0, -1), (0, 1), (-1, 0), (1, 0)] {
                let neighbor = Vector2i::new(coord.x + dx, coord.y + dy);
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    queue.push_back(neighbor);
                }
            }
        }
    }

    // -- Line and rectangle helpers -----------------------------------------

    /// Returns grid cells along a line from `a` to `b` (Bresenham's algorithm).
    pub fn line_cells(a: Vector2i, b: Vector2i) -> Vec<Vector2i> {
        let mut cells = Vec::new();
        let dx = (b.x - a.x).abs();
        let dy = -(b.y - a.y).abs();
        let sx = if a.x < b.x { 1 } else { -1 };
        let sy = if a.y < b.y { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = a.x;
        let mut y = a.y;

        loop {
            cells.push(Vector2i::new(x, y));
            if x == b.x && y == b.y {
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

    /// Returns all grid cells in a filled rectangle from `a` to `b`.
    pub fn rect_cells(a: Vector2i, b: Vector2i) -> Vec<Vector2i> {
        let min_x = a.x.min(b.x);
        let max_x = a.x.max(b.x);
        let min_y = a.y.min(b.y);
        let max_y = a.y.max(b.y);
        let mut cells = Vec::new();
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                cells.push(Vector2i::new(x, y));
            }
        }
        cells
    }

    // -- Picker -------------------------------------------------------------

    /// Picks the tile at the given coordinate and sets it as the current brush.
    /// Returns the picked source ID, or None if the cell is empty.
    pub fn pick_at(&mut self, coord: Vector2i) -> Option<i32> {
        let source_id = self.get_tile(coord)?;
        self.brush = TileBrush::single(source_id, Vector2i::ZERO);
        Some(source_id)
    }

    // -- Undo / Redo --------------------------------------------------------

    /// Undoes the last stroke. Returns true if an undo was performed.
    pub fn undo(&mut self) -> bool {
        let stroke = match self.undo_stack.pop() {
            Some(s) => s,
            None => return false,
        };
        let layer = stroke.layer_index;
        if layer >= self.layers.len() {
            return false;
        }
        // Reverse the changes.
        for change in &stroke.changes {
            match change.old_source_id {
                Some(old) => {
                    self.layers[layer].insert(change.coord, old);
                }
                None => {
                    self.layers[layer].remove(&change.coord);
                }
            }
        }
        self.redo_stack.push(stroke);
        true
    }

    /// Redoes the last undone stroke. Returns true if a redo was performed.
    pub fn redo(&mut self) -> bool {
        let stroke = match self.redo_stack.pop() {
            Some(s) => s,
            None => return false,
        };
        let layer = stroke.layer_index;
        if layer >= self.layers.len() {
            return false;
        }
        for change in &stroke.changes {
            match change.new_source_id {
                Some(new) => {
                    self.layers[layer].insert(change.coord, new);
                }
                None => {
                    self.layers[layer].remove(&change.coord);
                }
            }
        }
        self.undo_stack.push(stroke);
        true
    }

    /// Returns true if undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns true if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Returns the number of non-empty cells on the active layer.
    pub fn cell_count(&self) -> usize {
        self.layers
            .get(self.active_layer)
            .map(|l| l.len())
            .unwrap_or(0)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- TileToolMode -------------------------------------------------------

    #[test]
    fn tool_mode_default_is_paint() {
        assert_eq!(TileToolMode::default(), TileToolMode::Paint);
    }

    // -- TileBrush ----------------------------------------------------------

    #[test]
    fn brush_single_default() {
        let brush = TileBrush::default();
        assert!(brush.is_single());
        assert_eq!(brush.source_id, 0);
    }

    #[test]
    fn brush_cells_at_single() {
        let brush = TileBrush::single(5, Vector2i::new(1, 2));
        let cells = brush.cells_at(Vector2i::new(10, 20));
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].0, Vector2i::new(10, 20));
        assert_eq!(cells[0].1, 5);
    }

    #[test]
    fn brush_cells_at_pattern() {
        let brush = TileBrush {
            source_id: 1,
            atlas_coords: Vector2i::ZERO,
            alternative_id: 0,
            pattern: vec![
                (Vector2i::new(0, 0), 1, Vector2i::ZERO),
                (Vector2i::new(1, 0), 1, Vector2i::new(1, 0)),
                (Vector2i::new(0, 1), 1, Vector2i::new(0, 1)),
            ],
        };
        let cells = brush.cells_at(Vector2i::new(5, 5));
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].0, Vector2i::new(5, 5));
        assert_eq!(cells[1].0, Vector2i::new(6, 5));
        assert_eq!(cells[2].0, Vector2i::new(5, 6));
    }

    // -- GridCoordConverter -------------------------------------------------

    #[test]
    fn grid_world_to_grid() {
        let conv = GridCoordConverter::new(Vector2i::new(16, 16), Vector2::ZERO);
        assert_eq!(
            conv.world_to_grid(Vector2::new(24.0, 8.0)),
            Vector2i::new(1, 0)
        );
        assert_eq!(
            conv.world_to_grid(Vector2::new(-1.0, -1.0)),
            Vector2i::new(-1, -1)
        );
    }

    #[test]
    fn grid_to_world_center() {
        let conv = GridCoordConverter::new(Vector2i::new(16, 16), Vector2::ZERO);
        let center = conv.grid_to_world_center(Vector2i::new(0, 0));
        assert!((center.x - 8.0).abs() < 0.01);
        assert!((center.y - 8.0).abs() < 0.01);
    }

    #[test]
    fn grid_to_world_topleft() {
        let conv = GridCoordConverter::new(Vector2i::new(16, 16), Vector2::new(100.0, 200.0));
        let pos = conv.grid_to_world(Vector2i::new(2, 3));
        assert!((pos.x - 132.0).abs() < 0.01);
        assert!((pos.y - 248.0).abs() < 0.01);
    }

    #[test]
    fn grid_roundtrip() {
        let conv = GridCoordConverter::new(Vector2i::new(32, 32), Vector2::new(10.0, 20.0));
        let cell = Vector2i::new(3, 4);
        let world = conv.grid_to_world_center(cell);
        let back = conv.world_to_grid(world);
        assert_eq!(back, cell);
    }

    // -- TilemapEditor: basic painting --------------------------------------

    #[test]
    fn editor_creation() {
        let editor = TilemapEditor::new(Vector2i::new(16, 16), 2);
        assert_eq!(editor.layer_count(), 2);
        assert_eq!(editor.tool_mode, TileToolMode::Paint);
        assert_eq!(editor.cell_count(), 0);
    }

    #[test]
    fn editor_paint_and_get() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(5, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(3, 4));
        editor.end_stroke();
        assert_eq!(editor.get_tile(Vector2i::new(3, 4)), Some(5));
        assert_eq!(editor.cell_count(), 1);
    }

    #[test]
    fn editor_erase() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();
        assert_eq!(editor.get_tile(Vector2i::new(0, 0)), Some(1));

        editor.begin_stroke();
        editor.erase_at(Vector2i::new(0, 0));
        editor.end_stroke();
        assert_eq!(editor.get_tile(Vector2i::new(0, 0)), None);
        assert_eq!(editor.cell_count(), 0);
    }

    #[test]
    fn editor_paint_stroke_recorded() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.paint_at(Vector2i::new(1, 0));
        let stroke = editor.end_stroke().unwrap();
        assert_eq!(stroke.len(), 2);
    }

    #[test]
    fn editor_paint_same_tile_no_duplicate_record() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.paint_at(Vector2i::new(0, 0)); // same cell, same tile — no change
        let stroke = editor.end_stroke().unwrap();
        assert_eq!(stroke.len(), 1);
    }

    // -- Undo / Redo --------------------------------------------------------

    #[test]
    fn editor_undo_paint() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();
        assert_eq!(editor.cell_count(), 1);

        assert!(editor.undo());
        assert_eq!(editor.get_tile(Vector2i::new(0, 0)), None);
        assert_eq!(editor.cell_count(), 0);
    }

    #[test]
    fn editor_redo_paint() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();

        editor.undo();
        assert!(editor.redo());
        assert_eq!(editor.get_tile(Vector2i::new(0, 0)), Some(1));
    }

    #[test]
    fn editor_undo_erase() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();

        editor.begin_stroke();
        editor.erase_at(Vector2i::new(0, 0));
        editor.end_stroke();
        assert_eq!(editor.cell_count(), 0);

        editor.undo();
        assert_eq!(editor.get_tile(Vector2i::new(0, 0)), Some(1));
    }

    #[test]
    fn editor_new_stroke_clears_redo() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();

        editor.undo();
        assert!(editor.can_redo());

        editor.brush = TileBrush::single(2, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(1, 1));
        editor.end_stroke();
        assert!(!editor.can_redo());
    }

    #[test]
    fn editor_undo_empty_returns_false() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        assert!(!editor.undo());
        assert!(!editor.can_undo());
    }

    // -- Bucket fill --------------------------------------------------------

    #[test]
    fn editor_bucket_fill_empty_region() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        // Pre-paint a border to contain the fill.
        editor.brush = TileBrush::single(99, Vector2i::ZERO);
        editor.begin_stroke();
        for x in -1..=3 {
            editor.paint_at(Vector2i::new(x, -1));
            editor.paint_at(Vector2i::new(x, 3));
        }
        for y in 0..3 {
            editor.paint_at(Vector2i::new(-1, y));
            editor.paint_at(Vector2i::new(3, y));
        }
        editor.end_stroke();

        // Bucket fill the interior.
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.bucket_fill(Vector2i::new(0, 0));
        let stroke = editor.end_stroke().unwrap();
        // Interior is 3x3 = 9 cells.
        assert_eq!(stroke.len(), 9);
        assert_eq!(editor.get_tile(Vector2i::new(0, 0)), Some(1));
        assert_eq!(editor.get_tile(Vector2i::new(2, 2)), Some(1));
    }

    #[test]
    fn bucket_fill_same_tile_no_op() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();

        editor.begin_stroke();
        editor.bucket_fill(Vector2i::new(0, 0)); // already tile 1
        assert!(editor.end_stroke().is_none()); // no changes
    }

    // -- Picker -------------------------------------------------------------

    #[test]
    fn editor_picker() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        editor.brush = TileBrush::single(7, Vector2i::ZERO);
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(5, 5));
        editor.end_stroke();

        editor.brush = TileBrush::default(); // reset
        let picked = editor.pick_at(Vector2i::new(5, 5));
        assert_eq!(picked, Some(7));
        assert_eq!(editor.brush.source_id, 7);
    }

    #[test]
    fn editor_picker_empty_cell() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        assert_eq!(editor.pick_at(Vector2i::new(0, 0)), None);
    }

    // -- Line and rectangle helpers -----------------------------------------

    #[test]
    fn line_cells_horizontal() {
        let cells = TilemapEditor::line_cells(Vector2i::new(0, 0), Vector2i::new(3, 0));
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], Vector2i::new(0, 0));
        assert_eq!(cells[3], Vector2i::new(3, 0));
    }

    #[test]
    fn line_cells_vertical() {
        let cells = TilemapEditor::line_cells(Vector2i::new(0, 0), Vector2i::new(0, 4));
        assert_eq!(cells.len(), 5);
    }

    #[test]
    fn line_cells_diagonal() {
        let cells = TilemapEditor::line_cells(Vector2i::new(0, 0), Vector2i::new(3, 3));
        assert_eq!(cells.len(), 4); // Bresenham diagonal
        assert_eq!(cells[0], Vector2i::new(0, 0));
        assert_eq!(cells[3], Vector2i::new(3, 3));
    }

    #[test]
    fn line_cells_single_point() {
        let cells = TilemapEditor::line_cells(Vector2i::new(5, 5), Vector2i::new(5, 5));
        assert_eq!(cells.len(), 1);
    }

    #[test]
    fn rect_cells_counts() {
        let cells = TilemapEditor::rect_cells(Vector2i::new(0, 0), Vector2i::new(2, 3));
        assert_eq!(cells.len(), 12); // 3 * 4
    }

    #[test]
    fn rect_cells_normalizes() {
        let a = TilemapEditor::rect_cells(Vector2i::new(0, 0), Vector2i::new(2, 2));
        let b = TilemapEditor::rect_cells(Vector2i::new(2, 2), Vector2i::new(0, 0));
        assert_eq!(a.len(), b.len());
    }

    // -- Layer management ---------------------------------------------------

    #[test]
    fn editor_layers() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 1);
        assert_eq!(editor.layer_count(), 1);
        let idx = editor.add_layer();
        assert_eq!(idx, 1);
        assert_eq!(editor.layer_count(), 2);
    }

    #[test]
    fn editor_paint_on_different_layers() {
        let mut editor = TilemapEditor::new(Vector2i::new(16, 16), 2);
        editor.brush = TileBrush::single(1, Vector2i::ZERO);
        editor.active_layer = 0;
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();

        editor.brush = TileBrush::single(2, Vector2i::ZERO);
        editor.active_layer = 1;
        editor.begin_stroke();
        editor.paint_at(Vector2i::new(0, 0));
        editor.end_stroke();

        assert_eq!(editor.get_tile_on_layer(0, Vector2i::new(0, 0)), Some(1));
        assert_eq!(editor.get_tile_on_layer(1, Vector2i::new(0, 0)), Some(2));
    }
}
