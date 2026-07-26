//! 2D canvas lights and occluders (pat-p0s82.6).
//!
//! Implements a bounded Godot-compatible slice of `PointLight2D`,
//! `DirectionalLight2D`, and `LightOccluder2D`:
//!
//! - Point lights fall off with distance (`energy`, `texture_scale` as range)
//! - Directional lights apply a constant directional contribution
//! - Occluders are closed or open polylines that block light rays (hard shadows)
//! - `light_mask` / `item_mask` bitmasks gate which lights affect which items
//!
//! Shading is applied as a post-pass over a [`FrameBuffer`]: each pixel's RGB
//! is multiplied by the accumulated light (plus ambient).

use gdcore::math::{Color, Vector2};

use crate::renderer::FrameBuffer;

/// Which light type drives the contribution model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light2DKind {
    /// Omnidirectional point light (Godot `PointLight2D`).
    Point,
    /// Infinite directional light (Godot `DirectionalLight2D`).
    Directional,
}

/// How a light's contribution combines with the canvas beneath it
/// (Godot `Light2D.blend_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Light2DBlendMode {
    /// Composite the light over the canvas (soft brighten). Godot's default.
    #[default]
    Mix,
    /// Add the light contribution to the canvas (brighten).
    Add,
    /// Subtract the light contribution from the canvas (darken).
    Sub,
}

/// Combines a `contribution` (a light's colored intensity at a pixel) with the
/// `base` canvas color under the given [`Light2DBlendMode`], per RGB channel and
/// clamped to `[0, 1]`. Alpha is taken from `base` (lights affect RGB only).
pub fn composite_light(base: Color, contribution: Color, mode: Light2DBlendMode) -> Color {
    let ch = |b: f32, c: f32| -> f32 {
        match mode {
            Light2DBlendMode::Add => b + c,
            Light2DBlendMode::Sub => b - c,
            // Composite the light over the base (screen-style soft brighten).
            Light2DBlendMode::Mix => b + c * (1.0 - b),
        }
        .clamp(0.0, 1.0)
    };
    Color::new(
        ch(base.r, contribution.r),
        ch(base.g, contribution.g),
        ch(base.b, contribution.b),
        base.a,
    )
}

/// A 2D light affecting canvas items.
#[derive(Debug, Clone)]
pub struct Light2D {
    /// Point vs directional.
    pub kind: Light2DKind,
    /// World-space position (point lights) / origin (directional).
    pub position: Vector2,
    /// Direction the light faces (directional: unit vector toward the lit side;
    /// point: unused for falloff, used for optional spot cone later).
    pub direction: Vector2,
    /// Light color (multiplies albedo).
    pub color: Color,
    /// Energy multiplier (Godot default 1.0).
    pub energy: f32,
    /// Effective range for point lights (`texture_scale` proxy). 0 = unlimited.
    pub range: f32,
    /// Bitmask; item is lit only when `(light_mask & item_mask) != 0`.
    pub light_mask: u32,
    /// Whether this light is enabled.
    pub enabled: bool,
    /// Whether this light casts shadows against occluders.
    pub shadow_enabled: bool,
    /// How this light combines with the canvas (`Light2D.blend_mode`).
    pub blend_mode: Light2DBlendMode,
    /// Which canvas items this light reaches (`Light2D.range_item_cull_mask`).
    ///
    /// Distinct from [`light_mask`](Self::light_mask), which is the layer the
    /// light itself sits on: Godot tests an *item's* `light_mask` against this
    /// (`renderer_canvas_render.h`), so a light and a receiver can be on
    /// different layers and still interact (pat-0q2kx.130).
    pub range_item_cull_mask: u32,
    /// Which occluders cast shadows for this light
    /// (`Light2D.shadow_item_cull_mask`) — separate from
    /// [`range_item_cull_mask`](Self::range_item_cull_mask), so geometry can lit
    /// without shadowing and vice versa.
    pub shadow_item_cull_mask: u32,
    /// The colour a shadowed pixel is tinted toward (`Light2D.shadow_color`).
    ///
    /// Its **alpha is the shadow's opacity**: Godot's default is opaque black
    /// (the light is fully blocked), while `a < 1` lets some of the light
    /// through, which is what makes soft coloured shadows possible.
    pub shadow_color: Color,
    /// Shadow edge filtering (`Light2D.shadow_filter`).
    pub shadow_filter: ShadowFilter,
    /// Filter kernel spread in pixels (`Light2D.shadow_filter_smooth`).
    pub shadow_filter_smooth: f32,
    /// Godot `Light2D.editor_only` — the light is a authoring aid and must not
    /// contribute to a running game's frame.
    pub editor_only: bool,
    /// Godot `PointLight2D.offset` — shifts the light texture's centre.
    pub offset: Vector2,
    /// Godot `PointLight2D.texture_scale` — scales the light footprint.
    pub texture_scale: f32,
    /// Godot `PointLight2D.texture` path, when one is authored. The footprint
    /// falls back to the built-in radial gradient when absent.
    pub texture_path: Option<String>,
}

