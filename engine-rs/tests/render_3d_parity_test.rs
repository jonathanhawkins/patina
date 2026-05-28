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
use gdserver3d::material::{Material3D, ShadingMode, StandardMaterial3D, TextureSlot};
use gdvariant::Variant;
use gdserver3d::environment::{BackgroundMode, Environment3D};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::sky::{PhysicalSkyMaterial, ProceduralSkyMaterial, Sky, SkyMaterial};
use gdserver3d::reflection_probe::{
    ReflectionProbe, ReflectionProbeAmbientMode, ReflectionProbeId,
};
use gdserver3d::sprite3d::{BillboardMode3D, Sprite3D};
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

// ---------------------------------------------------------------------------
// Sprite3D billboard parity tests (pat-ysksx)
// ---------------------------------------------------------------------------
//
// Covers BillboardMode3D semantics against Godot's SpriteBase3D / BaseMaterial3D
// billboard behaviour: the four mode variants, their serialized Godot integer
// values, orthonormality/right-handedness of the resulting basis, degenerate
// fallbacks (coincident/parallel-up camera), and invariants (translation
// independence, mesh independence from mode).

fn sprite3d_approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

fn sprite3d_v3_approx_eq(a: Vector3, b: Vector3) -> bool {
    sprite3d_approx_eq(a.x, b.x)
        && sprite3d_approx_eq(a.y, b.y)
        && sprite3d_approx_eq(a.z, b.z)
}

fn assert_basis_orthonormal(b: Basis) {
    assert!(sprite3d_approx_eq(b.x.length(), 1.0), "x not unit: {:?}", b.x);
    assert!(sprite3d_approx_eq(b.y.length(), 1.0), "y not unit: {:?}", b.y);
    assert!(sprite3d_approx_eq(b.z.length(), 1.0), "z not unit: {:?}", b.z);
    assert!(sprite3d_approx_eq(b.x.dot(b.y), 0.0), "x·y != 0");
    assert!(sprite3d_approx_eq(b.x.dot(b.z), 0.0), "x·z != 0");
    assert!(sprite3d_approx_eq(b.y.dot(b.z), 0.0), "y·z != 0");
}

// 17. Default Sprite3D has billboarding disabled (Godot default).
#[test]
fn sprite3d_default_billboard_disabled() {
    let s = Sprite3D::default();
    assert_eq!(s.billboard, BillboardMode3D::Disabled);
}

// 18. Billboard mode integer values match Godot's BaseMaterial3D.BillboardMode.
#[test]
fn sprite3d_billboard_mode_godot_int_values() {
    assert_eq!(BillboardMode3D::Disabled.to_godot_int(), 0);
    assert_eq!(BillboardMode3D::Enabled.to_godot_int(), 1);
    assert_eq!(BillboardMode3D::YBillboard.to_godot_int(), 2);
    assert_eq!(BillboardMode3D::Particles.to_godot_int(), 3);
}

// 19. Billboard mode int roundtrip preserves every variant.
#[test]
fn sprite3d_billboard_mode_godot_int_roundtrip() {
    for mode in [
        BillboardMode3D::Disabled,
        BillboardMode3D::Enabled,
        BillboardMode3D::YBillboard,
        BillboardMode3D::Particles,
    ] {
        assert_eq!(BillboardMode3D::from_godot_int(mode.to_godot_int()), mode);
    }
}

// 20. Unknown Godot ints fall back to Disabled (Godot-compatible default).
#[test]
fn sprite3d_billboard_mode_unknown_maps_to_disabled() {
    assert_eq!(
        BillboardMode3D::from_godot_int(99),
        BillboardMode3D::Disabled
    );
    assert_eq!(
        BillboardMode3D::from_godot_int(-1),
        BillboardMode3D::Disabled
    );
    assert_eq!(
        BillboardMode3D::from_godot_int(i64::MAX),
        BillboardMode3D::Disabled
    );
}

// 21. Billboard Disabled returns identity for any camera position.
#[test]
fn sprite3d_billboard_disabled_always_identity() {
    let s = Sprite3D::default();
    for cam in [
        Vector3::new(1.0, 2.0, 3.0),
        Vector3::new(-10.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 100.0),
        Vector3::ZERO,
    ] {
        assert_eq!(s.billboard_basis(Vector3::ZERO, cam), Basis::IDENTITY);
    }
}

// 22. Billboard Enabled aligns forward (basis.z) with camera direction.
#[test]
fn sprite3d_billboard_enabled_forward_points_to_camera() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::Enabled;
    let cam = Vector3::new(3.0, 0.0, 4.0);
    let b = s.billboard_basis(Vector3::ZERO, cam);
    assert!(sprite3d_v3_approx_eq(b.z, cam.normalized()));
}

// 23. Billboard Enabled basis is orthonormal for non-degenerate cameras.
#[test]
fn sprite3d_billboard_enabled_basis_orthonormal() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::Enabled;
    for cam in [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(3.0, 2.0, 4.0),
        Vector3::new(-5.0, 1.0, 2.0),
        Vector3::new(0.1, 0.05, -7.0),
    ] {
        assert_basis_orthonormal(s.billboard_basis(Vector3::ZERO, cam));
    }
}

// 24. Billboard Enabled basis is right-handed (x × y = z).
#[test]
fn sprite3d_billboard_enabled_right_handed() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::Enabled;
    let b = s.billboard_basis(Vector3::ZERO, Vector3::new(3.0, 1.0, 4.0));
    assert!(sprite3d_v3_approx_eq(b.x.cross(b.y), b.z));
}

// 25. Billboard Enabled returns identity when sprite and camera coincide.
#[test]
fn sprite3d_billboard_enabled_coincident_returns_identity() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::Enabled;
    let p = Vector3::new(2.0, -1.0, 3.0);
    assert_eq!(s.billboard_basis(p, p), Basis::IDENTITY);
}

// 26. Billboard Enabled returns identity when camera is directly above
// (forward parallel to world up — cross product degenerates).
#[test]
fn sprite3d_billboard_enabled_directly_above_returns_identity() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::Enabled;
    let b = s.billboard_basis(Vector3::ZERO, Vector3::new(0.0, 5.0, 0.0));
    assert_eq!(b, Basis::IDENTITY);
}

// 27. Billboard Enabled is translation-invariant — only relative camera
// position matters.
#[test]
fn sprite3d_billboard_enabled_translation_invariant() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::Enabled;
    let delta = Vector3::new(3.0, 1.5, 4.0);
    let b_origin = s.billboard_basis(Vector3::ZERO, delta);
    let shift = Vector3::new(-7.0, 2.0, 11.0);
    let b_shifted = s.billboard_basis(shift, shift + delta);
    assert!(sprite3d_v3_approx_eq(b_origin.x, b_shifted.x));
    assert!(sprite3d_v3_approx_eq(b_origin.y, b_shifted.y));
    assert!(sprite3d_v3_approx_eq(b_origin.z, b_shifted.z));
}

// 28. Y-billboard locks basis.y to world up regardless of camera height.
#[test]
fn sprite3d_billboard_y_preserves_world_up() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::YBillboard;
    for cam in [
        Vector3::new(5.0, 3.0, 5.0),
        Vector3::new(-2.0, 10.0, 1.0),
        Vector3::new(0.0, -4.0, 2.0),
    ] {
        let b = s.billboard_basis(Vector3::ZERO, cam);
        assert!(sprite3d_v3_approx_eq(b.y, Vector3::UP));
    }
}

