//! 3D viewport environment preview for the editor.
//!
//! Renders a visual preview of a scene's [`Environment3D`] into a
//! [`FrameBuffer`], showing:
//!
//! - **Sky**: A vertical gradient based on the sky material colors
//!   (procedural, panoramic placeholder, or physical sky).
//! - **Fog**: A color overlay applied when fog is enabled.
//! - **Ambient light**: An indicator showing the ambient light color and
//!   energy level.
//!
//! This is the editor-side preview — it does not go through the full 3D
//! render pipeline. Instead it produces a quick visual approximation that
//! helps users see the environment settings while editing.

use gdcore::math::Color;
use gdrender2d::renderer::FrameBuffer;
use gdserver3d::environment::{BackgroundMode, Environment3D};
use gdserver3d::sky::{SkyMaterial, ProceduralSkyMaterial, PhysicalSkyMaterial};

/// Renders an environment preview into a framebuffer.
///
/// The preview shows the sky gradient, fog overlay, and ambient light
/// indicator based on the environment's settings. The result is suitable
/// for displaying in the editor's 3D viewport background.
pub fn render_environment_preview(env: &Environment3D, width: u32, height: u32) -> FrameBuffer {
    let mut fb = FrameBuffer::new(width, height, Color::BLACK);

    // Step 1: Render sky/background
    render_background(&mut fb, env);

    // Step 2: Apply fog overlay
    if env.fog_enabled {
        apply_fog_overlay(&mut fb, env);
    }

    // Step 3: Draw ambient light indicator (bottom-left corner)
    draw_ambient_indicator(&mut fb, env);

    fb
}

/// Computes the sky color at a given vertical position (0.0 = top, 1.0 = bottom).
///
/// For procedural sky materials, this interpolates between the sky/ground
/// colors using the material's curve parameters. For other material types,
/// reasonable approximations are used.
pub fn sky_color_at(env: &Environment3D, t: f32) -> Color {
    match env.background_mode {
        BackgroundMode::ClearColor => Color::new(0.3, 0.3, 0.3, 1.0),
        BackgroundMode::CustomColor => env.background_color,
        BackgroundMode::Sky => {
            if let Some(sky) = &env.sky {
                sky_material_color_at(&sky.material, t)
            } else {
                Color::new(0.3, 0.3, 0.3, 1.0)
            }
        }
        _ => env.background_color,
    }
}

/// Interpolates sky material colors at vertical position `t`.
fn sky_material_color_at(material: &SkyMaterial, t: f32) -> Color {
    match material {
        SkyMaterial::Procedural(mat) => procedural_sky_color(mat, t),
        SkyMaterial::Physical(mat) => physical_sky_color(mat, t),
        SkyMaterial::Panoramic(_) => {
            // For panoramic skies we can't render the actual texture in preview,
            // so show a placeholder gradient indicating a panoramic sky is set.
            let gray = lerp(0.5, 0.3, t);
            Color::new(gray, gray, gray * 1.1, 1.0)
        }
    }
}

/// Computes procedural sky color using Godot's gradient model.
///
/// The sky hemisphere (t < 0.5) interpolates from `sky_top_color` to
/// `sky_horizon_color` using `sky_curve`. The ground hemisphere (t >= 0.5)
/// interpolates from `ground_horizon_color` to `ground_bottom_color`
/// using `ground_curve`.
fn procedural_sky_color(mat: &ProceduralSkyMaterial, t: f32) -> Color {
    if t < 0.5 {
        // Sky hemisphere: top (t=0) to horizon (t=0.5)
        let sky_t = t * 2.0; // 0..1
        let curved_t = sky_t.powf(1.0 / mat.sky_curve.max(0.001));
        lerp_color(
            &mat.sky_top_color,
            &mat.sky_horizon_color,
            curved_t,
            mat.sky_energy_multiplier,
        )
    } else {
        // Ground hemisphere: horizon (t=0.5) to bottom (t=1.0)
        let ground_t = (t - 0.5) * 2.0; // 0..1
        let curved_t = ground_t.powf(1.0 / mat.ground_curve.max(0.001));
        lerp_color(
            &mat.ground_horizon_color,
            &mat.ground_bottom_color,
            curved_t,
            mat.ground_energy_multiplier,
        )
    }
}