/// Shadow edge filtering (Godot `Light2D.ShadowFilter`).
///
/// The variants match Godot's enum order so an authored integer maps directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowFilter {
    /// Hard shadow edges — one ray per pixel.
    #[default]
    None,
    /// 5-tap percentage-closer filter.
    Pcf5,
    /// 13-tap percentage-closer filter.
    Pcf13,
}

impl ShadowFilter {
    /// Maps Godot's `shadow_filter` integer (0 = NONE, 1 = PCF5, 2 = PCF13).
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            1 => ShadowFilter::Pcf5,
            2 => ShadowFilter::Pcf13,
            _ => ShadowFilter::None,
        }
    }

    /// How many shadow rays this filter samples.
    pub fn taps(self) -> u32 {
        match self {
            ShadowFilter::None => 1,
            ShadowFilter::Pcf5 => 5,
            ShadowFilter::Pcf13 => 13,
        }
    }
}

/// The Godot defaults every light carries beyond the constructor arguments,
/// factored out so [`Light2D::point`] and [`Light2D::directional`] cannot drift
/// apart as fields are added (pat-0q2kx.130).
fn light_feature_defaults() -> (u32, u32, Color, ShadowFilter, f32, bool, Vector2, f32) {
    (
        // Godot's `range_item_cull_mask` / `shadow_item_cull_mask` default to
        // layer 1, the same layer a fresh CanvasItem's `light_mask` is on.
        1,
        1,
        // Opaque black: a shadowed pixel receives none of this light.
        Color::new(0.0, 0.0, 0.0, 1.0),
        ShadowFilter::None,
        0.0,
        false,
        Vector2::new(0.0, 0.0),
        1.0,
    )
}

impl Light2D {
    /// Create a point light at `position`.
    pub fn point(position: Vector2, color: Color, energy: f32, range: f32) -> Self {
        let (
            range_item_cull_mask,
            shadow_item_cull_mask,
            shadow_color,
            shadow_filter,
            shadow_filter_smooth,
            editor_only,
            offset,
            texture_scale,
        ) = light_feature_defaults();
        Self {
            kind: Light2DKind::Point,
            position,
            direction: Vector2::new(0.0, -1.0),
            color,
            energy,
            range,
            light_mask: 1,
            enabled: true,
            shadow_enabled: true,
            blend_mode: Light2DBlendMode::Mix,
            range_item_cull_mask,
            shadow_item_cull_mask,
            shadow_color,
            shadow_filter,
            shadow_filter_smooth,
            editor_only,
            offset,
            texture_scale,
            texture_path: None,
        }
    }

