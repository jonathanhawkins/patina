//! Integration tests for 3D viewport environment preview rendering.
//!
//! Verifies that the editor can render environment previews for various
//! configurations of sky, fog, and ambient lighting.

use gdcore::math::Color;
use gdeditor::environment_preview::{
    render_environment_preview, sky_color_at, EnvironmentPreviewInfo,
};
use gdserver3d::environment::{AmbientSource, BackgroundMode, Environment3D, ToneMapper};
use gdserver3d::sky::{
    PanoramicSkyMaterial, PhysicalSkyMaterial, ProceduralSkyMaterial, Sky, SkyMaterial,
    SkyProcessMode,
};

// ---------------------------------------------------------------------------
// Sky rendering
// ---------------------------------------------------------------------------

#[test]
fn procedural_sky_gradient_is_smooth() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 16, 128);

    // Verify gradient smoothness: adjacent rows should not jump more than
    // a reasonable amount in any channel.
    for y in 1..128u32 {
        let prev = fb.pixels[((y - 1) * 16) as usize];
        let curr = fb.pixels[(y * 16) as usize];
        let max_jump = (prev.r - curr.r)
            .abs()
            .max((prev.g - curr.g).abs())
            .max((prev.b - curr.b).abs());
        assert!(
            max_jump < 0.15,
            "row {} to {} has excessive jump {:.4}",
            y - 1,
            y,
            max_jump
        );
    }
}

#[test]
fn procedural_sky_custom_colors_reflected() {
    let custom_mat = ProceduralSkyMaterial {
        sky_top_color: Color::new(0.0, 0.0, 1.0, 1.0),
        sky_horizon_color: Color::new(1.0, 1.0, 1.0, 1.0),
        ground_bottom_color: Color::new(0.1, 0.05, 0.0, 1.0),
        ground_horizon_color: Color::new(0.8, 0.7, 0.5, 1.0),
        ..Default::default()
    };
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Procedural(custom_mat),
            ..Default::default()
        }),
        ..Default::default()
    };

    // Top should be blue-ish
    let top = sky_color_at(&env, 0.0);
    assert!(top.b > top.r, "sky top should be blue, got {:?}", top);

    // Bottom should have low blue and some red/green
    let bottom = sky_color_at(&env, 1.0);
    assert!(
        bottom.r > bottom.b,
        "ground should be warm-toned, got {:?}",
        bottom
    );
}

#[test]
fn physical_sky_has_visible_gradient() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Physical(PhysicalSkyMaterial::default()),
            process_mode: SkyProcessMode::Automatic,
            radiance_size: 256,
        }),
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 16, 64);

    let top = fb.pixels[0];
    let bottom = fb.pixels[(63 * 16) as usize];
    let differs = (top.r - bottom.r).abs() > 0.01
        || (top.g - bottom.g).abs() > 0.01
        || (top.b - bottom.b).abs() > 0.01;
    assert!(
        differs,
        "physical sky should have gradient, top={:?} bottom={:?}",
        top, bottom
    );
}

#[test]
fn panoramic_sky_placeholder_renders() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
                panorama_path: "res://my_hdr.exr".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 16, 16);
    let nonblack = fb
        .pixels
        .iter()
        .filter(|c| c.r > 0.01 || c.g > 0.01 || c.b > 0.01)
        .count();
    assert!(
        nonblack > 0,
        "panoramic placeholder should produce visible pixels"
    );
}

// ---------------------------------------------------------------------------
// Background modes
// ---------------------------------------------------------------------------

#[test]
fn clear_color_mode_produces_gray() {
    let env = Environment3D {
        background_mode: BackgroundMode::ClearColor,
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 8, 8);
    let pixel = fb.pixels[0];
    assert!(pixel.r > 0.1, "clear color should show a visible gray");
}

#[test]
fn keep_mode_uses_background_color() {
    let env = Environment3D {
        background_mode: BackgroundMode::Keep,
        background_color: Color::new(0.2, 0.4, 0.6, 1.0),
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 8, 8);
    let pixel = fb.pixels[0];
    assert!((pixel.r - 0.2).abs() < 1e-5);
    assert!((pixel.g - 0.4).abs() < 1e-5);
    assert!((pixel.b - 0.6).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// Fog
// ---------------------------------------------------------------------------

#[test]
fn fog_zero_density_has_no_effect() {
    let env_no_fog = Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: Color::new(0.5, 0.5, 0.5, 1.0),
        ..Default::default()
    };
    let env_fog_zero = Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: Color::new(0.5, 0.5, 0.5, 1.0),
        fog_enabled: true,
        fog_density: 0.0,
        fog_light_color: Color::new(1.0, 0.0, 0.0, 1.0),
        ..Default::default()
    };
    let fb1 = render_environment_preview(&env_no_fog, 8, 8);
    let fb2 = render_environment_preview(&env_fog_zero, 8, 8);
    // With zero density, fog should have no visible effect on center pixels
    assert!(
        (fb1.pixels[0].r - fb2.pixels[0].r).abs() < 1e-5,
        "zero fog density should not change colors"
    );
}

#[test]
fn fog_density_range_clamped() {
    let env_over = Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: Color::new(0.0, 0.0, 0.0, 1.0),
        fog_enabled: true,
        fog_density: 5.0, // > 1.0, should clamp
        fog_light_color: Color::new(0.6, 0.6, 0.6, 1.0),
        ..Default::default()
    };
    let fb = render_environment_preview(&env_over, 8, 8);
    // Should be fully fog color (clamped to 1.0)
    let pixel = fb.pixels[0];
    assert!(
        (pixel.r - 0.6).abs() < 0.01,
        "density > 1.0 should clamp to full fog"
    );
}

