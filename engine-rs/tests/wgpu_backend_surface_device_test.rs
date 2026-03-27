//! pat-b6wu8: wgpu backend integration with Surface and Device setup.
//!
//! Integration tests exercising the WgpuRenderer public API from outside the
//! crate, validating Surface creation, Device info population, draw batch
//! lifecycle, render frame correctness, resize behaviour, and RenderingServer2D
//! trait conformance.
//!
//! All tests run without the `gpu` feature (software fallback path), so they
//! work in CI without a GPU. GPU-specific paths are tested inline in the crate.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::wgpu_backend::{BackendType, DeviceInfo, DrawBatch, SurfaceConfig, WgpuRenderer};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::texture::Texture2D;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

const EPSILON: f32 = 1e-4;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

// ===========================================================================
// 1. SurfaceConfig defaults
// ===========================================================================

/// SurfaceConfig default should be 1280x720 with vsync enabled.
#[test]
fn b6wu8_surface_config_defaults() {
    let config = SurfaceConfig::default();
    assert_eq!(config.width, 1280);
    assert_eq!(config.height, 720);
    assert!(config.vsync);
}

/// SurfaceConfig Clone produces identical values.
#[test]
fn b6wu8_surface_config_clone() {
    let config = SurfaceConfig {
        width: 1920,
        height: 1080,
        vsync: false,
    };
    let cloned = config.clone();
    assert_eq!(cloned.width, 1920);
    assert_eq!(cloned.height, 1080);
    assert!(!cloned.vsync);
}

// ===========================================================================
// 2. DeviceInfo and BackendType
// ===========================================================================

/// DeviceInfo equality and clone.
#[test]
fn b6wu8_device_info_equality_and_clone() {
    let info = DeviceInfo {
        adapter_name: "Test Adapter".to_string(),
        backend: BackendType::Metal,
        max_texture_size: 16384,
    };
    let cloned = info.clone();
    assert_eq!(info, cloned);
    assert_eq!(info.adapter_name, "Test Adapter");
    assert_eq!(info.backend, BackendType::Metal);
    assert_eq!(info.max_texture_size, 16384);
}

/// BackendType variants are distinct.
#[test]
fn b6wu8_backend_type_variants() {
    let variants = [
        BackendType::Vulkan,
        BackendType::Metal,
        BackendType::Dx12,
        BackendType::OpenGl,
        BackendType::Software,
    ];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

/// BackendType can be used as a HashMap key (Hash + Eq).
#[test]
fn b6wu8_backend_type_hashable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(BackendType::Metal);
    set.insert(BackendType::Vulkan);
    set.insert(BackendType::Metal); // duplicate
    assert_eq!(set.len(), 2);
}

// ===========================================================================
// 3. WgpuRenderer creation and surface lifecycle
// ===========================================================================

/// New renderer starts with no surface, no device info, no batches.
#[test]
fn b6wu8_renderer_initial_state() {
    let renderer = WgpuRenderer::new();
    assert!(!renderer.has_surface());
    assert!(renderer.device_info().is_none());
    assert_eq!(renderer.pending_batch_count(), 0);
    assert_eq!(renderer.clear_color(), Color::BLACK);
    assert_eq!(renderer.frames_rendered(), 0);
    assert!(!renderer.has_render_target());
}

/// create_surface populates device info and marks surface as created.
#[test]
fn b6wu8_create_surface_populates_device_info() {
    let mut renderer = WgpuRenderer::new();
    renderer.create_surface(SurfaceConfig {
        width: 640,
        height: 480,
        vsync: true,
    });
    assert!(renderer.has_surface());
    let info = renderer.device_info().expect("should have device info");
    assert!(!info.adapter_name.is_empty());
    assert!(info.max_texture_size >= 2048);
    assert_eq!(renderer.surface_config().width, 640);
    assert_eq!(renderer.surface_config().height, 480);
}