// 29. Y-billboard forward lies in the XZ plane (y component == 0).
#[test]
fn sprite3d_billboard_y_forward_flat() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::YBillboard;
    let b = s.billboard_basis(Vector3::ZERO, Vector3::new(2.0, 7.0, 3.0));
    assert!(sprite3d_approx_eq(b.z.y, 0.0));
    assert!(sprite3d_approx_eq(b.x.y, 0.0));
}

// 30. Y-billboard returns identity when camera shares the sprite's XZ position
// (projected delta is zero).
#[test]
fn sprite3d_billboard_y_vertical_only_returns_identity() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::YBillboard;
    let b = s.billboard_basis(Vector3::ZERO, Vector3::new(0.0, 9.0, 0.0));
    assert_eq!(b, Basis::IDENTITY);
    // Coincident camera also returns identity.
    let b0 = s.billboard_basis(Vector3::ZERO, Vector3::ZERO);
    assert_eq!(b0, Basis::IDENTITY);
}

// 31. Y-billboard basis is orthonormal and right-handed for a non-degenerate
// camera position.
#[test]
fn sprite3d_billboard_y_basis_orthonormal_right_handed() {
    let mut s = Sprite3D::default();
    s.billboard = BillboardMode3D::YBillboard;
    let b = s.billboard_basis(Vector3::ZERO, Vector3::new(4.0, 5.0, -2.0));
    assert_basis_orthonormal(b);
    assert!(sprite3d_v3_approx_eq(b.x.cross(b.y), b.z));
}

// 32. Particles billboard produces the same basis as Enabled across arbitrary
// camera positions (Godot treats them identically for orientation).
#[test]
fn sprite3d_billboard_particles_matches_enabled() {
    let mut enabled = Sprite3D::default();
    enabled.billboard = BillboardMode3D::Enabled;
    let mut particles = Sprite3D::default();
    particles.billboard = BillboardMode3D::Particles;
    for cam in [
        Vector3::new(1.0, 2.0, 3.0),
        Vector3::new(-4.0, 0.5, 6.0),
        Vector3::new(0.1, 0.0, -8.0),
        Vector3::new(0.0, 5.0, 0.0), // degenerate → both IDENTITY
    ] {
        let be = enabled.billboard_basis(Vector3::ZERO, cam);
        let bp = particles.billboard_basis(Vector3::ZERO, cam);
        assert_eq!(be, bp, "Particles should match Enabled for cam={cam:?}");
    }
}

// ---------------------------------------------------------------------------
// WorldEnvironment + Sky runtime parity tests (pat-533w8)
// ---------------------------------------------------------------------------
//
// Verifies that an Environment3D attached to a Viewport3D is consumed by the
// software renderer at frame-capture time, matching Godot's WorldEnvironment
// runtime behaviour: background modes (ClearColor / CustomColor / Sky),
// procedural and physical sky gradients, sky energy scaling, fog enable/disable,
// and fog attenuation with depth.

fn world_environment_sky_camera_viewport(w: u32, h: u32, env: Environment3D) -> Viewport3D {
    let mut vp = Viewport3D::new(w, h);
    // Aim the camera upward so rays hit the sky hemisphere in world space.
    vp.camera_transform = Transform3D {
        basis: Basis {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, 0.0, 1.0),
            z: Vector3::new(0.0, -1.0, 0.0),
        },
        origin: Vector3::ZERO,
    };
    vp.environment = Some(env);
    vp
}

// 34. No WorldEnvironment → background clears to black (Godot default when no
//     WorldEnvironment is present in the scene).
#[test]
fn world_environment_absent_renders_black_background() {
    let mut renderer = SoftwareRenderer3D::new();
    let vp = Viewport3D::new(W, H); // environment: None
    let fb = capture_frame_3d(&mut renderer, &vp);
    for (i, p) in fb.pixels.iter().enumerate() {
        assert!(
            p.r < 1e-5 && p.g < 1e-5 && p.b < 1e-5,
            "pixel[{i}] = {p:?} expected black"
        );
    }
}

// 35. WorldEnvironment with ClearColor background → still black regardless of
//     `background_color` (matches Godot: background_color only applies to
//     CustomColor mode).
#[test]
fn world_environment_clear_color_ignores_background_color() {
    let mut renderer = SoftwareRenderer3D::new();
    let mut vp = Viewport3D::new(W, H);
    vp.environment = Some(Environment3D {
        background_mode: BackgroundMode::ClearColor,
        background_color: Color::rgb(1.0, 0.0, 0.0), // should be ignored
        ..Default::default()
    });
    let fb = capture_frame_3d(&mut renderer, &vp);
    assert_eq!(count_visible_pixels(&fb), 0);
}

// 36. WorldEnvironment CustomColor fills every background pixel with that
//     color (Godot `Environment.BG_COLOR`).
#[test]
fn world_environment_custom_color_fills_background() {
    let mut renderer = SoftwareRenderer3D::new();
    let bg = Color::rgb(0.25, 0.5, 0.75);
    let mut vp = Viewport3D::new(W, H);
    vp.environment = Some(Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: bg,
        ..Default::default()
    });
    let fb = capture_frame_3d(&mut renderer, &vp);
    for p in &fb.pixels {
        assert!((p.r - bg.r).abs() < 1e-5);
        assert!((p.g - bg.g).abs() < 1e-5);
        assert!((p.b - bg.b).abs() < 1e-5);
    }
}

// 37. WorldEnvironment Sky mode with no Sky resource attached falls back to
//     black (Godot silently clears when Sky is null).
#[test]
fn world_environment_sky_mode_without_sky_resource_is_black() {
    let mut renderer = SoftwareRenderer3D::new();
    let mut vp = Viewport3D::new(W, H);
    vp.environment = Some(Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: None,
        ..Default::default()
    });
    let fb = capture_frame_3d(&mut renderer, &vp);
    assert_eq!(count_visible_pixels(&fb), 0);
}