// ---------------------------------------------------------------------------
// Ambient light indicator
// ---------------------------------------------------------------------------

#[test]
fn ambient_color_indicator_visible() {
    let env = Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: Color::new(0.0, 0.0, 0.0, 1.0),
        ambient_source: AmbientSource::Color,
        ambient_color: Color::new(0.0, 1.0, 0.0, 1.0),
        ambient_energy: 1.0,
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 64, 64);
    // The indicator is at bottom-left (x=4..20, y=44..60)
    // Check that some pixels in the indicator area have green
    let mut has_green = false;
    for y in 44..60u32 {
        for x in 4..20u32 {
            let idx = (y * 64 + x) as usize;
            if fb.pixels[idx].g > 0.3 {
                has_green = true;
                break;
            }
        }
    }
    assert!(has_green, "ambient indicator should show green");
}

#[test]
fn ambient_sky_source_shows_indicator() {
    let env = Environment3D {
        ambient_source: AmbientSource::Sky,
        ambient_energy: 1.0,
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 64, 64);
    // Sky-derived ambient should still draw an indicator
    let indicator_pixel = fb.pixels[(52 * 64 + 10) as usize];
    // The indicator for sky source uses a blue-ish tint
    assert!(
        indicator_pixel.b > 0.01 || indicator_pixel.r > 0.01,
        "sky ambient should draw indicator"
    );
}

// ---------------------------------------------------------------------------
// Energy multiplier
// ---------------------------------------------------------------------------

#[test]
fn background_energy_multiplier_scales_output() {
    let env_low = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        background_energy_multiplier: 0.5,
        ..Default::default()
    };
    let env_high = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        background_energy_multiplier: 1.5,
        ..Default::default()
    };
    let fb_low = render_environment_preview(&env_low, 8, 8);
    let fb_high = render_environment_preview(&env_high, 8, 8);

    let sum_low: f32 = fb_low.pixels.iter().map(|c| c.r + c.g + c.b).sum();
    let sum_high: f32 = fb_high.pixels.iter().map(|c| c.r + c.g + c.b).sum();
    assert!(
        sum_high > sum_low,
        "higher energy should produce brighter output"
    );
}

// ---------------------------------------------------------------------------
// EnvironmentPreviewInfo
// ---------------------------------------------------------------------------

#[test]
fn preview_info_custom_color_description() {
    let env = Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: Color::new(0.1, 0.2, 0.3, 1.0),
        ..Default::default()
    };
    let info = EnvironmentPreviewInfo::from_environment(&env);
    assert!(info.background_description.contains("Custom Color"));
    assert!(info.background_description.contains("0.10"));
}

#[test]
fn preview_info_physical_sky() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Physical(PhysicalSkyMaterial::default()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let info = EnvironmentPreviewInfo::from_environment(&env);
    assert_eq!(info.sky_description, Some("Physical Sky".to_string()));
}

#[test]
fn preview_info_panoramic_with_path() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
                panorama_path: "res://sky.hdr".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let info = EnvironmentPreviewInfo::from_environment(&env);
    assert!(info.sky_description.as_ref().unwrap().contains("sky.hdr"));
}

#[test]
fn preview_info_tone_mappers() {
    for (mapper, name) in [
        (ToneMapper::Linear, "Linear"),
        (ToneMapper::Reinhard, "Reinhard"),
        (ToneMapper::Filmic, "Filmic"),
        (ToneMapper::Aces, "ACES"),
    ] {
        let env = Environment3D {
            tone_mapper: mapper,
            ..Default::default()
        };
        let info = EnvironmentPreviewInfo::from_environment(&env);
        assert_eq!(info.tone_mapper_name, name);
    }
}

#[test]
fn preview_info_fog_disabled() {
    let env = Environment3D::default();
    let info = EnvironmentPreviewInfo::from_environment(&env);
    assert!(!info.fog_active);
    assert!(info.fog_description.is_none());
}

#[test]
fn preview_info_fog_enabled() {
    let env = Environment3D {
        fog_enabled: true,
        fog_density: 0.05,
        fog_light_color: Color::new(0.8, 0.8, 0.9, 1.0),
        ..Default::default()
    };
    let info = EnvironmentPreviewInfo::from_environment(&env);
    assert!(info.fog_active);
    let desc = info.fog_description.unwrap();
    assert!(desc.contains("0.050"));
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn tiny_framebuffer_1x1() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        fog_enabled: true,
        fog_density: 0.1,
        ambient_source: AmbientSource::Color,
        ambient_color: Color::new(1.0, 0.0, 0.0, 1.0),
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 1, 1);
    assert_eq!(fb.pixels.len(), 1);
}

#[test]
fn large_framebuffer_renders() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        ..Default::default()
    };
    let fb = render_environment_preview(&env, 512, 512);
    assert_eq!(fb.pixels.len(), 512 * 512);
    let nonblack = fb
        .pixels
        .iter()
        .filter(|c| c.r > 0.01 || c.g > 0.01 || c.b > 0.01)
        .count();
    assert!(nonblack > 0);
}

#[test]
fn sky_without_sky_resource_falls_back() {
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: None,
        ..Default::default()
    };
    // Should not panic, should fall back to gray
    let fb = render_environment_preview(&env, 8, 8);
    assert!(fb.pixels[0].r > 0.1);
}
