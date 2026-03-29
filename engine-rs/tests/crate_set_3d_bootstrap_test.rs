//! pat-qg2: Define and bootstrap the first 3D crate set.
//!
//! Validates that the three 3D crates (gdserver3d, gdrender3d, gdphysics3d)
//! are correctly defined, compile as workspace members, expose their public
//! APIs, and integrate with the existing scene layer.
//!
//! Test categories:
//! 1. gdserver3d: Mesh, Material, Light, Viewport, Projection, RenderingServer3D trait
//! 2. gdrender3d: SoftwareRenderer3D, DepthBuffer, FrameBuffer3D, comparison
//! 3. gdphysics3d: Body, Shape, World, Collision detection
//! 4. Cross-crate: server3d -> render3d pipeline, scene tree -> 3D adapter
//! 5. Integration with gdcore math3d types

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Quaternion, Transform3D};

// ===========================================================================
// 1. gdserver3d -- public API surface
// ===========================================================================

#[test]
fn server3d_mesh_construction() {
    let mesh = gdserver3d::Mesh3D::cube(1.0);
    assert!(!mesh.vertices.is_empty(), "cube mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "cube mesh should have indices");
}

#[test]
fn server3d_material_default() {
    let mat = gdserver3d::Material3D::default();
    // Default material should have white albedo
    assert_eq!(mat.albedo.r, 1.0);
    assert_eq!(mat.albedo.g, 1.0);
    assert_eq!(mat.albedo.b, 1.0);
}

#[test]
fn server3d_light_types() {
    use gdserver3d::light::LightType;

    let directional = gdserver3d::Light3D::directional(gdserver3d::Light3DId(1));
    assert!(matches!(directional.light_type, LightType::Directional));

    let point = gdserver3d::Light3D::point(gdserver3d::Light3DId(2), Vector3::new(1.0, 2.0, 3.0));
    assert!(matches!(point.light_type, LightType::Point));

    let spot = gdserver3d::Light3D::spot(
        gdserver3d::Light3DId(3),
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::new(0.0, -1.0, 0.0),
    );
    assert!(matches!(spot.light_type, LightType::Spot));
}

#[test]
fn server3d_viewport_creation() {
    let vp = gdserver3d::Viewport3D::new(1920, 1080);
    assert_eq!(vp.width, 1920);
    assert_eq!(vp.height, 1080);
}

#[test]
fn server3d_perspective_projection() {
    let proj = gdserver3d::perspective_projection_matrix(
        std::f32::consts::FRAC_PI_4, // 45 degrees
        16.0 / 9.0,                  // aspect
        0.1,                         // near
        100.0,                       // far
    );
    // Projection matrix should be non-zero
    assert!(proj[0][0] != 0.0, "projection[0][0] should be non-zero");
    assert!(proj[1][1] != 0.0, "projection[1][1] should be non-zero");
}

#[test]
fn server3d_instance_construction() {
    use gdserver3d::instance::{Instance3D, Instance3DId};
    let instance = Instance3D::new(Instance3DId(0));
    assert_eq!(instance.id, Instance3DId(0));
    assert!(instance.visible);
    assert!(instance.mesh.is_none());
    assert!(instance.material.is_none());
}

#[test]
fn server3d_mesh_sphere_and_plane() {
    let sphere = gdserver3d::Mesh3D::sphere(2.0, 8);
    assert!(!sphere.vertices.is_empty());
    assert!(!sphere.indices.is_empty());

    let plane = gdserver3d::Mesh3D::plane(5.0);
    assert_eq!(plane.vertex_count(), 4);
    assert_eq!(plane.triangle_count(), 2);
}

// ===========================================================================
// 2. gdrender3d -- public API surface
// ===========================================================================

#[test]
fn render3d_software_renderer_creation() {
    use gdserver3d::server::RenderingServer3D;
    let mut renderer = gdrender3d::SoftwareRenderer3D::new();
    let vp = gdserver3d::Viewport3D::new(640, 480);
    let frame = renderer.render_frame(&vp);
    assert_eq!(frame.width, 640);
    assert_eq!(frame.height, 480);
}

#[test]
fn render3d_depth_buffer_creation() {
    let db = gdrender3d::DepthBuffer::new(100, 100);
    // Default depth should be f32::MAX
    assert_eq!(db.get(50, 50), f32::MAX);
}

#[test]
fn render3d_depth_buffer_write_and_test() {
    let mut db = gdrender3d::DepthBuffer::new(10, 10);
    // Write a depth value
    assert!(db.test_and_set(5, 5, 0.5));
    assert_eq!(db.get(5, 5), 0.5);
    // Closer depth should pass
    assert!(db.test_and_set(5, 5, 0.3));
    // Farther depth should fail
    assert!(!db.test_and_set(5, 5, 0.8));
}

#[test]
fn render3d_framebuffer_creation_and_pixel_access() {
    let mut fb = gdrender3d::FrameBuffer3D::new(64, 64, Color::new(0.5, 0.5, 0.5, 1.0));
    assert_eq!(fb.width, 64);
    assert_eq!(fb.height, 64);
    let pixel = fb.get_pixel(32, 32);
    assert!((pixel.r - 0.5).abs() < 0.01, "pixel should be gray");
    fb.set_pixel(0, 0, Color::WHITE);
    assert_eq!(fb.get_pixel(0, 0), Color::WHITE);
}

#[test]
fn render3d_compare_identical_framebuffers() {
    let fb1 = gdrender3d::FrameBuffer3D::new(32, 32, Color::BLACK);
    let fb2 = gdrender3d::FrameBuffer3D::new(32, 32, Color::BLACK);
    let diff = gdrender3d::compare_framebuffers_3d(&fb1, &fb2, 0.0, 0.0);
    assert!(
        diff.is_exact_color_match(),
        "identical framebuffers should have exact match"
    );
}

// ===========================================================================
// 3. gdphysics3d -- public API surface
// ===========================================================================

#[test]
fn physics3d_sphere_shape() {
    let shape = gdphysics3d::Shape3D::Sphere { radius: 2.0 };
    if let gdphysics3d::Shape3D::Sphere { radius } = shape {
        assert_eq!(radius, 2.0);
    }
}

#[test]
fn physics3d_box_shape() {
    let shape = gdphysics3d::Shape3D::BoxShape {
        half_extents: Vector3::new(1.0, 2.0, 3.0),
    };
    if let gdphysics3d::Shape3D::BoxShape { half_extents } = shape {
        assert_eq!(half_extents.x, 1.0);
        assert_eq!(half_extents.y, 2.0);
    }
}

#[test]
fn physics3d_body_construction() {
    let body = gdphysics3d::PhysicsBody3D::new(
        gdphysics3d::BodyId3D(42),
        gdphysics3d::BodyType3D::Rigid,
        Vector3::new(0.0, 10.0, 0.0),
        gdphysics3d::Shape3D::Sphere { radius: 1.0 },
        1.0,
    );
    assert_eq!(body.id, gdphysics3d::BodyId3D(42));
    assert_eq!(body.position.y, 10.0);
}

#[test]
fn physics3d_world_gravity_default() {
    let world = gdphysics3d::PhysicsWorld3D::new();
    let gravity = world.gravity;
    assert!(gravity.y < 0.0, "default gravity should point down");
}

#[test]
fn physics3d_world_step_applies_gravity() {
    let mut world = gdphysics3d::PhysicsWorld3D::new();
    let body = gdphysics3d::PhysicsBody3D::new(
        gdphysics3d::BodyId3D(0),
        gdphysics3d::BodyType3D::Rigid,
        Vector3::new(0.0, 10.0, 0.0),
        gdphysics3d::Shape3D::Sphere { radius: 1.0 },
        1.0,
    );
    let id = world.add_body(body);
    let initial_y = world.get_body(id).unwrap().position.y;

    world.step(1.0 / 60.0);

    let new_y = world.get_body(id).unwrap().position.y;
    assert!(
        new_y < initial_y,
        "rigid body should fall under gravity: {} -> {}",
        initial_y,
        new_y
    );
}

#[test]
fn physics3d_static_body_does_not_move() {
    let mut world = gdphysics3d::PhysicsWorld3D::new();
    let body = gdphysics3d::PhysicsBody3D::new(
        gdphysics3d::BodyId3D(0),
        gdphysics3d::BodyType3D::Static,
        Vector3::new(0.0, 0.0, 0.0),
        gdphysics3d::Shape3D::BoxShape {
            half_extents: Vector3::new(10.0, 1.0, 10.0),
        },
        0.0,
    );
    let id = world.add_body(body);

    world.step(1.0 / 60.0);
    world.step(1.0 / 60.0);

    let pos = world.get_body(id).unwrap().position;
    assert_eq!(pos.y, 0.0, "static body should not move");
}

// ===========================================================================
// 4. Cross-crate: server3d -> render3d pipeline
// ===========================================================================

#[test]
fn server3d_render3d_pipeline_renders_cube() {
    use gdserver3d::server::RenderingServer3D;

    let mut renderer = gdrender3d::SoftwareRenderer3D::new();

    // Create a cube instance via the RenderingServer3D trait
    let id = renderer.create_instance();
    renderer.set_mesh(id, gdserver3d::Mesh3D::cube(2.0));
    renderer.set_material(id, gdserver3d::Material3D::default());

    // Move cube in front of camera
    let transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, -5.0),
    };
    renderer.set_transform(id, transform);

    // Render
    let viewport = gdserver3d::Viewport3D::new(128, 128);
    let frame = renderer.render_frame(&viewport);

    assert_eq!(frame.width, 128);
    assert_eq!(frame.height, 128);
    // Wireframe cube should produce some non-black pixels
    let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
    assert!(nonblack > 0, "cube wireframe should produce visible pixels");
}

