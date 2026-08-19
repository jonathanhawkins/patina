//! Sprite3D / Label3D / AnimatedSprite3D geometry for the editor 3D viewport
//! (pat-s3rrv.92).
//!
//! These three classes all descend from `Node3D`, so `collect_scene_content`
//! already emits a render item for each of them — with no mesh and no draw kind,
//! which is why the viewport showed three identical grey marker cubes. Godot
//! draws them as a **textured quad** (`SpriteBase3D::_draw`,
//! `scene/3d/sprite_3d.cpp`), so that is what this module builds: a two-triangle
//! mesh with UVs, plus the material carrying the decoded texture.
//!
//! ⚠️ **Lives in `gdeditor`, not in `render_server_3d.rs`.** The runtime render
//! server already has private `sprite3d_from_node` / `label3d_from_node`
//! extractors, but they yield a `TextureSlot { path }` — a path, not pixels —
//! because the runtime resolves textures later in its own pipeline. The editor
//! needs decoded pixels at list-build time, while the `SceneTree` is still in
//! scope. Building here also keeps this off a file another agent holds a lease
//! on, and mirrors how `gridmap_items` already lives on the editor side.

use gdcore::math::{Color, Vector2, Vector3};
use gdrender2d::font::{char_bitmap, CHAR_GAP, CHAR_HEIGHT, CHAR_WIDTH};
use gdscene::node::NodeId;
use gdscene::render_server_3d::MaterialTextureCache;
use gdscene::SceneTree;
use gdserver3d::material::{Material3D, ShadingMode, TextureImage};
use gdserver3d::mesh::{Mesh3D, PrimitiveType};
use gdvariant::Variant;

/// `SpriteBase3D.pixel_size` default — the world size of one texture pixel
/// (`doc/classes/SpriteBase3D.xml`).
pub const SPRITE3D_PIXEL_SIZE: f32 = 0.01;

/// `Label3D.pixel_size` default.
///
/// ⚠️ **HALF its `SpriteBase3D` neighbour's, not the same number.** Label3D
/// declares its own `pixel_size` defaulting to 0.005
/// (`doc/classes/Label3D.xml`), so sharing one constant between the two would
/// draw every 3D label at double size.
pub const LABEL3D_PIXEL_SIZE: f32 = 0.005;

/// `SpriteBase3D.axis` default: `Vector3.AXIS_Z`, so an unrotated sprite faces
/// +Z and spans its local XY plane.
pub const AXIS_Z: i64 = 2;

/// The quad Godot draws for a sprite of `tex_w` x `tex_h` texels.
///
/// Pure geometry, deliberately separated from texture decoding so the placement
/// rules can be tested without a PNG on disk.
///
/// `centered` (Godot's default) puts the quad's CENTRE on the node origin;
/// otherwise the origin is its top-left corner. `offset` is in texels and so is
/// scaled by `pixel_size` like the size itself — an offset in world units would
/// drift as soon as `pixel_size` changed.
pub fn sprite_quad_mesh(
    tex_w: u32,
    tex_h: u32,
    pixel_size: f32,
    centered: bool,
    offset: Vector2,
    flip_h: bool,
    flip_v: bool,
    axis: i64,
) -> Mesh3D {
    let w = tex_w.max(1) as f32 * pixel_size;
    let h = tex_h.max(1) as f32 * pixel_size;

    // Top-left of the quad in the sprite's own plane.
    let mut left = offset.x * pixel_size;
    let mut top = offset.y * pixel_size;
    if centered {
        left -= w * 0.5;
        top += h * 0.5;
    } else {
        top += h;
    }
    let right = left + w;
    let bottom = top - h;

    // ⚠️ `axis` names the axis the quad FACES, so the quad spans the OTHER two.
    // A sprite with axis = X is not an X-aligned quad; it is a YZ quad whose
    // normal points along X.
    let plane = |u: f32, v: f32| -> Vector3 {
        match axis {
            0 => Vector3::new(0.0, v, -u),
            1 => Vector3::new(u, 0.0, -v),
            _ => Vector3::new(u, v, 0.0),
        }
    };
    let normal = match axis {
        0 => Vector3::new(1.0, 0.0, 0.0),
        1 => Vector3::new(0.0, 1.0, 0.0),
        _ => Vector3::new(0.0, 0.0, 1.0),
    };

    // UVs run top-left origin, matching image row order; flipping swaps the ends
    // rather than negating, so the quad keeps its winding and stays front-facing.
    let (u0, u1) = if flip_h { (1.0, 0.0) } else { (0.0, 1.0) };
    let (v0, v1) = if flip_v { (1.0, 0.0) } else { (0.0, 1.0) };

    Mesh3D {
        vertices: vec![
            plane(left, top),
            plane(right, top),
            plane(right, bottom),
            plane(left, bottom),
        ],
        normals: vec![normal; 4],
        uvs: vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
        indices: vec![0, 1, 2, 0, 2, 3],
        primitive_type: PrimitiveType::Triangles,
        ..Default::default()
    }
}