    /// Create a directional light shining along `direction` (will be normalized).
    pub fn directional(direction: Vector2, color: Color, energy: f32) -> Self {
        let len = (direction.x * direction.x + direction.y * direction.y).sqrt();
        let dir = if len > 1e-6 {
            Vector2::new(direction.x / len, direction.y / len)
        } else {
            Vector2::new(0.0, -1.0)
        };
        let (
            range_item_cull_mask,
            shadow_item_cull_mask,
            shadow_color,
            shadow_filter,
            shadow_filter_smooth,
            editor_only,
            offset,
            texture_scale,
        ) = light_feature_defaults();
        Self {
            kind: Light2DKind::Directional,
            position: Vector2::new(0.0, 0.0),
            direction: dir,
            color,
            energy,
            range: 0.0,
            light_mask: 1,
            enabled: true,
            shadow_enabled: true,
            blend_mode: Light2DBlendMode::Mix,
            range_item_cull_mask,
            shadow_item_cull_mask,
            shadow_color,
            shadow_filter,
            shadow_filter_smooth,
            editor_only,
            offset,
            texture_scale,
            texture_path: None,
        }
    }
}

/// A 2D light occluder polyline (Godot `LightOccluder2D` polygon slice).
#[derive(Debug, Clone)]
pub struct LightOccluder2D {
    /// World-space vertices of the occluder polygon / polyline.
    pub points: Vec<Vector2>,
    /// When true, last vertex connects to first (closed polygon).
    pub closed: bool,
    /// Items with matching mask bits are occluded by this shape.
    pub light_mask: u32,
}

impl LightOccluder2D {
    /// Create an occluder from points.
    pub fn new(points: Vec<Vector2>, closed: bool) -> Self {
        Self {
            points,
            closed,
            light_mask: 1,
        }
    }
}

/// Ambient light added before per-light contributions (Godot canvas modulate ambient).
#[derive(Debug, Clone, Copy)]
pub struct CanvasAmbient {
    /// Ambient color * energy as RGB factors.
    pub color: Color,
}

impl Default for CanvasAmbient {
    fn default() -> Self {
        Self {
            // Godot-like dim ambient so unlit areas are not pure black.
            color: Color::new(0.15, 0.15, 0.15, 1.0),
        }
    }
}

/// Compute RGB light factor at `world` for an item with `item_mask`.
///
/// Returns a linear RGB multiplier (can exceed 1.0 for overbright lights).
pub fn light_factor_at(
    world: Vector2,
    item_mask: u32,
    lights: &[Light2D],
    occluders: &[LightOccluder2D],
    ambient: CanvasAmbient,
) -> Color {
    let mut r = ambient.color.r;
    let mut g = ambient.color.g;
    let mut b = ambient.color.b;

    for light in lights {
        if !light.enabled {
            continue;
        }
        if (light.light_mask & item_mask) == 0 {
            continue;
        }

        let mut intensity = light.energy;
        match light.kind {
            Light2DKind::Point => {
                let dx = world.x - light.position.x;
                let dy = world.y - light.position.y;
                let dist = (dx * dx + dy * dy).sqrt();
                if light.range > 0.0 {
                    if dist >= light.range {
                        continue;
                    }
                    // Linear falloff to zero at range (simple Godot-like).
                    intensity *= 1.0 - dist / light.range;
                } else {
                    // Soft inverse falloff
                    intensity *= 1.0 / (1.0 + dist * 0.05);
                }
            }
            Light2DKind::Directional => {
                // Constant intensity for directional (no distance).
            }
        }

        if light.shadow_enabled && intensity > 0.0 {
            let origin = match light.kind {
                Light2DKind::Point => light.position,
                // For directional, cast from far along -direction.
                Light2DKind::Directional => Vector2::new(
                    world.x - light.direction.x * 10_000.0,
                    world.y - light.direction.y * 10_000.0,
                ),
            };
            if is_occluded(origin, world, occluders, light.light_mask) {
                continue;
            }
        }

        r += light.color.r * intensity;
        g += light.color.g * intensity;
        b += light.color.b * intensity;
    }

    Color::new(r, g, b, 1.0)
}

