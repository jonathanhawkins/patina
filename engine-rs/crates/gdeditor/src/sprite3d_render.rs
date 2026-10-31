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
use gdcore::math3d::Basis;
use gdrender2d::font::{char_bitmap, CHAR_GAP, CHAR_HEIGHT, CHAR_WIDTH};
use gdscene::node::NodeId;
use gdscene::render_server_3d::MaterialTextureCache;
use gdscene::SceneTree;
use gdserver3d::material::{CullMode, Material3D, ShadingMode, TextureImage};
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
    // The whole texture as one frame — the 1x1 sheet every existing caller means.
    sprite_quad_mesh_framed(
        tex_w, tex_h, pixel_size, centered, offset, flip_h, flip_v, axis, 1, 1, 0,
    )
}

/// [`sprite_quad_mesh`] for one cell of an `hframes` x `vframes` sprite sheet
/// (pat-mgixk.34 part 2).
///
/// Godot's `SpriteBase3D` treats the texture as a grid and draws a single cell:
/// `hframes`/`vframes` divide it and `frame` indexes it row-major
/// (`scene/3d/sprite_3d.cpp`). Two things follow, and missing either is a visible
/// bug rather than a subtle one:
///
/// 1. **The quad shrinks.** A frame of a 2x1 sheet is half the texture's width, so
///    the quad is half as wide. Sizing it from the full texture would stretch one
///    cell across the whole sprite.
/// 2. **The UVs cover only that cell**, so the other frames are not sampled.
///
/// ⚠️ Added as a separate entry point rather than three more parameters on
/// `sprite_quad_mesh`, which already takes eight: its four unit tests pin the
/// unframed geometry exactly, and every existing call site means "the whole
/// texture". Delegating keeps both true by construction.
///
/// `frame` wraps rather than clamping or panicking: Godot stores it as an int a
/// script can set past the end, and an out-of-range frame showing cell 0 is far
/// easier to recognise than a blank sprite.
#[allow(clippy::too_many_arguments)]
pub fn sprite_quad_mesh_framed(
    tex_w: u32,
    tex_h: u32,
    pixel_size: f32,
    centered: bool,
    offset: Vector2,
    flip_h: bool,
    flip_v: bool,
    axis: i64,
    hframes: i64,
    vframes: i64,
    frame: i64,
) -> Mesh3D {
    // `max(1)`: Godot's properties have a minimum of 1, but a `.tscn` can carry 0
    // and dividing by it would blow up the quad instead of drawing something.
    let hf = hframes.max(1) as u32;
    let vf = vframes.max(1) as u32;
    let cells = (hf as i64) * (vf as i64);
    // `rem_euclid`, not `%`: a negative `frame` must land on a real cell, and `%`
    // would give a negative index.
    let cell = frame.rem_euclid(cells.max(1)) as u32;
    let (col, row) = (cell % hf, cell / hf);

    // One CELL's worth of pixels, not the whole sheet (point 1 above).
    let w = (tex_w.max(1) / hf).max(1) as f32 * pixel_size;
    let h = (tex_h.max(1) / vf).max(1) as f32 * pixel_size;

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
    //
    // Spanning only the selected cell is point 2 above. With the default 1x1 sheet
    // `du`/`dv` are 1.0 and the cell origin is 0, so this reduces to the full-range
    // 0..1 the unframed callers had.
    let du = 1.0 / hf as f32;
    let dv = 1.0 / vf as f32;
    let (cu, cv) = (col as f32 * du, row as f32 * dv);
    let (u0, u1) = if flip_h { (cu + du, cu) } else { (cu, cu + du) };
    let (v0, v1) = if flip_v { (cv + dv, cv) } else { (cv, cv + dv) };

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
    //
    // ⚠️ AnimatedSprite3D belongs in this list, and its absence is the whole of
    // pat-mgixk.34. `billboard_geometry` below has always ROUTED
    // "AnimatedSprite3D" here, and its doc comment has always claimed playback
    // reaches the viewport — but this guard rejected the class, so every
    // AnimatedSprite3D fell through to the default marker cube with its texture
    // computed, published and then discarded. The routing and the guard
    // disagreed, and the comment documented the routing.
    if !matches!(node.class_name(), "Sprite3D" | "AnimatedSprite3D") {
        return None;
    }
    // `process_animated_sprites` publishes the current SpriteFrames frame as a
    // plain path String (scene_tree.rs `set_property("texture", ...)`), which is
    // the same shape a `.tscn`-authored Sprite3D carries, so one read serves
    // both. A `Variant::Resource` is accepted too: a script that assigns an
    // already-loaded texture (`$Sprite.texture = load(..)`) stores the resource
    // rather than its path, and dropping that case would make a scripted
    // assignment silently invisible the same way this bug did.
    let path = match node.get_property("texture") {
        Variant::String(p) if !p.is_empty() => p,
        Variant::Resource(r) if !r.path.is_empty() => r.path.clone(),
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

    // Sprite-sheet frame selection (pat-mgixk.34 part 2). Godot's defaults are
    // `hframes = vframes = 1`, `frame = 0`, which is the whole texture — so an
    // unauthored sprite is unchanged and an `AnimatedSprite3D`, which selects its
    // image through `SpriteFrames` instead, is unaffected.
    let int_prop = |key: &str, default: i64| match node.get_property(key) {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        _ => default,
    };
    let hframes = int_prop("hframes", 1);
    let vframes = int_prop("vframes", 1);
    // `frame_coords` is Godot's (column, row) spelling of the same selection, and
    // in Godot both are views of ONE value (`sprite_3d.cpp` keeps them in step).
    // Patina stores them as separate properties, so precedence decides.
    //
    // ⚠️ A ZERO `frame_coords` must NOT win over an authored `frame`, and reading
    // it unconditionally is a real bug I shipped and had to fix here:
    // `packed_scene::node_with_class_defaults` seeds every ClassDB default onto an
    // instantiated node, and ClassDB declares `frame_coords` with a default of
    // `Vector2i(0, 0)` (class_db.rs:2395). So EVERY `.tscn`-instantiated Sprite3D
    // carries a zero `frame_coords`, which beat `frame = 1` and selected cell 0 —
    // the test showed frame 1 still drawing the red (left) cell.
    //
    // This is the same shape as the identity-`transform`-beats-`position` bug that
    // pat-mgixk.29 fixed in `node_with_class_defaults`: **a seeded default must
    // never beat an authored value.** Treating only a NON-ZERO `frame_coords` as
    // authored is safe in both directions — a genuine `(0, 0)` means cell 0, which
    // is what the `frame` fallback yields anyway.
    let coords_cell = match node.get_property("frame_coords") {
        Variant::Vector2i(v) if v.x != 0 || v.y != 0 => {
            Some((v.y as i64) * hframes.max(1) + (v.x as i64))
        }
        Variant::Vector2(v) if v.x != 0.0 || v.y != 0.0 => {
            Some((v.y as i64) * hframes.max(1) + (v.x as i64))
        }
        _ => None,
    };
    let frame = coords_cell.unwrap_or_else(|| int_prop("frame", 0));
    let mesh = sprite_quad_mesh_framed(
        texture.width,
        texture.height,
        pixel_size,
        centered,
        offset,
        flag("flip_h"),
        flag("flip_v"),
        axis,
        hframes,
        vframes,
        frame,
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
        // ⚠️ BOTH fields, and `cull_mode` is the load-bearing one. `double_sided`
        // is "retained for existing callers" (`gdserver3d::material`) and the
        // software rasterizer NEVER READS IT — `gdrender3d` mentions it only in
        // a comment. Culling is decided solely by `cull_mode`, which defaults to
        // `Back`, and `sprite_quad_mesh`'s winding is clockwise as seen from the
        // camera the sprite faces. So every Sprite3D quad was built correctly,
        // handed to the renderer correctly, and then discarded by
        // `render_modes::should_cull` before it shaded a single pixel
        // (pat-mgixk.34).
        double_sided: true,
        cull_mode: CullMode::Disabled,
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
    let (width, height, mask) = glyph_mask(text, scale)?;
    let mut pixels = vec![TRANSPARENT; (width * height) as usize];
    for (i, set) in mask.iter().enumerate() {
        if *set {
            pixels[i] = color;
        }
    }
    Some(TextureImage {
        width,
        height,
        pixels,
    })
}

/// Fully transparent, the colour every uncovered texel keeps.
const TRANSPARENT: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// Godot's `Label3D.outline_size` default (`doc/classes/Label3D.xml`): no
/// outline.
pub const LABEL3D_OUTLINE_SIZE: i64 = 0;

/// An outline wider than this is clamped. The dilation below is
/// `O(w * h * r^2)`, and Godot's own property caps at 127 — a `.tscn` carrying
/// a four-digit value would otherwise stall the frame rather than draw a thick
/// outline.
const MAX_OUTLINE_PX: i64 = 64;

/// Which texels the glyphs of `text` cover, and the image size that holds them.
///
/// 🔑 Extracted so `rasterize_text` and [`rasterize_text_outlined`] cannot
/// disagree about layout. The line/cell spacing rule here is the one thing both
/// must share: a second copy would drift, and the outline would then sit beside
/// glyphs drawn at different positions.
fn glyph_mask(text: &str, scale: u32) -> Option<(u32, u32, Vec<bool>)> {
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

    let mut mask = vec![false; (width * height) as usize];
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
                                mask[(y * width + x) as usize] = true;
                            }
                        }
                    }
                }
            }
        }
    }
    Some((width, height, mask))
}

