//! TileMapLayer rasterization.
//!
//! The tile map data model ([`gdscene::tilemap`]) is a pure data structure with
//! no rendering; the editor scene renderer therefore drew a `TileMap`/
//! `TileMapLayer` node as a bare marker dot. This module rasterizes that data:
//! for each enabled layer (ordered by `z_index`), every populated cell is drawn
//! by blitting the tile's atlas sub-region into the framebuffer at the cell's
//! pixel position, mirroring Godot's `TileMapLayer` draw.
//!
//! Supported parity features:
//! - **Atlas blit per cell** — each cell samples a `tile_size`-sized sub-region
//!   of the tileset texture, selected by the cell's `atlas_coords`.
//! - **Per-layer z ordering** — layers are drawn from lowest `z_index` to
//!   highest so higher layers paint over lower ones, independent of their
//!   storage order.
//! - **Animated tiles** — a tile source may declare an animation ([`TileAnimation`]);
//!   the displayed frame advances horizontally through the atlas over time.

use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::draw;
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::tilemap::TileMapData;

/// Describes how an animated tile source advances through consecutive frames.
///
/// Frames are laid out horizontally in the atlas: frame `n` is offset `n`
/// tiles to the right of the tile's base `atlas_coords`. Matches Godot's
/// atlas-source animation (`animation_frames` / `animation_speed`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileAnimation {
    /// Number of animation frames (>= 1). A value of 1 means "not animated".
    pub frames: u32,
    /// Frames advanced per second. Non-positive disables advancing.
    pub fps: f32,
}

impl TileAnimation {
    /// Returns the frame index shown at `time` seconds, wrapping at `frames`.
    ///
    /// A single-frame or non-advancing animation always yields frame `0`.
    pub fn frame_at(&self, time: f32) -> u32 {
        if self.frames <= 1 || self.fps <= 0.0 {
            return 0;
        }
        let raw = (time * self.fps).floor() as i64;
        raw.rem_euclid(self.frames as i64) as u32
    }
}