/// `SpriteBase3D.shaded` / `Label3D.shaded`, which BOTH default to **false**
/// (`doc/classes/SpriteBase3D.xml`, `doc/classes/Label3D.xml`).
///
/// ⚠️ Sprites and labels are UNLIT unless asked otherwise — the opposite of
/// every mesh material's default. Shading them would tint a sprite by whatever
/// happens to light the scene, so an artist's colours would arrive wrong and
/// text would go dim in a dark scene, which is exactly what the flag exists to
/// prevent.
fn shading_for(node: &gdscene::node::Node) -> ShadingMode {
    match node.get_property("shaded") {
        Variant::Bool(true) => ShadingMode::Lambert,
        _ => ShadingMode::Unlit,
    }
}

/// Reads a float-ish property, accepting the `Int` a hand-written `.tscn` can
/// carry where Godot would write a float.
fn as_f32(v: Variant) -> Option<f32> {
    match v {
        Variant::Float(f) => Some(f as f32),
        Variant::Int(i) => Some(i as f32),
        _ => None,
    }
}

/// The quad and material for a `Sprite3D`, or `None` when it has no texture.
///
/// ⚠️ Returning `None` for a texture-less sprite is the Godot behaviour and also
/// the useful one: `SpriteBase3D::_draw` returns early with no texture, and
/// drawing the default marker cube instead would put a grey box where the scene
/// says nothing should appear.
pub fn sprite3d_geometry(
    tree: &SceneTree,
    nid: NodeId,
    cache: &mut MaterialTextureCache,
) -> Option<(Mesh3D, Material3D)> {
    let node = tree.get_node(nid)?;
    // ⚠️ CLASS GUARD FIRST. `texture` is a common property name — MeshInstance3D
    // and Decal both carry one — so without this any textured node in the scene
    // would be replaced by a flat quad. Declining by class is what keeps this
    // additive to the existing render list rather than a reinterpretation of it.
    if node.class_name() != "Sprite3D" {
        return None;
    }
    let path = match node.get_property("texture") {
        Variant::String(p) if !p.is_empty() => p,
        _ => return None,
    };
    let texture = cache.get_or_load(&path)?;

    let pixel_size = as_f32(node.get_property("pixel_size")).unwrap_or(SPRITE3D_PIXEL_SIZE);
    let centered = match node.get_property("centered") {
        Variant::Bool(b) => b,
        _ => true,
    };
    let offset = match node.get_property("offset") {
        Variant::Vector2(v) => v,
        _ => Vector2::new(0.0, 0.0),
    };
    let flag = |k: &str| matches!(node.get_property(k), Variant::Bool(true));
    let axis = match node.get_property("axis") {
        Variant::Int(i) => i,
        _ => AXIS_Z,
    };

    let mesh = sprite_quad_mesh(
        texture.width,
        texture.height,
        pixel_size,
        centered,
        offset,
        flag("flip_h"),
        flag("flip_v"),
        axis,
    );

    let modulate = match node.get_property("modulate") {
        Variant::Color(c) => c,
        // `modulate` defaults to opaque white (`doc/classes/SpriteBase3D.xml`),
        // which multiplies the texture through unchanged.
        _ => Color::WHITE,
    };
    let material = Material3D {
        albedo: modulate,
        albedo_texture: Some(texture),
        shading_mode: shading_for(node),
        double_sided: true,
        ..Default::default()
    };
    Some((mesh, material))
}