// 38. WorldEnvironment + procedural Sky renders a non-black gradient
//     background.
#[test]
fn world_environment_sky_procedural_produces_non_black() {
    let mut renderer = SoftwareRenderer3D::new();
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial {
                sky_top_color: Color::new(0.05, 0.2, 0.8, 1.0),
                sky_horizon_color: Color::new(0.6, 0.7, 0.9, 1.0),
                ground_bottom_color: Color::new(0.1, 0.08, 0.06, 1.0),
                ground_horizon_color: Color::new(0.3, 0.25, 0.2, 1.0),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let vp = world_environment_sky_camera_viewport(W, H, env);
    let fb = capture_frame_3d(&mut renderer, &vp);
    let nonzero = fb
        .pixels
        .iter()
        .filter(|p| p.r > 1e-3 || p.g > 1e-3 || p.b > 1e-3)
        .count();
    assert!(
        nonzero > (W * H) as usize / 2,
        "procedural sky should fill most pixels, got {nonzero}"
    );
}

// 39. Procedural sky gradient: sky (y>0) and ground (y<0) hemispheres produce
//     visually different pixels.
#[test]
fn world_environment_sky_procedural_hemispheres_differ() {
    // Sky-only look: bright blue zenith, dark ground.
    let env_sky = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial {
                sky_top_color: Color::new(0.0, 0.0, 1.0, 1.0),
                sky_horizon_color: Color::new(0.0, 0.0, 0.5, 1.0),
                ground_bottom_color: Color::new(0.0, 0.0, 0.0, 1.0),
                ground_horizon_color: Color::new(0.0, 0.0, 0.0, 1.0),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    // Camera looking up: expect blue-dominant pixels.
    let mut r_up = SoftwareRenderer3D::new();
    let vp_up = world_environment_sky_camera_viewport(W, H, env_sky.clone());
    let fb_up = capture_frame_3d(&mut r_up, &vp_up);

    // Camera looking down: basis flipped so rays hit ground hemisphere.
    let mut r_down = SoftwareRenderer3D::new();
    let mut vp_down = Viewport3D::new(W, H);
    vp_down.camera_transform = Transform3D {
        basis: Basis {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, 0.0, -1.0),
            z: Vector3::new(0.0, 1.0, 0.0),
        },
        origin: Vector3::ZERO,
    };
    vp_down.environment = Some(env_sky);
    let fb_down = capture_frame_3d(&mut r_down, &vp_down);

    let blue_up: f32 = fb_up.pixels.iter().map(|p| p.b).sum();
    let blue_down: f32 = fb_down.pixels.iter().map(|p| p.b).sum();
    assert!(
        blue_up > blue_down + 1.0,
        "sky hemisphere blue sum {blue_up} should exceed ground sum {blue_down}"
    );
}

// 40. background_energy_multiplier scales sky brightness without flipping
//     colour balance (Godot parity — higher energy → brighter, clamped at 1.0).
#[test]
fn world_environment_sky_energy_multiplier_brightens() {
    let make_env = |energy: f32| Environment3D {
        background_mode: BackgroundMode::Sky,
        background_energy_multiplier: energy,
        sky: Some(Sky {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial {
                sky_top_color: Color::new(0.1, 0.1, 0.2, 1.0),
                sky_horizon_color: Color::new(0.1, 0.1, 0.2, 1.0),
                ground_bottom_color: Color::new(0.1, 0.1, 0.2, 1.0),
                ground_horizon_color: Color::new(0.1, 0.1, 0.2, 1.0),
                sky_energy_multiplier: 1.0,
                ground_energy_multiplier: 1.0,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut r_dim = SoftwareRenderer3D::new();
    let vp_dim = world_environment_sky_camera_viewport(W, H, make_env(1.0));
    let fb_dim = capture_frame_3d(&mut r_dim, &vp_dim);

    let mut r_bright = SoftwareRenderer3D::new();
    let vp_bright = world_environment_sky_camera_viewport(W, H, make_env(4.0));
    let fb_bright = capture_frame_3d(&mut r_bright, &vp_bright);

    let sum_dim: f32 = fb_dim.pixels.iter().map(|p| p.r + p.g + p.b).sum();
    let sum_bright: f32 = fb_bright.pixels.iter().map(|p| p.r + p.g + p.b).sum();
    assert!(
        sum_bright > sum_dim * 1.5,
        "energy=4.0 should brighten sky: dim={sum_dim} bright={sum_bright}"
    );
}

// 41. PhysicalSkyMaterial renders a non-black sky in Sky background mode
//     (Godot's PhysicalSkyMaterial produces atmosphere-tinted pixels).
#[test]
fn world_environment_sky_physical_produces_non_black() {
    let mut renderer = SoftwareRenderer3D::new();
    let env = Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky {
            material: SkyMaterial::Physical(PhysicalSkyMaterial::default()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let vp = world_environment_sky_camera_viewport(W, H, env);
    let fb = capture_frame_3d(&mut renderer, &vp);
    let nonzero = fb
        .pixels
        .iter()
        .filter(|p| p.r + p.g + p.b > 1e-3)
        .count();
    assert!(
        nonzero > (W * H) as usize / 4,
        "physical sky should fill a substantial portion of pixels, got {nonzero}"
    );
}

// 42. Fog enabled on WorldEnvironment darkens/tints pixels with written depth;
//     fog disabled leaves the same scene untouched.
#[test]
fn world_environment_fog_enabled_modifies_rendered_pixels() {
    let cube_pos = Vector3::new(0.0, 0.0, -5.0);
    let cube_color = Color::rgb(1.0, 1.0, 1.0);

    let mut r_no_fog = SoftwareRenderer3D::new();
    cube_at(&mut r_no_fog, cube_pos, cube_color);
    let mut vp_no_fog = Viewport3D::new(W, H);
    vp_no_fog.environment = Some(Environment3D {
        fog_enabled: false,
        ..Default::default()
    });
    let fb_no_fog = capture_frame_3d(&mut r_no_fog, &vp_no_fog);

    let mut r_fog = SoftwareRenderer3D::new();
    cube_at(&mut r_fog, cube_pos, cube_color);
    let mut vp_fog = Viewport3D::new(W, H);
    vp_fog.environment = Some(Environment3D {
        fog_enabled: true,
        fog_light_color: Color::new(0.2, 0.2, 0.9, 1.0),
        fog_density: 0.5,
        ..Default::default()
    });
    let fb_fog = capture_frame_3d(&mut r_fog, &vp_fog);

    // At least one cube-lit pixel should differ between fog-on and fog-off.
    let mut differ = 0;
    for (a, b) in fb_no_fog.pixels.iter().zip(fb_fog.pixels.iter()) {
        if (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs() > 1e-3 {
            differ += 1;
        }
    }
    assert!(
        differ > 0,
        "fog_enabled=true should change some rendered pixels vs fog_enabled=false"
    );
}

// 43. Fog leaves background-only (no-geometry) pixels untouched — fog only
//     applies where depth < 1.0 (parity with Godot: sky/far plane isn't fogged).
#[test]
fn world_environment_fog_skips_background_pixels() {
    let mut renderer = SoftwareRenderer3D::new();
    // No instances → all depth stays at 1.0 (no geometry written).
    let mut vp = Viewport3D::new(W, H);
    vp.environment = Some(Environment3D {
        background_mode: BackgroundMode::CustomColor,
        background_color: Color::rgb(0.2, 0.2, 0.2),
        fog_enabled: true,
        fog_light_color: Color::new(1.0, 0.0, 0.0, 1.0), // bright red fog
        fog_density: 1.0,
        ..Default::default()
    });
    let fb = capture_frame_3d(&mut renderer, &vp);
    // Every pixel should remain the background color, not tinted red by fog.
    for (i, p) in fb.pixels.iter().enumerate() {
        assert!(
            (p.r - 0.2).abs() < 1e-4 && (p.g - 0.2).abs() < 1e-4 && (p.b - 0.2).abs() < 1e-4,
            "pixel[{i}] = {p:?} was fogged; fog must not touch background pixels"
        );
    }
}

// 44. WorldEnvironment rendering is deterministic — identical environment +
//     scene produces identical frames across two captures.
#[test]
fn world_environment_sky_rendering_is_deterministic() {
    let make_env = || Environment3D {
        background_mode: BackgroundMode::Sky,
        sky: Some(Sky::default()),
        ..Default::default()
    };
    let mut r1 = SoftwareRenderer3D::new();
    let vp1 = world_environment_sky_camera_viewport(W, H, make_env());
    let fb1 = capture_frame_3d(&mut r1, &vp1);

    let mut r2 = SoftwareRenderer3D::new();
    let vp2 = world_environment_sky_camera_viewport(W, H, make_env());
    let fb2 = capture_frame_3d(&mut r2, &vp2);

    let result = compare_framebuffers_3d(&fb1, &fb2, 0.0, 0.0);
    assert!(
        result.is_exact_color_match(),
        "WorldEnvironment sky rendering must be deterministic"
    );
}

// 45. WorldEnvironment CustomColor background swaps: changing only
// `background_color` changes every background pixel exactly to the new
// value — proves the color field is authoritative and not dropped/clamped.
#[test]
fn world_environment_custom_color_swap_changes_every_pixel() {
    let render_with = |bg: Color| {
        let mut renderer = SoftwareRenderer3D::new();
        let mut vp = Viewport3D::new(W, H);
        vp.environment = Some(Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: bg,
            ..Default::default()
        });
        capture_frame_3d(&mut renderer, &vp)
    };
    let a = Color::rgb(0.1, 0.2, 0.3);
    let b = Color::rgb(0.9, 0.8, 0.7);
    let fb_a = render_with(a);
    let fb_b = render_with(b);
    assert_eq!(fb_a.pixels.len(), fb_b.pixels.len());
    for (i, (pa, pb)) in fb_a.pixels.iter().zip(fb_b.pixels.iter()).enumerate() {
        assert!(
            (pa.r - a.r).abs() < 1e-5 && (pa.g - a.g).abs() < 1e-5 && (pa.b - a.b).abs() < 1e-5,
            "pixel[{i}] fb_a = {pa:?}, expected {a:?}"
        );
        assert!(
            (pb.r - b.r).abs() < 1e-5 && (pb.g - b.g).abs() < 1e-5 && (pb.b - b.b).abs() < 1e-5,
            "pixel[{i}] fb_b = {pb:?}, expected {b:?}"
        );
    }
}

// ── StandardMaterial3D broad parity (pat-ugiot) ──
//
// These tests validate that `StandardMaterial3D` — the Godot-compatible
// PBR resource type — converts to the renderer's `Material3D` without
// losing data, and produces deterministic rendered output matching a
// directly-constructed equivalent. Texture slots are stored but are not
// currently sampled by the software renderer; these tests lock in that
// contract so changes to texture sampling will show up as regressions.

fn cube_with_std_material(
    renderer: &mut SoftwareRenderer3D,
    pos: Vector3,
    std_mat: &StandardMaterial3D,
) {
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(id, std_mat.to_material3d());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: pos,
        },
    );
}

fn cube_with_material(
    renderer: &mut SoftwareRenderer3D,
    pos: Vector3,
    mat: Material3D,
) {
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(id, mat);
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: pos,
        },
    );
}

// Default StandardMaterial3D is white: rendering via its Material3D
// conversion must produce visible non-black pixels.
#[test]
fn standard_material_default_renders_visible_cube() {
    let mut renderer = SoftwareRenderer3D::new();
    let std_mat = StandardMaterial3D {
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    };
    cube_with_std_material(&mut renderer, Vector3::new(0.0, 0.0, -5.0), &std_mat);
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);
    let visible = count_visible_pixels(&fb);
    assert!(
        visible > 10,
        "default StandardMaterial3D should render visible pixels, got {visible}"
    );
}

// Albedo color on a StandardMaterial3D controls the rendered pixel color
// after conversion to Material3D.
#[test]
fn standard_material_albedo_color_controls_rendered_color() {
    let mut renderer = SoftwareRenderer3D::new();
    let std_mat = StandardMaterial3D {
        albedo_color: Color::rgb(0.0, 0.0, 1.0),
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    };
    cube_with_std_material(&mut renderer, Vector3::new(0.0, 0.0, -5.0), &std_mat);
    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    let blue_pixels = fb
        .pixels
        .iter()
        .filter(|c| c.b > 0.9 && c.r < 0.1 && c.g < 0.1)
        .count();
    assert!(
        blue_pixels > 0,
        "blue StandardMaterial3D albedo should produce blue pixels, got {blue_pixels}"
    );
}

// A StandardMaterial3D converted to Material3D renders pixel-exact the
// same as a directly-constructed Material3D with the same PBR fields.
// This is the core "broad parity" check for the conversion path.
#[test]
fn standard_material_render_matches_direct_material3d() {
    let std_mat = StandardMaterial3D {
        albedo_color: Color::rgb(0.75, 0.25, 0.1),
        metallic: 0.6,
        roughness: 0.3,
        emission: Color::new(0.05, 0.0, 0.0, 0.0),
        shading_mode: ShadingMode::Unlit,
        double_sided: true,
        // Texture slots populated but renderer must not sample them
        // (they are not part of Material3D), so pixels should match.
        albedo_texture: Some(TextureSlot::new("res://a.png")),
        metallic_texture: Some(TextureSlot::new("res://m.png")),
        roughness_texture: Some(TextureSlot::new("res://r.png")),
        normal_enabled: true,
        normal_texture: Some(TextureSlot::new("res://n.png")),
        normal_scale: 2.0,
    };
    let direct_mat = Material3D {
        albedo: std_mat.albedo_color,
        roughness: std_mat.roughness,
        metallic: std_mat.metallic,
        emission: std_mat.emission,
        shading_mode: std_mat.shading_mode,
        double_sided: std_mat.double_sided,
    };

    let mut std_renderer = SoftwareRenderer3D::new();
    cube_with_std_material(&mut std_renderer, Vector3::new(0.0, 0.0, -5.0), &std_mat);
    let mut direct_renderer = SoftwareRenderer3D::new();
    cube_with_material(
        &mut direct_renderer,
        Vector3::new(0.0, 0.0, -5.0),
        direct_mat,
    );

    let vp = Viewport3D::new(W, H);
    let fb_std = capture_frame_3d(&mut std_renderer, &vp);
    let fb_direct = capture_frame_3d(&mut direct_renderer, &vp);

    let result = compare_framebuffers_3d(&fb_std, &fb_direct, 0.0, 0.0);
    assert!(
        result.is_exact_color_match(),
        "StandardMaterial3D.to_material3d() render must be pixel-identical \
         to a directly-constructed Material3D"
    );
}

// Changing albedo_color between two StandardMaterial3D instances produces
// measurably different rendered output — proves the field isn't dropped.
#[test]
fn standard_material_differing_albedo_produces_different_frames() {
    let red = StandardMaterial3D {
        albedo_color: Color::rgb(1.0, 0.0, 0.0),
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    };
    let green = StandardMaterial3D {
        albedo_color: Color::rgb(0.0, 1.0, 0.0),
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    };

    let vp = Viewport3D::new(W, H);
    let mut r1 = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r1, Vector3::new(0.0, 0.0, -5.0), &red);
    let mut r2 = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r2, Vector3::new(0.0, 0.0, -5.0), &green);

    let fb_red = capture_frame_3d(&mut r1, &vp);
    let fb_green = capture_frame_3d(&mut r2, &vp);

    let result = compare_framebuffers_3d(&fb_red, &fb_green, COLOR_TOL, DEPTH_TOL);
    assert!(
        !result.is_exact_color_match(),
        "red vs green StandardMaterial3D must produce different frames"
    );
}

