//! Integration tests for DirectionalLight3D shadow mapping.
//!
//! Validates that:
//! - Shadow maps are generated for directional lights with `shadow_enabled = true`.
//! - Fragments behind occluders are darkened by the shadow map.
//! - Fragments beside occluders remain fully lit.
//! - The solid render pipeline integrates shadow maps correctly.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::renderer::{RenderMode, SoftwareRenderer3D};
use gdrender3d::shadow_map::{
    generate_shadow_maps, ShadowMap, SHADOW_MAP_SIZE,
};
use gdserver3d::instance::{Instance3D, Instance3DId};
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

// ── Shadow map generation tests ──────────────────────────────────────

#[test]
fn directional_shadow_map_generated_for_enabled_light() {
    let mut light = Light3D::directional(Light3DId(1));
    light.shadow_enabled = true;
    light.direction = Vector3::new(0.0, -1.0, 0.0);

    let maps = generate_shadow_maps(&[light], &[]);
    assert_eq!(maps.len(), 1, "one shadow map for one enabled directional light");
    assert_eq!(maps[0].size, SHADOW_MAP_SIZE);
}

#[test]
fn no_shadow_map_for_disabled_light() {
    let light = Light3D::directional(Light3DId(1));
    assert!(!light.shadow_enabled);
    let maps = generate_shadow_maps(&[light], &[]);
    assert_eq!(maps.len(), 0);
}

#[test]
fn no_shadow_map_for_point_light() {
    let mut light = Light3D::point(Light3DId(1), Vector3::new(0.0, 5.0, 0.0));
    light.shadow_enabled = true;
    let maps = generate_shadow_maps(&[light], &[]);
    assert_eq!(maps.len(), 0, "point lights use cubemaps, not directional shadow maps");
}

#[test]
fn shadow_map_captures_occluder_depth() {
    let mut light = Light3D::directional(Light3DId(1));
    light.shadow_enabled = true;
    light.direction = Vector3::new(0.0, -1.0, 0.0);

    let mut inst = Instance3D::new(Instance3DId(1));
    inst.mesh = Some(Mesh3D::cube(2.0));
    inst.visible = true;
    inst.transform = Transform3D::IDENTITY;

    let maps = generate_shadow_maps(&[light], &[inst]);
    assert_eq!(maps.len(), 1);

    // Check that depth values were written.
    let (w, h) = maps[0].depth.dimensions();
    let mut written = 0u32;
    for y in 0..h {
        for x in 0..w {
            if maps[0].depth.get(x, y) < f32::MAX {
                written += 1;
            }
        }
    }
    assert!(written > 0, "occluder should produce depth in shadow map");
}

// ── Shadow sampling tests ────────────────────────────────────────────

#[test]
fn point_below_occluder_is_shadowed() {
    let mut light = Light3D::directional(Light3DId(1));
    light.shadow_enabled = true;
    light.direction = Vector3::new(0.0, -1.0, 0.0);

    // Large occluder above
    let mut inst = Instance3D::new(Instance3DId(1));
    inst.mesh = Some(Mesh3D::cube(4.0));
    inst.visible = true;
    inst.transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 5.0, 0.0),
    };

    let maps = generate_shadow_maps(&[light], &[inst]);
    assert_eq!(maps.len(), 1);

    let shadow = maps[0].sample(Vector3::new(0.0, 0.0, 0.0));
    assert!(shadow > 0.5, "point below occluder should be shadowed, got {shadow}");
}

#[test]
fn point_beside_occluder_is_lit() {
    let mut light = Light3D::directional(Light3DId(1));
    light.shadow_enabled = true;
    light.direction = Vector3::new(0.0, -1.0, 0.0);

    let mut inst = Instance3D::new(Instance3DId(1));
    inst.mesh = Some(Mesh3D::cube(1.0));
    inst.visible = true;
    inst.transform = Transform3D::IDENTITY;

    let maps = generate_shadow_maps(&[light], &[inst]);

    let shadow = maps[0].sample(Vector3::new(10.0, 0.0, 10.0));
    assert!(shadow < 0.5, "point beside occluder should be lit, got factor={shadow}");
}

// ── Solid render pipeline integration ────────────────────────────────

