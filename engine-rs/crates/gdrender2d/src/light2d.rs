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
}

impl Light2D {
    /// Create a point light at `position`.
    pub fn point(position: Vector2, color: Color, energy: f32, range: f32) -> Self {
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
            let factor = light_factor_at(world, item_mask, lights, occluders, ambient);
            let idx = (y * fb.width + x) as usize;
            let p = fb.pixels[idx];
            fb.pixels[idx] = Color::new(
                (p.r * factor.r).min(1.0),
                (p.g * factor.g).min(1.0),
                (p.b * factor.b).min(1.0),
                p.a,
            );
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
