//! TileMap collision integration.
//!
//! Generates static physics bodies from tilemap tiles that have collision shapes,
//! placing them at the correct world positions.

use gdcore::math::{Rect2, Vector2, Vector2i};

use crate::body::{BodyId, BodyType, PhysicsBody2D};
use crate::shape::Shape2D;
use crate::world::PhysicsWorld2D;

/// Information about a tile that has a collision shape.
#[derive(Debug, Clone)]
pub struct TileCollisionInfo {
    /// Grid coordinates of the tile.
    pub cell: Vector2i,
    /// Collision shape as a rectangle (in local tile space). Used when
    /// [`polygon`](Self::polygon) is empty.
    pub collision_rect: Rect2,
    /// Optional convex collision polygon in local tile space (Godot tile
    /// collision polygons). When non-empty it takes precedence over
    /// `collision_rect` and produces a [`Shape2D::ConvexPolygon`] body.
    pub polygon: Vec<Vector2>,
    /// When true the generated body is a one-way collider (Godot tile
    /// `one_way_collision`): bodies pass through from the back but are blocked
    /// from the one-way direction (default up).
    pub one_way: bool,
}

impl TileCollisionInfo {
    /// A rectangle-collider tile (the common case; no polygon, two-sided).
    pub fn rect(cell: Vector2i, collision_rect: Rect2) -> Self {
        Self {
            cell,
            collision_rect,
            polygon: Vec::new(),
            one_way: false,
        }
    }

    /// A convex-polygon-collider tile, optionally one-way.
    pub fn polygon(cell: Vector2i, polygon: Vec<Vector2>, one_way: bool) -> Self {
        Self {
            cell,
            collision_rect: Rect2::new(Vector2::ZERO, Vector2::ZERO),
            polygon,
            one_way,
        }
    }
}

/// Result of generating physics bodies from a tilemap.
#[derive(Debug)]
pub struct TileMapCollisionResult {
    /// Body IDs created, one per tile with collision.
    pub body_ids: Vec<BodyId>,
    /// Number of tiles processed.
    pub tile_count: usize,
}

/// Generates static physics bodies from tilemap collision data.
///
/// For each tile with collision shapes, creates a [`PhysicsBody2D`] of type
/// [`BodyType::Static`] positioned at the tile's world-space center. The
/// collision shape uses the tile's collision rectangle converted to
/// half-extents.
///
/// # Arguments
///
/// * `world` - The physics world to add bodies to.
/// * `tiles` - Tiles that have collision shapes, with their grid coordinates.
/// * `tile_size` - Size of each tile in pixels.
/// * `tilemap_offset` - World-space offset of the tilemap origin.
pub fn tilemap_to_physics(
    world: &mut PhysicsWorld2D,
    tiles: &[TileCollisionInfo],
    tile_size: Vector2i,
    tilemap_offset: Vector2,
) -> TileMapCollisionResult {
    let mut body_ids = Vec::with_capacity(tiles.len());

    for tile in tiles {
        // Calculate tile center in world space
        let local_pos = Vector2::new(
            (tile.cell.x as f32 + 0.5) * tile_size.x as f32,
            (tile.cell.y as f32 + 0.5) * tile_size.y as f32,
        );
        let world_pos = local_pos + tilemap_offset;

        // A polygon collider takes precedence over the rectangle; otherwise
        // convert the collision rect to half-extents for a rectangle shape.
        let shape = if !tile.polygon.is_empty() {
            Shape2D::convex_polygon(&tile.polygon)
        } else {
            let half_extents =
                if tile.collision_rect.size.x > 0.0 && tile.collision_rect.size.y > 0.0 {
                    Vector2::new(
                        tile.collision_rect.size.x * 0.5,
                        tile.collision_rect.size.y * 0.5,
                    )
                } else {
                    // Default to tile-sized collision
                    Vector2::new(tile_size.x as f32 * 0.5, tile_size.y as f32 * 0.5)
                };
            Shape2D::Rectangle { half_extents }
        };

        let mut body = PhysicsBody2D::new(BodyId(0), BodyType::Static, world_pos, shape, 0.0);
        // Honor the tile's one-way collision flag (Godot `one_way_collision`).
        body.one_way_collision = tile.one_way;
        let id = world.add_body(body);
        body_ids.push(id);
    }

    TileMapCollisionResult {
        tile_count: tiles.len(),
        body_ids,
    }
}