/// `text` in `color`, ringed by `outline_px` texels of `outline_color`
/// (Godot `Label3D.outline_size` / `outline_modulate`).
///
/// ⚠️ **Both colours are baked into the texture, so the caller must set the
/// material's albedo to WHITE.** `albedo_texture` MULTIPLIES the flat albedo
/// (`gdrender3d::shader`'s own
/// `sample_material_albedo_texture_multiplies_flat_color`), and Patina draws a
/// Label3D as ONE quad — so there is no second pass to give the outline a
/// colour of its own. Leaving albedo at `modulate` would tint the outline by the
/// glyph colour, and squaring `modulate` on the glyphs.
///
/// The image is PADDED by `outline_px` on all four sides: dilating in place
/// would clip the ring on every glyph touching an edge, which for a single-line
/// label is all of them.
pub fn rasterize_text_outlined(
    text: &str,
    scale: u32,
    color: Color,
    outline_px: u32,
    outline_color: Color,
) -> Option<TextureImage> {
    let (cw, ch, mask) = glyph_mask(text, scale)?;
    if outline_px == 0 {
        // No ring to draw, so no padding either — this is `rasterize_text` with
        // the glyph colour baked in.
        let mut pixels = vec![TRANSPARENT; (cw * ch) as usize];
        for (i, set) in mask.iter().enumerate() {
            if *set {
                pixels[i] = color;
            }
        }
        return Some(TextureImage {
            width: cw,
            height: ch,
            pixels,
        });
    }

    let r = outline_px as i64;
    let width = cw + 2 * outline_px;
    let height = ch + 2 * outline_px;
    let mut pixels = vec![TRANSPARENT; (width * height) as usize];

    let covered = |cx: i64, cy: i64| -> bool {
        cx >= 0
            && cy >= 0
            && cx < cw as i64
            && cy < ch as i64
            && mask[(cy * cw as i64 + cx) as usize]
    };

    for py in 0..height as i64 {
        for px in 0..width as i64 {
            let (cx, cy) = (px - r, py - r);
            if covered(cx, cy) {
                pixels[(py * width as i64 + px) as usize] = color;
                continue;
            }
            // Euclidean, not Chebyshev: Godot's outline is a rounded dilation,
            // and a square halo around every glyph reads as a box rather than
            // an outline.
            let mut near = false;
            'ring: for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy > r * r {
                        continue;
                    }
                    if covered(cx + dx, cy + dy) {
                        near = true;
                        break 'ring;
                    }
                }
            }
            if near {
                pixels[(py * width as i64 + px) as usize] = outline_color;
            }
        }
    }
    Some(TextureImage {
        width,
        height,
        pixels,
    })
}

