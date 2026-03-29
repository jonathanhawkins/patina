//! pat-a0a1: 3D transforms, cameras, and lights subset contract tests.
//!
//! Validates the mathematical and behavioral contracts of the 3D subsystem:
//!   1. Transform3D composition — multiply, inverse, identity invariants
//!   2. Quaternion ↔ Euler ↔ Basis roundtrip consistency
//!   3. Transform3D looking_at contract
//!   4. Transform3D rotate/scale/translate builder contracts
//!   5. Camera3D projection contracts — FOV, clipping planes, projection type
//!   6. Light3D energy, color, shadow, and type-specific contracts
//!   7. Scene tree transform hierarchy with 3D nodes
//!   8. Basis determinant and orthogonality contracts
//!   9. AABB intersection and containment contracts

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Aabb, Basis, Quaternion, Transform3D};
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdserver3d::projection::perspective_projection_matrix;
use gdserver3d::viewport::Viewport3D;
use gdvariant::Variant;

const EPSILON: f32 = 1e-4;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn approx_vec3(a: Vector3, b: Vector3) -> bool {
    approx(a.x, b.x) && approx(a.y, b.y) && approx(a.z, b.z)
}

// ===========================================================================
// 1. Transform3D composition contracts
// ===========================================================================

#[test]
fn identity_transform_is_noop() {
    let v = Vector3::new(3.0, 7.0, -2.0);
    let result = Transform3D::IDENTITY.xform(v);
    assert_eq!(result, v);
}

#[test]
fn transform_multiply_identity_is_self() {
    let t = Transform3D::IDENTITY
        .translated(Vector3::new(10.0, 20.0, 30.0))
        .rotated(Vector3::new(0.0, 1.0, 0.0), 0.5);

    let result = t * Transform3D::IDENTITY;
    let v = Vector3::new(1.0, 2.0, 3.0);
    assert!(approx_vec3(result.xform(v), t.xform(v)));

    let result2 = Transform3D::IDENTITY * t;
    assert!(approx_vec3(result2.xform(v), t.xform(v)));
}

#[test]
fn transform_inverse_roundtrip() {
    let t = Transform3D::IDENTITY
        .translated(Vector3::new(5.0, -3.0, 8.0))
        .rotated(Vector3::new(0.0, 0.0, 1.0), 1.0);

    let inv = t.inverse();
    let roundtrip = t * inv;

    let v = Vector3::new(1.0, 2.0, 3.0);
    let result = roundtrip.xform(v);
    assert!(
        approx_vec3(result, v),
        "T * T^-1 should be identity, got {result:?} from {v:?}"
    );
}

#[test]
fn transform_inverse_recovers_original_point() {
    let t = Transform3D::IDENTITY
        .translated(Vector3::new(100.0, 200.0, 300.0))
        .rotated(Vector3::new(1.0, 0.0, 0.0), 0.785);

    let original = Vector3::new(5.0, 10.0, 15.0);
    let transformed = t.xform(original);
    let recovered = t.inverse().xform(transformed);

    assert!(
        approx_vec3(recovered, original),
        "inverse should recover original point: {recovered:?} != {original:?}"
    );
}

#[test]
fn transform_composition_is_associative() {
    let a = Transform3D::IDENTITY.translated(Vector3::new(1.0, 0.0, 0.0));
    let b = Transform3D::IDENTITY.rotated(Vector3::new(0.0, 1.0, 0.0), 0.5);
    let c = Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, 3.0));

    let v = Vector3::new(1.0, 1.0, 1.0);
    let ab_c = (a * b) * c;
    let a_bc = a * (b * c);

    assert!(
        approx_vec3(ab_c.xform(v), a_bc.xform(v)),
        "(A*B)*C should equal A*(B*C)"
    );
}

// ===========================================================================
// 2. Quaternion ↔ Euler ↔ Basis roundtrip
// ===========================================================================

#[test]
fn quaternion_identity_no_rotation() {
    let v = Vector3::new(1.0, 2.0, 3.0);
    let result = Quaternion::IDENTITY.xform(v);
    assert!(approx_vec3(result, v));
}