/// The fraction of `light`'s rays that reach `world` — 1.0 fully lit, 0.0 fully
/// shadowed, and a fraction in between for a filtered (PCF) edge.
///
/// `shadow_filter` decides how many rays are cast: a single ray gives Godot's
/// hard `SHADOW_FILTER_NONE` edge, while PCF5/PCF13 spread the sample origins
/// over `shadow_filter_smooth` pixels perpendicular to the light direction and
/// average the results, which is what softens the penumbra.
pub fn shadow_visibility(world: Vector2, light: &Light2D, occluders: &[LightOccluder2D]) -> f32 {
    if !light.shadow_enabled {
        return 1.0;
    }
    let origin = match light.kind {
        Light2DKind::Point => light.position,
        // For directional, cast from far along -direction.
        Light2DKind::Directional => Vector2::new(
            world.x - light.direction.x * 10_000.0,
            world.y - light.direction.y * 10_000.0,
        ),
    };

    let taps = light.shadow_filter.taps();
    if taps <= 1 || light.shadow_filter_smooth <= 0.0 {
        return if is_occluded(origin, world, occluders, light.shadow_item_cull_mask) {
            0.0
        } else {
            1.0
        };
    }

    // Offset the sample origins perpendicular to the light ray so the taps
    // straddle the shadow edge rather than repeating the same test.
    let dx = world.x - origin.x;
    let dy = world.y - origin.y;
    let len = (dx * dx + dy * dy).sqrt();
    let (px, py) = if len > 1e-6 {
        (-dy / len, dx / len)
    } else {
        (1.0, 0.0)
    };

    let half = (taps / 2) as i32;
    let step = light.shadow_filter_smooth / taps as f32;
    let mut visible = 0u32;
    for tap in -half..=half {
        let o = tap as f32 * step;
        let sample = Vector2::new(origin.x + px * o, origin.y + py * o);
        if !is_occluded(sample, world, occluders, light.shadow_item_cull_mask) {
            visible += 1;
        }
    }
    visible as f32 / (half * 2 + 1) as f32
}

/// The colour `light` contributes at `world`, already attenuated by distance and
/// by any shadow, or `None` when the light does not apply to this item at all.
///
/// A shadowed pixel is **not** simply unlit: Godot tints it toward
/// `shadow_color`, whose alpha is the shadow's opacity. The attenuation is
/// multiplicative — `(1 - a) + a * shadow_color_channel` — so a shadow can never
/// brighten a pixel, and Godot's default opaque black yields exactly the
/// fully-unlit result the hard-shadow path produced before (pat-0q2kx.130).
pub fn light_contribution_at(
    world: Vector2,
    item_mask: u32,
    light: &Light2D,
    occluders: &[LightOccluder2D],
) -> Option<Color> {
    if !light.enabled || light.editor_only {
        return None;
    }
    // Godot tests the *item's* light_mask against the light's item cull mask.
    if (light.range_item_cull_mask & item_mask) == 0 {
        return None;
    }

    let mut intensity = light.energy;
    if let Light2DKind::Point = light.kind {
        // `offset` shifts the light footprint's centre; `texture_scale` scales
        // its reach, exactly as it scales the light texture in Godot.
        let centre = Vector2::new(
            light.position.x + light.offset.x,
            light.position.y + light.offset.y,
        );
        let dx = world.x - centre.x;
        let dy = world.y - centre.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let range = light.range * light.texture_scale.max(0.0);
        if range > 0.0 {
            if dist >= range {
                return None;
            }
            intensity *= 1.0 - dist / range;
        } else {
            intensity *= 1.0 / (1.0 + dist * 0.05);
        }
    }
    if intensity <= 0.0 {
        return None;
    }

    let visibility = shadow_visibility(world, light, occluders);
    let a = light.shadow_color.a.clamp(0.0, 1.0);
    let shadowed = |channel: f32, shadow_channel: f32| -> f32 {
        let lit = channel * intensity;
        let in_shadow = lit * ((1.0 - a) + a * shadow_channel);
        in_shadow + (lit - in_shadow) * visibility
    };
    Some(Color::new(
        shadowed(light.color.r, light.shadow_color.r),
        shadowed(light.color.g, light.shadow_color.g),
        shadowed(light.color.b, light.shadow_color.b),
        1.0,
    ))
}