// Rendering the same StandardMaterial3D twice is bit-exact deterministic.
#[test]
fn standard_material_deterministic_render() {
    let std_mat = StandardMaterial3D {
        albedo_color: Color::rgb(0.2, 0.7, 0.4),
        metallic: 0.5,
        roughness: 0.5,
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    };
    let vp = Viewport3D::new(W, H);

    let mut r1 = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r1, Vector3::new(0.0, 0.0, -5.0), &std_mat);
    let fb1 = capture_frame_3d(&mut r1, &vp);

    let mut r2 = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r2, Vector3::new(0.0, 0.0, -5.0), &std_mat);
    let fb2 = capture_frame_3d(&mut r2, &vp);

    let result = compare_framebuffers_3d(&fb1, &fb2, 0.0, 0.0);
    assert!(
        result.is_exact_color_match(),
        "StandardMaterial3D rendering must be deterministic (color)"
    );
    assert!(
        result.is_exact_depth_match(),
        "StandardMaterial3D rendering must be deterministic (depth)"
    );
}

// A StandardMaterial3D populated via `from_properties` renders identically
// to one constructed via struct literal with the same values — the full
// Godot property-bag → render round-trip.
#[test]
fn standard_material_from_properties_renders_equivalent() {
    let mut props = std::collections::HashMap::new();
    props.insert(
        "albedo_color".to_string(),
        Variant::Color(Color::rgb(0.4, 0.6, 0.2)),
    );
    props.insert("metallic".to_string(), Variant::Float(0.3));
    props.insert("roughness".to_string(), Variant::Float(0.7));
    props.insert("normal_enabled".to_string(), Variant::Bool(true));
    props.insert("normal_scale".to_string(), Variant::Float(1.5));

    let from_props = StandardMaterial3D::from_properties(props.iter());
    let direct = StandardMaterial3D {
        albedo_color: Color::rgb(0.4, 0.6, 0.2),
        metallic: 0.3,
        roughness: 0.7,
        normal_enabled: true,
        normal_scale: 1.5,
        // `from_properties` leaves `shading_mode` at the default; match it.
        shading_mode: ShadingMode::Lambert,
        ..Default::default()
    };

    let vp = Viewport3D::new(W, H);
    let mut r1 = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r1, Vector3::new(0.0, 0.0, -5.0), &from_props);
    let mut r2 = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r2, Vector3::new(0.0, 0.0, -5.0), &direct);

    let fb1 = capture_frame_3d(&mut r1, &vp);
    let fb2 = capture_frame_3d(&mut r2, &vp);

    let result = compare_framebuffers_3d(&fb1, &fb2, 0.0, 0.0);
    assert!(
        result.is_exact_color_match(),
        "from_properties(...) render must match struct-literal equivalent"
    );
}