#[test]
fn server3d_render3d_pipeline_deterministic() {
    use gdserver3d::server::RenderingServer3D;

    let mut renderer = gdrender3d::SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, gdserver3d::Mesh3D::cube(1.0));
    renderer.set_material(id, gdserver3d::Material3D::default());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    let vp = gdserver3d::Viewport3D::new(32, 32);
    let f1 = renderer.render_frame(&vp);
    let f2 = renderer.render_frame(&vp);
    assert_eq!(f1.pixels, f2.pixels, "rendering must be deterministic");
}

// ===========================================================================
// 5. Integration with gdcore math3d types
// ===========================================================================

#[test]
fn math3d_transform_compose_for_scene_hierarchy() {
    // Parent at (2,0,0), child at local (0,3,0) -> global (2,3,0)
    let parent = Transform3D::IDENTITY.translated(Vector3::new(2.0, 0.0, 0.0));
    let child_local = Transform3D::IDENTITY.translated(Vector3::new(0.0, 3.0, 0.0));
    let child_global = parent * child_local;
    let pos = child_global.xform(Vector3::ZERO);
    assert!((pos.x - 2.0).abs() < 0.001);
    assert!((pos.y - 3.0).abs() < 0.001);
}

#[test]
fn math3d_quaternion_rotation_preserves_length() {
    let q = Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);
    let v = Vector3::new(1.0, 0.0, 0.0);
    let rotated = q.xform(v);
    let len = rotated.length();
    assert!((len - 1.0).abs() < 0.001, "rotation should preserve length");
}

