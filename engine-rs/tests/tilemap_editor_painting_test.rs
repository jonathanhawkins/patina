//! Tests for tilemap editor painting and erasing tools.
//!
//! Covers: TileBrush, paint_line, paint_rect, flood_fill, erase operations,
//! apply_brush dispatch, and TileMapStore painting/erasing variants.

use gdcore::math::Vector2i;
use gdscene::node::Node;
use gdscene::tilemap::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_grid(w: usize, h: usize) -> TileGrid {
    TileGrid::new(w, h)
}

fn make_store_with_layers() -> (TileMapStore, gdscene::node::NodeId) {
    let mut store = TileMapStore::new();
    let node = Node::new("TileMap", "TileMap");
    let nid = node.id();
    let ts = TileSet::new(Vector2i::new(16, 16));
    let mut data = TileMapData::new(ts);
    data.add_layer(TileMapLayer::new("Ground"));
    data.add_layer(TileMapLayer::new("Objects"));
    store.insert(nid, data);
    (store, nid)
}

fn count_nonzero(grid: &TileGrid) -> usize {
    grid.cells.iter().filter(|&&c| c != 0).count()
}

// ---------------------------------------------------------------------------
// TileBrush
// ---------------------------------------------------------------------------

#[test]
fn brush_defaults() {
    let b = TileBrush::new();
    assert_eq!(b.tile_id, 1);
    assert_eq!(b.mode, TilePaintMode::Single);
    assert_eq!(b.layer, 0);
}

#[test]
fn brush_with_tile() {
    let b = TileBrush::with_tile(5);
    assert_eq!(b.tile_id, 5);
}

#[test]
fn brush_default_trait() {
    let b = TileBrush::default();
    assert_eq!(b.tile_id, 1);
}

// ---------------------------------------------------------------------------
// paint_line (TileGrid)
// ---------------------------------------------------------------------------

#[test]
fn paint_line_horizontal() {
    let mut grid = make_grid(10, 10);
    let painted = paint_line(&mut grid, Vector2i::new(1, 3), Vector2i::new(5, 3), 2);
    assert_eq!(painted.len(), 5);
    for x in 1..=5 {
        assert_eq!(grid.get(x, 3), Some(2));
    }
}

#[test]
fn paint_line_vertical() {
    let mut grid = make_grid(10, 10);
    let painted = paint_line(&mut grid, Vector2i::new(4, 0), Vector2i::new(4, 6), 3);
    assert_eq!(painted.len(), 7);
    for y in 0..=6 {
        assert_eq!(grid.get(4, y), Some(3));
    }
}

#[test]
fn paint_line_diagonal() {
    let mut grid = make_grid(10, 10);
    let painted = paint_line(&mut grid, Vector2i::new(0, 0), Vector2i::new(4, 4), 1);
    // Diagonal should hit all 5 cells on the diagonal
    assert_eq!(painted.len(), 5);
    for i in 0..5 {
        assert_eq!(grid.get(i, i), Some(1));
    }
}

#[test]
fn paint_line_single_point() {
    let mut grid = make_grid(10, 10);
    let painted = paint_line(&mut grid, Vector2i::new(3, 3), Vector2i::new(3, 3), 5);
    assert_eq!(painted.len(), 1);
    assert_eq!(grid.get(3, 3), Some(5));
}

#[test]
fn paint_line_reverse_direction() {
    let mut grid = make_grid(10, 10);
    let painted = paint_line(&mut grid, Vector2i::new(5, 3), Vector2i::new(1, 3), 2);
    assert_eq!(painted.len(), 5);
    for x in 1..=5 {
        assert_eq!(grid.get(x, 3), Some(2));
    }
}

#[test]
fn paint_line_out_of_bounds_clipped() {
    let mut grid = make_grid(5, 5);
    let painted = paint_line(&mut grid, Vector2i::new(0, 0), Vector2i::new(9, 0), 1);
    // Only 5 cells should actually be set (0-4)
    assert_eq!(painted.len(), 5);
    assert_eq!(count_nonzero(&grid), 5);
}

// ---------------------------------------------------------------------------
// paint_rect (TileGrid)
// ---------------------------------------------------------------------------