/// Renders every enabled layer of `data` into `fb`.
///
/// Layers are drawn from lowest to highest `z_index` (ties keep storage order),
/// so higher-z layers paint over lower ones. For each populated cell the tile's
/// atlas sub-region is blitted at `origin + cell * tile_size`. `time` selects
/// animation frames; `animations` maps a tile source id to its [`TileAnimation`]
/// (sources absent from the map are static).
pub fn render_tilemap(
    fb: &mut FrameBuffer,
    data: &TileMapData,
    atlas: &Texture2D,
    origin: Vector2,
    time: f32,
    animations: &HashMap<i32, TileAnimation>,
) {
    let tile_w = data.tile_set.tile_size.x as f32;
    let tile_h = data.tile_set.tile_size.y as f32;
    if tile_w <= 0.0 || tile_h <= 0.0 {
        return;
    }

    // Draw order: ascending z_index, ties broken by storage order (stable).
    let mut order: Vec<usize> = (0..data.layers.len()).collect();
    order.sort_by_key(|&i| (data.layers[i].z_index, i));

    for &layer_idx in &order {
        let layer = &data.layers[layer_idx];
        if !layer.enabled {
            continue;
        }

        // Deterministic cell iteration (HashMap order is not stable).
        let mut cells: Vec<_> = layer.cells.iter().collect();
        cells.sort_by_key(|(coord, _)| (coord.y, coord.x));

        for (coord, cell) in cells {
            let Some(def) = data.tile_set.get_tile(cell.source_id) else {
                continue;
            };

            // Animated tiles step horizontally through the atlas.
            let frame = animations
                .get(&cell.source_id)
                .map(|anim| anim.frame_at(time))
                .unwrap_or(0);

            // Source region: the tile source's base origin, offset by the
            // cell's atlas coordinate and the current animation frame.
            let src = Rect2::new(
                Vector2::new(
                    def.texture_region.position.x
                        + (cell.atlas_coords.x as f32 + frame as f32) * tile_w,
                    def.texture_region.position.y + cell.atlas_coords.y as f32 * tile_h,
                ),
                Vector2::new(tile_w, tile_h),
            );

            // Destination: this cell's pixel rectangle in the framebuffer.
            let dst = Rect2::new(
                Vector2::new(
                    origin.x + coord.x as f32 * tile_w,
                    origin.y + coord.y as f32 * tile_h,
                ),
                Vector2::new(tile_w, tile_h),
            );

            draw::draw_texture_region(fb, atlas, dst, src, Color::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2i;
    use gdscene::tilemap::{TileData, TileDefinition, TileMapData, TileMapLayer, TileSet};

    fn red() -> Color {
        Color::new(1.0, 0.0, 0.0, 1.0)
    }
    fn green() -> Color {
        Color::new(0.0, 1.0, 0.0, 1.0)
    }
    fn blue() -> Color {
        Color::new(0.0, 0.0, 1.0, 1.0)
    }
    fn yellow() -> Color {
        Color::new(1.0, 1.0, 0.0, 1.0)
    }

    /// An 8x2 atlas of four 2x2 tile columns: RED, GREEN, BLUE, YELLOW.
    fn test_atlas() -> Texture2D {
        let (r, g, b, y) = (red(), green(), blue(), yellow());
        let row = [r, r, g, g, b, b, y, y];
        let mut pixels = Vec::with_capacity(16);
        pixels.extend_from_slice(&row); // y = 0
        pixels.extend_from_slice(&row); // y = 1
        Texture2D {
            width: 8,
            height: 2,
            pixels,
        }
    }

    /// A tile definition spanning the whole 8x2 atlas at origin (0,0).
    fn atlas_def() -> TileDefinition {
        TileDefinition {
            texture_path: String::new(),
            texture_region: Rect2::new(Vector2::ZERO, Vector2::new(8.0, 2.0)),
            collision_shapes: Vec::new(),
            custom_data_keys: Vec::new(),
        }
    }

    fn tileset_with_source(source_id: i32) -> TileSet {
        let mut ts = TileSet::new(Vector2i::new(2, 2));
        ts.add_tile(source_id, atlas_def());
        ts
    }

    #[test]
    fn frame_at_wraps_and_handles_static() {
        let anim = TileAnimation {
            frames: 3,
            fps: 4.0,
        };
        assert_eq!(anim.frame_at(0.0), 0);
        assert_eq!(anim.frame_at(0.25), 1); // 0.25 * 4 = 1.0
        assert_eq!(anim.frame_at(0.5), 2); // 2.0
        assert_eq!(anim.frame_at(0.75), 0); // 3.0 % 3 = 0 (wrap)

        // A single-frame or zero-speed animation never advances.
        assert_eq!(TileAnimation { frames: 1, fps: 10.0 }.frame_at(5.0), 0);
        assert_eq!(TileAnimation { frames: 4, fps: 0.0 }.frame_at(5.0), 0);
    }

    #[test]
    fn renders_cell_from_atlas_region() {
        let atlas = test_atlas();
        let mut data = TileMapData::new(tileset_with_source(1));
        let mut layer = TileMapLayer::new("ground");
        // Cell (1,0) uses atlas column 2 (BLUE) via atlas_coords (2,0).
        layer
            .cells
            .insert(Vector2i::new(1, 0), TileData::with_atlas(1, Vector2i::new(2, 0), 0));
        data.add_layer(layer);

        let mut fb = FrameBuffer::new(8, 4, Color::BLACK);
        render_tilemap(&mut fb, &data, &atlas, Vector2::ZERO, 0.0, &HashMap::new());

        // Cell (1,0) → dest x[2,4), y[0,2): BLUE.
        assert_eq!(fb.get_pixel(2, 0), blue());
        assert_eq!(fb.get_pixel(3, 1), blue());
        // Empty cell (0,0) stays the clear color.
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn origin_offsets_cell_placement() {
        let atlas = test_atlas();
        let mut data = TileMapData::new(tileset_with_source(1));
        let mut layer = TileMapLayer::new("ground");
        // Cell (0,0) uses atlas column 0 (RED).
        layer
            .cells
            .insert(Vector2i::new(0, 0), TileData::with_atlas(1, Vector2i::new(0, 0), 0));
        data.add_layer(layer);

        let mut fb = FrameBuffer::new(8, 8, Color::BLACK);
        // Shift the whole map down-right by (4, 2) px.
        render_tilemap(&mut fb, &data, &atlas, Vector2::new(4.0, 2.0), 0.0, &HashMap::new());

        // Origin shifted → the tile lands at x[4,6), y[2,4).
        assert_eq!(fb.get_pixel(4, 2), red());
        assert_eq!(fb.get_pixel(5, 3), red());
        // Its un-shifted location is untouched.
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn higher_z_layer_draws_on_top() {
        let atlas = test_atlas();
        let mut data = TileMapData::new(tileset_with_source(1));

        // Layer added FIRST but with the HIGHER z_index: RED at (0,0).
        let mut top = TileMapLayer::new("top");
        top.z_index = 1;
        top.cells
            .insert(Vector2i::new(0, 0), TileData::with_atlas(1, Vector2i::new(0, 0), 0)); // RED
        data.add_layer(top);

        // Layer added SECOND but with the LOWER z_index: BLUE at (0,0).
        let mut bottom = TileMapLayer::new("bottom");
        bottom.z_index = 0;
        bottom
            .cells
            .insert(Vector2i::new(0, 0), TileData::with_atlas(1, Vector2i::new(2, 0), 0)); // BLUE
        data.add_layer(bottom);

        let mut fb = FrameBuffer::new(4, 4, Color::BLACK);
        render_tilemap(&mut fb, &data, &atlas, Vector2::ZERO, 0.0, &HashMap::new());

        // Lower z (BLUE) drawn first, higher z (RED) painted on top → RED wins.
        // Drawing in storage order instead would leave BLUE on top, so RED
        // proves the z_index sort is applied.
        assert_eq!(fb.get_pixel(0, 0), red());
    }

    #[test]
    fn disabled_layer_is_skipped() {
        let atlas = test_atlas();
        let mut data = TileMapData::new(tileset_with_source(1));
        let mut layer = TileMapLayer::new("hidden");
        layer.enabled = false;
        layer
            .cells
            .insert(Vector2i::new(0, 0), TileData::with_atlas(1, Vector2i::new(0, 0), 0));
        data.add_layer(layer);

        let mut fb = FrameBuffer::new(4, 4, Color::BLACK);
        render_tilemap(&mut fb, &data, &atlas, Vector2::ZERO, 0.0, &HashMap::new());

        // Nothing drawn: the framebuffer is still clear.
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
        assert_eq!(fb.get_pixel(1, 1), Color::BLACK);
    }

    #[test]
    fn animated_tile_advances_frames() {
        let atlas = test_atlas();
        let mut data = TileMapData::new(tileset_with_source(2));
        let mut layer = TileMapLayer::new("anim");
        // Base atlas_coords (0,0): frame 0 = col0 (RED), frame 1 = col1 (GREEN).
        layer
            .cells
            .insert(Vector2i::new(0, 0), TileData::with_atlas(2, Vector2i::new(0, 0), 0));
        data.add_layer(layer);

        let mut anims = HashMap::new();
        anims.insert(2, TileAnimation { frames: 2, fps: 10.0 });

        // time = 0 → frame 0 → RED.
        let mut fb0 = FrameBuffer::new(4, 4, Color::BLACK);
        render_tilemap(&mut fb0, &data, &atlas, Vector2::ZERO, 0.0, &anims);
        assert_eq!(fb0.get_pixel(0, 0), red());

        // time = 0.15s @ 10fps → frame 1 → GREEN.
        let mut fb1 = FrameBuffer::new(4, 4, Color::BLACK);
        render_tilemap(&mut fb1, &data, &atlas, Vector2::ZERO, 0.15, &anims);
        assert_eq!(fb1.get_pixel(0, 0), green());

        // time = 0.25s → 2.5 → frame 2 % 2 = 0 (wrap) → RED again.
        let mut fb2 = FrameBuffer::new(4, 4, Color::BLACK);
        render_tilemap(&mut fb2, &data, &atlas, Vector2::ZERO, 0.25, &anims);
        assert_eq!(fb2.get_pixel(0, 0), red());
    }

    #[test]
    fn multiple_cells_render_side_by_side() {
        let atlas = test_atlas();
        let mut data = TileMapData::new(tileset_with_source(1));
        let mut layer = TileMapLayer::new("ground");
        // A row of three distinct tiles.
        layer
            .cells
            .insert(Vector2i::new(0, 0), TileData::with_atlas(1, Vector2i::new(0, 0), 0)); // RED
        layer
            .cells
            .insert(Vector2i::new(1, 0), TileData::with_atlas(1, Vector2i::new(1, 0), 0)); // GREEN
        layer
            .cells
            .insert(Vector2i::new(2, 0), TileData::with_atlas(1, Vector2i::new(3, 0), 0)); // YELLOW
        data.add_layer(layer);

        let mut fb = FrameBuffer::new(8, 4, Color::BLACK);
        render_tilemap(&mut fb, &data, &atlas, Vector2::ZERO, 0.0, &HashMap::new());

        assert_eq!(fb.get_pixel(0, 0), red());
        assert_eq!(fb.get_pixel(2, 0), green());
        assert_eq!(fb.get_pixel(4, 0), yellow());
    }
}