/// Godot's `Label3D.font_size` default (`doc/classes/Label3D.xml`).
pub const LABEL3D_FONT_SIZE: i64 = 32;

/// Rasterizes `text` into an RGBA image with the engine's built-in 5x7 bitmap
/// font, transparent everywhere the glyphs do not cover.
///
/// ⚠️ **An APPROXIMATION of Godot's text, and deliberately so.** Godot renders
/// Label3D through a TextServer with a real TTF face; Patina's only font is the
/// 5x7 bitmap in `gdrender2d::font`, which is what the 2D canvas already draws
/// with. Matching Godot's exact glyph shapes is a font-stack problem, not a
/// Label3D one — what this has to get right is that a Label3D produces READABLE
/// TEXT PIXELS sized by `font_size` and `pixel_size`, instead of a marker cube.
///
/// Returns `None` for empty text: Godot draws nothing, and a zero-area quad
/// would divide by zero when the UVs are built.
pub fn rasterize_text(text: &str, scale: u32, color: Color) -> Option<TextureImage> {
    let scale = scale.max(1);
    let lines: Vec<&str> = text.split('\n').collect();
    let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    if cols == 0 {
        return None;
    }
    // Same spacing rule as `font::text_width` / `text_height`: a gap BETWEEN
    // cells only, never trailing, so a one-character label is exactly one cell.
    let width = (cols as u32 * CHAR_WIDTH + (cols as u32 - 1) * CHAR_GAP) * scale;
    let rows = lines.len() as u32;
    let height = (rows * CHAR_HEIGHT + (rows - 1) * CHAR_GAP) * scale;

    let clear = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    let mut pixels = vec![clear; (width * height) as usize];
    for (li, line) in lines.iter().enumerate() {
        for (ci, ch) in line.chars().enumerate() {
            let glyph = char_bitmap(ch);
            for (ry, row) in glyph.iter().enumerate() {
                for rx in 0..CHAR_WIDTH {
                    // MSB is the leftmost column WITHIN THE 5-BIT CELL, so the
                    // shift is off CHAR_WIDTH, not off the u8's own width.
                    if (row >> (CHAR_WIDTH - 1 - rx)) & 1 == 0 {
                        continue;
                    }
                    let x0 = (ci as u32 * (CHAR_WIDTH + CHAR_GAP) + rx) * scale;
                    let y0 = (li as u32 * (CHAR_HEIGHT + CHAR_GAP) + ry as u32) * scale;
                    // One source texel covers a `scale` x `scale` block.
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let (x, y) = (x0 + dx, y0 + dy);
                            if x < width && y < height {
                                pixels[(y * width + x) as usize] = color;
                            }
                        }
                    }
                }
            }
        }
    }
    Some(TextureImage {
        width,
        height,
        pixels,
    })
}

/// The quad and material for a `Label3D`, or `None` when it has no text.
pub fn label3d_geometry(tree: &SceneTree, nid: NodeId) -> Option<(Mesh3D, Material3D)> {
    let node = tree.get_node(nid)?;
    let text = match node.get_property("text") {
        Variant::String(t) if !t.is_empty() => t,
        _ => return None,
    };
    let modulate = match node.get_property("modulate") {
        Variant::Color(c) => c,
        _ => Color::WHITE,
    };
    let font_size = match node.get_property("font_size") {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        _ => LABEL3D_FONT_SIZE,
    };
    // Godot sizes glyphs in points against a real face; the built-in cell is
    // CHAR_HEIGHT tall, so the nearest whole-cell scale is the honest mapping.
    let scale = (font_size.max(1) as u32 / CHAR_HEIGHT).max(1);
    let texture = rasterize_text(&text, scale, Color::WHITE)?;

    // ⚠️ LABEL3D_PIXEL_SIZE, not SPRITE3D_PIXEL_SIZE — 0.005 against 0.01.
    let pixel_size = as_f32(node.get_property("pixel_size")).unwrap_or(LABEL3D_PIXEL_SIZE);
    // `horizontal_alignment` and `vertical_alignment` both default to CENTER
    // (`= 1`), which is the same placement `centered` gives a sprite.
    let mesh = sprite_quad_mesh(
        texture.width,
        texture.height,
        pixel_size,
        true,
        Vector2::new(0.0, 0.0),
        false,
        false,
        AXIS_Z,
    );
    let material = Material3D {
        albedo: modulate,
        albedo_texture: Some(std::sync::Arc::new(texture)),
        shading_mode: shading_for(node),
        // `double_sided` defaults to TRUE on Label3D (`doc/classes/Label3D.xml`),
        // unlike most materials — text is meant to be legible from behind.
        double_sided: true,
        ..Default::default()
    };
    Some((mesh, material))
}