#[test]
fn paint_rect_basic() {
    let mut grid = make_grid(10, 10);
    let painted = paint_rect(&mut grid, Vector2i::new(1, 1), Vector2i::new(3, 3), 4);
    assert_eq!(painted.len(), 9); // 3x3
    for y in 1..=3 {
        for x in 1..=3 {
            assert_eq!(grid.get(x, y), Some(4));
        }
    }
}

#[test]
fn paint_rect_reversed_corners() {
    let mut grid = make_grid(10, 10);
    let painted = paint_rect(&mut grid, Vector2i::new(3, 3), Vector2i::new(1, 1), 4);
    assert_eq!(painted.len(), 9);
}

#[test]
fn paint_rect_single_cell() {
    let mut grid = make_grid(10, 10);
    let painted = paint_rect(&mut grid, Vector2i::new(2, 2), Vector2i::new(2, 2), 7);
    assert_eq!(painted.len(), 1);
    assert_eq!(grid.get(2, 2), Some(7));
}

#[test]
fn paint_rect_row() {
    let mut grid = make_grid(10, 10);
    let painted = paint_rect(&mut grid, Vector2i::new(0, 0), Vector2i::new(4, 0), 1);
    assert_eq!(painted.len(), 5);
}

#[test]
fn paint_rect_column() {
    let mut grid = make_grid(10, 10);
    let painted = paint_rect(&mut grid, Vector2i::new(0, 0), Vector2i::new(0, 4), 1);
    assert_eq!(painted.len(), 5);
}

// ---------------------------------------------------------------------------
// erase_line / erase_rect (TileGrid)
// ---------------------------------------------------------------------------

#[test]
fn erase_line_clears_cells() {
    let mut grid = make_grid(10, 10);
    paint_line(&mut grid, Vector2i::new(0, 0), Vector2i::new(4, 0), 5);
    assert_eq!(count_nonzero(&grid), 5);
    erase_line(&mut grid, Vector2i::new(1, 0), Vector2i::new(3, 0));
    assert_eq!(grid.get(0, 0), Some(5));
    assert_eq!(grid.get(1, 0), Some(0));
    assert_eq!(grid.get(2, 0), Some(0));
    assert_eq!(grid.get(3, 0), Some(0));
    assert_eq!(grid.get(4, 0), Some(5));
}

#[test]
fn erase_rect_clears_cells() {
    let mut grid = make_grid(10, 10);
    paint_rect(&mut grid, Vector2i::new(0, 0), Vector2i::new(4, 4), 3);
    assert_eq!(count_nonzero(&grid), 25);
    erase_rect(&mut grid, Vector2i::new(1, 1), Vector2i::new(3, 3));
    // Border cells should remain, inner 3x3 should be erased
    assert_eq!(count_nonzero(&grid), 16);
}

// ---------------------------------------------------------------------------
// flood_fill (TileGrid)
// ---------------------------------------------------------------------------

#[test]
fn flood_fill_empty_grid() {
    let mut grid = make_grid(5, 5);
    let filled = flood_fill(&mut grid, Vector2i::new(0, 0), 1);
    assert_eq!(filled.len(), 25); // fills entire 5x5 grid
    assert_eq!(count_nonzero(&grid), 25);
}

#[test]
fn flood_fill_bounded_by_walls() {
    let mut grid = make_grid(7, 7);
    // Draw a wall box from (2,2) to (4,4)
    for i in 2..=4 {
        grid.set(i, 2, 2); // top
        grid.set(i, 4, 2); // bottom
        grid.set(2, i, 2); // left
        grid.set(4, i, 2); // right
    }
    // Fill inside the box at (3,3)
    let filled = flood_fill(&mut grid, Vector2i::new(3, 3), 5);
    assert_eq!(filled.len(), 1); // only (3,3) is open inside
    assert_eq!(grid.get(3, 3), Some(5));
}

#[test]
fn flood_fill_same_tile_is_noop() {
    let mut grid = make_grid(5, 5);
    grid.set(2, 2, 3);
    let filled = flood_fill(&mut grid, Vector2i::new(2, 2), 3);
    assert!(filled.is_empty());
}