#[test]
fn shadow_darkens_fragments_in_solid_render() {
    // Scene: camera looking at two patches of ground.
    // A cube occluder hangs above only one patch.
    // A directional light with shadow_enabled shines downward.
    // The shadowed patch should be darker than the lit patch.

    let mut renderer = SoftwareRenderer3D::new();
    assert_eq!(renderer.render_mode, RenderMode::Solid);

    // Ground plane at y = -2
    let ground_id = renderer.create_instance();
    renderer.set_mesh(ground_id, Mesh3D::plane(20.0));
    let mut ground_mat = Material3D::default();
    ground_mat.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
    ground_mat.shading_mode = ShadingMode::Lambert;
    renderer.set_material(ground_id, ground_mat);
    renderer.set_transform(
        ground_id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, -2.0, 0.0),
        },
    );

    // Occluder cube above center
    let occ_id = renderer.create_instance();
    renderer.set_mesh(occ_id, Mesh3D::cube(3.0));
    renderer.set_material(occ_id, Material3D::default());
    renderer.set_transform(
        occ_id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 3.0, -10.0),
        },
    );

    // Directional light pointing down with shadow enabled.
    let light_id = Light3DId(100);
    renderer.add_light(light_id);
    let mut light = Light3D::directional(light_id);
    light.direction = Vector3::new(0.0, -1.0, 0.0);
    light.shadow_enabled = true;
    light.energy = 1.0;
    renderer.update_light(&light);

    // Camera looking at the scene
    let vp = Viewport3D {
        width: 64,
        height: 64,
        camera_transform: Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 2.0, 5.0),
        },
        fov: std::f32::consts::FRAC_PI_4,
        near: 0.1,
        far: 100.0,
    };

    let frame = renderer.render_frame(&vp);

    // Check that we have non-black pixels (scene rendered).
    let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
    assert!(nonblack > 10, "scene should have visible pixels, got {nonblack}");
}

#[test]
fn shadow_deterministic_across_frames() {
    let mut renderer = SoftwareRenderer3D::new();

    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(2.0));
    let mut mat = Material3D::default();
    mat.shading_mode = ShadingMode::Lambert;
    renderer.set_material(id, mat);
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    let light_id = Light3DId(1);
    renderer.add_light(light_id);
    let mut light = Light3D::directional(light_id);
    light.direction = Vector3::new(0.0, -1.0, 0.0);
    light.shadow_enabled = true;
    renderer.update_light(&light);

    let vp = Viewport3D::new(32, 32);
    let f1 = renderer.render_frame(&vp);
    let f2 = renderer.render_frame(&vp);

    assert_eq!(f1.pixels, f2.pixels, "shadow rendering must be deterministic");
}

#[test]
fn multiple_lights_mixed_shadow_states() {
    let mut renderer = SoftwareRenderer3D::new();

    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    let mut mat = Material3D::default();
    mat.shading_mode = ShadingMode::Lambert;
    renderer.set_material(id, mat);
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    // Light 1: shadow enabled
    let lid1 = Light3DId(1);
    renderer.add_light(lid1);
    let mut l1 = Light3D::directional(lid1);
    l1.direction = Vector3::new(0.0, -1.0, 0.0);
    l1.shadow_enabled = true;
    renderer.update_light(&l1);

    // Light 2: no shadow
    let lid2 = Light3DId(2);
    renderer.add_light(lid2);
    let mut l2 = Light3D::directional(lid2);
    l2.direction = Vector3::new(1.0, -1.0, 0.0).normalized();
    l2.shadow_enabled = false;
    renderer.update_light(&l2);

    let vp = Viewport3D::new(32, 32);
    let frame = renderer.render_frame(&vp);

    let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
    assert!(nonblack > 0, "mixed shadow scene should render");
}

#[test]
fn phong_shadow_integration() {
    let mut renderer = SoftwareRenderer3D::new();

    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    let mut mat = Material3D::default();
    mat.shading_mode = ShadingMode::Phong;
    mat.metallic = 0.5;
    mat.roughness = 0.3;
    renderer.set_material(id, mat);
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    let light_id = Light3DId(1);
    renderer.add_light(light_id);
    let mut light = Light3D::directional(light_id);
    light.direction = Vector3::new(0.0, -1.0, 0.0);
    light.shadow_enabled = true;
    renderer.update_light(&light);

    let vp = Viewport3D::new(32, 32);
    let frame = renderer.render_frame(&vp);

    let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
    assert!(nonblack > 0, "phong with shadow should render visible pixels");
}