/// create_surface with custom config stores the config.
#[test]
fn b6wu8_create_surface_custom_config() {
    let mut renderer = WgpuRenderer::new();
    renderer.create_surface(SurfaceConfig {
        width: 3840,
        height: 2160,
        vsync: false,
    });
    assert_eq!(renderer.surface_config().width, 3840);
    assert_eq!(renderer.surface_config().height, 2160);
    assert!(!renderer.surface_config().vsync);
}

// ===========================================================================
// 4. Software fallback device info
// ===========================================================================

/// Without gpu feature, device info reports software adapter.
#[test]
fn b6wu8_software_fallback_device_info() {
    let mut renderer = WgpuRenderer::new();
    assert!(!renderer.has_gpu_context());
    renderer.create_surface(SurfaceConfig::default());
    let info = renderer.device_info().unwrap();
    assert_eq!(info.backend, BackendType::Software);
    assert_eq!(info.adapter_name, "Patina Software Adapter");
    assert_eq!(info.max_texture_size, 8192);
}

// ===========================================================================
// 5. Draw batch lifecycle
// ===========================================================================

/// Submit batches, verify count, present clears them.
#[test]
fn b6wu8_draw_batch_submit_and_present() {
    let mut renderer = WgpuRenderer::new();
    renderer.create_surface(SurfaceConfig::default());

    renderer.submit_draw_commands(DrawBatch { command_count: 10 });
    renderer.submit_draw_commands(DrawBatch { command_count: 5 });
    renderer.submit_draw_commands(DrawBatch { command_count: 20 });
    assert_eq!(renderer.pending_batch_count(), 3);

    renderer.present_frame();
    assert_eq!(renderer.pending_batch_count(), 0);
}

/// DrawBatch stores command count correctly.
#[test]
fn b6wu8_draw_batch_command_count() {
    let batch = DrawBatch { command_count: 42 };
    let cloned = batch.clone();
    assert_eq!(cloned.command_count, 42);
}

// ===========================================================================
// 6. Clear color
// ===========================================================================

/// Set and get clear color.
#[test]
fn b6wu8_clear_color_set_get() {
    let mut renderer = WgpuRenderer::new();
    assert_eq!(renderer.clear_color(), Color::BLACK);

    let red = Color::rgb(1.0, 0.0, 0.0);
    renderer.set_clear_color(red);
    assert!(approx_eq(renderer.clear_color().r, 1.0));
    assert!(approx_eq(renderer.clear_color().g, 0.0));
    assert!(approx_eq(renderer.clear_color().b, 0.0));

    let custom = Color::new(0.2, 0.4, 0.6, 0.8);
    renderer.set_clear_color(custom);
    assert!(approx_eq(renderer.clear_color().r, 0.2));
    assert!(approx_eq(renderer.clear_color().g, 0.4));
    assert!(approx_eq(renderer.clear_color().b, 0.6));
    assert!(approx_eq(renderer.clear_color().a, 0.8));
}

// ===========================================================================
// 7. Resize
// ===========================================================================

/// Resize updates the surface config dimensions.
#[test]
fn b6wu8_resize_updates_config() {
    let mut renderer = WgpuRenderer::new();
    renderer.create_surface(SurfaceConfig {
        width: 800,
        height: 600,
        vsync: true,
    });
    assert_eq!(renderer.surface_config().width, 800);
    assert_eq!(renderer.surface_config().height, 600);

    renderer.resize(1024, 768);
    assert_eq!(renderer.surface_config().width, 1024);
    assert_eq!(renderer.surface_config().height, 768);
}

/// Multiple resizes update correctly.
#[test]
fn b6wu8_resize_multiple_times() {
    let mut renderer = WgpuRenderer::new();
    renderer.create_surface(SurfaceConfig::default());

    for (w, h) in [(320, 240), (640, 480), (1920, 1080), (100, 100)] {
        renderer.resize(w, h);
        assert_eq!(renderer.surface_config().width, w);
        assert_eq!(renderer.surface_config().height, h);
    }
}

// ===========================================================================
// 8. RenderingServer2D trait: render_frame
// ===========================================================================