// ---------------------------------------------------------------------------
// Billboarding (pat-mgixk.34 section 2)
// ---------------------------------------------------------------------------

/// `SpriteBase3D.billboard` / `Label3D.billboard` = BILLBOARD_DISABLED.
pub const BILLBOARD_DISABLED: i64 = 0;
/// BILLBOARD_ENABLED: the quad faces the camera on both axes.
pub const BILLBOARD_ENABLED: i64 = 1;
/// BILLBOARD_FIXED_Y: the quad yaws toward the camera, keeping its own up axis.
pub const BILLBOARD_FIXED_Y: i64 = 2;

/// The world basis that turns `nid`'s quad to face the camera, or `None` when
/// the node does not billboard.
///
/// 🔑 Returning a BASIS rather than rotated vertices is what makes this correct
/// for the wired path. `Render3DItem.global_transform` is applied to the mesh
/// downstream, so a quad pre-rotated in local space would be rotated AGAIN by
/// the node's own transform. Godot does the same thing — `BILLBOARD_ENABLED`
/// replaces the instance basis with the camera's (`sprite_3d.cpp`,
/// `material_storage` billboard modes), keeping the origin.
///
/// The node's SCALE is preserved: the basis columns are the camera's axes
/// re-scaled by `node_basis.get_scale()`, so a scaled sprite stays scaled.
///
/// ⚠️ `Basis` here stores COLUMNS (`gdcore::math3d`, and `Basis::xform` sums
/// `x * v.x + y * v.y + z * v.z`). Columns X/Y are what the quad spans for the
/// default `axis = AXIS_Z`, so mapping them onto the camera's right/up is what
/// squares the quad to the screen. Written against the column convention on
/// purpose — a transposed basis is still orthonormal and would look like a
/// plausible rotation rather than a bug.
pub fn billboard_basis_for(
    tree: &SceneTree,
    nid: NodeId,
    node_basis: Basis,
    cam_right: Vector3,
    cam_up: Vector3,
) -> Option<Basis> {
    let node = tree.get_node(nid)?;
    if !matches!(
        node.class_name(),
        "Sprite3D" | "AnimatedSprite3D" | "Label3D"
    ) {
        return None;
    }
    let mode = match node.get_property("billboard") {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        _ => BILLBOARD_DISABLED,
    };
    let scale = node_basis.get_scale();
    match mode {
        BILLBOARD_ENABLED => {
            let x = cam_right.normalized();
            let y = cam_up.normalized();
            // Right x up points back toward the viewer, which is where the
            // quad's +Z face has to look.
            let z = x.cross(y).normalized();
            Some(scaled_columns(x, y, z, scale))
        }
        BILLBOARD_FIXED_Y => {
            // Godot locks the Y axis and yaws only, so a sprite stays upright
            // while following the camera around — the mode billboards trees and
            // characters with.
            let y = node_basis.y.normalized();
            // The camera's right with any component along the kept-up axis
            // removed, so the result is still perpendicular to `y`.
            let flat = cam_right - y * cam_right.dot(y);
            // Degenerate only when the camera looks straight down the kept axis,
            // where no yaw can face it; leaving the node's own basis alone is
            // better than emitting a zero basis that collapses the quad.
            if flat.length() < 1e-5 {
                return None;
            }
            let x = flat.normalized();
            let z = x.cross(y).normalized();
            Some(scaled_columns(x, y, z, scale))
        }
        _ => None,
    }
}