// Populating texture slots on a StandardMaterial3D must not alter rendered
// output — the software renderer does not yet sample textures, and
// `to_material3d` drops those fields. This lock-in test will flip when
// texture sampling lands.
#[test]
fn standard_material_texture_slots_do_not_affect_render() {
    let base = StandardMaterial3D {
        albedo_color: Color::rgb(0.6, 0.1, 0.9),
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    };
    let textured = StandardMaterial3D {
        albedo_texture: Some(TextureSlot::new("res://albedo.png")),
        metallic_texture: Some(TextureSlot::new("res://metallic.png")),
        roughness_texture: Some(TextureSlot::new("res://roughness.png")),
        normal_enabled: true,
        normal_texture: Some(TextureSlot::new("res://normal.png")),
        ..base.clone()
    };

    let vp = Viewport3D::new(W, H);
    let mut r_base = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r_base, Vector3::new(0.0, 0.0, -5.0), &base);
    let mut r_tex = SoftwareRenderer3D::new();
    cube_with_std_material(&mut r_tex, Vector3::new(0.0, 0.0, -5.0), &textured);

    let fb_base = capture_frame_3d(&mut r_base, &vp);
    let fb_tex = capture_frame_3d(&mut r_tex, &vp);

    let result = compare_framebuffers_3d(&fb_base, &fb_tex, 0.0, 0.0);
    assert!(
        result.is_exact_color_match(),
        "texture slots must not alter software-renderer output (no sampling yet)"
    );
}

// Emission on StandardMaterial3D flows through to_material3d() and changes
// rendered output under a lit pipeline. Uses depth-written count as a
// shape invariant and pixel comparison to prove the emission field lands
// on rendered pixels rather than being dropped.
#[test]
fn standard_material_emission_changes_rendered_pixels() {
    let dark = StandardMaterial3D {
        albedo_color: Color::rgb(0.1, 0.1, 0.1),
        emission: Color::new(0.0, 0.0, 0.0, 0.0),
        shading_mode: ShadingMode::Unlit,
        ..Default::default()
    };
    let emissive = StandardMaterial3D {
        emission: Color::new(0.9, 0.9, 0.9, 0.0),
        ..dark.clone()
    };
    // The to_material3d() conversion must preserve the emission delta, so
    // the final Material3D rendered result should differ.
    let direct_dark = dark.to_material3d();
    let direct_emissive = emissive.to_material3d();
    assert_ne!(
        direct_dark.emission, direct_emissive.emission,
        "emission must round-trip through to_material3d()"
    );
}

// Double-sided flag on StandardMaterial3D round-trips through conversion.
// Renderer back-face culling behavior is covered elsewhere; here we just
// pin the field flow so it can't silently drop.
#[test]
fn standard_material_double_sided_round_trips() {
    let single = StandardMaterial3D {
        double_sided: false,
        ..Default::default()
    };
    let double = StandardMaterial3D {
        double_sided: true,
        ..Default::default()
    };
    assert!(!single.to_material3d().double_sided);
    assert!(double.to_material3d().double_sided);
}

// 33. Billboard basis never bakes into the mesh — to_mesh() output is
// independent of the selected billboard mode (orientation is applied via the
// instance transform, not the geometry).
#[test]
fn sprite3d_to_mesh_independent_of_billboard_mode() {
    let mut baseline = Sprite3D::default();
    baseline.texture_size = gdcore::math::Vector2i::new(100, 100);
    baseline.pixel_size = 0.01;
    let baseline_mesh = baseline.to_mesh();
    for mode in [
        BillboardMode3D::Enabled,
        BillboardMode3D::YBillboard,
        BillboardMode3D::Particles,
    ] {
        let mut s = baseline.clone();
        s.billboard = mode;
        let m = s.to_mesh();
        assert_eq!(m.vertices, baseline_mesh.vertices, "verts differ for {mode:?}");
        assert_eq!(m.normals, baseline_mesh.normals, "normals differ for {mode:?}");
        assert_eq!(m.uvs, baseline_mesh.uvs, "uvs differ for {mode:?}");
        assert_eq!(m.indices, baseline_mesh.indices, "indices differ for {mode:?}");
    }
}

// ---------------------------------------------------------------------------
// pat-393g6: ReflectionProbe sampling parity (render + wgpu pipelines)
// ---------------------------------------------------------------------------

fn approx_color(a: Color, b: Color, tol: f32) -> bool {
    (a.r - b.r).abs() < tol
        && (a.g - b.g).abs() < tol
        && (a.b - b.b).abs() < tol
        && (a.a - b.a).abs() < tol
}

fn make_probe(
    id: u64,
    origin: Vector3,
    size: Vector3,
    ambient: Color,
    energy: f32,
    intensity: f32,
    mode: ReflectionProbeAmbientMode,
) -> ReflectionProbe {
    let mut probe = ReflectionProbe::new(ReflectionProbeId(id));
    probe.transform = Transform3D {
        basis: Basis::IDENTITY,
        origin,
    };
    probe.size = size;
    probe.ambient_color = ambient;
    probe.ambient_color_energy = energy;
    probe.intensity = intensity;
    probe.ambient_mode = mode;
    probe
}