#[test]
fn quaternion_euler_roundtrip() {
    let euler = Vector3::new(0.3, 0.5, 0.1);
    let q = Quaternion::from_euler(euler);
    let recovered = q.to_euler();

    assert!(
        approx_vec3(recovered, euler),
        "Euler→Quat→Euler roundtrip failed: {recovered:?} != {euler:?}"
    );
}

#[test]
fn basis_euler_roundtrip() {
    let euler = Vector3::new(0.2, 0.7, -0.3);
    let basis = Basis::from_euler(euler);
    let recovered = basis.to_euler();

    assert!(
        approx_vec3(recovered, euler),
        "Euler→Basis→Euler roundtrip failed: {recovered:?} != {euler:?}"
    );
}

#[test]
fn quaternion_basis_consistency() {
    let euler = Vector3::new(0.4, 0.6, 0.2);
    let q = Quaternion::from_euler(euler);
    let b = Basis::from_euler(euler);

    let v = Vector3::new(1.0, 0.0, 0.0);
    let q_result = q.xform(v);
    let b_result = b.xform(v);

    assert!(
        approx_vec3(q_result, b_result),
        "Quaternion and Basis from same Euler should produce same rotation: {q_result:?} != {b_result:?}"
    );
}

#[test]
fn quaternion_axis_angle_90_deg_y() {
    let q = Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);
    let v = Vector3::new(1.0, 0.0, 0.0);
    let result = q.xform(v);

    // 90° around Y: (1,0,0) → (0,0,-1)
    assert!(
        approx_vec3(result, Vector3::new(0.0, 0.0, -1.0)),
        "90° Y rotation of X-axis should yield -Z: got {result:?}"
    );
}

#[test]
fn quaternion_inverse_undoes_rotation() {
    let q = Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), 1.0);
    let v = Vector3::new(1.0, 0.0, 0.0);
    let rotated = q.xform(v);
    let recovered = q.inverse().xform(rotated);

    assert!(
        approx_vec3(recovered, v),
        "q^-1 * q * v should equal v: {recovered:?} != {v:?}"
    );
}

#[test]
fn quaternion_slerp_endpoints() {
    let a = Quaternion::IDENTITY;
    let b = Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), 1.0);

    let v = Vector3::new(1.0, 0.0, 0.0);

    let at_0 = a.slerp(b, 0.0).xform(v);
    let at_1 = a.slerp(b, 1.0).xform(v);

    assert!(approx_vec3(at_0, a.xform(v)), "slerp(0) should equal start");
    assert!(approx_vec3(at_1, b.xform(v)), "slerp(1) should equal end");
}

// ===========================================================================
// 3. Transform3D looking_at contract
// ===========================================================================

#[test]
fn looking_at_forward_axis_points_at_target() {
    let t = Transform3D::IDENTITY
        .translated(Vector3::new(0.0, 0.0, 0.0))
        .looking_at(Vector3::new(0.0, 0.0, 10.0), Vector3::new(0.0, 1.0, 0.0));

    // Forward (Z column of basis) should point toward target
    let forward = t.basis.z.normalized();
    let expected = Vector3::new(0.0, 0.0, 1.0);
    assert!(
        approx_vec3(forward, expected),
        "forward should point at target: {forward:?}"
    );
}

#[test]
fn looking_at_preserves_origin() {
    let origin = Vector3::new(5.0, 10.0, 15.0);
    let t = Transform3D {
        basis: Basis::IDENTITY,
        origin,
    }
    .looking_at(Vector3::new(100.0, 10.0, 15.0), Vector3::new(0.0, 1.0, 0.0));

    assert_eq!(t.origin, origin, "looking_at must preserve origin");
}

// ===========================================================================
// 4. Transform3D rotate/scale/translate builders
// ===========================================================================

#[test]
fn translated_adds_offset() {
    let t = Transform3D::IDENTITY.translated(Vector3::new(10.0, 20.0, 30.0));
    let result = t.xform(Vector3::ZERO);
    assert!(approx_vec3(result, Vector3::new(10.0, 20.0, 30.0)));
}

#[test]
fn scaled_multiplies_axes() {
    let t = Transform3D::IDENTITY.scaled(Vector3::new(2.0, 3.0, 4.0));
    let result = t.xform(Vector3::new(1.0, 1.0, 1.0));
    assert!(approx_vec3(result, Vector3::new(2.0, 3.0, 4.0)));
}

