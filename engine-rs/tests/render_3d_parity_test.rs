//! 3D render path parity tests (pat-5p5q).
//!
//! Validates the 3D render pipeline produces measurable, deterministic output
//! that can be compared against golden references and Godot oracle data.

use gdcore::math::Color;
use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::compare::{compare_framebuffers_3d, diff_image_3d};
use gdrender3d::renderer::FrameBuffer3D;
use gdrender3d::test_adapter::{
    assert_depth_3d, assert_pixel_color_3d, capture_frame_3d, count_depth_written,
    count_visible_pixels, frame_data_to_buffer_3d, save_ppm_3d,
};
use gdrender3d::SoftwareRenderer3D;
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

const W: u32 = 64;
const H: u32 = 64;
const COLOR_TOL: f64 = 0.02;
const DEPTH_TOL: f64 = 0.001;

fn cube_at(renderer: &mut SoftwareRenderer3D, pos: Vector3, color: Color) {
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    let mut mat = Material3D::default();
    mat.albedo = color;
    mat.shading_mode = ShadingMode::Unlit;
    renderer.set_material(id, mat);
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: pos,
        },
    );
}

// 1. Empty scene renders all black.
#[test]
fn empty_scene_all_black() {
    let mut renderer = SoftwareRenderer3D::new();
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);
    assert_eq!(count_visible_pixels(&fb), 0);
}

// 2. Single cube produces visible wireframe pixels.
#[test]
fn single_cube_visible() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, Vector3::new(0.0, 0.0, -5.0), Color::WHITE);
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);
    let visible = count_visible_pixels(&fb);
    assert!(
        visible > 10,
        "cube wireframe should produce >10 pixels, got {visible}"
    );
}

// 3. Deterministic rendering — identical input produces identical output.
#[test]
fn deterministic_3d_render() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, Vector3::new(0.0, 0.0, -5.0), Color::WHITE);
    let vp = Viewport3D::new(W, H);
    let fb1 = capture_frame_3d(&mut renderer, &vp);
    let fb2 = capture_frame_3d(&mut renderer, &vp);
    let result = compare_framebuffers_3d(&fb1, &fb2, 0.0, 0.0);
    assert!(
        result.is_exact_color_match(),
        "3D rendering must be deterministic (color)"
    );
}

// 4. Invisible instance produces no pixels.
#[test]
fn invisible_instance_produces_nothing() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_visible(id, false);
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);
    assert_eq!(count_visible_pixels(&fb), 0);
}

// 5. Material albedo controls pixel color.
#[test]
fn material_albedo_controls_color() {
    let mut renderer = SoftwareRenderer3D::new();
    let red = Color::rgb(1.0, 0.0, 0.0);
    cube_at(&mut renderer, Vector3::new(0.0, 0.0, -5.0), red);
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    let red_pixels = fb
        .pixels
        .iter()
        .filter(|c| c.r > 0.9 && c.g < 0.1 && c.b < 0.1)
        .count();
    assert!(
        red_pixels > 0,
        "red material should produce red wireframe pixels"
    );
}

// 6. Two cubes at different depths — both visible in wireframe.
#[test]
fn two_cubes_at_different_depths() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(
        &mut renderer,
        Vector3::new(-1.0, 0.0, -4.0),
        Color::rgb(1.0, 0.0, 0.0),
    );
    cube_at(
        &mut renderer,
        Vector3::new(1.0, 0.0, -8.0),
        Color::rgb(0.0, 1.0, 0.0),
    );
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    let red_px = fb.pixels.iter().filter(|c| c.r > 0.5 && c.g < 0.1).count();
    let green_px = fb.pixels.iter().filter(|c| c.g > 0.5 && c.r < 0.1).count();
    assert!(red_px > 0, "near red cube should be visible");
    assert!(green_px > 0, "far green cube should be visible");
}

// 7. Framebuffer comparison detects identical frames.
#[test]
fn compare_identical_frames() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, Vector3::new(0.0, 0.0, -5.0), Color::WHITE);
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);
    let result = compare_framebuffers_3d(&fb, &fb, COLOR_TOL, DEPTH_TOL);
    assert_eq!(result.color_match_ratio(), 1.0);
}

// 8. Framebuffer comparison detects differences.
#[test]
fn compare_detects_differences() {
    let mut renderer1 = SoftwareRenderer3D::new();
    cube_at(
        &mut renderer1,
        Vector3::new(0.0, 0.0, -5.0),
        Color::rgb(1.0, 0.0, 0.0),
    );
    let vp = Viewport3D::new(W, H);
    let fb1 = capture_frame_3d(&mut renderer1, &vp);

    let mut renderer2 = SoftwareRenderer3D::new();
    cube_at(
        &mut renderer2,
        Vector3::new(0.0, 0.0, -5.0),
        Color::rgb(0.0, 1.0, 0.0),
    );
    let fb2 = capture_frame_3d(&mut renderer2, &vp);

    let result = compare_framebuffers_3d(&fb1, &fb2, 0.0, DEPTH_TOL);
    assert!(
        result.color_match_ratio() < 1.0,
        "different colors should produce different frames"
    );
}