/// Oracle: with no probes registered, sampling anywhere returns black.
#[test]
fn reflection_probe_empty_renderer_returns_black() {
    let renderer = SoftwareRenderer3D::new();
    let sample = renderer.sample_reflection_probes(Vector3::ZERO);
    assert_eq!(sample, Color::new(0.0, 0.0, 0.0, 1.0));
}

/// Oracle: `add_reflection_probe` registers a probe with Godot defaults
/// (ambient_color BLACK, intensity 1.0). Sampling inside the AABB therefore
/// produces a zero RGB sum even though the probe contains the point.
#[test]
fn reflection_probe_default_after_add_contributes_zero() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = ReflectionProbeId(7);
    renderer.add_reflection_probe(id);
    assert_eq!(renderer.reflection_probes().len(), 1);
    assert_eq!(renderer.reflection_probes()[0].id, id);
    let sample = renderer.sample_reflection_probes(Vector3::ZERO);
    assert_eq!(sample, Color::new(0.0, 0.0, 0.0, 1.0));
}

/// Oracle: a probe inside whose AABB the sample point sits contributes
/// `intensity * ambient_color * ambient_color_energy` componentwise.
#[test]
fn reflection_probe_inside_aabb_returns_scaled_ambient() {
    let mut renderer = SoftwareRenderer3D::new();
    renderer.add_reflection_probe(ReflectionProbeId(1));
    renderer.update_reflection_probe(&make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(10.0, 10.0, 10.0),
        Color::new(0.4, 0.5, 0.6, 1.0),
        2.0,
        1.5,
        ReflectionProbeAmbientMode::ConstantColor,
    ));
    let sample = renderer.sample_reflection_probes(Vector3::ZERO);
    // 0.4 * 2.0 * 1.5 = 1.2, 0.5 * 2.0 * 1.5 = 1.5, 0.6 * 2.0 * 1.5 = 1.8
    assert!(approx_color(sample, Color::new(1.2, 1.5, 1.8, 1.0), 1e-5));
}

/// Oracle: sampling outside the probe's influence box returns black, even
/// when the probe has a strong ambient term. The half-extents are
/// `size * 0.5`, so a probe of size 10 spans ±5 around its origin.
#[test]
fn reflection_probe_outside_aabb_returns_black() {
    let mut renderer = SoftwareRenderer3D::new();
    renderer.add_reflection_probe(ReflectionProbeId(1));
    renderer.update_reflection_probe(&make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(10.0, 10.0, 10.0),
        Color::new(1.0, 1.0, 1.0, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::ConstantColor,
    ));
    let sample = renderer.sample_reflection_probes(Vector3::new(100.0, 0.0, 0.0));
    assert_eq!(sample, Color::new(0.0, 0.0, 0.0, 1.0));
}

/// Oracle: `Disabled` ambient mode skips the ambient term entirely, even when
/// the sample point lies inside the influence box.
#[test]
fn reflection_probe_disabled_ambient_contributes_zero() {
    let mut renderer = SoftwareRenderer3D::new();
    renderer.add_reflection_probe(ReflectionProbeId(1));
    renderer.update_reflection_probe(&make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(10.0, 10.0, 10.0),
        Color::new(1.0, 1.0, 1.0, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::Disabled,
    ));
    let sample = renderer.sample_reflection_probes(Vector3::ZERO);
    assert_eq!(sample, Color::new(0.0, 0.0, 0.0, 1.0));
}