#[test]
fn flood_fill_out_of_bounds_origin() {
    let mut grid = make_grid(5, 5);
    let filled = flood_fill(&mut grid, Vector2i::new(-1, -1), 1);
    assert!(filled.is_empty());
}

#[test]
fn flood_fill_replaces_only_matching() {
    let mut grid = make_grid(5, 1);
    // [0, 0, 3, 0, 0]
    grid.set(2, 0, 3);
    let filled = flood_fill(&mut grid, Vector2i::new(0, 0), 1);
    // Should fill cells 0 and 1 (stops at cell 2 which is 3)
    assert_eq!(filled.len(), 2);
    assert_eq!(grid.get(0, 0), Some(1));
    assert_eq!(grid.get(1, 0), Some(1));
    assert_eq!(grid.get(2, 0), Some(3)); // unchanged
}

// ---------------------------------------------------------------------------
// apply_brush
// ---------------------------------------------------------------------------

#[test]
fn apply_brush_single() {
    let mut grid = make_grid(10, 10);
    let brush = TileBrush::with_tile(3);
    let cells = apply_brush(&mut grid, &brush, Vector2i::new(5, 5), Vector2i::new(5, 5));
    assert_eq!(cells.len(), 1);
    assert_eq!(grid.get(5, 5), Some(3));
}

#[test]
fn apply_brush_line() {
    let mut grid = make_grid(10, 10);
    let mut brush = TileBrush::with_tile(2);
    brush.mode = TilePaintMode::Line;
    let cells = apply_brush(&mut grid, &brush, Vector2i::new(0, 0), Vector2i::new(3, 0));
    assert_eq!(cells.len(), 4);
}

#[test]
fn apply_brush_rect() {
    let mut grid = make_grid(10, 10);
    let mut brush = TileBrush::with_tile(4);
    brush.mode = TilePaintMode::Rect;
    let cells = apply_brush(&mut grid, &brush, Vector2i::new(1, 1), Vector2i::new(3, 3));
    assert_eq!(cells.len(), 9);
}

#[test]
fn apply_brush_flood_fill() {
    let mut grid = make_grid(3, 3);
    let mut brush = TileBrush::with_tile(7);
    brush.mode = TilePaintMode::FloodFill;
    let cells = apply_brush(&mut grid, &brush, Vector2i::new(1, 1), Vector2i::new(1, 1));
    assert_eq!(cells.len(), 9); // fills all 3x3
}

#[test]
fn apply_brush_erase_single() {
    let mut grid = make_grid(10, 10);
    grid.set(5, 5, 3);
    let mut brush = TileBrush::with_tile(0);
    brush.mode = TilePaintMode::Erase;
    let cells = apply_brush(&mut grid, &brush, Vector2i::new(5, 5), Vector2i::new(5, 5));
    assert_eq!(cells.len(), 1);
    assert_eq!(grid.get(5, 5), Some(0));
}

#[test]
fn apply_brush_erase_line() {
    let mut grid = make_grid(10, 10);
    paint_line(&mut grid, Vector2i::new(0, 0), Vector2i::new(4, 0), 5);
    let mut brush = TileBrush::with_tile(0);
    brush.mode = TilePaintMode::Erase;
    let cells = apply_brush(&mut grid, &brush, Vector2i::new(0, 0), Vector2i::new(4, 0));
    assert_eq!(cells.len(), 5);
    assert_eq!(count_nonzero(&grid), 0);
}

// ---------------------------------------------------------------------------
// TileMapStore paint/erase operations
// ---------------------------------------------------------------------------

#[test]
fn paint_line_store_horizontal() {
    let (mut store, nid) = make_store_with_layers();
    let td = TileData::new(1);
    let painted = paint_line_store(
        &mut store,
        nid,
        0,
        Vector2i::new(0, 0),
        Vector2i::new(3, 0),
        td,
    );
    assert_eq!(painted.len(), 4);
    for x in 0..=3 {
        assert!(get_cell(&store, nid, 0, Vector2i::new(x, 0)).is_some());
    }
}

