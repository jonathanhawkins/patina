//! pat-k22f: Crate-boundary validation for 3D architecture readiness.
//!
//! Validates that the existing crate structure supports 3D expansion:
//! - Core 3D math types exist in gdcore (foundation layer)
//! - Variant system supports all 3D types
//! - Scene tree can store 3D node properties without new crate deps
//! - 2D-specific crates don't leak into the foundation layers
//! - ClassDB can register 3D node classes
//!
//! These tests ensure the crate boundaries documented in
//! docs/3D_ARCHITECTURE_SPEC.md are correct and that 3D work can begin
//! on a stable base.

use std::sync::Mutex;

// ===========================================================================
// 1. gdcore: All 3D math types exist at the foundation layer
// ===========================================================================

#[test]
fn gdcore_has_vector3() {
    let v = gdcore::math::Vector3::new(1.0, 2.0, 3.0);
    assert_eq!(v.x, 1.0);
    assert_eq!(v.y, 2.0);
    assert_eq!(v.z, 3.0);
    assert!((v.length() - 3.7416573).abs() < 0.001);
}

#[test]
fn gdcore_has_quaternion() {
    let q = gdcore::math3d::Quaternion::IDENTITY;
    assert_eq!(q.w, 1.0);
    assert_eq!(q.x, 0.0);
    assert_eq!(q.y, 0.0);
    assert_eq!(q.z, 0.0);
}

#[test]
fn gdcore_has_basis() {
    let b = gdcore::math3d::Basis::IDENTITY;
    // Identity basis: diagonal is 1, off-diagonal is 0.
    assert_eq!(b.x.x, 1.0);
    assert_eq!(b.y.y, 1.0);
    assert_eq!(b.z.z, 1.0);
    assert_eq!(b.x.y, 0.0);
}

#[test]
fn gdcore_has_transform3d() {
    let t = gdcore::math3d::Transform3D::IDENTITY;
    assert_eq!(t.origin.x, 0.0);
    assert_eq!(t.origin.y, 0.0);
    assert_eq!(t.origin.z, 0.0);

    let translated = t.translated(gdcore::math::Vector3::new(5.0, 0.0, 0.0));
    assert_eq!(translated.origin.x, 5.0);
}

#[test]
fn gdcore_has_aabb() {
    let aabb = gdcore::math3d::Aabb {
        position: gdcore::math::Vector3::ZERO,
        size: gdcore::math::Vector3::ONE,
    };
    assert!(aabb.contains_point(gdcore::math::Vector3::new(0.5, 0.5, 0.5)));
    assert!(!aabb.contains_point(gdcore::math::Vector3::new(2.0, 0.0, 0.0)));
}

#[test]
fn gdcore_vector3_operations() {
    let a = gdcore::math::Vector3::new(1.0, 0.0, 0.0);
    let b = gdcore::math::Vector3::new(0.0, 1.0, 0.0);

    // Cross product of X and Y axes should be Z axis.
    let cross = a.cross(b);
    assert!((cross.x).abs() < 1e-6);
    assert!((cross.y).abs() < 1e-6);
    assert!((cross.z - 1.0).abs() < 1e-6);

    // Dot product of perpendicular vectors is 0.
    assert!((a.dot(b)).abs() < 1e-6);
}

#[test]
fn quaternion_euler_roundtrip() {
    use gdcore::math::Vector3;
    use gdcore::math3d::Quaternion;

    let euler = Vector3::new(0.5, 0.3, 0.1);
    let q = Quaternion::from_euler(euler);
    let back = q.to_euler();

    assert!((back.x - euler.x).abs() < 0.01);
    assert!((back.y - euler.y).abs() < 0.01);
    assert!((back.z - euler.z).abs() < 0.01);
}

#[test]
fn transform3d_composition() {
    use gdcore::math::Vector3;
    use gdcore::math3d::Transform3D;

    let t1 = Transform3D::IDENTITY.translated(Vector3::new(1.0, 0.0, 0.0));
    let t2 = Transform3D::IDENTITY.translated(Vector3::new(0.0, 2.0, 0.0));

    // Composing two translations should add the origins.
    let composed = t1 * t2;
    assert!((composed.origin.x - 1.0).abs() < 1e-6);
    assert!((composed.origin.y - 2.0).abs() < 1e-6);
}

// ===========================================================================
// 2. gdvariant: All 3D types representable as Variant
// ===========================================================================

#[test]
fn variant_supports_vector3() {
    use gdvariant::Variant;
    let v = Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0));
    match &v {
        Variant::Vector3(vec) => {
            assert_eq!(vec.x, 1.0);
            assert_eq!(vec.y, 2.0);
            assert_eq!(vec.z, 3.0);
        }
        _ => panic!("expected Vector3 variant"),
    }
}