/// Shades one pixel: ambient, then every light composited by its own
/// `blend_mode`.
///
/// This is the routing the audit called for — `composite_light` existed with
/// only test callers, so every light additively brightened regardless of its
/// mode and a `SUB` light could not darken anything (pat-0q2kx.130).
pub fn shade_pixel(
    base: Color,
    world: Vector2,
    item_mask: u32,
    lights: &[Light2D],
    occluders: &[LightOccluder2D],
    ambient: CanvasAmbient,
) -> Color {
    // Ambient is a multiplier on the albedo, as before.
    let mut out = Color::new(
        base.r * ambient.color.r,
        base.g * ambient.color.g,
        base.b * ambient.color.b,
        base.a,
    );
    for light in lights {
        let Some(contribution) = light_contribution_at(world, item_mask, light, occluders) else {
            continue;
        };
        // The light modulates the albedo it falls on, then composites by mode.
        let scaled = Color::new(
            contribution.r * base.r,
            contribution.g * base.g,
            contribution.b * base.b,
            1.0,
        );
        out = composite_light(out, scaled, light.blend_mode);
    }
    out
}

/// True if any occluder segment blocks the segment from `from` to `to`.
pub fn is_occluded(
    from: Vector2,
    to: Vector2,
    occluders: &[LightOccluder2D],
    light_mask: u32,
) -> bool {
    for occ in occluders {
        if (occ.light_mask & light_mask) == 0 {
            continue;
        }
        if occ.points.len() < 2 {
            continue;
        }
        let n = occ.points.len();
        let edges = if occ.closed { n } else { n - 1 };
        for i in 0..edges {
            let a = occ.points[i];
            let b = occ.points[(i + 1) % n];
            if segments_intersect(from, to, a, b) {
                // Ignore intersections very close to the sample point (self).
                let mid = Vector2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
                let ddx = mid.x - to.x;
                let ddy = mid.y - to.y;
                if (ddx * ddx + ddy * ddy).sqrt() < 0.5 {
                    continue;
                }
                return true;
            }
        }
    }
    false
}

/// Segment intersection (excluding shared endpoints / collinear overlap edge cases).
fn segments_intersect(p1: Vector2, p2: Vector2, q1: Vector2, q2: Vector2) -> bool {
    fn cross(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
        ax * by - ay * bx
    }
    let rx = p2.x - p1.x;
    let ry = p2.y - p1.y;
    let sx = q2.x - q1.x;
    let sy = q2.y - q1.y;
    let denom = cross(rx, ry, sx, sy);
    if denom.abs() < 1e-8 {
        return false;
    }
    let qpx = q1.x - p1.x;
    let qpy = q1.y - p1.y;
    let t = cross(qpx, qpy, sx, sy) / denom;
    let u = cross(qpx, qpy, rx, ry) / denom;
    // Strict interior of both segments to avoid endpoint noise.
    t > 0.001 && t < 0.999 && u > 0.001 && u < 0.999
}

/// Apply light shading in-place to a framebuffer.
///
/// Pixel `(x,y)` is treated as world position `(x + 0.5, y + 0.5)` unless
/// `world_from_pixel` remaps it. RGB is multiplied by the light factor.
pub fn apply_lights_to_framebuffer(
    fb: &mut FrameBuffer,
    lights: &[Light2D],
    occluders: &[LightOccluder2D],
    ambient: CanvasAmbient,
    item_mask: u32,
) {
    for y in 0..fb.height {
        for x in 0..fb.width {
            let world = Vector2::new(x as f32 + 0.5, y as f32 + 0.5);
            let idx = (y * fb.width + x) as usize;
            fb.pixels[idx] =
                shade_pixel(fb.pixels[idx], world, item_mask, lights, occluders, ambient);
        }
    }
}