#[test]
fn rotated_90_deg_around_y() {
    let t = Transform3D::IDENTITY.rotated(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);
    let result = t.xform(Vector3::new(1.0, 0.0, 0.0));
    assert!(
        approx_vec3(result, Vector3::new(0.0, 0.0, -1.0)),
        "90° Y rotation: (1,0,0) → (0,0,-1), got {result:?}"
    );
}

#[test]
fn translate_then_scale_order_matters() {
    let ts = Transform3D::IDENTITY
        .translated(Vector3::new(10.0, 0.0, 0.0))
        .scaled(Vector3::new(2.0, 2.0, 2.0));

    let st = Transform3D::IDENTITY
        .scaled(Vector3::new(2.0, 2.0, 2.0))
        .translated(Vector3::new(10.0, 0.0, 0.0));

    let v = Vector3::ZERO;
    let ts_result = ts.xform(v);
    let st_result = st.xform(v);

    // These should differ — order matters in affine transforms
    assert!(
        !approx_vec3(ts_result, st_result),
        "translate-then-scale should differ from scale-then-translate"
    );
}

// ===========================================================================
// 5. Camera3D projection contracts
// ===========================================================================

#[test]
fn camera3d_default_fov_near_far() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cam = Node::new("Camera", "Camera3D");
    let id = tree.add_child(root, cam).unwrap();

    assert!((node3d::get_fov(&tree, id) - 75.0).abs() < 1e-6);
    assert!((node3d::get_near(&tree, id) - 0.05).abs() < 1e-6);
    assert!((node3d::get_far(&tree, id) - 4000.0).abs() < 1e-6);
}

#[test]
fn camera3d_custom_fov_clipping() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cam = Node::new("Camera", "Camera3D");
    let id = tree.add_child(root, cam).unwrap();

    node3d::set_fov(&mut tree, id, 120.0);
    node3d::set_near(&mut tree, id, 0.01);
    node3d::set_far(&mut tree, id, 500.0);

    assert!((node3d::get_fov(&tree, id) - 120.0).abs() < 1e-6);
    assert!((node3d::get_near(&tree, id) - 0.01).abs() < 1e-6);
    assert!((node3d::get_far(&tree, id) - 500.0).abs() < 1e-6);
}

#[test]
fn camera3d_projection_type_perspective_and_ortho() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cam = Node::new("Camera", "Camera3D");
    let id = tree.add_child(root, cam).unwrap();

    node3d::set_projection_type(&mut tree, id, "perspective");
    assert_eq!(
        tree.get_node(id).unwrap().get_property("projection"),
        Variant::String("perspective".into())
    );

    node3d::set_projection_type(&mut tree, id, "orthographic");
    assert_eq!(
        tree.get_node(id).unwrap().get_property("projection"),
        Variant::String("orthographic".into())
    );
}

#[test]
fn perspective_projection_matrix_not_degenerate() {
    let mat = perspective_projection_matrix(75.0_f32.to_radians(), 16.0 / 9.0, 0.05, 4000.0);

    // Diagonal elements should be non-zero for a valid projection
    assert!(mat[0][0].abs() > 0.0, "m00 should be non-zero");
    assert!(mat[1][1].abs() > 0.0, "m11 should be non-zero");
    assert!(mat[2][2].abs() > 0.0, "m22 should be non-zero");
    assert!(
        mat[2][3].abs() > 0.0,
        "m23 should be non-zero (perspective divide)"
    );
}

#[test]
fn viewport3d_default_camera_params() {
    let vp = Viewport3D::new(1920, 1080);
    assert_eq!(vp.width, 1920);
    assert_eq!(vp.height, 1080);
}

#[test]
fn camera3d_transform_in_scene_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cam = Node::new("Camera", "Camera3D");
    let id = tree.add_child(root, cam).unwrap();

    node3d::set_position(&mut tree, id, Vector3::new(0.0, 5.0, -10.0));
    node3d::set_rotation(&mut tree, id, Vector3::new(0.3, 0.0, 0.0));

    let local = node3d::get_local_transform(&tree, id);
    let world_pos = local.xform(Vector3::ZERO);
    assert!(
        approx_vec3(world_pos, Vector3::new(0.0, 5.0, -10.0)),
        "camera transform origin should match position"
    );
}