#[test]
fn variant_supports_transform3d() {
    use gdvariant::Variant;
    let t = gdcore::math3d::Transform3D::IDENTITY;
    let v = Variant::Transform3D(t);
    assert!(matches!(v, Variant::Transform3D(_)));
}

#[test]
fn variant_supports_quaternion() {
    use gdvariant::Variant;
    let q = gdcore::math3d::Quaternion::IDENTITY;
    let v = Variant::Quaternion(q);
    assert!(matches!(v, Variant::Quaternion(_)));
}

#[test]
fn variant_supports_basis() {
    use gdvariant::Variant;
    let b = gdcore::math3d::Basis::IDENTITY;
    let v = Variant::Basis(b);
    assert!(matches!(v, Variant::Basis(_)));
}

#[test]
fn variant_supports_aabb() {
    use gdvariant::Variant;
    let a = gdcore::math3d::Aabb {
        position: gdcore::math::Vector3::ZERO,
        size: gdcore::math::Vector3::ONE,
    };
    let v = Variant::Aabb(a);
    assert!(matches!(v, Variant::Aabb(_)));
}

// ===========================================================================
// 3. ClassDB: Can register 3D node classes without new crate deps
// ===========================================================================

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    gdobject::class_db::clear_for_testing();
    guard
}

#[test]
fn classdb_registers_node3d_hierarchy() {
    let _g = setup();
    use gdcore::math::Vector3;
    use gdobject::class_db::*;
    use gdvariant::Variant;

    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(PropertyInfo::new("name", Variant::String(String::new()))),
    );
    register_class(
        ClassRegistration::new("Node3D")
            .parent("Node")
            .property(PropertyInfo::new(
                "position",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "rotation",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new("scale", Variant::Vector3(Vector3::ONE)))
            .property(PropertyInfo::new("visible", Variant::Bool(true)))
            .method(MethodInfo::new("get_position", 0))
            .method(MethodInfo::new("set_position", 1))
            .method(MethodInfo::new("get_global_position", 0))
            .method(MethodInfo::new("get_global_transform", 0))
            .method(MethodInfo::new("look_at", 1))
            .method(MethodInfo::new("rotate", 2)),
    );

    assert!(class_exists("Node3D"));
    assert!(is_parent_class("Node3D", "Node"));
    assert!(is_parent_class("Node3D", "Object"));

    let chain = inheritance_chain("Node3D");
    assert_eq!(chain, vec!["Node3D", "Node", "Object"]);
}

#[test]
fn classdb_registers_camera3d() {
    let _g = setup();
    use gdcore::math::Vector3;
    use gdobject::class_db::*;
    use gdvariant::Variant;

    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_class(
        ClassRegistration::new("Node3D")
            .parent("Node")
            .property(PropertyInfo::new(
                "position",
                Variant::Vector3(Vector3::ZERO),
            )),
    );
    register_class(
        ClassRegistration::new("Camera3D")
            .parent("Node3D")
            .property(PropertyInfo::new("fov", Variant::Float(75.0)))
            .property(PropertyInfo::new("near", Variant::Float(0.05)))
            .property(PropertyInfo::new("far", Variant::Float(4000.0)))
            .property(PropertyInfo::new("current", Variant::Bool(false)))
            .method(MethodInfo::new("make_current", 0))
            .method(MethodInfo::new("is_current", 0))
            .method(MethodInfo::new("get_fov", 0))
            .method(MethodInfo::new("set_fov", 1)),
    );

    assert!(class_exists("Camera3D"));
    assert!(is_parent_class("Camera3D", "Node3D"));
    // Verify properties exist (includes inherited).
    assert!(
        class_has_property("Camera3D", "fov"),
        "Camera3D should have fov property"
    );
    assert!(
        class_has_property("Camera3D", "position"),
        "Camera3D should inherit position from Node3D"
    );
    assert!(class_has_method("Camera3D", "make_current"));
}

#[test]
fn classdb_registers_rigidbody3d() {
    let _g = setup();
    use gdcore::math::Vector3;
    use gdobject::class_db::*;
    use gdvariant::Variant;

    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_class(
        ClassRegistration::new("Node3D")
            .parent("Node")
            .property(PropertyInfo::new(
                "position",
                Variant::Vector3(Vector3::ZERO),
            )),
    );
    register_class(
        ClassRegistration::new("RigidBody3D")
            .parent("Node3D")
            .property(PropertyInfo::new("mass", Variant::Float(1.0)))
            .property(PropertyInfo::new("gravity_scale", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "linear_velocity",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "angular_velocity",
                Variant::Vector3(Vector3::ZERO),
            ))
            .method(MethodInfo::new("apply_force", 1))
            .method(MethodInfo::new("apply_impulse", 1)),
    );

    assert!(class_exists("RigidBody3D"));
    assert!(is_parent_class("RigidBody3D", "Node3D"));
    // Verify properties exist (includes inherited).
    assert!(
        class_has_property("RigidBody3D", "mass"),
        "RigidBody3D should have mass property"
    );
    assert!(
        class_has_property("RigidBody3D", "position"),
        "RigidBody3D should inherit position from Node3D"
    );
    assert!(class_has_method("RigidBody3D", "apply_force"));
}