/// Godot's `TileMapLayer` physics quadrant edge, in cells
/// (`physics_quadrant_size`, default 16).
pub const DEFAULT_PHYSICS_QUADRANT_SIZE: i32 = 16;

/// The axis-aligned rectangle a tile polygon describes, in world space, or
/// `None` when the polygon is not one.
fn world_rect_of(tile: &TileCollisionInfo, center: Vector2) -> Option<(f32, f32, f32, f32)> {
    let pts: Vec<Vector2> = if tile.polygon.is_empty() {
        return None;
    } else {
        tile.polygon.iter().map(|p| *p + center).collect()
    };
    if pts.len() != 4 {
        return None;
    }
    let (mut xs, mut ys): (Vec<f32>, Vec<f32>) = pts.iter().map(|p| (p.x, p.y)).unzip();
    xs.sort_by(f32::total_cmp);
    xs.dedup();
    ys.sort_by(f32::total_cmp);
    ys.dedup();
    if xs.len() != 2 || ys.len() != 2 {
        return None;
    }
    // Every corner must be present (rules out a bow-tie of the same extents).
    for &x in &xs {
        for &y in &ys {
            if !pts.iter().any(|p| p.x == x && p.y == y) {
                return None;
            }
        }
    }
    Some((xs[0], ys[0], xs[1], ys[1]))
}

/// Generates the static bodies Godot 4.6's `TileMapLayer` builds: tiles are
/// grouped per physics quadrant ([`DEFAULT_PHYSICS_QUADRANT_SIZE`]) and per
/// cell ROW (`PhysicsBodyKey::y_origin`), and each group's polygons are MERGED
/// (`Geometry2D::merge_many_polygons`) before being cut back into convex shapes
/// (`tile_map_layer.cpp`, `_physics_update_cell`).
///
/// 🔑 Why it matters: with one body per tile, a box walking along a flat floor
/// meets a polygon VERTEX at every tile seam. Within the safe margin that vertex
/// produces its own contact pairs, so recovery and the rest pass see a slightly
/// different floor at every seam and the walker's height jitters by a few
/// thousandths of a pixel each 64 px — Godot's merged floor has no seams at all.
///
/// The merge implemented here is the case real tile sets are made of: runs of
/// horizontally ADJACENT axis-aligned rectangles that share their top and
/// bottom edges become one rectangle. Any other tile polygon keeps its own body,
/// exactly as [`tilemap_to_physics`] makes it.
pub fn tilemap_to_physics_merged(
    world: &mut PhysicsWorld2D,
    tiles: &[TileCollisionInfo],
    tile_size: Vector2i,
    tilemap_offset: Vector2,
    quadrant_size: i32,
) -> TileMapCollisionResult {
    use std::collections::BTreeMap;
    let q = quadrant_size.max(1);
    let floor_div = |v: i32| v.div_euclid(q);
    // (quadrant x, quadrant y, row, one_way) -> [(left, top, right, bottom)]
    #[allow(clippy::type_complexity)]
    let mut groups: BTreeMap<(i32, i32, i32, bool), Vec<(f32, f32, f32, f32)>> = BTreeMap::new();
    let mut singles: Vec<TileCollisionInfo> = Vec::new();
    for tile in tiles {
        let center = Vector2::new(
            (tile.cell.x as f32 + 0.5) * tile_size.x as f32,
            (tile.cell.y as f32 + 0.5) * tile_size.y as f32,
        ) + tilemap_offset;
        match world_rect_of(tile, center) {
            Some(r) => groups
                .entry((
                    floor_div(tile.cell.x),
                    floor_div(tile.cell.y),
                    tile.cell.y,
                    tile.one_way,
                ))
                .or_default()
                .push(r),
            None => singles.push(tile.clone()),
        }
    }

    let mut body_ids = Vec::new();
    for ((_, _, _, one_way), mut rects) in groups {
        rects.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut runs: Vec<(f32, f32, f32, f32)> = Vec::new();
        for r in rects {
            match runs.last_mut() {
                Some(last) if last.1 == r.1 && last.3 == r.3 && last.2 == r.0 => last.2 = r.2,
                _ => runs.push(r),
            }
        }
        for (l, t, r, b) in runs {
            let center = Vector2::new((l + r) * 0.5, (t + b) * 0.5);
            let (hw, hh) = ((r - l) * 0.5, (b - t) * 0.5);
            let shape = Shape2D::convex_polygon(&[
                Vector2::new(-hw, -hh),
                Vector2::new(hw, -hh),
                Vector2::new(hw, hh),
                Vector2::new(-hw, hh),
            ]);
            let mut body = PhysicsBody2D::new(BodyId(0), BodyType::Static, center, shape, 0.0);
            body.one_way_collision = one_way;
            body_ids.push(world.add_body(body));
        }
    }
    let rest = tilemap_to_physics(world, &singles, tile_size, tilemap_offset);
    body_ids.extend(rest.body_ids);
    TileMapCollisionResult {
        tile_count: tiles.len(),
        body_ids,
    }
}