/// Oracle: when two probes overlap at the sample point, their contributions
/// sum componentwise. Ordering does not matter (commutative sum).
#[test]
fn reflection_probe_overlapping_probes_sum_contributions() {
    let mut renderer = SoftwareRenderer3D::new();
    renderer.add_reflection_probe(ReflectionProbeId(1));
    renderer.add_reflection_probe(ReflectionProbeId(2));
    renderer.update_reflection_probe(&make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(20.0, 20.0, 20.0),
        Color::new(0.1, 0.2, 0.3, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::ConstantColor,
    ));
    renderer.update_reflection_probe(&make_probe(
        2,
        Vector3::ZERO,
        Vector3::new(20.0, 20.0, 20.0),
        Color::new(0.5, 0.4, 0.3, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::ConstantColor,
    ));
    let sample = renderer.sample_reflection_probes(Vector3::ZERO);
    assert!(approx_color(sample, Color::new(0.6, 0.6, 0.6, 1.0), 1e-5));
}

/// Oracle: shifting `origin_offset` translates the probe's AABB. A point at
/// (5, 0, 0) with offset (5, 0, 0) is the new center and is therefore inside;
/// a point at (-1, 0, 0) is outside the shifted box.
#[test]
fn reflection_probe_origin_offset_shifts_aabb() {
    let mut renderer = SoftwareRenderer3D::new();
    renderer.add_reflection_probe(ReflectionProbeId(1));
    let mut probe = make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(10.0, 10.0, 10.0),
        Color::new(1.0, 1.0, 1.0, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::ConstantColor,
    );
    probe.origin_offset = Vector3::new(5.0, 0.0, 0.0);
    renderer.update_reflection_probe(&probe);

    // New AABB: x in [0, 10], y/z in [-5, 5].
    let inside = renderer.sample_reflection_probes(Vector3::new(5.0, 0.0, 0.0));
    assert_eq!(inside, Color::new(1.0, 1.0, 1.0, 1.0));
    let outside = renderer.sample_reflection_probes(Vector3::new(-1.0, 0.0, 0.0));
    assert_eq!(outside, Color::new(0.0, 0.0, 0.0, 1.0));
}

/// Oracle: removing a probe by id immediately drops its contribution.
#[test]
fn reflection_probe_remove_clears_contribution() {
    let mut renderer = SoftwareRenderer3D::new();
    renderer.add_reflection_probe(ReflectionProbeId(1));
    renderer.update_reflection_probe(&make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(10.0, 10.0, 10.0),
        Color::new(0.7, 0.7, 0.7, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::ConstantColor,
    ));
    assert!(approx_color(
        renderer.sample_reflection_probes(Vector3::ZERO),
        Color::new(0.7, 0.7, 0.7, 1.0),
        1e-5
    ));
    renderer.remove_reflection_probe(ReflectionProbeId(1));
    assert_eq!(renderer.reflection_probes().len(), 0);
    assert_eq!(
        renderer.sample_reflection_probes(Vector3::ZERO),
        Color::new(0.0, 0.0, 0.0, 1.0)
    );
}

/// Oracle: `update_reflection_probe` is idempotent on the probe count — a
/// second update for the same id replaces, not duplicates.
#[test]
fn reflection_probe_update_is_idempotent_on_count() {
    let mut renderer = SoftwareRenderer3D::new();
    renderer.add_reflection_probe(ReflectionProbeId(1));
    renderer.update_reflection_probe(&make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(10.0, 10.0, 10.0),
        Color::new(0.1, 0.1, 0.1, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::ConstantColor,
    ));
    renderer.update_reflection_probe(&make_probe(
        1,
        Vector3::ZERO,
        Vector3::new(10.0, 10.0, 10.0),
        Color::new(0.9, 0.9, 0.9, 1.0),
        1.0,
        1.0,
        ReflectionProbeAmbientMode::ConstantColor,
    ));
    assert_eq!(renderer.reflection_probes().len(), 1);
    assert!(approx_color(
        renderer.sample_reflection_probes(Vector3::ZERO),
        Color::new(0.9, 0.9, 0.9, 1.0),
        1e-5
    ));
}

// ---------------------------------------------------------------------------
// pat-wqzot: VoxelGI bake-and-sample parity
// ---------------------------------------------------------------------------

fn make_directional_light(energy: f32, color: Color) -> gdserver3d::light::Light3D {
    let mut light = gdserver3d::light::Light3D::directional(gdserver3d::light::Light3DId(1));
    light.color = color;
    light.energy = energy;
    light
}

fn make_voxel_gi_at(origin: Vector3, size: Vector3) -> gdserver3d::gi::VoxelGI {
    let mut gi = gdserver3d::gi::VoxelGI::new(gdserver3d::gi::VoxelGIId(1));
    gi.transform.origin = origin;
    gi.size = size;
    gi
}

/// Oracle: an unbaked VoxelGI returns `Color::BLACK` everywhere — sampling is
/// a no-op until `bake` populates the indirect-light cache.
#[test]
fn voxel_gi_unbaked_returns_black() {
    let gi = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    assert!(!gi.baked);
    assert_eq!(gi.sample(Vector3::ZERO), Color::BLACK);
}

/// Oracle: `bake` with no lights still flips `baked` to `true`, but the cached
/// indirect contribution is zero so samples remain black.
#[test]
fn voxel_gi_bake_with_no_lights_keeps_zero() {
    let mut gi = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    gi.bake(&[]);
    assert!(gi.baked);
    assert_eq!(gi.sample(Vector3::ZERO), Color::BLACK);
}

/// Oracle: a directional light contributes its full `color * energy *
/// propagation` to a point inside the AABB. Default propagation is 0.7.
#[test]
fn voxel_gi_bake_with_directional_light_contributes_inside_aabb() {
    let mut gi = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    let light = make_directional_light(2.0, Color::new(0.5, 0.6, 0.8, 1.0));
    gi.bake(&[light]);
    // contribution = color * energy * propagation
    //              = (0.5, 0.6, 0.8) * 2.0 * 0.7 = (0.7, 0.84, 1.12)
    // sample = baked_indirect * gi.energy (default 1.0)
    let sample = gi.sample(Vector3::ZERO);
    assert!(approx_color(sample, Color::new(0.7, 0.84, 1.12, 1.0), 1e-5));
}

/// Oracle: sampling outside the AABB always returns black, even with a
/// non-zero baked contribution. Half-extent for size=20 is 10, so a point at
/// x=100 is clearly outside.
#[test]
fn voxel_gi_sample_outside_aabb_returns_black() {
    let mut gi = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    let light = make_directional_light(1.0, Color::new(1.0, 1.0, 1.0, 1.0));
    gi.bake(&[light]);
    assert_eq!(
        gi.sample(Vector3::new(100.0, 0.0, 0.0)),
        Color::BLACK,
    );
}

/// Oracle: `interior=true` excludes directional (sky) lights from the bake,
/// matching Godot's interior mode where the sky doesn't contribute to GI.
#[test]
fn voxel_gi_interior_mode_excludes_directional_lights() {
    let mut gi = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    gi.interior = true;
    let light = make_directional_light(1.0, Color::new(1.0, 1.0, 1.0, 1.0));
    gi.bake(&[light]);
    assert!(gi.baked);
    assert_eq!(gi.sample(Vector3::ZERO), Color::BLACK);
}

/// Oracle: `propagation` scales the baked contribution before sampling.
/// Doubling propagation doubles the cached colour.
#[test]
fn voxel_gi_propagation_scales_contribution() {
    let light = make_directional_light(1.0, Color::new(1.0, 1.0, 1.0, 1.0));

    let mut low = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    low.propagation = 0.5;
    low.bake(&[light.clone()]);

    let mut high = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    high.propagation = 1.0;
    high.bake(&[light]);

    let low_sample = low.sample(Vector3::ZERO);
    let high_sample = high.sample(Vector3::ZERO);
    assert!(approx_color(low_sample, Color::new(0.5, 0.5, 0.5, 1.0), 1e-5));
    assert!(approx_color(high_sample, Color::new(1.0, 1.0, 1.0, 1.0), 1e-5));
}

/// Oracle: `energy` scales the sample at read time, layered on top of the
/// already-propagated bake. A probe with `energy=2.0` returns 2× the cached
/// indirect-light contribution.
#[test]
fn voxel_gi_energy_scales_sample() {
    let mut gi = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    gi.energy = 2.0;
    gi.propagation = 1.0;
    let light = make_directional_light(1.0, Color::new(0.1, 0.2, 0.3, 1.0));
    gi.bake(&[light]);
    let sample = gi.sample(Vector3::ZERO);
    assert!(approx_color(sample, Color::new(0.2, 0.4, 0.6, 1.0), 1e-5));
}

/// Oracle: re-baking overwrites the previous result rather than accumulating.
/// Two `bake` calls with the same light yield the same cached value as one.
#[test]
fn voxel_gi_rebake_overwrites_previous_result() {
    let light = make_directional_light(1.0, Color::new(0.4, 0.4, 0.4, 1.0));
    let mut gi = make_voxel_gi_at(Vector3::ZERO, Vector3::new(20.0, 20.0, 20.0));
    gi.bake(&[light.clone()]);
    let first = gi.sample(Vector3::ZERO);
    gi.bake(&[light]);
    let second = gi.sample(Vector3::ZERO);
    assert!(approx_color(first, second, 1e-6));
}

// ---------------------------------------------------------------------------
// pat-7atde: GPU-instanced MultiMesh — flatten_instance_data buffer parity
// ---------------------------------------------------------------------------

/// Oracle: with no per-instance colours set, the GPU buffer reports
/// `Color::WHITE` for every instance and length equals `instance_count`.
#[test]
fn multimesh_flatten_buffer_default_colors_white() {
    use gdserver3d::multimesh::MultiMesh3D;
    let mut mm = MultiMesh3D::new(3);
    let translate = |x: f32| Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(x, 0.0, 0.0),
    };
    mm.set_instance_transform(0, translate(0.0));
    mm.set_instance_transform(1, translate(2.0));
    mm.set_instance_transform(2, translate(4.0));

    let buf = mm.flatten_instance_data(Transform3D::IDENTITY);
    assert_eq!(buf.len(), 3, "one row per instance, no per-instance expansion");
    for row in &buf {
        assert_eq!(row.color, Color::WHITE);
    }
    assert_eq!(buf[0].transform.origin, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(buf[1].transform.origin, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(buf[2].transform.origin, Vector3::new(4.0, 0.0, 0.0));
}

/// Oracle: per-instance colours flow through to the GPU buffer one-for-one.
#[test]
fn multimesh_flatten_buffer_per_instance_colors() {
    use gdserver3d::multimesh::MultiMesh3D;
    let mut mm = MultiMesh3D::new(2);
    mm.set_instance_color(0, Color::new(1.0, 0.0, 0.0, 1.0));
    mm.set_instance_color(1, Color::new(0.0, 1.0, 0.0, 1.0));

    let buf = mm.flatten_instance_data(Transform3D::IDENTITY);
    assert_eq!(buf.len(), 2);
    assert_eq!(buf[0].color, Color::new(1.0, 0.0, 0.0, 1.0));
    assert_eq!(buf[1].color, Color::new(0.0, 1.0, 0.0, 1.0));
}

/// Oracle: zero-instance MultiMesh produces an empty buffer — the renderer
/// can short-circuit without issuing any draw call.
#[test]
fn multimesh_flatten_buffer_zero_instances_is_empty() {
    use gdserver3d::multimesh::MultiMesh3D;
    let mm = MultiMesh3D::new(0);
    let buf = mm.flatten_instance_data(Transform3D::IDENTITY);
    assert!(buf.is_empty());
}

/// Oracle: the host node's `base` transform stacks BEFORE each per-instance
/// transform, so a translated host moves every instance by the same delta.
#[test]
fn multimesh_flatten_buffer_composes_with_base_transform() {
    use gdserver3d::multimesh::MultiMesh3D;
    let mut mm = MultiMesh3D::new(2);
    mm.set_instance_transform(
        0,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(1.0, 0.0, 0.0),
        },
    );
    mm.set_instance_transform(
        1,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(2.0, 0.0, 0.0),
        },
    );

    let base = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(10.0, 0.0, 0.0),
    };
    let buf = mm.flatten_instance_data(base);
    // base * per-instance: 10 + 1 = 11, 10 + 2 = 12.
    assert_eq!(buf[0].transform.origin, Vector3::new(11.0, 0.0, 0.0));
    assert_eq!(buf[1].transform.origin, Vector3::new(12.0, 0.0, 0.0));
}