#[test]
fn classdb_instantiate_node3d_with_defaults() {
    let _g = setup();
    use gdcore::math::Vector3;
    use gdobject::class_db::*;
    use gdobject::object::GodotObject;
    use gdvariant::Variant;

    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_class(
        ClassRegistration::new("Node3D")
            .parent("Node")
            .property(PropertyInfo::new(
                "position",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new("scale", Variant::Vector3(Vector3::ONE)))
            .property(PropertyInfo::new("visible", Variant::Bool(true))),
    );

    let obj = instantiate("Node3D").expect("should instantiate Node3D");
    assert_eq!(obj.get_class(), "Node3D");
    assert_eq!(
        obj.get_property("position"),
        Variant::Vector3(Vector3::ZERO)
    );
    assert_eq!(obj.get_property("scale"), Variant::Vector3(Vector3::ONE));
    assert_eq!(obj.get_property("visible"), Variant::Bool(true));
}

// ===========================================================================
// 4. Transform3D operations match Godot semantics
// ===========================================================================

#[test]
fn transform3d_looking_at_produces_valid_basis() {
    use gdcore::math::Vector3;
    use gdcore::math3d::Transform3D;

    let eye = Vector3::new(0.0, 0.0, 5.0);
    let target = Vector3::ZERO;
    let up = Vector3::new(0.0, 1.0, 0.0);

    let t = Transform3D::IDENTITY.translated(eye).looking_at(target, up);
    // Origin should still be at eye position after looking_at.
    assert!((t.origin.x - eye.x).abs() < 1e-6);
    assert!((t.origin.y - eye.y).abs() < 1e-6);
    assert!((t.origin.z - eye.z).abs() < 1e-6);

    // The basis should be orthonormal (determinant ~1).
    let det = t.basis.determinant();
    assert!(
        (det - 1.0).abs() < 0.01 || (det + 1.0).abs() < 0.01,
        "looking_at basis should be orthonormal, det={det}"
    );
}

#[test]
fn aabb_merge_and_intersection() {
    use gdcore::math::Vector3;
    use gdcore::math3d::Aabb;

    let a = Aabb {
        position: Vector3::ZERO,
        size: Vector3::new(2.0, 2.0, 2.0),
    };
    let b = Aabb {
        position: Vector3::new(1.0, 1.0, 1.0),
        size: Vector3::new(2.0, 2.0, 2.0),
    };

    assert!(a.intersects(b), "overlapping AABBs should intersect");

    let merged = a.merge(b);
    assert!(merged.contains_point(Vector3::ZERO));
    assert!(merged.contains_point(Vector3::new(2.9, 2.9, 2.9)));
}

// ===========================================================================
// 5. Workspace structure: crate count and names
// ===========================================================================

#[test]
fn workspace_has_expected_crate_count() {
    // The workspace currently has 13 member crates.
    // This test documents the baseline before 3D crates are added.
    let cargo_toml = std::fs::read_to_string(format!(
        "{}/../engine-rs/Cargo.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| {
        std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
            .expect("should read workspace Cargo.toml")
    });

    let member_count = cargo_toml.matches("crates/gd").count()
        + cargo_toml.matches("crates/patina-runner").count();

    assert!(
        member_count >= 13,
        "expected at least 13 workspace members, got {member_count}"
    );
}

#[test]
fn architecture_spec_exists() {
    let spec_path = format!(
        "{}/../docs/3D_ARCHITECTURE_SPEC.md",
        env!("CARGO_MANIFEST_DIR")
    );
    assert!(
        std::path::Path::new(&spec_path).exists(),
        "3D architecture spec should exist at docs/3D_ARCHITECTURE_SPEC.md"
    );
}

// ===========================================================================
// pat-4mq: 3D server crate boundary contracts
// ===========================================================================

/// gdserver2d::server3d re-exports match gdserver3d canonical types.
#[test]
fn gdserver2d_server3d_reexports_from_gdserver3d() {
    // Viewport3D from both paths must be the same type.
    let vp_via_server3d = gdserver3d::viewport::Viewport3D::new(800, 600);
    let vp_via_server2d = gdserver2d::server3d::Viewport3D::new(800, 600);
    assert_eq!(vp_via_server3d.width, vp_via_server2d.width);
    assert_eq!(vp_via_server3d.height, vp_via_server2d.height);
    assert!((vp_via_server3d.fov - vp_via_server2d.fov).abs() < f32::EPSILON);

    // Instance3DId from both paths must have matching inner values.
    let id_a = gdserver3d::instance::Instance3DId(42);
    let id_b = gdserver2d::server3d::Instance3DId(42);
    assert_eq!(id_a.0, id_b.0);

    // perspective_projection_matrix from both paths must produce identical results.
    let fov = std::f32::consts::FRAC_PI_4;
    let m_a = gdserver3d::projection::perspective_projection_matrix(fov, 1.0, 0.1, 100.0);
    let m_b = gdserver2d::server3d::perspective_projection_matrix(fov, 1.0, 0.1, 100.0);
    assert_eq!(m_a, m_b, "projection matrices from both paths must match");
}

/// gdserver2d::mesh re-exports match gdserver3d::mesh canonical types.
#[test]
fn gdserver2d_mesh_reexports_from_gdserver3d() {
    let cube_a = gdserver3d::mesh::Mesh3D::cube(1.0);
    let cube_b = gdserver2d::Mesh3D::cube(1.0);
    assert_eq!(
        cube_a.vertices.len(),
        cube_b.vertices.len(),
        "mesh vertex count must match"
    );
    assert_eq!(
        cube_a.indices.len(),
        cube_b.indices.len(),
        "mesh index count must match"
    );
}

/// The gdserver3d crate is a workspace member and builds independently.
#[test]
fn gdserver3d_has_all_submodules() {
    // Verify the canonical 3D server exports are accessible.
    let _id = gdserver3d::Instance3DId(1);
    let _light = gdserver3d::Light3DId(1);
    let _mesh = gdserver3d::Mesh3D::cube(1.0);
    let _mat = gdserver3d::Material3D::default();
    let _vp = gdserver3d::Viewport3D::new(640, 480);
    let _proj = gdserver3d::perspective_projection_matrix(1.0, 1.0, 0.1, 100.0);
}

/// The gdrender3d crate implements the gdserver3d::RenderingServer3D trait.
#[test]
fn gdrender3d_implements_rendering_server_3d_trait() {
    use gdserver3d::server::RenderingServer3D;
    let mut renderer = gdrender3d::SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, gdserver3d::Mesh3D::cube(1.0));
    renderer.set_material(id, gdserver3d::Material3D::default());
    let vp = gdserver3d::Viewport3D::new(32, 32);
    let frame = renderer.render_frame(&vp);
    assert_eq!(frame.width, 32);
    assert_eq!(frame.height, 32);
    renderer.free_instance(id);
}