/// Approximates physical sky colors using Rayleigh/Mie scattering.
///
/// This is a simplified approximation — the full physical sky requires
/// GPU-side atmospheric scattering. We produce a recognizable gradient
/// using the material's color parameters.
fn physical_sky_color(mat: &PhysicalSkyMaterial, t: f32) -> Color {
    if t < 0.5 {
        let sky_t = t * 2.0;
        // Blend Rayleigh color (zenith) toward a brighter horizon tint
        let horizon = Color::new(
            mat.rayleigh_color.r + mat.mie_color.r * 0.3,
            mat.rayleigh_color.g + mat.mie_color.g * 0.3,
            mat.rayleigh_color.b + mat.mie_color.b * 0.3,
            1.0,
        );
        lerp_color(&mat.rayleigh_color, &horizon, sky_t, mat.energy_multiplier)
    } else {
        let ground_t = (t - 0.5) * 2.0;
        let horizon = Color::new(
            mat.rayleigh_color.r * 0.5 + mat.mie_color.r * 0.3,
            mat.rayleigh_color.g * 0.5 + mat.mie_color.g * 0.3,
            mat.rayleigh_color.b * 0.5 + mat.mie_color.b * 0.3,
            1.0,
        );
        lerp_color(&horizon, &mat.ground_color, ground_t, mat.energy_multiplier)
    }
}

/// Renders the background (sky gradient or solid color) into the framebuffer.
fn render_background(fb: &mut FrameBuffer, env: &Environment3D) {
    let h = fb.height;
    let w = fb.width;
    for y in 0..h {
        let t = y as f32 / h.max(1) as f32;
        let color = sky_color_at(env, t);
        let energy = env.background_energy_multiplier;
        let final_color = Color::new(
            (color.r * energy).min(1.0),
            (color.g * energy).min(1.0),
            (color.b * energy).min(1.0),
            1.0,
        );
        for x in 0..w {
            fb.pixels[(y * w + x) as usize] = final_color;
        }
    }
}

/// Applies a fog color overlay to the framebuffer.
///
/// Blends the fog color into existing pixels based on fog density.
/// Higher density = more fog color blended in.
fn apply_fog_overlay(fb: &mut FrameBuffer, env: &Environment3D) {
    let fog_alpha = env.fog_density.clamp(0.0, 1.0);
    let fog = &env.fog_light_color;
    for pixel in fb.pixels.iter_mut() {
        pixel.r = lerp(pixel.r, fog.r, fog_alpha);
        pixel.g = lerp(pixel.g, fog.g, fog_alpha);
        pixel.b = lerp(pixel.b, fog.b, fog_alpha);
    }
}

/// Draws an ambient light color indicator in the bottom-left corner.
///
/// Renders a small colored rectangle showing the ambient light color
/// and energy level, helping the user see at a glance what ambient
/// lighting is configured.
fn draw_ambient_indicator(fb: &mut FrameBuffer, env: &Environment3D) {
    use gdserver3d::environment::AmbientSource;

    let color = match env.ambient_source {
        AmbientSource::Disabled => return,
        AmbientSource::Color => Color::new(
            (env.ambient_color.r * env.ambient_energy).min(1.0),
            (env.ambient_color.g * env.ambient_energy).min(1.0),
            (env.ambient_color.b * env.ambient_energy).min(1.0),
            0.8,
        ),
        AmbientSource::Background | AmbientSource::Sky => {
            // For background/sky ambient, show a derived indicator
            Color::new(
                0.4 * env.ambient_energy,
                0.4 * env.ambient_energy,
                0.5 * env.ambient_energy,
                0.6,
            )
        }
    };

    // Draw a 16x16 indicator square at (4, height-20)
    let indicator_size: u32 = 16;
    let margin: u32 = 4;
    let start_x = margin;
    let start_y = fb.height.saturating_sub(margin + indicator_size);

    for y in start_y..start_y.saturating_add(indicator_size).min(fb.height) {
        for x in start_x..start_x.saturating_add(indicator_size).min(fb.width) {
            let idx = (y * fb.width + x) as usize;
            if idx < fb.pixels.len() {
                // Alpha blend
                let bg = &fb.pixels[idx];
                let a = color.a;
                fb.pixels[idx] = Color::new(
                    bg.r * (1.0 - a) + color.r * a,
                    bg.g * (1.0 - a) + color.g * a,
                    bg.b * (1.0 - a) + color.b * a,
                    1.0,
                );
            }
        }
    }
}