/// WgpuRenderer implements RenderingServer2D::render_frame and produces
/// a framebuffer with correct dimensions.
#[test]
fn b6wu8_render_frame_produces_framebuffer() {
    let mut renderer = WgpuRenderer::new();
    let vp = Viewport::new(32, 24, Color::rgb(0.0, 0.0, 1.0));
    let frame = renderer.render_frame(&vp);

    assert_eq!(frame.width, 32);
    assert_eq!(frame.height, 24);
    assert_eq!(frame.pixels.len(), (32 * 24) as usize);
}

/// render_frame increments frames_rendered counter.
#[test]
fn b6wu8_render_frame_increments_counter() {
    let mut renderer = WgpuRenderer::new();
    assert_eq!(renderer.frames_rendered(), 0);

    let vp = Viewport::new(8, 8, Color::BLACK);
    for i in 1..=5 {
        renderer.render_frame(&vp);
        assert_eq!(renderer.frames_rendered(), i);
    }
}

/// render_frame clears to the viewport's clear color.
#[test]
fn b6wu8_render_frame_clear_color() {
    let mut renderer = WgpuRenderer::new();
    let red = Color::rgb(1.0, 0.0, 0.0);
    let vp = Viewport::new(4, 4, red);
    let frame = renderer.render_frame(&vp);

    // All pixels should be red (viewport clear color).
    for pixel in &frame.pixels {
        assert!(approx_eq(pixel.r, 1.0), "r={}", pixel.r);
        assert!(approx_eq(pixel.g, 0.0), "g={}", pixel.g);
        assert!(approx_eq(pixel.b, 0.0), "b={}", pixel.b);
    }
}

// ===========================================================================
// 9. WgpuRenderer vs SoftwareRenderer parity
// ===========================================================================

/// Both renderers produce identical output for the same viewport with draw
/// commands (rect + circle).
#[test]
fn b6wu8_wgpu_software_parity_draw_commands() {
    let make_viewport = || {
        let mut vp = Viewport::new(20, 20, Color::BLACK);
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(8.0, 8.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });
        item.commands.push(DrawCommand::DrawCircle {
            center: Vector2::new(15.0, 15.0),
            radius: 3.0,
            color: Color::rgb(0.0, 1.0, 0.0),
        });
        vp.add_canvas_item(item);
        vp
    };

    let mut wgpu_r = WgpuRenderer::new();
    let mut sw_r = SoftwareRenderer::new();

    let frame_wgpu = wgpu_r.render_frame(&make_viewport());
    let frame_sw = sw_r.render_frame(&make_viewport());

    assert_eq!(frame_wgpu.width, frame_sw.width);
    assert_eq!(frame_wgpu.height, frame_sw.height);
    assert_eq!(frame_wgpu.pixels.len(), frame_sw.pixels.len());
    assert_eq!(frame_wgpu.pixels, frame_sw.pixels);
}

/// Parity with multiple canvas items and visibility.
#[test]
fn b6wu8_wgpu_software_parity_visibility() {
    let make_viewport = || {
        let mut vp = Viewport::new(16, 16, Color::BLACK);

        // Visible item: blue rect.
        let mut visible = CanvasItem::new(CanvasItemId(1));
        visible.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(8.0, 8.0)),
            color: Color::rgb(0.0, 0.0, 1.0),
            filled: true,
        });
        vp.add_canvas_item(visible);

        // Invisible item: should not appear.
        let mut invisible = CanvasItem::new(CanvasItemId(2));
        invisible.visible = false;
        invisible.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(8.0, 8.0), Vector2::new(8.0, 8.0)),
            color: Color::rgb(1.0, 1.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(invisible);
        vp
    };

    let mut wgpu_r = WgpuRenderer::new();
    let mut sw_r = SoftwareRenderer::new();

    let frame_wgpu = wgpu_r.render_frame(&make_viewport());
    let frame_sw = sw_r.render_frame(&make_viewport());

    assert_eq!(frame_wgpu.pixels, frame_sw.pixels);
}

