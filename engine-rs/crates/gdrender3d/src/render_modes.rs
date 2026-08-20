//! Material render-mode decisions: transparency, culling, depth (pat-u3l8c.34).
//!
//! `material.rs` only had `double_sided` and the raster was opaque-only. This
//! module provides the pure decision logic a 3D rasterizer needs: back-to-front
//! sorting for alpha blending, alpha-scissor discard, face culling
//! (front/back/disabled) via screen-space winding, and depth-draw modes.

use gdcore::math::{Color, Vector2};

/// The mode enums are defined beside `StandardMaterial3D` in `gdserver3d` and
/// re-exported here (pat-s3rrv.30).
///
/// They started out defined in this module, which made them unusable by the
/// very struct that needs to carry them: `gdrender3d` depends on `gdserver3d`,
/// not the reverse. Re-exporting keeps one definition, so a material's
/// `cull_mode` and [`should_cull`]'s parameter are the same type by
/// construction rather than by convention.
pub use gdserver3d::material::{BlendMode, CullMode, DepthDrawMode, Transparency};

/// Composites `src` over `dst` per Godot's `BaseMaterial3D.blend_mode`
/// (pat-s3rrv.30).
///
/// ⚠️ Every equation here is transcribed from the GL blend state Godot sets in
/// `drivers/gles3/rasterizer_scene_gles3.cpp:3255-3285`, because the mode names
/// do not describe the maths:
///
/// * `Sub` uses `GL_FUNC_REVERSE_SUBTRACT` — it is `dst - src`, NOT `src - dst`.
///   Reading it as "subtract the source" gets the sign backwards on every pixel.
/// * `Mul` multiplies by the DESTINATION (`GL_DST_COLOR`, `GL_ZERO`) and leaves
///   destination alpha alone; it is a modulate, not a 50/50 mix.
/// * `Add` and `Sub` scale the source by its own alpha (`GL_SRC_ALPHA`) in the
///   transparent pass, so a fully transparent additive fragment contributes
///   nothing rather than its full colour.
/// * `Mix` uses the transparent-background variant, whose alpha is
///   `src.a + dst.a * (1 - src.a)`. Against an opaque target (`dst.a == 1`) that
///   is identical to the opaque-background variant Godot otherwise selects, so
///   the two agree wherever it matters here.
pub fn blend_pixel(mode: BlendMode, src: Color, dst: Color) -> Color {
    let a = src.a.clamp(0.0, 1.0);
    let c = |v: f32| v.clamp(0.0, 1.0);
    match mode {
        BlendMode::Mix => Color::new(
            c(src.r * a + dst.r * (1.0 - a)),
            c(src.g * a + dst.g * (1.0 - a)),
            c(src.b * a + dst.b * (1.0 - a)),
            c(a + dst.a * (1.0 - a)),
        ),
        BlendMode::Add => Color::new(
            c(src.r * a + dst.r),
            c(src.g * a + dst.g),
            c(src.b * a + dst.b),
            c(src.a * a + dst.a),
        ),
        // Reverse subtract: the destination is what gets darkened.
        BlendMode::Sub => Color::new(
            c(dst.r - src.r * a),
            c(dst.g - src.g * a),
            c(dst.b - src.b * a),
            c(dst.a - src.a * a),
        ),
        BlendMode::Mul => Color::new(
            c(src.r * dst.r),
            c(src.g * dst.g),
            c(src.b * dst.b),
            // `GL_ZERO, GL_ONE` on the alpha channel: the destination keeps its
            // own alpha rather than being modulated.
            c(dst.a),
        ),
        BlendMode::PremultAlpha => Color::new(
            c(src.r + dst.r * (1.0 - a)),
            c(src.g + dst.g * (1.0 - a)),
            c(src.b + dst.b * (1.0 - a)),
            c(src.a + dst.a * (1.0 - a)),
        ),
    }
}

/// Signed area of a screen-space triangle.
///
/// ⚠️ **Screen space has Y growing DOWNWARD**, which inverts handedness against
/// the usual y-up reading of this formula. `clip_to_screen` maps NDC with
/// `y = (1 - ndc_y) * 0.5 * height`, so a FRONT face — counter-clockwise in NDC,
/// the OpenGL/Godot/wgpu `FrontFace::Ccw` convention — comes out CLOCKWISE here
/// and its signed area is **negative**. A back face is positive.
///
/// The sign is therefore the opposite of what "positive = counter-clockwise =
/// front-facing" would suggest, and reading it the y-up way is exactly the bug
/// [`should_cull`] carried (pat-s3rrv.122).
pub fn signed_area(a: Vector2, b: Vector2, c: Vector2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)
}

/// Whether a screen-space triangle `(a, b, c)` should be culled under `mode`.
///
/// ⚠️ The comparisons look backwards and are not: see [`signed_area`]. In the
/// y-DOWN space the only caller works in (`renderer.rs`, on vertices straight
/// out of `clip_to_screen`), a front face has a NEGATIVE signed area, so
/// `Back` — Godot's default, which keeps front faces — must cull the positive
/// ones (pat-s3rrv.122).
///
/// Before this, `Back` culled every FRONT face and let the far side of a mesh
/// draw instead. On a closed convex opaque mesh that is nearly invisible — the
/// silhouette is identical and only the shading and UV direction change — which
/// is why it survived: it shows up as a mirrored texture and a face lit by the
/// wrong normal. The wgpu path never had it (`FrontFace::Ccw` +
/// `Face::Back` in `wgpu_pipeline.rs`), so the two backends disagreed about
/// which side of a mesh you were looking at.
///
/// A degenerate (zero-area) triangle is culled by both modes, as before.
pub fn should_cull(a: Vector2, b: Vector2, c: Vector2, mode: CullMode) -> bool {
    let area = signed_area(a, b, c);
    match mode {
        CullMode::Disabled => false,
        CullMode::Back => area >= 0.0,
        CullMode::Front => area <= 0.0,
    }
}