/// Describes the visible elements in an environment preview.
///
/// Used by the editor UI to display labels and tooltips alongside
/// the rendered preview.
#[derive(Debug, Clone)]
pub struct EnvironmentPreviewInfo {
    /// Human-readable description of the background mode.
    pub background_description: String,
    /// Human-readable description of the sky material (if any).
    pub sky_description: Option<String>,
    /// Whether fog is active.
    pub fog_active: bool,
    /// Human-readable fog description.
    pub fog_description: Option<String>,
    /// Human-readable ambient light description.
    pub ambient_description: String,
    /// The tone mapper in use.
    pub tone_mapper_name: String,
}

impl EnvironmentPreviewInfo {
    /// Builds info from an environment resource.
    pub fn from_environment(env: &Environment3D) -> Self {
        use gdserver3d::environment::{AmbientSource, ToneMapper};

        let background_description = match env.background_mode {
            BackgroundMode::ClearColor => "Clear Color".to_string(),
            BackgroundMode::CustomColor => format!(
                "Custom Color ({:.2}, {:.2}, {:.2})",
                env.background_color.r, env.background_color.g, env.background_color.b
            ),
            BackgroundMode::Sky => "Sky".to_string(),
            BackgroundMode::Canvas => "Canvas".to_string(),
            BackgroundMode::Keep => "Keep".to_string(),
            BackgroundMode::CameraFeed => "Camera Feed".to_string(),
        };

        let sky_description = env.sky.as_ref().map(|sky| match &sky.material {
            SkyMaterial::Procedural(_) => "Procedural Sky".to_string(),
            SkyMaterial::Panoramic(mat) => {
                if mat.panorama_path.is_empty() {
                    "Panoramic Sky (no texture)".to_string()
                } else {
                    format!("Panoramic Sky ({})", mat.panorama_path)
                }
            }
            SkyMaterial::Physical(_) => "Physical Sky".to_string(),
        });

        let fog_description = if env.fog_enabled {
            Some(format!(
                "Density: {:.3}, Color: ({:.2}, {:.2}, {:.2})",
                env.fog_density, env.fog_light_color.r, env.fog_light_color.g, env.fog_light_color.b
            ))
        } else {
            None
        };

        let ambient_description = match env.ambient_source {
            AmbientSource::Disabled => "Disabled".to_string(),
            AmbientSource::Background => format!("From Background (energy: {:.2})", env.ambient_energy),
            AmbientSource::Color => format!(
                "Color ({:.2}, {:.2}, {:.2}), energy: {:.2}",
                env.ambient_color.r, env.ambient_color.g, env.ambient_color.b, env.ambient_energy
            ),
            AmbientSource::Sky => format!("From Sky (energy: {:.2})", env.ambient_energy),
        };

        let tone_mapper_name = match env.tone_mapper {
            ToneMapper::Linear => "Linear",
            ToneMapper::Reinhard => "Reinhard",
            ToneMapper::Filmic => "Filmic",
            ToneMapper::Aces => "ACES",
        }
        .to_string();

        Self {
            background_description,
            sky_description,
            fog_active: env.fog_enabled,
            fog_description,
            ambient_description,
            tone_mapper_name,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_color(a: &Color, b: &Color, t: f32, energy: f32) -> Color {
    Color::new(
        (lerp(a.r, b.r, t) * energy).min(1.0),
        (lerp(a.g, b.g, t) * energy).min(1.0),
        (lerp(a.b, b.b, t) * energy).min(1.0),
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdserver3d::environment::AmbientSource;
    use gdserver3d::sky::{Sky, SkyProcessMode};

    #[test]
    fn default_environment_renders() {
        let env = Environment3D::default();
        let fb = render_environment_preview(&env, 64, 64);
        assert_eq!(fb.width, 64);
        assert_eq!(fb.height, 64);
        assert_eq!(fb.pixels.len(), 64 * 64);
    }

    #[test]
    fn procedural_sky_gradient_top_bottom_differ() {
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky::default()),
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 32, 64);
        // Top row should differ from bottom row (sky vs ground colors)
        let top = fb.pixels[0];
        let bottom = fb.pixels[(63 * 32) as usize];
        let differs = (top.r - bottom.r).abs() > 0.01
            || (top.g - bottom.g).abs() > 0.01
            || (top.b - bottom.b).abs() > 0.01;
        assert!(differs, "sky top and bottom should have different colors");
    }

    #[test]
    fn custom_color_fills_uniformly() {
        let env = Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: Color::new(0.5, 0.2, 0.8, 1.0),
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 16, 16);
        // All pixels should be the custom color (no gradient)
        for pixel in &fb.pixels {
            assert!((pixel.r - 0.5).abs() < 1e-5);
            assert!((pixel.g - 0.2).abs() < 1e-5);
            assert!((pixel.b - 0.8).abs() < 1e-5);
        }
    }

    #[test]
    fn fog_overlay_tints_pixels() {
        let env = Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: Color::new(0.0, 0.0, 0.0, 1.0),
            fog_enabled: true,
            fog_light_color: Color::new(1.0, 1.0, 1.0, 1.0),
            fog_density: 0.5,
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 8, 8);
        // With black background and white fog at 0.5 density, pixels should be ~0.5 gray
        // (skip the ambient indicator area)
        let mid = fb.pixels[0];
        assert!((mid.r - 0.5).abs() < 0.01, "fog should tint to ~0.5, got {}", mid.r);
    }