// ===========================================================================
// 10. Texture registration
// ===========================================================================

/// Register a texture and verify it's stored for draw commands.
#[test]
fn b6wu8_register_texture() {
    let mut renderer = WgpuRenderer::new();

    // Create a 2x2 solid texture.
    let tex = Texture2D::solid(2, 2, Color::rgb(1.0, 0.0, 0.0));
    renderer.register_texture("res://test.png", tex);

    // Render a viewport to verify no panic with registered texture.
    let vp = Viewport::new(4, 4, Color::BLACK);
    let frame = renderer.render_frame(&vp);
    assert_eq!(frame.width, 4);
    assert_eq!(frame.height, 4);
}

// ===========================================================================
// 11. Multi-frame stability
// ===========================================================================

/// Running many render frames does not leak or crash.
#[test]
fn b6wu8_multi_frame_stability() {
    let mut renderer = WgpuRenderer::new();
    renderer.create_surface(SurfaceConfig {
        width: 16,
        height: 16,
        vsync: false,
    });

    let vp = Viewport::new(16, 16, Color::BLACK);
    for _ in 0..100 {
        renderer.render_frame(&vp);
        renderer.submit_draw_commands(DrawBatch { command_count: 1 });
        renderer.present_frame();
    }
    assert_eq!(renderer.frames_rendered(), 100);
    assert_eq!(renderer.pending_batch_count(), 0);
}

// ===========================================================================
// 12. Determinism: same input produces same output
// ===========================================================================

/// Two independent renders of the same viewport produce identical results.
#[test]
fn b6wu8_render_determinism() {
    let make_viewport = || {
        let mut vp = Viewport::new(10, 10, Color::rgb(0.1, 0.2, 0.3));
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(1.0, 1.0), Vector2::new(5.0, 5.0)),
            color: Color::rgb(0.8, 0.2, 0.5),
            filled: true,
        });
        vp.add_canvas_item(item);
        vp
    };

    let mut r1 = WgpuRenderer::new();
    let mut r2 = WgpuRenderer::new();

    let frame1 = r1.render_frame(&make_viewport());
    let frame2 = r2.render_frame(&make_viewport());

    assert_eq!(frame1.pixels, frame2.pixels);
}

// ===========================================================================
// 13. Debug formatting
// ===========================================================================

/// WgpuRenderer Debug does not panic.
#[test]
fn b6wu8_renderer_debug() {
    let renderer = WgpuRenderer::new();
    let debug = format!("{:?}", renderer);
    assert!(debug.contains("WgpuRenderer"));
}

/// SurfaceConfig Debug does not panic.
#[test]
fn b6wu8_surface_config_debug() {
    let config = SurfaceConfig::default();
    let debug = format!("{:?}", config);
    assert!(debug.contains("SurfaceConfig"));
}

/// DeviceInfo Debug does not panic.
#[test]
fn b6wu8_device_info_debug() {
    let info = DeviceInfo {
        adapter_name: "Test".to_string(),
        backend: BackendType::Software,
        max_texture_size: 4096,
    };
    let debug = format!("{:?}", info);
    assert!(debug.contains("Test"));
    assert!(debug.contains("4096"));
}

/// BackendType Debug shows variant name.
#[test]
fn b6wu8_backend_type_debug() {
    assert!(format!("{:?}", BackendType::Vulkan).contains("Vulkan"));
    assert!(format!("{:?}", BackendType::Metal).contains("Metal"));
    assert!(format!("{:?}", BackendType::Dx12).contains("Dx12"));
    assert!(format!("{:?}", BackendType::OpenGl).contains("OpenGl"));
    assert!(format!("{:?}", BackendType::Software).contains("Software"));
}

// ===========================================================================
// 14. has_gpu_context returns false without gpu feature
// ===========================================================================

#[test]
fn b6wu8_has_gpu_context_false_without_feature() {
    let renderer = WgpuRenderer::new();
    // Without the `gpu` feature, this should always be false.
    assert!(!renderer.has_gpu_context());
}