/// The quad for whichever of the three billboard classes `nid` is, or `None`.
///
/// ⚠️ AnimatedSprite3D is handled by the **Sprite3D path on purpose**. Its
/// current frame is published as the node's own `texture` by
/// `SceneTree::process_animated_sprites`, exactly as it is for the 2D canvas, so
/// reading `texture` here is what makes playback reach the viewport without a
/// second animation path that could disagree about which frame is showing.
pub fn billboard_geometry(
    tree: &SceneTree,
    nid: NodeId,
    cache: &mut MaterialTextureCache,
) -> Option<(Mesh3D, Material3D)> {
    match tree.get_node(nid)?.class_name() {
        "Sprite3D" | "AnimatedSprite3D" => sprite3d_geometry(tree, nid, cache),
        "Label3D" => label3d_geometry(tree, nid),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Godot's own diagram in `sprite_3d.cpp:107-115` fixes the corner order in
    /// 3D as `0` top-left, `1` top-right, `2` bottom-right, `3` bottom-left.
    /// Every assertion here is written against that diagram.
    fn quad(centered: bool, flip_h: bool, flip_v: bool, axis: i64) -> Mesh3D {
        sprite_quad_mesh(
            100,
            50,
            SPRITE3D_PIXEL_SIZE,
            centered,
            Vector2::new(0.0, 0.0),
            flip_h,
            flip_v,
            axis,
        )
    }

    #[test]
    fn a_centered_quad_straddles_the_origin() {
        let m = quad(true, false, false, AXIS_Z);
        // 100 x 50 texels at 0.01 world units per texel = 1.0 x 0.5.
        assert_eq!(m.vertices[0], Vector3::new(-0.5, 0.25, 0.0), "top-left");
        assert_eq!(m.vertices[1], Vector3::new(0.5, 0.25, 0.0), "top-right");
        assert_eq!(m.vertices[2], Vector3::new(0.5, -0.25, 0.0), "bottom-right");
        assert_eq!(m.vertices[3], Vector3::new(-0.5, -0.25, 0.0), "bottom-left");
    }

    /// ⚠️ The uncentered quad extends UP from the origin, not down. Godot
    /// applies **no Y inversion for `axis == AXIS_Z`** (`sprite_3d.cpp:179` only
    /// negates for the X and Y axes), so the 2D rect's `position.y` is the
    /// BOTTOM edge in 3D. Reading the 2D "top-left origin" convention across
    /// into 3D would hang every uncentered sprite on the wrong side of its node.
    #[test]
    fn an_uncentered_quad_sits_above_its_origin() {
        let m = quad(false, false, false, AXIS_Z);
        assert_eq!(m.vertices[3], Vector3::new(0.0, 0.0, 0.0), "origin corner");
        assert_eq!(m.vertices[1], Vector3::new(1.0, 0.5, 0.0), "far corner");
    }

    #[test]
    fn offset_is_in_texels_so_it_tracks_pixel_size() {
        // An offset of 100 texels at 0.01 is exactly one quad width across.
        let m = sprite_quad_mesh(
            100,
            50,
            SPRITE3D_PIXEL_SIZE,
            true,
            Vector2::new(100.0, 0.0),
            false,
            false,
            AXIS_Z,
        );
        assert_eq!(m.vertices[0].x, 0.5, "shifted by a full width");
        assert_eq!(m.vertices[1].x, 1.5);
    }

    #[test]
    fn uvs_follow_the_corner_order_and_the_flip_swaps() {
        assert_eq!(
            quad(true, false, false, AXIS_Z).uvs,
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        );
        // `SWAP(uvs[0], uvs[1]); SWAP(uvs[2], uvs[3])` (sprite_3d.cpp:153).
        assert_eq!(
            quad(true, true, false, AXIS_Z).uvs,
            vec![[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
        );
        // `SWAP(uvs[0], uvs[3]); SWAP(uvs[1], uvs[2])` (sprite_3d.cpp:158).
        assert_eq!(
            quad(true, false, true, AXIS_Z).uvs,
            vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
        );
    }

    /// ⚠️ `axis` names the axis the quad FACES, so the quad spans the OTHER
    /// two. Reading it as "the axis the quad lies along" builds a quad rotated
    /// 90 degrees from Godot's — which still renders, just edge-on.
    #[test]
    fn axis_names_the_facing_not_the_plane() {
        let x = quad(true, false, false, 0);
        assert_eq!(x.normals[0], Vector3::new(1.0, 0.0, 0.0));
        assert!(
            x.vertices.iter().all(|v| v.x == 0.0),
            "an X-facing quad is flat in X, spanning Y and Z: {:?}",
            x.vertices
        );
        let y = quad(true, false, false, 1);
        assert_eq!(y.normals[0], Vector3::new(0.0, 1.0, 0.0));
        assert!(
            y.vertices.iter().all(|v| v.y == 0.0),
            "a Y-facing quad is flat in Y: {:?}",
            y.vertices
        );
    }

    fn opaque(img: &TextureImage, x: u32, y: u32) -> bool {
        img.pixels[(y * img.width + x) as usize].a > 0.0
    }

    /// ⚠️ **THE COLUMN ORDER IS THE WHOLE POINT.** `char_bitmap` packs the
    /// leftmost column in the most significant bit OF THE 5-BIT CELL, so the
    /// shift is off `CHAR_WIDTH` and not off the `u8`'s own width. Getting it
    /// wrong still produces text-shaped pixels — mirrored, or shifted three
    /// columns into the neighbouring glyph — which a "some pixels are lit"
    /// assertion cannot catch. 'A' opens with `0b01110`: dark, lit, lit, lit,
    /// dark.
    #[test]
    fn a_glyph_lands_in_the_right_columns() {
        let img = rasterize_text("A", 1, Color::WHITE).expect("one glyph rasterizes");
        assert_eq!((img.width, img.height), (CHAR_WIDTH, CHAR_HEIGHT));
        let top: Vec<bool> = (0..CHAR_WIDTH).map(|x| opaque(&img, x, 0)).collect();
        assert_eq!(top, vec![false, true, true, true, false], "0b01110");
    }

    #[test]
    fn text_dimensions_gap_between_cells_only() {
        // Two 5-wide cells with ONE gap between them, and no trailing gap.
        let two = rasterize_text("AB", 1, Color::WHITE).expect("rasterizes");
        assert_eq!(two.width, CHAR_WIDTH * 2 + CHAR_GAP);
        assert_eq!(two.height, CHAR_HEIGHT);

        let lines = rasterize_text("A\nB", 1, Color::WHITE).expect("rasterizes");
        assert_eq!(lines.width, CHAR_WIDTH, "width is the LONGEST line");
        assert_eq!(lines.height, CHAR_HEIGHT * 2 + CHAR_GAP);
    }

    #[test]
    fn scale_multiplies_both_axes() {
        let big = rasterize_text("A", 3, Color::WHITE).expect("rasterizes");
        assert_eq!((big.width, big.height), (CHAR_WIDTH * 3, CHAR_HEIGHT * 3));
        // The lit column at x=1 becomes a 3-wide band at x=3..6.
        assert!(!opaque(&big, 2, 0), "still dark at the end of column 0");
        assert!(opaque(&big, 3, 0) && opaque(&big, 5, 0), "column 1 widened");
    }

    /// Empty text draws nothing in Godot, and a zero-area quad would divide by
    /// zero building UVs.
    #[test]
    fn empty_text_rasterizes_to_nothing() {
        assert!(rasterize_text("", 1, Color::WHITE).is_none());
    }

    /// A positive control for the two `all(..)` assertions above: those pass
    /// vacuously if the quad ever came back empty.
    #[test]
    fn every_quad_is_two_triangles_over_four_corners() {
        let m = quad(true, false, false, AXIS_Z);
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.normals.len(), 4);
        assert_eq!(m.uvs.len(), 4);
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(m.primitive_type, PrimitiveType::Triangles);
    }
}