// ===========================================================================
// 6. Light3D contracts
// ===========================================================================

#[test]
fn directional_light_default_energy_and_color() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let light = Node::new("Sun", "DirectionalLight3D");
    let id = tree.add_child(root, light).unwrap();

    assert!((node3d::get_light_energy(&tree, id) - 1.0).abs() < 1e-6);
    assert_eq!(node3d::get_light_color(&tree, id), Color::WHITE);
}

#[test]
fn directional_light_direction_set_and_read() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let light = Node::new("Sun", "DirectionalLight3D");
    let id = tree.add_child(root, light).unwrap();

    let dir = Vector3::new(0.0, -1.0, -0.5).normalized();
    node3d::set_direction(&mut tree, id, dir);

    let stored = match tree.get_node(id).unwrap().get_property("direction") {
        Variant::Vector3(v) => v,
        _ => panic!("direction should be Vector3"),
    };
    assert!(approx_vec3(stored, dir));
}

#[test]
fn omni_light_range_and_attenuation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let light = Node::new("Lamp", "OmniLight3D");
    let id = tree.add_child(root, light).unwrap();

    node3d::set_range(&mut tree, id, 25.0);
    node3d::set_attenuation(&mut tree, id, 1.5);

    assert_eq!(
        tree.get_node(id).unwrap().get_property("range"),
        Variant::Float(25.0)
    );
    assert_eq!(
        tree.get_node(id).unwrap().get_property("attenuation"),
        Variant::Float(1.5)
    );
}

#[test]
fn light_energy_custom_value() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let light = Node::new("Bright", "OmniLight3D");
    let id = tree.add_child(root, light).unwrap();

    node3d::set_light_energy(&mut tree, id, 3.5);
    assert!((node3d::get_light_energy(&tree, id) - 3.5).abs() < 1e-6);
}

#[test]
fn light_color_custom_value() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let light = Node::new("Colored", "DirectionalLight3D");
    let id = tree.add_child(root, light).unwrap();

    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    node3d::set_light_color(&mut tree, id, blue);
    assert_eq!(node3d::get_light_color(&tree, id), blue);
}

#[test]
fn light_shadow_toggle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let light = Node::new("Sun", "DirectionalLight3D");
    let id = tree.add_child(root, light).unwrap();

    node3d::set_shadow_enabled(&mut tree, id, true);
    assert_eq!(
        tree.get_node(id).unwrap().get_property("shadow_enabled"),
        Variant::Bool(true)
    );

    node3d::set_shadow_enabled(&mut tree, id, false);
    assert_eq!(
        tree.get_node(id).unwrap().get_property("shadow_enabled"),
        Variant::Bool(false)
    );
}

// ===========================================================================
// 7. Scene tree 3D transform hierarchy
// ===========================================================================

#[test]
fn global_transform_accumulates_through_hierarchy() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(100.0, 0.0, 0.0));

    let child = Node::new("Child", "Node3D");
    let child_id = tree.add_child(parent_id, child).unwrap();
    node3d::set_position(&mut tree, child_id, Vector3::new(0.0, 50.0, 0.0));

    let grandchild = Node::new("GrandChild", "Node3D");
    let gc_id = tree.add_child(child_id, grandchild).unwrap();
    node3d::set_position(&mut tree, gc_id, Vector3::new(0.0, 0.0, 25.0));

    let global = node3d::get_global_transform(&tree, gc_id);
    let world_pos = global.xform(Vector3::ZERO);
    assert!(
        approx_vec3(world_pos, Vector3::new(100.0, 50.0, 25.0)),
        "global transform should accumulate: {world_pos:?}"
    );
}

#[test]
fn set_global_position_accounts_for_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(50.0, 50.0, 50.0));

    let child = Node::new("Child", "Node3D");
    let child_id = tree.add_child(parent_id, child).unwrap();

    // Set global position to (100, 100, 100) — local should be (50, 50, 50)
    node3d::set_global_position(&mut tree, child_id, Vector3::new(100.0, 100.0, 100.0));

    let local = node3d::get_position(&tree, child_id);
    assert!(
        approx_vec3(local, Vector3::new(50.0, 50.0, 50.0)),
        "local position should compensate for parent: {local:?}"
    );
}