#[test]
fn paint_rect_store_basic() {
    let (mut store, nid) = make_store_with_layers();
    let td = TileData::new(2);
    let painted = paint_rect_store(
        &mut store,
        nid,
        0,
        Vector2i::new(0, 0),
        Vector2i::new(2, 2),
        td,
    );
    assert_eq!(painted.len(), 9);
    let used = get_used_cells(&store, nid, 0);
    assert_eq!(used.len(), 9);
}

#[test]
fn erase_line_store_removes_cells() {
    let (mut store, nid) = make_store_with_layers();
    let td = TileData::new(1);
    paint_line_store(
        &mut store,
        nid,
        0,
        Vector2i::new(0, 0),
        Vector2i::new(4, 0),
        td,
    );
    let erased = erase_line_store(&mut store, nid, 0, Vector2i::new(1, 0), Vector2i::new(3, 0));
    assert_eq!(erased.len(), 3);
    assert!(get_cell(&store, nid, 0, Vector2i::new(0, 0)).is_some());
    assert!(get_cell(&store, nid, 0, Vector2i::new(1, 0)).is_none());
    assert!(get_cell(&store, nid, 0, Vector2i::new(4, 0)).is_some());
}

#[test]
fn erase_rect_store_removes_cells() {
    let (mut store, nid) = make_store_with_layers();
    let td = TileData::new(3);
    paint_rect_store(
        &mut store,
        nid,
        0,
        Vector2i::new(0, 0),
        Vector2i::new(4, 4),
        td,
    );
    assert_eq!(get_used_cells(&store, nid, 0).len(), 25);
    let erased = erase_rect_store(&mut store, nid, 0, Vector2i::new(1, 1), Vector2i::new(3, 3));
    assert_eq!(erased.len(), 9);
    assert_eq!(get_used_cells(&store, nid, 0).len(), 16);
}

#[test]
fn paint_store_different_layers_independent() {
    let (mut store, nid) = make_store_with_layers();
    paint_line_store(
        &mut store,
        nid,
        0,
        Vector2i::new(0, 0),
        Vector2i::new(2, 0),
        TileData::new(1),
    );
    paint_line_store(
        &mut store,
        nid,
        1,
        Vector2i::new(0, 0),
        Vector2i::new(2, 0),
        TileData::new(2),
    );

    assert_eq!(
        get_cell(&store, nid, 0, Vector2i::new(0, 0))
            .unwrap()
            .source_id,
        1
    );
    assert_eq!(
        get_cell(&store, nid, 1, Vector2i::new(0, 0))
            .unwrap()
            .source_id,
        2
    );

    erase_line_store(&mut store, nid, 0, Vector2i::new(0, 0), Vector2i::new(2, 0));
    assert!(get_cell(&store, nid, 0, Vector2i::new(0, 0)).is_none());
    assert!(get_cell(&store, nid, 1, Vector2i::new(0, 0)).is_some());
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn paint_line_store_invalid_layer() {
    let (mut store, nid) = make_store_with_layers();
    let painted = paint_line_store(
        &mut store,
        nid,
        99,
        Vector2i::new(0, 0),
        Vector2i::new(3, 0),
        TileData::new(1),
    );
    assert!(painted.is_empty());
}

#[test]
fn flood_fill_l_shaped_region() {
    let mut grid = make_grid(5, 5);
    // Create an L-shaped wall:
    // .....
    // .#...
    // .#...
    // .###.
    // .....
    grid.set(1, 1, 1);
    grid.set(1, 2, 1);
    grid.set(1, 3, 1);
    grid.set(2, 3, 1);
    grid.set(3, 3, 1);

    // Fill from (0,0) should fill all zeros connected to (0,0)
    let filled = flood_fill(&mut grid, Vector2i::new(0, 0), 9);
    // The L-wall has 5 cells of value 1. The remaining 20 cells are 0.
    // Starting from (0,0), the flood should reach most of the grid except
    // maybe some cells blocked by the L.
    // (0,0) connects freely everywhere except blocked by the L
    // Actually the L doesn't fully enclose any region, so all 20 zeros connect.
    assert_eq!(filled.len(), 20);
}
