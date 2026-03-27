//! Integration tests for the 3D render pipeline vertex/fragment shader path
//! and shader material support (pat-wdtx0).
//!
//! Validates that:
//! 1. `set_shader_material` wires custom shaders into the render pipeline
//! 2. Custom `ShaderMaterial3D` overrides standard material colors
//! 3. Unshaded render mode bypasses lighting
//! 4. The full vertex → rasterize → fragment pipeline produces correct output
//! 5. Shader materials with uniforms affect rendered output

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::renderer::FrameBuffer3D;
use gdrender3d::test_adapter::{capture_frame_3d, count_visible_pixels};
use gdrender3d::SoftwareRenderer3D;
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
use gdserver3d::viewport::Viewport3D;
use gdvariant::variant::Variant;

const W: u32 = 64;
const H: u32 = 64;

fn count_colored_pixels(fb: &FrameBuffer3D, pred: impl Fn(&Color) -> bool) -> usize {
    fb.pixels.iter().filter(|c| pred(c)).count()
}

// ── 1. set_shader_material stores and activates the shader material ──

#[test]
fn shader_material_overrides_standard_material_color() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));

    // Set a red standard material.
    let mat = Material3D {
        albedo: Color::new(1.0, 0.0, 0.0, 1.0),
        shading_mode: ShadingMode::Unlit,
        ..Material3D::default()
    };
    renderer.set_material(id, mat);

    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    // Render without shader material — should be red.
    let vp = Viewport3D::new(W, H);
    let fb_no_shader = capture_frame_3d(&mut renderer, &vp);
    let red_count = count_colored_pixels(&fb_no_shader, |c| c.r > 0.9 && c.g < 0.1 && c.b < 0.1);
    assert!(
        red_count > 50,
        "without shader material, cube should be red ({red_count} red pixels)"
    );

    // Now attach a shader material that overrides to green via albedo_color uniform.
    let mut shader_mat = ShaderMaterial3D::new();
    let shader = Shader3D::new(
        ShaderType3D::Spatial,
        "shader_type spatial;\nrender_mode unshaded;\nuniform vec4 albedo_color : source_color;",
    );
    shader_mat.shader = Some(shader);
    shader_mat.set_shader_parameter("albedo_color", Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)));
    renderer.set_shader_material(id, shader_mat);

    // Render with shader material — should be green.
    let fb_with_shader = capture_frame_3d(&mut renderer, &vp);
    let green_count =
        count_colored_pixels(&fb_with_shader, |c| c.g > 0.9 && c.r < 0.1 && c.b < 0.1);
    assert!(
        green_count > 50,
        "with shader material, cube should be green ({green_count} green pixels)"
    );

    // Verify red pixels are gone.
    let red_after = count_colored_pixels(&fb_with_shader, |c| c.r > 0.9 && c.g < 0.1 && c.b < 0.1);
    assert_eq!(
        red_after, 0,
        "shader material should fully override standard material color"
    );
}

// ── 2. Unshaded shader material bypasses lighting ──

#[test]
fn unshaded_shader_material_ignores_lights() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(id, Material3D::default());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    // Attach an unshaded shader material with a specific color.
    let mut shader_mat = ShaderMaterial3D::new();
    let shader = Shader3D::new(
        ShaderType3D::Spatial,
        "shader_type spatial;\nrender_mode unshaded;\nuniform vec4 albedo_color : source_color;",
    );
    shader_mat.shader = Some(shader);
    shader_mat.set_shader_parameter(
        "albedo_color",
        Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)),
    );
    renderer.set_shader_material(id, shader_mat);

    // Add a bright directional light.
    use gdserver3d::light::Light3DId;
    renderer.add_light(Light3DId(1));

    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    // Unshaded means the exact albedo_color should appear, not affected by lighting.
    let exact_gray = count_colored_pixels(&fb, |c| {
        (c.r - 0.5).abs() < 0.05 && (c.g - 0.5).abs() < 0.05 && (c.b - 0.5).abs() < 0.05
    });
    assert!(
        exact_gray > 50,
        "unshaded shader should produce uniform gray regardless of lights ({exact_gray} matching pixels)"
    );
}

// ── 3. Shader material with no shader falls back gracefully ──

#[test]
fn shader_material_without_shader_does_not_crash() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(
        id,
        Material3D {
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            shading_mode: ShadingMode::Unlit,
            ..Material3D::default()
        },
    );
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    // Attach a shader material with no actual shader.
    let shader_mat = ShaderMaterial3D::new();
    renderer.set_shader_material(id, shader_mat);

    let vp = Viewport3D::new(W, H);
    // Should not panic — graceful fallback.
    let fb = capture_frame_3d(&mut renderer, &vp);
    let visible = count_visible_pixels(&fb);
    assert!(visible > 0, "should render something even with empty shader material");
}

// ── 4. Vertex shader correctly transforms geometry ──

#[test]
fn vertex_shader_transforms_affect_screen_position() {
    let mut renderer = SoftwareRenderer3D::new();

    // Place cube slightly off to the left (within the frustum at z=-8).
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(
        id,
        Material3D {
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            shading_mode: ShadingMode::Unlit,
            ..Material3D::default()
        },
    );
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(-2.0, 0.0, -8.0),
        },
    );

    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    // Count red pixels in left half vs right half.
    let mut left_red = 0;
    let mut right_red = 0;
    for y in 0..H {
        for x in 0..W {
            let c = fb.pixels[(y * W + x) as usize];
            if c.r > 0.9 && c.g < 0.1 {
                if x < W / 2 {
                    left_red += 1;
                } else {
                    right_red += 1;
                }
            }
        }
    }

    assert!(
        left_red > 0,
        "cube should be visible ({left_red} left, {right_red} right)"
    );
    assert!(
        left_red > right_red,
        "cube at x=-2 should have more pixels on the left side ({left_red} left vs {right_red} right)"
    );
}