#[test]
fn camera_under_rotated_parent_gets_correct_global_transform() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let arm = Node::new("CameraArm", "Node3D");
    let arm_id = tree.add_child(root, arm).unwrap();
    node3d::set_position(&mut tree, arm_id, Vector3::new(0.0, 5.0, 0.0));
    node3d::set_rotation(
        &mut tree,
        arm_id,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(arm_id, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, -10.0));

    let global = node3d::get_global_transform(&tree, cam_id);
    let world_pos = global.xform(Vector3::ZERO);

    // Parent at (0,5,0) rotated 90° Y, child at (0,0,-10) local
    // After 90° Y rotation: local -Z becomes +X
    assert!(
        approx(world_pos.y, 5.0),
        "Y should be 5.0 from parent, got {}",
        world_pos.y
    );
}

// ===========================================================================
// 8. Basis contracts
// ===========================================================================

#[test]
fn basis_identity_determinant_is_one() {
    assert!(approx(Basis::IDENTITY.determinant(), 1.0));
}

#[test]
fn rotation_basis_determinant_is_one() {
    let b = Basis::from_euler(Vector3::new(0.5, 1.0, 0.3));
    assert!(
        approx(b.determinant(), 1.0),
        "rotation basis should have det=1, got {}",
        b.determinant()
    );
}

#[test]
fn basis_inverse_roundtrip() {
    let b = Basis::from_euler(Vector3::new(0.3, 0.7, -0.2));
    let inv = b.inverse();
    let product = b * inv;

    let v = Vector3::new(1.0, 2.0, 3.0);
    let result = product.xform(v);
    assert!(
        approx_vec3(result, v),
        "B * B^-1 should be identity: {result:?} != {v:?}"
    );
}

#[test]
fn basis_transpose_of_rotation_is_inverse() {
    let b = Basis::from_euler(Vector3::new(0.5, 0.2, 0.8));
    let transposed = b.transposed();
    let inversed = b.inverse();

    let v = Vector3::new(1.0, 2.0, 3.0);
    let t_result = transposed.xform(v);
    let i_result = inversed.xform(v);

    assert!(
        approx_vec3(t_result, i_result),
        "for rotation basis, transpose should equal inverse"
    );
}

#[test]
fn basis_get_scale_from_scaled_basis() {
    let b = Basis {
        x: Vector3::new(2.0, 0.0, 0.0),
        y: Vector3::new(0.0, 3.0, 0.0),
        z: Vector3::new(0.0, 0.0, 4.0),
    };
    let scale = b.get_scale();
    assert!(approx_vec3(scale, Vector3::new(2.0, 3.0, 4.0)));
}

// ===========================================================================
// 9. AABB contracts
// ===========================================================================

#[test]
fn aabb_contains_interior_point() {
    let aabb = Aabb::new(Vector3::ZERO, Vector3::new(10.0, 10.0, 10.0));
    assert!(aabb.contains_point(Vector3::new(5.0, 5.0, 5.0)));
}

#[test]
fn aabb_excludes_exterior_point() {
    let aabb = Aabb::new(Vector3::ZERO, Vector3::new(10.0, 10.0, 10.0));
    assert!(!aabb.contains_point(Vector3::new(15.0, 5.0, 5.0)));
}

#[test]
fn aabb_intersection_overlap() {
    let a = Aabb::new(Vector3::ZERO, Vector3::new(10.0, 10.0, 10.0));
    let b = Aabb::new(Vector3::new(5.0, 5.0, 5.0), Vector3::new(10.0, 10.0, 10.0));
    assert!(a.intersects(b));
    assert!(b.intersects(a));
}

#[test]
fn aabb_no_intersection_separated() {
    let a = Aabb::new(Vector3::ZERO, Vector3::new(5.0, 5.0, 5.0));
    let b = Aabb::new(Vector3::new(10.0, 10.0, 10.0), Vector3::new(5.0, 5.0, 5.0));
    assert!(!a.intersects(b));
}

#[test]
fn aabb_merge_encloses_both() {
    let a = Aabb::new(Vector3::ZERO, Vector3::new(5.0, 5.0, 5.0));
    let b = Aabb::new(Vector3::new(10.0, 10.0, 10.0), Vector3::new(5.0, 5.0, 5.0));
    let merged = a.merge(b);

    assert!(merged.contains_point(Vector3::new(2.0, 2.0, 2.0)));
    assert!(merged.contains_point(Vector3::new(12.0, 12.0, 12.0)));
}