/// The gdphysics3d crate builds and provides core physics types.
#[test]
fn gdphysics3d_core_types_accessible() {
    use gdphysics3d::{BodyId3D, BodyType3D, PhysicsBody3D, PhysicsWorld3D, Shape3D};
    let mut world = PhysicsWorld3D::new();
    let body = PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        gdcore::math::Vector3::new(0.0, 10.0, 0.0),
        Shape3D::Sphere { radius: 1.0 },
        1.0,
    );
    let id = world.add_body(body);
    assert!(world.get_body(id).is_some());
    world.step(1.0 / 60.0);
    let pos = world.get_body(id).unwrap().position;
    assert!(pos.y < 10.0, "rigid body should fall under gravity");
}

/// gdscene render3d feature enables the 3D render server adapter.
#[test]
fn gdscene_render3d_feature_provides_adapter() {
    use gdscene::render_server_3d::RenderServer3DAdapter;
    let tree = gdscene::SceneTree::new();
    let mut adapter = RenderServer3DAdapter::new(16, 16);
    let (snapshot, frame) = adapter.render_frame(&tree);
    assert_eq!(snapshot.frame_number, 1);
    assert_eq!(frame.width, 16);
}

/// gdscene physics3d feature enables the 3D physics server bridge.
#[test]
fn gdscene_physics3d_feature_provides_bridge() {
    use gdscene::physics_server_3d::PhysicsServer3D;
    let tree = gdscene::SceneTree::new();
    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 0);
}

/// The 3D crate dependency graph is correct: gdrender3d depends on gdserver3d.
#[test]
fn dependency_graph_render3d_depends_on_server3d() {
    // This test verifies the compile-time boundary by using types from both
    // crates in a single expression. If gdrender3d did not depend on gdserver3d,
    // these types would be incompatible and this would fail to compile.
    use gdserver3d::server::RenderingServer3D;
    let mut renderer = gdrender3d::SoftwareRenderer3D::new();
    let vp = gdserver3d::Viewport3D::new(8, 8);
    let _frame: gdserver3d::FrameData3D = renderer.render_frame(&vp);
}