// ── 5. Multiple shading modes produce different results ──

#[test]
fn different_shading_modes_produce_different_output() {
    use gdserver3d::light::Light3DId;

    let vp = Viewport3D::new(W, H);
    let pos = Vector3::new(0.0, 0.0, -5.0);
    let transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: pos,
    };

    // Render with Unlit.
    let mut r_unlit = SoftwareRenderer3D::new();
    let id = r_unlit.create_instance();
    r_unlit.set_mesh(id, Mesh3D::cube(1.0));
    r_unlit.set_material(
        id,
        Material3D {
            albedo: Color::WHITE,
            shading_mode: ShadingMode::Unlit,
            ..Material3D::default()
        },
    );
    r_unlit.set_transform(id, transform);
    r_unlit.add_light(Light3DId(1));
    let fb_unlit = capture_frame_3d(&mut r_unlit, &vp);

    // Render with Lambert.
    let mut r_lambert = SoftwareRenderer3D::new();
    let id = r_lambert.create_instance();
    r_lambert.set_mesh(id, Mesh3D::cube(1.0));
    r_lambert.set_material(
        id,
        Material3D {
            albedo: Color::WHITE,
            shading_mode: ShadingMode::Lambert,
            ..Material3D::default()
        },
    );
    r_lambert.set_transform(id, transform);
    r_lambert.add_light(Light3DId(1));
    let fb_lambert = capture_frame_3d(&mut r_lambert, &vp);

    // The pixel data should differ because Lambert applies lighting.
    let mut differences = 0;
    for i in 0..fb_unlit.pixels.len() {
        let u = &fb_unlit.pixels[i];
        let l = &fb_lambert.pixels[i];
        if (u.r - l.r).abs() > 0.01 || (u.g - l.g).abs() > 0.01 || (u.b - l.b).abs() > 0.01 {
            differences += 1;
        }
    }

    assert!(
        differences > 10,
        "Unlit vs Lambert should produce different pixels ({differences} differ)"
    );
}

// ── 6. Depth buffer correctness with shader pipeline ──

#[test]
fn shader_pipeline_writes_correct_depth() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(
        id,
        Material3D {
            albedo: Color::WHITE,
            shading_mode: ShadingMode::Unlit,
            ..Material3D::default()
        },
    );
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    // Every visible pixel should have depth < 1.0 (written).
    let visible = count_visible_pixels(&fb);
    let depth_written = fb.depth.iter().filter(|d| **d < 1.0).count();
    assert!(visible > 0);
    assert_eq!(
        visible, depth_written,
        "every visible pixel must have depth written"
    );

    // Depth values should be in valid NDC range.
    for d in &fb.depth {
        assert!(
            *d >= 0.0 && *d <= 1.0,
            "depth {d} should be in [0, 1]"
        );
    }
}

// ── 7. Perspective-correct interpolation produces smooth gradients ──

#[test]
fn perspective_correct_interpolation_smooth_uv() {
    // Render a sphere with Phong shading and verify the output has smooth
    // gradients (no flat-color banding across the whole surface).
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::sphere(1.0, 16));
    renderer.set_material(
        id,
        Material3D {
            albedo: Color::WHITE,
            shading_mode: ShadingMode::Phong,
            roughness: 0.5,
            metallic: 0.3,
            ..Material3D::default()
        },
    );
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -3.0),
        },
    );

    use gdserver3d::light::{Light3D, Light3DId};
    let mut light = Light3D::directional(Light3DId(1));
    light.direction = Vector3::new(-1.0, -1.0, -1.0);
    light.energy = 1.0;
    renderer.add_light(Light3DId(1));
    renderer.update_light(&light);

    let vp = Viewport3D::new(W, H);
    let fb = capture_frame_3d(&mut renderer, &vp);

    // Collect unique non-black brightness values to check for smooth gradation.
    let mut brightness_values: Vec<f32> = fb
        .pixels
        .iter()
        .filter(|c| **c != Color::BLACK)
        .map(|c| c.r + c.g + c.b)
        .collect();
    brightness_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    brightness_values.dedup_by(|a, b| (*a - *b).abs() < 0.01);

    // A Phong-lit sphere should produce many distinct brightness levels.
    assert!(
        brightness_values.len() > 5,
        "Phong-lit sphere should have smooth gradients, got {} distinct levels",
        brightness_values.len()
    );
}

// ── 8. Deterministic rendering with shader material ──

#[test]
fn shader_material_rendering_is_deterministic() {
    let vp = Viewport3D::new(W, H);

    let setup = || {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));
        renderer.set_material(id, Material3D::default());
        renderer.set_transform(
            id,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.0, -5.0),
            },
        );

        let mut shader_mat = ShaderMaterial3D::new();
        let shader = Shader3D::new(
            ShaderType3D::Spatial,
            "shader_type spatial;\nrender_mode unshaded;\nuniform vec4 albedo_color : source_color;",
        );
        shader_mat.shader = Some(shader);
        shader_mat.set_shader_parameter(
            "albedo_color",
            Variant::Color(Color::new(0.3, 0.6, 0.9, 1.0)),
        );
        renderer.set_shader_material(id, shader_mat);
        renderer
    };

    let mut r1 = setup();
    let mut r2 = setup();
    let fb1 = capture_frame_3d(&mut r1, &vp);
    let fb2 = capture_frame_3d(&mut r2, &vp);

    assert_eq!(
        fb1.pixels, fb2.pixels,
        "shader material rendering must be deterministic"
    );
    assert_eq!(
        fb1.depth, fb2.depth,
        "shader material depth must be deterministic"
    );
}