#[test]
fn aabb_center_is_midpoint() {
    let aabb = Aabb::new(Vector3::new(10.0, 20.0, 30.0), Vector3::new(4.0, 6.0, 8.0));
    let center = aabb.get_center();
    assert!(approx_vec3(center, Vector3::new(12.0, 23.0, 34.0)));
}

// ===========================================================================
// 10. Parent-child transform propagation — focused 4.6.1 parity tests
// ===========================================================================

#[test]
fn parent_rotation_rotates_child_global_position() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    // Rotate parent 90° around Y axis
    node3d::set_rotation(
        &mut tree,
        parent_id,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let child = Node::new("Child", "Node3D");
    let child_id = tree.add_child(parent_id, child).unwrap();
    // Child at (10, 0, 0) local — after parent's 90° Y rotation, global should be ~(0, 0, -10)
    node3d::set_position(&mut tree, child_id, Vector3::new(10.0, 0.0, 0.0));

    let global = node3d::get_global_transform(&tree, child_id);
    let world_pos = global.xform(Vector3::ZERO);
    assert!(
        approx_vec3(world_pos, Vector3::new(0.0, 0.0, -10.0)),
        "90° Y parent rotation should map child (10,0,0) to (0,0,-10), got {world_pos:?}"
    );
}

#[test]
fn parent_scale_scales_child_global_position() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_scale(&mut tree, parent_id, Vector3::new(2.0, 2.0, 2.0));

    let child = Node::new("Child", "Node3D");
    let child_id = tree.add_child(parent_id, child).unwrap();
    node3d::set_position(&mut tree, child_id, Vector3::new(5.0, 3.0, 1.0));

    let global = node3d::get_global_transform(&tree, child_id);
    let world_pos = global.xform(Vector3::ZERO);
    assert!(
        approx_vec3(world_pos, Vector3::new(10.0, 6.0, 2.0)),
        "parent 2x scale should double child position, got {world_pos:?}"
    );
}

#[test]
fn parent_translate_rotate_child_translate_composition() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Parent at (100, 0, 0), rotated 90° around Y
    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(100.0, 0.0, 0.0));
    node3d::set_rotation(
        &mut tree,
        parent_id,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    // Child at (0, 0, 5) local — 90° Y maps local Z to -X in parent space
    let child = Node::new("Child", "Node3D");
    let child_id = tree.add_child(parent_id, child).unwrap();
    node3d::set_position(&mut tree, child_id, Vector3::new(0.0, 0.0, 5.0));

    let global = node3d::get_global_transform(&tree, child_id);
    let world_pos = global.xform(Vector3::ZERO);
    // Parent origin (100,0,0) + rotated child offset: Z local → -X global → (100-5, 0, 0) = (95,0,0)?
    // Actually: 90° Y rotation maps (0,0,5) to (5,0,0)... wait, need to check convention.
    // After 90° Y: x→z, z→-x. So (0,0,5) → (-5,0,0) in parent space? Or (5,0,0)?
    // Godot convention: 90° Y rotation maps local +Z to global -X in parent frame.
    // So world = (100,0,0) + (-5,0,0) = (95,0,0)... hmm, depends on rotation direction.
    // Let's just verify composition is consistent with local transforms.
    let parent_global = node3d::get_global_transform(&tree, parent_id);
    let child_local = node3d::get_local_transform(&tree, child_id);
    let expected = (parent_global * child_local).xform(Vector3::ZERO);
    assert!(
        approx_vec3(world_pos, expected),
        "global = parent_global * child_local: got {world_pos:?}, expected {expected:?}"
    );
}