/// An orthonormal basis re-scaled per axis, so billboarding never silently
/// resets a scaled node to unit size.
fn scaled_columns(x: Vector3, y: Vector3, z: Vector3, scale: Vector3) -> Basis {
    Basis {
        x: x * scale.x,
        y: y * scale.y,
        z: z * scale.z,
    }
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

    // Godot `Label3D.outline_size` / `outline_modulate`.
    let outline_size = match node.get_property("outline_size") {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        _ => LABEL3D_OUTLINE_SIZE,
    }
    .clamp(0, MAX_OUTLINE_PX);
    let outline_modulate = match node.get_property("outline_modulate") {
        Variant::Color(c) => c,
        // Godot's default is opaque black (`doc/classes/Label3D.xml`).
        _ => Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    };
    // `outline_size` is in units of Godot's font pixels, but this texture's
    // glyphs are `CHAR_HEIGHT * scale` tall rather than `font_size` tall — the
    // built-in 5x7 cell cannot hit an arbitrary point size. Converting through
    // that ratio keeps the outline the same FRACTION of the glyph it is in
    // Godot, which is what "has outline-colour pixels around the glyphs" means
    // at any font size.
    //
    // `max(1)` once non-zero: an authored outline must never round down to no
    // outline at all, which would look exactly like the property being ignored.
    let outline_px = if outline_size == 0 {
        0
    } else {
        ((outline_size * (CHAR_HEIGHT as i64) * (scale as i64)) / font_size.max(1)).max(1) as u32
    };

    // ⚠️ Two different colour conventions, and the albedo below depends on
    // which ran. With an outline the texture carries BOTH colours and the
    // albedo must be neutral; without one the long-standing path keeps the
    // glyphs white in the texture and tints them through the albedo. Kept
    // branch-wise rather than unified so the no-outline case stays
    // bit-identical to what already shipped and was verified.
    let (texture, albedo) = if outline_px > 0 {
        (
            rasterize_text_outlined(&text, scale, modulate, outline_px, outline_modulate)?,
            Color::WHITE,
        )
    } else {
        (rasterize_text(&text, scale, Color::WHITE)?, modulate)
    };

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
        albedo,
        albedo_texture: Some(std::sync::Arc::new(texture)),
        shading_mode: shading_for(node),
        // `double_sided` defaults to TRUE on Label3D (`doc/classes/Label3D.xml`),
        // unlike most materials — text is meant to be legible from behind.
        //
        // ⚠️ `cull_mode` is what the rasterizer actually honours — see the note
        // on the Sprite3D material above. Without it the label's quad is culled
        // by winding and the text never reaches a pixel (pat-mgixk.34).
        double_sided: true,
        cull_mode: CullMode::Disabled,
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
///
/// ⚠️ **Routing a class here is only half of it — `sprite3d_geometry`'s class
/// guard must name it too.** This arm listed "AnimatedSprite3D" from the start
/// while that guard accepted only "Sprite3D", so the comment above described an
/// intent the code did not have and every AnimatedSprite3D silently drew a
/// marker cube (pat-mgixk.34). Add to both or neither.
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