// 9. Diff image produces visual output.
#[test]
fn diff_image_produces_output() {
    let a = FrameBuffer3D::new(8, 8, Color::BLACK);
    let mut b = FrameBuffer3D::new(8, 8, Color::BLACK);
    b.set_pixel(4, 4, Color::WHITE);
    let diff = diff_image_3d(&a, &b);
    assert_eq!(diff.width, 8);
    assert_eq!(diff.height, 8);
    // The differing pixel should be red-ish.
    let p = diff.get_pixel(4, 4);
    assert!(p.r > 0.0, "diff pixel should have red component");
}

// 10. frame_data_to_buffer_3d preserves data.
#[test]
fn frame_data_to_buffer_preserves() {
    let frame = gdserver3d::server::FrameData3D {
        width: 4,
        height: 4,
        pixels: vec![Color::rgb(0.25, 0.5, 0.75); 16],
        depth: vec![0.42; 16],
    };
    let fb = frame_data_to_buffer_3d(&frame);
    assert_eq!(fb.width, 4);
    assert_pixel_color_3d(&fb, 0, 0, Color::rgb(0.25, 0.5, 0.75), 0.001);
    assert_depth_3d(&fb, 0, 0, 0.42, 0.001);
}

// 11. Freed instance no longer renders.
#[test]
fn freed_instance_not_rendered() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );
    let vp = Viewport3D::new(W, H);

    let fb_before = capture_frame_3d(&mut renderer, &vp);
    assert!(count_visible_pixels(&fb_before) > 0);

    renderer.free_instance(id);
    let fb_after = capture_frame_3d(&mut renderer, &vp);
    assert_eq!(count_visible_pixels(&fb_after), 0);
}

// 12. save_ppm_3d writes valid PPM file.
#[test]
fn save_ppm_creates_file() {
    let fb = FrameBuffer3D::new(2, 2, Color::rgb(1.0, 0.0, 0.0));
    let path = "/tmp/patina_test_render_3d.ppm";
    save_ppm_3d(&fb, path).expect("failed to write PPM");
    let content = std::fs::read_to_string(path).expect("failed to read PPM");
    assert!(content.starts_with("P3"));
    assert!(content.contains("255 0 0"));
    let _ = std::fs::remove_file(path);
}

// 13. Sphere primitive renders differently from cube.
#[test]
fn sphere_renders_differently_from_cube() {
    let vp = Viewport3D::new(W, H);

    let mut r1 = SoftwareRenderer3D::new();
    cube_at(&mut r1, Vector3::new(0.0, 0.0, -5.0), Color::WHITE);
    let fb_cube = capture_frame_3d(&mut r1, &vp);

    let mut r2 = SoftwareRenderer3D::new();
    let id = r2.create_instance();
    r2.set_mesh(id, Mesh3D::sphere(1.0, 8));
    r2.set_material(id, Material3D::default());
    r2.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );
    let fb_sphere = capture_frame_3d(&mut r2, &vp);

    let result = compare_framebuffers_3d(&fb_cube, &fb_sphere, 0.0, 1.0);
    assert!(
        result.color_match_ratio() < 0.99,
        "sphere and cube should produce visually different frames"
    );
}

// 14. Depth buffer data propagates through render path (pat-fie).
#[test]
fn depth_data_propagates_through_render() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, Vector3::new(0.0, 0.0, -5.0), Color::WHITE);
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    let depth_written = count_depth_written(&fb);
    let visible = count_visible_pixels(&fb);
    assert!(
        depth_written > 0,
        "depth buffer should have written values for visible wireframe"
    );
    assert_eq!(
        depth_written, visible,
        "every visible pixel should have a corresponding depth write"
    );
}

// 15. Depth values are within valid [0, 1) range for rendered pixels.
#[test]
fn depth_values_in_valid_range() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, Vector3::new(0.0, 0.0, -5.0), Color::WHITE);
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    for (i, &d) in fb.depth.iter().enumerate() {
        assert!(
            (0.0..=1.0).contains(&d),
            "depth[{i}] = {d} is outside [0.0, 1.0]"
        );
    }
}

// 16. Multiple primitives stress test — 10 objects.
#[test]
fn stress_10_objects() {
    let mut renderer = SoftwareRenderer3D::new();
    for i in 0..10 {
        let x = (i as f32 - 4.5) * 1.5;
        let color = Color::rgb(i as f32 / 9.0, 1.0 - i as f32 / 9.0, 0.5);
        cube_at(&mut renderer, Vector3::new(x, 0.0, -10.0), color);
    }
    let vp = Viewport3D::new(128, 64);
    let fb = capture_frame_3d(&mut renderer, &vp);
    let visible = count_visible_pixels(&fb);
    assert!(
        visible > 50,
        "10 cubes should produce significant pixel coverage, got {visible}"
    );
}