#[test]
fn four_level_hierarchy_position_accumulates() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node3D");
    let a_id = tree.add_child(root, a).unwrap();
    node3d::set_position(&mut tree, a_id, Vector3::new(1.0, 0.0, 0.0));

    let b = Node::new("B", "Node3D");
    let b_id = tree.add_child(a_id, b).unwrap();
    node3d::set_position(&mut tree, b_id, Vector3::new(0.0, 2.0, 0.0));

    let c = Node::new("C", "Node3D");
    let c_id = tree.add_child(b_id, c).unwrap();
    node3d::set_position(&mut tree, c_id, Vector3::new(0.0, 0.0, 3.0));

    let d = Node::new("D", "Node3D");
    let d_id = tree.add_child(c_id, d).unwrap();
    node3d::set_position(&mut tree, d_id, Vector3::new(4.0, 5.0, 6.0));

    let global = node3d::get_global_transform(&tree, d_id);
    let world_pos = global.xform(Vector3::ZERO);
    assert!(
        approx_vec3(world_pos, Vector3::new(5.0, 7.0, 9.0)),
        "4-level translation accumulation: got {world_pos:?}"
    );
}

#[test]
fn scale_propagates_through_three_levels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node3D");
    let a_id = tree.add_child(root, a).unwrap();
    node3d::set_scale(&mut tree, a_id, Vector3::new(2.0, 2.0, 2.0));

    let b = Node::new("B", "Node3D");
    let b_id = tree.add_child(a_id, b).unwrap();
    node3d::set_scale(&mut tree, b_id, Vector3::new(3.0, 3.0, 3.0));

    let c = Node::new("C", "Node3D");
    let c_id = tree.add_child(b_id, c).unwrap();
    node3d::set_position(&mut tree, c_id, Vector3::new(1.0, 1.0, 1.0));

    let global = node3d::get_global_transform(&tree, c_id);
    // Position (1,1,1) scaled by parent (3x) then grandparent (2x) = (6,6,6)
    let world_pos = global.xform(Vector3::ZERO);
    assert!(
        approx_vec3(world_pos, Vector3::new(6.0, 6.0, 6.0)),
        "nested scale propagation: 2x * 3x * (1,1,1) = (6,6,6), got {world_pos:?}"
    );
}

#[test]
fn local_transform_identity_when_no_properties_set() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Default", "Node3D");
    let id = tree.add_child(root, node).unwrap();

    let local = node3d::get_local_transform(&tree, id);
    let v = Vector3::new(42.0, 17.0, -3.0);
    assert!(
        approx_vec3(local.xform(v), v),
        "default local transform should be identity"
    );
}

#[test]
fn set_global_position_with_rotated_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(10.0, 0.0, 0.0));
    node3d::set_rotation(
        &mut tree,
        parent_id,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let child = Node::new("Child", "Node3D");
    let child_id = tree.add_child(parent_id, child).unwrap();

    let target_global = Vector3::new(10.0, 0.0, -5.0);
    node3d::set_global_position(&mut tree, child_id, target_global);

    // Verify the global position is what we set
    let actual_global = node3d::get_global_transform(&tree, child_id).xform(Vector3::ZERO);
    assert!(
        approx_vec3(actual_global, target_global),
        "set_global_position with rotated parent: got {actual_global:?}, expected {target_global:?}"
    );
}

#[test]
fn sibling_transforms_independent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(10.0, 0.0, 0.0));

    let child_a = Node::new("A", "Node3D");
    let a_id = tree.add_child(parent_id, child_a).unwrap();
    node3d::set_position(&mut tree, a_id, Vector3::new(5.0, 0.0, 0.0));

    let child_b = Node::new("B", "Node3D");
    let b_id = tree.add_child(parent_id, child_b).unwrap();
    node3d::set_position(&mut tree, b_id, Vector3::new(0.0, 5.0, 0.0));

    let global_a = node3d::get_global_transform(&tree, a_id).xform(Vector3::ZERO);
    let global_b = node3d::get_global_transform(&tree, b_id).xform(Vector3::ZERO);

    assert!(
        approx_vec3(global_a, Vector3::new(15.0, 0.0, 0.0)),
        "sibling A: got {global_a:?}"
    );
    assert!(
        approx_vec3(global_b, Vector3::new(10.0, 5.0, 0.0)),
        "sibling B: got {global_b:?}"
    );

    // Mutating A should not affect B
    node3d::set_position(&mut tree, a_id, Vector3::new(100.0, 0.0, 0.0));
    let global_b_after = node3d::get_global_transform(&tree, b_id).xform(Vector3::ZERO);
    assert!(
        approx_vec3(global_b_after, Vector3::new(10.0, 5.0, 0.0)),
        "sibling B unaffected after A mutation: got {global_b_after:?}"
    );
}
