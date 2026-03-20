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
    /// Collision shape as a rectangle (in local tile space).
    pub collision_rect: Rect2,
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

        // Convert collision rect to half-extents for the rectangle shape
        let half_extents = if tile.collision_rect.size.x > 0.0 && tile.collision_rect.size.y > 0.0 {
            Vector2::new(
                tile.collision_rect.size.x * 0.5,
                tile.collision_rect.size.y * 0.5,
            )
        } else {
            // Default to tile-sized collision
            Vector2::new(tile_size.x as f32 * 0.5, tile_size.y as f32 * 0.5)
        };

        let shape = Shape2D::Rectangle { half_extents };
        let body = PhysicsBody2D::new(BodyId(0), BodyType::Static, world_pos, shape, 0.0);
        let id = world.add_body(body);
        body_ids.push(id);
    }

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
            collision_lookup(source_id).map(|rect| TileCollisionInfo {
                cell,
                collision_rect: rect,
            })
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
        let tiles = vec![TileCollisionInfo {
            cell: Vector2i::new(0, 0),
            collision_rect: full_tile_rect(),
        }];

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
            TileCollisionInfo {
                cell: Vector2i::new(0, 0),
                collision_rect: full_tile_rect(),
            },
            TileCollisionInfo {
                cell: Vector2i::new(3, 2),
                collision_rect: full_tile_rect(),
            },
            TileCollisionInfo {
                cell: Vector2i::new(5, 7),
                collision_rect: full_tile_rect(),
            },
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
        let tiles = vec![TileCollisionInfo {
            cell: Vector2i::new(0, 0),
            collision_rect: full_tile_rect(),
        }];
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
        let tiles = vec![TileCollisionInfo {
            cell: Vector2i::new(0, 0),
            collision_rect: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
        }];

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
        let tiles = vec![TileCollisionInfo {
            cell: Vector2i::new(0, 0),
            collision_rect: Rect2::new(Vector2::ZERO, Vector2::ZERO), // zero size
        }];

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
}