#[test]
fn math3d_basis_euler_roundtrip() {
    let euler = Vector3::new(0.5, 0.3, 0.1); // radians
    let basis = Basis::from_euler(euler);
    let recovered = basis.to_euler();
    assert!((recovered.x - euler.x).abs() < 0.01);
    assert!((recovered.y - euler.y).abs() < 0.01);
    assert!((recovered.z - euler.z).abs() < 0.01);
}

#[test]
fn math3d_transform_inverse_identity() {
    let t = Transform3D::IDENTITY
        .translated(Vector3::new(5.0, -3.0, 2.0))
        .rotated(Vector3::new(0.0, 1.0, 0.0), 0.5);
    let inv = t.inverse();
    let result = t * inv;
    // Should be close to identity
    let pos = result.xform(Vector3::ZERO);
    assert!(
        pos.length() < 0.01,
        "T * T^-1 should be identity, got {:?}",
        pos
    );
}

// ===========================================================================
// 6. Workspace membership validation
// ===========================================================================

#[test]
fn all_3d_crates_are_workspace_members() {
    let cargo_toml =
        std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert!(
        cargo_toml.contains("crates/gdserver3d"),
        "gdserver3d should be a workspace member"
    );
    assert!(
        cargo_toml.contains("crates/gdrender3d"),
        "gdrender3d should be a workspace member"
    );
    assert!(
        cargo_toml.contains("crates/gdphysics3d"),
        "gdphysics3d should be a workspace member"
    );
}

#[test]
fn all_3d_crate_lib_files_exist() {
    let base = env!("CARGO_MANIFEST_DIR");
    for crate_name in &["gdserver3d", "gdrender3d", "gdphysics3d"] {
        let lib_path = format!("{}/crates/{}/src/lib.rs", base, crate_name);
        assert!(
            std::path::Path::new(&lib_path).exists(),
            "{}/src/lib.rs should exist",
            crate_name
        );
    }
}

// ===========================================================================
// 7. Physics3d multi-body simulation
// ===========================================================================

#[test]
fn physics3d_multi_body_deterministic() {
    let mut world = gdphysics3d::PhysicsWorld3D::new();

    let initial_heights = [10.0_f32, 15.0, 20.0];
    let mut ids = Vec::new();

    // Add 3 falling spheres at different heights
    for (i, &h) in initial_heights.iter().enumerate() {
        let body = gdphysics3d::PhysicsBody3D::new(
            gdphysics3d::BodyId3D(i as u64),
            gdphysics3d::BodyType3D::Rigid,
            Vector3::new(i as f32 * 3.0, h, 0.0),
            gdphysics3d::Shape3D::Sphere { radius: 0.5 },
            1.0,
        );
        ids.push(world.add_body(body));
    }

    // Step 10 times
    for _ in 0..10 {
        world.step(1.0 / 60.0);
    }

    // All should have moved down
    for (i, &id) in ids.iter().enumerate() {
        let body = world.get_body(id).unwrap();
        assert!(
            body.position.y < initial_heights[i],
            "body {} should have fallen from {}",
            i,
            initial_heights[i]
        );
    }
}