/// Shades `fb` where each pixel's receiving item may have a different
/// `light_mask`, read from `item_masks` (one entry per pixel, row-major).
///
/// Godot culls lights **per canvas item**, not per frame: a light whose
/// `range_item_cull_mask` misses an item leaves that item untouched while still
/// lighting its neighbours. A whole-frame mask cannot express that, which is why
/// the Play path builds a per-pixel mask buffer (pat-0q2kx.130). A pixel whose
/// mask is 0 belongs to no item (background) and is left alone.
pub fn apply_lights_with_item_masks(
    fb: &mut FrameBuffer,
    item_masks: &[u32],
    lights: &[Light2D],
    occluders: &[LightOccluder2D],
    ambient: CanvasAmbient,
) {
    for y in 0..fb.height {
        for x in 0..fb.width {
            let idx = (y * fb.width + x) as usize;
            let mask = item_masks.get(idx).copied().unwrap_or(0);
            if mask == 0 {
                continue;
            }
            let world = Vector2::new(x as f32 + 0.5, y as f32 + 0.5);
            fb.pixels[idx] = shade_pixel(fb.pixels[idx], world, mask, lights, occluders, ambient);
        }
    }
}

/// Register common ClassDB light class names if ClassDB is available via gdobject.
/// Kept here as documentation of the node surface; registration lives in gdobject.
pub const LIGHT2D_CLASS_NAMES: &[&str] = &[
    "Light2D",
    "PointLight2D",
    "DirectionalLight2D",
    "LightOccluder2D",
];

#[cfg(test)]
mod tests {
    use super::*;

    // pat-nzgdu.72: Light2D blend-mode compositing (add/sub/mix) + default.
    #[test]
    fn light2d_blend_modes_composite() {
        let base = Color::new(0.5, 0.5, 0.5, 1.0);
        let light = Color::new(0.4, 0.4, 0.4, 1.0);

        let add = composite_light(base, light, Light2DBlendMode::Add);
        assert!((add.r - 0.9).abs() < 1e-5, "ADD brightens, got {}", add.r);

        let sub = composite_light(base, light, Light2DBlendMode::Sub);
        assert!((sub.r - 0.1).abs() < 1e-5, "SUB darkens, got {}", sub.r);

        let mix = composite_light(base, light, Light2DBlendMode::Mix);
        // Mix = 0.5 + 0.4*(1-0.5) = 0.7 — brighter than base, softer than Add.
        assert!(
            (mix.r - 0.7).abs() < 1e-5,
            "MIX soft-brightens, got {}",
            mix.r
        );
        assert!(mix.r < add.r, "MIX must be softer than ADD");
        assert!(sub.r < base.r, "SUB must darken below the base");

        // Clamp: Add saturates at 1.0, Sub floors at 0.0.
        let hot = composite_light(
            Color::new(0.9, 0.9, 0.9, 1.0),
            Color::new(0.5, 0.5, 0.5, 1.0),
            Light2DBlendMode::Add,
        );
        assert_eq!(hot.r, 1.0);
        let cold = composite_light(
            Color::new(0.2, 0.2, 0.2, 1.0),
            Color::new(0.5, 0.5, 0.5, 1.0),
            Light2DBlendMode::Sub,
        );
        assert_eq!(cold.r, 0.0);

        // Alpha comes from the base (lights affect RGB only).
        assert_eq!(add.a, 1.0);

        // A freshly-constructed light defaults to Mix (Godot's default).
        let l = Light2D::point(Vector2::new(0.0, 0.0), Color::WHITE, 1.0, 100.0);
        assert_eq!(l.blend_mode, Light2DBlendMode::Mix);
        assert_eq!(Light2DBlendMode::default(), Light2DBlendMode::Mix);
    }