    #[test]
    fn ambient_indicator_draws_for_color_source() {
        let env = Environment3D {
            ambient_source: AmbientSource::Color,
            ambient_color: Color::new(1.0, 0.0, 0.0, 1.0),
            ambient_energy: 1.0,
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 32, 32);
        // The indicator is at bottom-left (x=4..20, y=12..28)
        // Check that some pixels in that area are reddish
        let indicator_pixel = fb.pixels[(20 * 32 + 8) as usize];
        assert!(indicator_pixel.r > 0.3, "ambient indicator should show red");
    }

    #[test]
    fn ambient_disabled_no_indicator() {
        let env = Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: Color::new(0.5, 0.5, 0.5, 1.0),
            ambient_source: AmbientSource::Disabled,
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 32, 32);
        // With disabled ambient, indicator area should match background
        let indicator_pixel = fb.pixels[(20 * 32 + 8) as usize];
        assert!(
            (indicator_pixel.r - 0.5).abs() < 0.01,
            "disabled ambient should not draw indicator"
        );
    }

    #[test]
    fn sky_color_at_returns_gradient_values() {
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky::default()),
            ..Default::default()
        };
        let top = sky_color_at(&env, 0.0);
        let mid = sky_color_at(&env, 0.5);
        let bottom = sky_color_at(&env, 1.0);
        // Top should be near sky_top_color default
        assert!(top.r > 0.1, "sky top should have color");
        // Horizon should be near sky_horizon_color
        assert!(mid.r > 0.1, "horizon should have color");
        // Bottom should be near ground_bottom_color
        assert!(bottom.r > 0.01, "ground should have some color");
    }

    #[test]
    fn physical_sky_renders() {
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky {
                material: SkyMaterial::Physical(PhysicalSkyMaterial::default()),
                process_mode: SkyProcessMode::Automatic,
                radiance_size: 256,
            }),
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 32, 32);
        // Should produce non-black pixels
        let nonblack = fb.pixels.iter().filter(|c| c.r > 0.01 || c.g > 0.01 || c.b > 0.01).count();
        assert!(nonblack > 0, "physical sky should produce visible pixels");
    }

    #[test]
    fn panoramic_sky_renders_placeholder() {
        use gdserver3d::sky::PanoramicSkyMaterial;
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky {
                material: SkyMaterial::Panoramic(PanoramicSkyMaterial::default()),
                process_mode: SkyProcessMode::Automatic,
                radiance_size: 256,
            }),
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 32, 32);
        let nonblack = fb.pixels.iter().filter(|c| c.r > 0.01 || c.g > 0.01 || c.b > 0.01).count();
        assert!(nonblack > 0, "panoramic sky should produce visible placeholder");
    }

    #[test]
    fn zero_size_framebuffer() {
        let env = Environment3D::default();
        let fb = render_environment_preview(&env, 0, 0);
        assert_eq!(fb.pixels.len(), 0);
    }

    #[test]
    fn environment_preview_info_default() {
        let env = Environment3D::default();
        let info = EnvironmentPreviewInfo::from_environment(&env);
        assert_eq!(info.background_description, "Clear Color");
        assert!(info.sky_description.is_none());
        assert!(!info.fog_active);
        assert!(info.fog_description.is_none());
        assert_eq!(info.tone_mapper_name, "Linear");
    }

    #[test]
    fn environment_preview_info_with_sky_and_fog() {
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky::default()),
            fog_enabled: true,
            fog_density: 0.02,
            fog_light_color: Color::new(0.5, 0.5, 0.6, 1.0),
            ambient_source: AmbientSource::Sky,
            ambient_energy: 0.75,
            ..Default::default()
        };
        let info = EnvironmentPreviewInfo::from_environment(&env);
        assert_eq!(info.background_description, "Sky");
        assert_eq!(info.sky_description, Some("Procedural Sky".to_string()));
        assert!(info.fog_active);
        assert!(info.fog_description.is_some());
        assert!(info.ambient_description.contains("Sky"));
    }

    #[test]
    fn energy_multiplier_brightens_sky() {
        let env_normal = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky::default()),
            background_energy_multiplier: 1.0,
            ..Default::default()
        };
        let env_bright = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky::default()),
            background_energy_multiplier: 2.0,
            ..Default::default()
        };
        let fb_normal = render_environment_preview(&env_normal, 8, 8);
        let fb_bright = render_environment_preview(&env_bright, 8, 8);
        // Bright version should have higher or equal pixel values
        let sum_normal: f32 = fb_normal.pixels.iter().map(|c| c.r + c.g + c.b).sum();
        let sum_bright: f32 = fb_bright.pixels.iter().map(|c| c.r + c.g + c.b).sum();
        assert!(
            sum_bright >= sum_normal,
            "higher energy should produce brighter output"
        );
    }

    #[test]
    fn high_fog_density_dominates_color() {
        let env = Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: Color::new(0.0, 0.0, 0.0, 1.0),
            fog_enabled: true,
            fog_light_color: Color::new(0.8, 0.7, 0.6, 1.0),
            fog_density: 1.0,
            ..Default::default()
        };
        let fb = render_environment_preview(&env, 8, 8);
        // At density 1.0, pixels should be fully fog color
        let pixel = fb.pixels[0];
        assert!((pixel.r - 0.8).abs() < 0.01);
        assert!((pixel.g - 0.7).abs() < 0.01);
        assert!((pixel.b - 0.6).abs() < 0.01);
    }
}