/// Convenience: generates [`TileCollisionInfo`] from a grid of tile IDs and a
/// lookup function that returns the collision rect for a tile source ID (if any).
pub fn collect_collision_tiles<F>(
    cells: &[(Vector2i, i32)],
    collision_lookup: F,
) -> Vec<TileCollisionInfo>
where
    F: Fn(i32) -> Option<Rect2>,
{
    cells
        .iter()
        .filter_map(|&(cell, source_id)| {
            collision_lookup(source_id).map(|rect| TileCollisionInfo::rect(cell, rect))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_size() -> Vector2i {
        Vector2i::new(16, 16)
    }

    fn full_tile_rect() -> Rect2 {
        Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0))
    }

    #[test]
    fn generate_single_body_from_tile() {
        let mut world = PhysicsWorld2D::new();
        let tiles = vec![TileCollisionInfo::rect(
            Vector2i::new(0, 0),
            full_tile_rect(),
        )];

        let result = tilemap_to_physics(&mut world, &tiles, tile_size(), Vector2::ZERO);

        assert_eq!(result.tile_count, 1);
        assert_eq!(result.body_ids.len(), 1);
        assert_eq!(world.body_count(), 1);

        let body = world.get_body(result.body_ids[0]).unwrap();
        assert_eq!(body.body_type, BodyType::Static);
        // Center of cell (0,0) with 16x16 tiles = (8, 8)
        assert!((body.position.x - 8.0).abs() < 1e-5);
        assert!((body.position.y - 8.0).abs() < 1e-5);
    }

    #[test]
    fn body_positions_match_tile_coordinates() {
        let mut world = PhysicsWorld2D::new();
        let tiles = vec![
            TileCollisionInfo::rect(Vector2i::new(0, 0), full_tile_rect()),
            TileCollisionInfo::rect(Vector2i::new(3, 2), full_tile_rect()),
            TileCollisionInfo::rect(Vector2i::new(5, 7), full_tile_rect()),
        ];

        let result = tilemap_to_physics(&mut world, &tiles, tile_size(), Vector2::ZERO);

        assert_eq!(result.body_ids.len(), 3);

        // Cell (0,0) -> center at (8, 8)
        let b0 = world.get_body(result.body_ids[0]).unwrap();
        assert!((b0.position.x - 8.0).abs() < 1e-5);
        assert!((b0.position.y - 8.0).abs() < 1e-5);

        // Cell (3,2) -> center at (56, 40)
        let b1 = world.get_body(result.body_ids[1]).unwrap();
        assert!((b1.position.x - 56.0).abs() < 1e-5);
        assert!((b1.position.y - 40.0).abs() < 1e-5);

        // Cell (5,7) -> center at (88, 120)
        let b2 = world.get_body(result.body_ids[2]).unwrap();
        assert!((b2.position.x - 88.0).abs() < 1e-5);
        assert!((b2.position.y - 120.0).abs() < 1e-5);
    }

    #[test]
    fn tilemap_offset_shifts_bodies() {
        let mut world = PhysicsWorld2D::new();
        let tiles = vec![TileCollisionInfo::rect(
            Vector2i::new(0, 0),
            full_tile_rect(),
        )];
        let offset = Vector2::new(100.0, 200.0);

        let result = tilemap_to_physics(&mut world, &tiles, tile_size(), offset);

        let body = world.get_body(result.body_ids[0]).unwrap();
        assert!((body.position.x - 108.0).abs() < 1e-5);
        assert!((body.position.y - 208.0).abs() < 1e-5);
    }

    #[test]
    fn empty_tiles_produces_no_bodies() {
        let mut world = PhysicsWorld2D::new();
        let result = tilemap_to_physics(&mut world, &[], tile_size(), Vector2::ZERO);

        assert_eq!(result.tile_count, 0);
        assert!(result.body_ids.is_empty());
        assert_eq!(world.body_count(), 0);
    }

    #[test]
    fn bodies_have_correct_shape_size() {
        let mut world = PhysicsWorld2D::new();
        let tiles = vec![TileCollisionInfo::rect(
            Vector2i::new(0, 0),
            Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
        )];

        let result = tilemap_to_physics(&mut world, &tiles, tile_size(), Vector2::ZERO);
        let body = world.get_body(result.body_ids[0]).unwrap();

        match body.shape {
            Shape2D::Rectangle { half_extents } => {
                assert!((half_extents.x - 8.0).abs() < 1e-5);
                assert!((half_extents.y - 8.0).abs() < 1e-5);
            }
            _ => panic!("Expected rectangle shape"),
        }
    }

    #[test]
    fn collect_collision_tiles_filters_correctly() {
        let cells = vec![
            (Vector2i::new(0, 0), 1), // has collision
            (Vector2i::new(1, 0), 2), // no collision
            (Vector2i::new(2, 0), 1), // has collision
            (Vector2i::new(3, 0), 3), // no collision
        ];

        let collision_tiles = collect_collision_tiles(&cells, |source_id| {
            if source_id == 1 {
                Some(full_tile_rect())
            } else {
                None
            }
        });

        assert_eq!(collision_tiles.len(), 2);
        assert_eq!(collision_tiles[0].cell, Vector2i::new(0, 0));
        assert_eq!(collision_tiles[1].cell, Vector2i::new(2, 0));
    }

    #[test]
    fn default_shape_for_zero_size_collision_rect() {
        let mut world = PhysicsWorld2D::new();
        let tiles = vec![TileCollisionInfo::rect(
            Vector2i::new(0, 0),
            Rect2::new(Vector2::ZERO, Vector2::ZERO), // zero size
        )];

        let result = tilemap_to_physics(&mut world, &tiles, tile_size(), Vector2::ZERO);
        let body = world.get_body(result.body_ids[0]).unwrap();

        // Should default to tile-sized collision
        match body.shape {
            Shape2D::Rectangle { half_extents } => {
                assert!((half_extents.x - 8.0).abs() < 1e-5);
                assert!((half_extents.y - 8.0).abs() < 1e-5);
            }
            _ => panic!("Expected rectangle shape"),
        }
    }

    #[test]
    fn polygon_tile_creates_convex_polygon_one_way_body() {
        // A tile carrying a collision polygon + one-way flag produces a
        // ConvexPolygon body with `one_way_collision` set (pat-u3l8c.52).
        let mut world = PhysicsWorld2D::new();
        let poly = vec![
            Vector2::new(-8.0, -8.0),
            Vector2::new(8.0, -8.0),
            Vector2::new(0.0, 8.0),
        ];
        let tiles = vec![TileCollisionInfo::polygon(Vector2i::new(0, 0), poly, true)];

        let result = tilemap_to_physics(&mut world, &tiles, tile_size(), Vector2::ZERO);
        let body = world.get_body(result.body_ids[0]).unwrap();

        assert!(
            matches!(body.shape, Shape2D::ConvexPolygon { .. }),
            "a polygon tile should produce a ConvexPolygon body, got {:?}",
            body.shape
        );
        assert!(
            body.one_way_collision,
            "the tile's one-way flag should be honored on the body"
        );
    }

    #[test]
    fn rect_tile_is_two_sided_by_default() {
        // Rectangle tiles remain two-sided (not one-way) unless flagged.
        let mut world = PhysicsWorld2D::new();
        let tiles = vec![TileCollisionInfo::rect(
            Vector2i::new(0, 0),
            full_tile_rect(),
        )];
        let result = tilemap_to_physics(&mut world, &tiles, tile_size(), Vector2::ZERO);
        let body = world.get_body(result.body_ids[0]).unwrap();
        assert!(!body.one_way_collision);
        assert!(matches!(body.shape, Shape2D::Rectangle { .. }));
    }
}