/// Whether the material must be depth-sorted / blended as transparent.
pub fn is_transparent(transparency: Transparency) -> bool {
    matches!(transparency, Transparency::Alpha)
}

/// For `AlphaScissor`, whether a fragment with the given `alpha` is discarded.
pub fn alpha_scissor_discard(transparency: Transparency, alpha: f32, threshold: f32) -> bool {
    matches!(transparency, Transparency::AlphaScissor) && alpha < threshold
}

/// Whether a fragment writes to the depth buffer given its transparency.
pub fn writes_depth(mode: DepthDrawMode, transparent: bool) -> bool {
    match mode {
        DepthDrawMode::Always => true,
        DepthDrawMode::Never => false,
        DepthDrawMode::OpaqueOnly => !transparent,
    }
}

/// Sorts drawables back-to-front (farthest first) for correct alpha blending.
///
/// Each item is `(view_depth, payload)` where a larger depth is farther from
/// the camera.
pub fn sort_back_to_front<T>(items: &mut [(f32, T)]) {
    items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
}

#[cfg(test)]
mod tests {
    use super::*;

    // ⚠️ These are named for what they are TO THE RENDERER, not for the
    // handedness of the raw numbers. Screen Y grows downward, so the triangle
    // that looks counter-clockwise when read as ordinary y-up math is the one
    // facing AWAY from the camera. Naming them `ccw`/`cw` is what let the
    // inverted cull look correct for as long as it did (pat-s3rrv.122).

    // A FRONT-facing screen triangle: clockwise on screen, signed area < 0.
    fn screen_front() -> (Vector2, Vector2, Vector2) {
        (
            Vector2::new(0.0, 0.0),
            Vector2::new(0.0, 1.0),
            Vector2::new(1.0, 0.0),
        )
    }
    // A BACK-facing screen triangle: counter-clockwise on screen, area > 0.
    fn screen_back() -> (Vector2, Vector2, Vector2) {
        (
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.0, 1.0),
        )
    }

    #[test]
    fn winding_sign_is_correct() {
        // Unchanged arithmetic, restated in the space it is used in: the
        // BACK-facing triangle is the one with positive signed area.
        let (a, b, c) = screen_back();
        assert!(signed_area(a, b, c) > 0.0);
        let (a, b, c) = screen_front();
        assert!(signed_area(a, b, c) < 0.0);
    }

    #[test]
    fn cull_back_removes_back_faces_only() {
        let (a, b, c) = screen_front();
        assert!(
            !should_cull(a, b, c, CullMode::Back),
            "front face must be kept"
        );
        let (a, b, c) = screen_back();
        assert!(
            should_cull(a, b, c, CullMode::Back),
            "back face must be culled"
        );
    }

    #[test]
    fn cull_front_removes_front_faces_only() {
        let (a, b, c) = screen_front();
        assert!(should_cull(a, b, c, CullMode::Front));
        let (a, b, c) = screen_back();
        assert!(!should_cull(a, b, c, CullMode::Front));
    }

    #[test]
    fn cull_disabled_keeps_everything() {
        let (a, b, c) = screen_front();
        assert!(!should_cull(a, b, c, CullMode::Disabled));
        let (a, b, c) = screen_back();
        assert!(!should_cull(a, b, c, CullMode::Disabled));
    }

    /// A degenerate triangle has no facing, and both modes drop it. Pinned
    /// because the fix moved the boundary from `<=`/`>=` to `>=`/`<=` and a
    /// slip to strict `>`/`<` would start drawing zero-area slivers.
    #[test]
    fn cull_drops_degenerate_triangles_in_both_modes() {
        let a = Vector2::new(0.0, 0.0);
        let b = Vector2::new(1.0, 1.0);
        let c = Vector2::new(2.0, 2.0);
        assert_eq!(signed_area(a, b, c), 0.0);
        assert!(should_cull(a, b, c, CullMode::Back));
        assert!(should_cull(a, b, c, CullMode::Front));
        assert!(!should_cull(a, b, c, CullMode::Disabled));
    }

    #[test]
    fn alpha_scissor_discards_below_threshold() {
        assert!(alpha_scissor_discard(Transparency::AlphaScissor, 0.3, 0.5));
        assert!(!alpha_scissor_discard(Transparency::AlphaScissor, 0.8, 0.5));
        // Non-scissor modes never scissor-discard.
        assert!(!alpha_scissor_discard(Transparency::Alpha, 0.1, 0.5));
        assert!(!alpha_scissor_discard(Transparency::Disabled, 0.0, 0.5));
    }

    #[test]
    fn is_transparent_only_for_alpha() {
        assert!(is_transparent(Transparency::Alpha));
        assert!(!is_transparent(Transparency::Disabled));
        assert!(!is_transparent(Transparency::AlphaScissor));
    }

    #[test]
    fn depth_write_policy() {
        assert!(writes_depth(DepthDrawMode::Always, true));
        assert!(!writes_depth(DepthDrawMode::Never, false));
        assert!(!writes_depth(DepthDrawMode::OpaqueOnly, true)); // transparent skips
        assert!(writes_depth(DepthDrawMode::OpaqueOnly, false)); // opaque writes
    }

    #[test]
    fn back_to_front_sort_orders_farthest_first() {
        let mut items = vec![(1.0, 'a'), (5.0, 'b'), (3.0, 'c')];
        sort_back_to_front(&mut items);
        let order: Vec<char> = items.iter().map(|(_, p)| *p).collect();
        assert_eq!(order, vec!['b', 'c', 'a']);
    }
}