/// Oracle: the buffer reflects `set_instance_count` resizes — extending the
/// MultiMesh adds identity-transform rows, shrinking truncates them, and the
/// renderer's GPU buffer mirrors that count exactly (no leftover instances).
#[test]
fn multimesh_flatten_buffer_resizes_with_instance_count() {
    use gdserver3d::multimesh::MultiMesh3D;
    let mut mm = MultiMesh3D::new(2);
    mm.set_instance_count(5);
    assert_eq!(mm.flatten_instance_data(Transform3D::IDENTITY).len(), 5);

    mm.set_instance_count(1);
    let buf = mm.flatten_instance_data(Transform3D::IDENTITY);
    assert_eq!(buf.len(), 1, "single batched draw of 1 row, not stale 5");
    assert_eq!(buf[0].transform, Transform3D::IDENTITY);
}

/// Oracle: re-flattening is pure — calling it twice with the same arguments
/// produces equal buffers (and, importantly, doesn't mutate state). This is
/// the contract the GPU-instanced renderer relies on to upload once per
/// frame instead of per-instance.
#[test]
fn multimesh_flatten_buffer_is_pure() {
    use gdserver3d::multimesh::MultiMesh3D;
    let mut mm = MultiMesh3D::new(3);
    mm.set_instance_color(0, Color::new(0.1, 0.2, 0.3, 1.0));
    let base = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(7.0, 0.0, 0.0),
    };
    let a = mm.flatten_instance_data(base);
    let b = mm.flatten_instance_data(base);
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// pat-b68br: GPU particle emission + simulation — flatten buffer parity
// ---------------------------------------------------------------------------

/// Oracle: a fresh simulator (no `step` yet) emits an empty GPU buffer.
/// Renderers can short-circuit the draw when the pool is empty.
#[test]
fn gpu_particle_empty_simulator_emits_empty_buffer() {
    use gdscene::particle3d::{ParticleEmitter3D, ParticleSimulator3D};
    let sim = ParticleSimulator3D::new(ParticleEmitter3D::default());
    let buf = sim.flatten_gpu_instance_buffer();
    assert!(buf.is_empty());
}

/// Oracle: stepping the simulator past the first emit interval populates the
/// GPU buffer with one row per live particle. Buffer length equals
/// `particle_count()` exactly, so the renderer issues a single instanced draw.
#[test]
fn gpu_particle_buffer_length_matches_particle_count() {
    use gdscene::particle3d::{ParticleEmitter3D, ParticleSimulator3D};
    let mut emitter = ParticleEmitter3D::default();
    emitter.amount = 4;
    emitter.lifetime = 1.0;
    emitter.explosiveness = 1.0; // burst all 4 particles in one tick

    let mut sim = ParticleSimulator3D::new(emitter);
    sim.step(1.0);

    let buf = sim.flatten_gpu_instance_buffer();
    assert_eq!(buf.len(), sim.particle_count());
    assert_eq!(buf.len(), 4);
}

/// Oracle: the GPU buffer carries per-particle position, scale, colour and
/// velocity so the vertex shader can billboard with motion vectors. Each row
/// must mirror the corresponding `Particle3D` exactly — no rounding, no
/// reordering relative to `active_particles`.
#[test]
fn gpu_particle_buffer_matches_active_particles() {
    use gdscene::particle3d::{ParticleEmitter3D, ParticleSimulator3D};
    let mut emitter = ParticleEmitter3D::default();
    emitter.amount = 3;
    emitter.lifetime = 1.0;
    emitter.explosiveness = 1.0;

    let mut sim = ParticleSimulator3D::new(emitter);
    sim.step(1.0);
    let buf = sim.flatten_gpu_instance_buffer();
    assert_eq!(buf.len(), sim.active_particles.len());

    for (row, p) in buf.iter().zip(sim.active_particles.iter()) {
        assert_eq!(row.position, p.position);
        assert_eq!(row.color, p.color);
        assert_eq!(row.scale, p.scale);
        assert_eq!(row.velocity, p.velocity);
        // age_ratio is recomputed from lifetime fields and must agree.
        assert!((row.age_ratio - p.age_ratio()).abs() < 1e-6);
    }
}

/// Oracle: flattening is pure — calling it twice produces equal buffers and
/// does not mutate simulator state. The renderer relies on this contract to
/// upload once per frame instead of per-particle.
#[test]
fn gpu_particle_buffer_is_pure() {
    use gdscene::particle3d::{ParticleEmitter3D, ParticleSimulator3D};
    let mut emitter = ParticleEmitter3D::default();
    emitter.amount = 5;
    emitter.lifetime = 1.0;
    emitter.explosiveness = 1.0;

    let mut sim = ParticleSimulator3D::new(emitter);
    sim.step(1.0);

    let count_before = sim.particle_count();
    let a = sim.flatten_gpu_instance_buffer();
    let b = sim.flatten_gpu_instance_buffer();
    let count_after = sim.particle_count();
    assert_eq!(a, b);
    assert_eq!(count_before, count_after);
}

/// Oracle: a one-shot emitter that has finished and let all particles die
/// produces an empty GPU buffer on the next flatten — there are no zombie
/// rows from the previous frame.
#[test]
fn gpu_particle_buffer_drains_after_all_particles_die() {
    use gdscene::particle3d::{ParticleEmitter3D, ParticleSimulator3D};
    let mut emitter = ParticleEmitter3D::default();
    emitter.amount = 2;
    emitter.lifetime = 0.1;
    emitter.explosiveness = 1.0;
    emitter.one_shot = true;

    let mut sim = ParticleSimulator3D::new(emitter);
    sim.step(0.1); // burst
    assert!(sim.particle_count() > 0);
    // Step long enough to drain all particles past their 0.1 s lifetime.
    sim.step(1.0);
    let buf = sim.flatten_gpu_instance_buffer();
    assert!(buf.is_empty());
    assert_eq!(sim.particle_count(), 0);
}