    #[test]
    fn point_light_brighter_near_source() {
        let light = Light2D::point(Vector2::new(0.0, 0.0), Color::WHITE, 1.0, 100.0);
        let near = light_factor_at(
            Vector2::new(5.0, 0.0),
            1,
            &[light.clone()],
            &[],
            CanvasAmbient {
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            },
        );
        let far = light_factor_at(
            Vector2::new(80.0, 0.0),
            1,
            &[light],
            &[],
            CanvasAmbient {
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            },
        );
        assert!(
            near.r > far.r,
            "near ({}) should be brighter than far ({})",
            near.r,
            far.r
        );
    }

    #[test]
    fn directional_light_constant() {
        let light = Light2D::directional(Vector2::new(0.0, 1.0), Color::WHITE, 0.8);
        let a = light_factor_at(
            Vector2::new(0.0, 0.0),
            1,
            &[light.clone()],
            &[],
            CanvasAmbient {
                color: Color::BLACK,
            },
        );
        let b = light_factor_at(
            Vector2::new(500.0, 500.0),
            1,
            &[light],
            &[],
            CanvasAmbient {
                color: Color::BLACK,
            },
        );
        assert!((a.r - b.r).abs() < 1e-4);
        assert!((a.r - 0.8).abs() < 1e-4);
    }

    #[test]
    fn occluder_blocks_point_light() {
        let light = Light2D::point(Vector2::new(0.0, 0.0), Color::WHITE, 1.0, 200.0);
        // Vertical wall at x=10 from y=-10 to y=10
        let occ = LightOccluder2D::new(
            vec![Vector2::new(10.0, -10.0), Vector2::new(10.0, 10.0)],
            false,
        );
        let ambient = CanvasAmbient {
            color: Color::BLACK,
        };
        let lit = light_factor_at(
            Vector2::new(5.0, 0.0),
            1,
            &[light.clone()],
            &[occ.clone()],
            ambient,
        );
        let shadow = light_factor_at(Vector2::new(20.0, 0.0), 1, &[light], &[occ], ambient);
        assert!(lit.r > 0.1, "front of wall should be lit: {}", lit.r);
        assert!(
            shadow.r < 0.01,
            "behind wall should be shadowed: {}",
            shadow.r
        );
    }

    #[test]
    fn light_mask_filters() {
        let mut light = Light2D::point(Vector2::new(0.0, 0.0), Color::WHITE, 1.0, 50.0);
        light.light_mask = 0b10; // layer 2 only
        let ambient = CanvasAmbient {
            color: Color::BLACK,
        };
        let on = light_factor_at(Vector2::new(1.0, 0.0), 0b10, &[light.clone()], &[], ambient);
        let off = light_factor_at(Vector2::new(1.0, 0.0), 0b01, &[light], &[], ambient);
        assert!(on.r > 0.5);
        assert!(off.r < 0.01);
    }

    #[test]
    fn framebuffer_shading_darkens_unlit_region() {
        let mut fb = FrameBuffer::new(32, 32, Color::WHITE);
        let light = Light2D::point(Vector2::new(4.0, 4.0), Color::WHITE, 2.0, 12.0);
        apply_lights_to_framebuffer(
            &mut fb,
            &[light],
            &[],
            CanvasAmbient {
                color: Color::new(0.05, 0.05, 0.05, 1.0),
            },
            1,
        );
        let near = fb.get_pixel(4, 4);
        let far = fb.get_pixel(30, 30);
        assert!(
            near.r > far.r,
            "near pixel {} should exceed far {}",
            near.r,
            far.r
        );
    }

    #[test]
    fn class_names_documented() {
        assert!(LIGHT2D_CLASS_NAMES.contains(&"PointLight2D"));
        assert!(LIGHT2D_CLASS_NAMES.contains(&"DirectionalLight2D"));
        assert!(LIGHT2D_CLASS_NAMES.contains(&"LightOccluder2D"));
    }
}
