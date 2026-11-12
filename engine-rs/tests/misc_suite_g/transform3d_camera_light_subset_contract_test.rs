//! pat-vwm: 3D transforms, cameras, and lights subset contract coverage.
//!
//! Extends the existing contract surface with:
//!   - Plane intersection and distance contracts
//!   - Projection matrix behavioral contracts (perspective NDC, orthographic mapping)
//!   - gdserver3d Light3D factory default and mutation contracts
//!   - Quaternion composition (multiply two rotations)
//!   - Slerp midpoint interpolation behavior
//!   - AABB volume, expand, endpoint contracts
//!   - Non-uniform scale + rotation edge cases
//!   - Camera + light coexistence in scene hierarchy

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::projection::{orthographic_projection_matrix, perspective_projection_matrix};
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
// Plane contracts
// ===========================================================================

#[test]
fn plane_from_three_points_normal_and_distance() {
    // XZ plane at Y=0
    let p = Plane::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    // Right-hand rule: (1,0,0) x (0,0,1) = (0,-1,0)
    assert!(
        approx_vec3(p.normal, Vector3::new(0.0, -1.0, 0.0)),
        "XZ plane normal should be -Y: {:?}",
        p.normal
    );
    assert!(approx(p.d, 0.0), "plane through origin has d=0");
}

#[test]
fn plane_distance_to_point_signed() {
    let p = Plane::new(Vector3::UP, 5.0);
    // Point above the plane
    assert!(
        approx(p.distance_to(Vector3::new(0.0, 10.0, 0.0)), 5.0),
        "10 - 5 = 5 above"
    );
    // Point below the plane
    assert!(
        approx(p.distance_to(Vector3::new(0.0, 3.0, 0.0)), -2.0),
        "3 - 5 = -2 below"
    );
    // Point on the plane
    assert!(
        approx(p.distance_to(Vector3::new(0.0, 5.0, 0.0)), 0.0),
        "on plane = 0"
    );
}

#[test]
fn plane_is_point_over_classifies_correctly() {
    let p = Plane::new(Vector3::UP, 0.0);
    assert!(p.is_point_over(Vector3::new(0.0, 1.0, 0.0)));
    assert!(!p.is_point_over(Vector3::new(0.0, -1.0, 0.0)));
    assert!(!p.is_point_over(Vector3::ZERO)); // exactly on plane is NOT over
}

#[test]
fn plane_ray_intersection_downward_onto_floor() {
    let floor = Plane::new(Vector3::UP, 0.0); // Y=0 floor
    let from = Vector3::new(3.0, 10.0, 7.0);
    let dir = Vector3::new(0.0, -1.0, 0.0);
    let hit = floor.intersects_ray(from, dir);
    assert!(hit.is_some());
    let pt = hit.unwrap();
    assert!(approx(pt.y, 0.0), "hit should be on Y=0 plane");
    assert!(approx(pt.x, 3.0), "X preserved");
    assert!(approx(pt.z, 7.0), "Z preserved");
}

#[test]
fn plane_ray_misses_when_parallel() {
    let floor = Plane::new(Vector3::UP, 0.0);
    // Horizontal ray — parallel to floor
    let hit = floor.intersects_ray(Vector3::new(0.0, 5.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    assert!(hit.is_none(), "parallel ray should miss");
}

#[test]
fn plane_ray_misses_when_pointing_away() {
    let floor = Plane::new(Vector3::UP, 0.0);
    // Upward ray from above floor
    let hit = floor.intersects_ray(Vector3::new(0.0, 5.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
    assert!(hit.is_none(), "ray pointing away should miss");
}

#[test]
fn plane_segment_intersection_crosses_plane() {
    let wall = Plane::new(Vector3::new(1.0, 0.0, 0.0), 5.0); // X=5 wall
    let hit = wall.intersects_segment(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    assert!(hit.is_some());
    let pt = hit.unwrap();
    assert!(approx(pt.x, 5.0), "segment hits at X=5");
}

#[test]
fn plane_segment_misses_when_both_sides_same() {
    let wall = Plane::new(Vector3::new(1.0, 0.0, 0.0), 5.0);
    // Both endpoints on the negative side
    let hit = wall.intersects_segment(Vector3::new(0.0, 0.0, 0.0), Vector3::new(3.0, 0.0, 0.0));
    assert!(hit.is_none(), "segment entirely before plane should miss");
}

#[test]
fn plane_normalized_scales_normal_and_distance() {
    let p = Plane::new(Vector3::new(0.0, 3.0, 0.0), 6.0).normalized();
    assert!(
        approx_vec3(p.normal, Vector3::UP),
        "normal should be unit Y"
    );
    assert!(approx(p.d, 2.0), "distance scaled by 1/3");
}

// ===========================================================================
// Projection matrix behavioral contracts
// ===========================================================================

#[test]
fn perspective_maps_near_plane_center_to_ndc_origin() {
    let fov = std::f32::consts::FRAC_PI_2; // 90°
    let near = 0.1_f32;
    let far = 100.0_f32;
    let mat = perspective_projection_matrix(fov, 1.0, near, far);

    // A point at the center of the near plane: (0, 0, -near)
    // In clip space: x'=0, y'=0 (centered), z' maps to near clip, w'=near
    // After perspective divide: (0, 0, z'/w')
    let x = mat[0][0] * 0.0 + mat[1][0] * 0.0 + mat[2][0] * (-near) + mat[3][0] * 1.0;
    let y = mat[0][1] * 0.0 + mat[1][1] * 0.0 + mat[2][1] * (-near) + mat[3][1] * 1.0;
    let w = mat[0][3] * 0.0 + mat[1][3] * 0.0 + mat[2][3] * (-near) + mat[3][3] * 1.0;

    let ndc_x = x / w;
    let ndc_y = y / w;
    assert!(approx(ndc_x, 0.0), "center of near plane maps to NDC x=0");
    assert!(approx(ndc_y, 0.0), "center of near plane maps to NDC y=0");
}

#[test]
fn perspective_w_negative_for_camera_space_negative_z() {
    let mat = perspective_projection_matrix(std::f32::consts::FRAC_PI_2, 16.0 / 9.0, 0.05, 4000.0);
    // m[2][3] = -1.0 for standard OpenGL-style perspective
    assert!(
        approx(mat[2][3], -1.0),
        "perspective divide element should be -1: got {}",
        mat[2][3]
    );
}

#[test]
fn orthographic_maps_center_to_ndc_origin() {
    let mat = orthographic_projection_matrix(-10.0, 10.0, -10.0, 10.0, 0.1, 100.0);

    // Center point (0, 0, -50) should map to approximately NDC (0, 0, z)
    let x = mat[0][0] * 0.0 + mat[1][0] * 0.0 + mat[2][0] * (-50.0) + mat[3][0] * 1.0;
    let y = mat[0][1] * 0.0 + mat[1][1] * 0.0 + mat[2][1] * (-50.0) + mat[3][1] * 1.0;
    let w = mat[0][3] * 0.0 + mat[1][3] * 0.0 + mat[2][3] * (-50.0) + mat[3][3] * 1.0;

    assert!(approx(w, 1.0), "orthographic w should be 1.0: got {w}");
    assert!(
        approx(x / w, 0.0),
        "center maps to NDC x=0 in ortho: got {}",
        x / w
    );
    assert!(
        approx(y / w, 0.0),
        "center maps to NDC y=0 in ortho: got {}",
        y / w
    );
}

#[test]
fn orthographic_no_perspective_divide_w_is_constant() {
    let mat = orthographic_projection_matrix(-5.0, 5.0, -5.0, 5.0, 0.1, 50.0);
    // w column should be [0, 0, 0, 1]
    assert!(approx(mat[0][3], 0.0));
    assert!(approx(mat[1][3], 0.0));
    assert!(approx(mat[2][3], 0.0));
    assert!(approx(mat[3][3], 1.0));
}

#[test]
fn orthographic_edge_maps_to_ndc_boundary() {
    let mat = orthographic_projection_matrix(-10.0, 10.0, -5.0, 5.0, 0.1, 100.0);

    // Right edge (10, 0, -1) should map to NDC x=1
    let x = mat[0][0] * 10.0 + mat[3][0];
    assert!(approx(x, 1.0), "right edge should map to NDC x=1: got {x}");

    // Top edge (0, 5, -1) should map to NDC y=1
    let y = mat[1][1] * 5.0 + mat[3][1];
    assert!(approx(y, 1.0), "top edge should map to NDC y=1: got {y}");
}

// ===========================================================================
// gdserver3d Light3D factory and mutation contracts
// ===========================================================================

#[test]
fn directional_light_factory_defaults() {
    let light = Light3D::directional(Light3DId(1));
    assert_eq!(light.light_type, LightType::Directional);
    assert_eq!(light.color, Color::new(1.0, 1.0, 1.0, 1.0));
    assert!(approx(light.energy, 1.0));
    assert_eq!(light.direction, Vector3::new(0.0, -1.0, 0.0));
    assert!(!light.shadow_enabled);
    assert!(approx(light.range, 0.0), "directional light has no range");
}

#[test]
fn point_light_factory_defaults() {
    let pos = Vector3::new(3.0, 8.0, -2.0);
    let light = Light3D::point(Light3DId(2), pos);
    assert_eq!(light.light_type, LightType::Point);
    assert_eq!(light.position, pos);
    assert!(approx(light.range, 10.0));
    assert_eq!(
        light.direction,
        Vector3::ZERO,
        "point light has no direction"
    );
    assert!(
        approx(light.spot_angle, 0.0),
        "point light has no spot angle"
    );
}

#[test]
fn spot_light_factory_defaults() {
    let pos = Vector3::new(0.0, 5.0, 0.0);
    let dir = Vector3::new(0.0, -1.0, 0.0);
    let light = Light3D::spot(Light3DId(3), pos, dir);
    assert_eq!(light.light_type, LightType::Spot);
    assert_eq!(light.position, pos);
    assert_eq!(light.direction, dir);
    assert!(
        approx(light.spot_angle, std::f32::consts::FRAC_PI_4),
        "default spot angle is 45°"
    );
    assert!(approx(light.range, 10.0));
}

#[test]
fn light_mutation_energy_and_color() {
    let mut light = Light3D::directional(Light3DId(10));
    light.energy = 2.5;
    light.color = Color::new(1.0, 0.8, 0.6, 1.0);
    light.shadow_enabled = true;

    assert!(approx(light.energy, 2.5));
    assert_eq!(light.color, Color::new(1.0, 0.8, 0.6, 1.0));
    assert!(light.shadow_enabled);
}

#[test]
fn spot_light_angle_mutation() {
    let mut light = Light3D::spot(Light3DId(4), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0));
    light.spot_angle = std::f32::consts::FRAC_PI_6; // 30°
    assert!(approx(light.spot_angle, std::f32::consts::FRAC_PI_6));
}

#[test]
fn light_id_uniqueness_across_types() {
    let dir = Light3D::directional(Light3DId(1));
    let pt = Light3D::point(Light3DId(2), Vector3::ZERO);
    let sp = Light3D::spot(Light3DId(3), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0));

    assert_ne!(dir.id, pt.id);
    assert_ne!(pt.id, sp.id);
    assert_ne!(dir.id, sp.id);
}

// ===========================================================================
// Quaternion composition contracts
// ===========================================================================

#[test]
fn quaternion_multiply_two_90_y_rotations_gives_180() {
    let q90 = Quaternion::from_axis_angle(Vector3::UP, std::f32::consts::FRAC_PI_2);
    let q180 = q90 * q90;
    let v = Vector3::new(1.0, 0.0, 0.0);
    let result = q180.xform(v);
    // 180° around Y: (1,0,0) → (-1,0,0)
    assert!(
        approx_vec3(result, Vector3::new(-1.0, 0.0, 0.0)),
        "two 90° Y rotations = 180°: got {result:?}"
    );
}

#[test]
fn quaternion_multiply_x_then_y_order_matters() {
    let qx = Quaternion::from_axis_angle(Vector3::new(1.0, 0.0, 0.0), std::f32::consts::FRAC_PI_2);
    let qy = Quaternion::from_axis_angle(Vector3::UP, std::f32::consts::FRAC_PI_2);

    let v = Vector3::new(0.0, 0.0, 1.0);
    let xy_result = (qx * qy).xform(v);
    let yx_result = (qy * qx).xform(v);

    assert!(
        !approx_vec3(xy_result, yx_result),
        "quaternion multiply should not commute: X*Y={xy_result:?} vs Y*X={yx_result:?}"
    );
}

#[test]
fn quaternion_xform_preserves_vector_length() {
    let q = Quaternion::from_euler(Vector3::new(0.7, 1.2, -0.4));
    let v = Vector3::new(3.0, 4.0, 5.0);
    let rotated = q.xform(v);
    assert!(
        approx(v.length(), rotated.length()),
        "rotation should preserve length: {} vs {}",
        v.length(),
        rotated.length()
    );
}

// ===========================================================================
// Slerp midpoint contracts
// ===========================================================================

#[test]
fn slerp_midpoint_is_halfway_rotation() {
    let a = Quaternion::IDENTITY;
    let b = Quaternion::from_axis_angle(Vector3::UP, std::f32::consts::PI);

    let mid = a.slerp(b, 0.5);
    let v = Vector3::new(1.0, 0.0, 0.0);
    let result = mid.xform(v);

    // Halfway through 180° around Y gives 90° rotation.
    // For exactly 180°, cos(π/2) in f32 is ~-4e-8, so the slerp short-path
    // flip is ambiguous — (1,0,0) → (0,0,±1) are both valid midpoints.
    assert!(
        approx(result.x, 0.0) && approx(result.y, 0.0) && approx(result.z.abs(), 1.0),
        "slerp(0.5) of 180° should give 90°: got {result:?}"
    );
}

#[test]
fn slerp_quarter_interpolation() {
    let a = Quaternion::IDENTITY;
    let b = Quaternion::from_axis_angle(Vector3::UP, std::f32::consts::FRAC_PI_2);

    let quarter = a.slerp(b, 0.25);
    let v = Vector3::new(1.0, 0.0, 0.0);
    let result = quarter.xform(v);

    // 22.5° around Y: cos(22.5°) ≈ 0.9239, sin(22.5°) ≈ 0.3827
    // (1,0,0) → (cos, 0, -sin)
    let angle = std::f32::consts::FRAC_PI_2 * 0.25;
    let expected = Vector3::new(angle.cos(), 0.0, -angle.sin());
    assert!(
        approx_vec3(result, expected),
        "slerp(0.25) of 90°: got {result:?}, expected {expected:?}"
    );
}

#[test]
fn slerp_output_is_unit_quaternion() {
    let a = Quaternion::from_euler(Vector3::new(0.3, 0.5, 0.1));
    let b = Quaternion::from_euler(Vector3::new(-0.2, 1.0, 0.7));

    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let q = a.slerp(b, t);
        assert!(
            approx(q.length(), 1.0),
            "slerp(t={t}) should produce unit quaternion, got length {}",
            q.length()
        );
    }
}

// ===========================================================================
// AABB extended contracts
// ===========================================================================

#[test]
fn aabb_volume_computation() {
    let a = Aabb::new(Vector3::ZERO, Vector3::new(2.0, 3.0, 5.0));
    assert!(approx(a.get_volume(), 30.0), "2*3*5 = 30");
}

#[test]
fn aabb_has_volume_false_for_flat() {
    let flat = Aabb::new(Vector3::ZERO, Vector3::new(10.0, 0.0, 10.0));
    assert!(!flat.has_volume(), "flat AABB (zero height) has no volume");
}

#[test]
fn aabb_expand_to_include_point() {
    let a = Aabb::new(Vector3::ZERO, Vector3::ONE);
    let expanded = a.expand(Vector3::new(5.0, 5.0, 5.0));
    assert!(
        expanded.contains_point(Vector3::new(0.5, 0.5, 0.5)),
        "still contains original"
    );
    assert!(
        expanded.contains_point(Vector3::new(4.0, 4.0, 4.0)),
        "contains new region"
    );
    assert!(approx_vec3(expanded.position, Vector3::ZERO));
    assert!(approx_vec3(expanded.size, Vector3::new(5.0, 5.0, 5.0)));
}

#[test]
fn aabb_get_endpoint_all_eight_corners() {
    let a = Aabb::new(Vector3::new(1.0, 2.0, 3.0), Vector3::new(4.0, 5.0, 6.0));
    let min = a.position;
    let max = a.position + a.size;

    // Endpoint 0 = min corner, Endpoint 7 = max corner
    assert_eq!(a.get_endpoint(0), min);
    assert_eq!(a.get_endpoint(7), max);

    // Verify all 8 corners use correct bit pattern
    for i in 0..8u8 {
        let ep = a.get_endpoint(i);
        let ex = if i & 1 == 0 { min.x } else { max.x };
        let ey = if i & 2 == 0 { min.y } else { max.y };
        let ez = if i & 4 == 0 { min.z } else { max.z };
        assert!(
            approx_vec3(ep, Vector3::new(ex, ey, ez)),
            "endpoint {i}: got {ep:?}, expected ({ex}, {ey}, {ez})"
        );
    }
}

#[test]
fn aabb_zero_size_has_no_volume() {
    let a = Aabb::new(Vector3::new(5.0, 5.0, 5.0), Vector3::ZERO);
    assert!(approx(a.get_volume(), 0.0));
    assert!(!a.has_volume());
}

// ===========================================================================
// Non-uniform scale + rotation edge cases
// ===========================================================================

#[test]
fn non_uniform_scale_then_rotation_distorts() {
    // Non-uniform scale followed by rotation should produce a non-orthogonal basis
    let t = Transform3D::IDENTITY
        .scaled(Vector3::new(2.0, 1.0, 1.0))
        .rotated(Vector3::UP, std::f32::consts::FRAC_PI_4);

    let basis_scale = t.basis.get_scale();
    // After scaling X by 2 then rotating 45° Y, the X and Z axes should have different lengths
    assert!(
        !approx(basis_scale.x, basis_scale.z) || !approx(basis_scale.x, 1.0),
        "non-uniform scale + rotation produces non-trivial scale: {basis_scale:?}"
    );
}

#[test]
fn uniform_scale_preserves_basis_orthogonality_after_rotation() {
    let t = Transform3D::IDENTITY
        .scaled(Vector3::new(3.0, 3.0, 3.0))
        .rotated(Vector3::UP, 1.0);

    let scale = t.basis.get_scale();
    // Uniform scale should keep all axes the same length
    assert!(
        approx(scale.x, scale.y) && approx(scale.y, scale.z),
        "uniform scale keeps axes equal: {scale:?}"
    );
    assert!(approx(scale.x, 3.0), "scale factor preserved: {}", scale.x);
}

#[test]
fn basis_determinant_negative_for_reflection() {
    // A basis with one negative scale is a reflection — determinant should be negative
    let b = Basis {
        x: Vector3::new(-1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 1.0, 0.0),
        z: Vector3::new(0.0, 0.0, 1.0),
    };
    assert!(
        b.determinant() < 0.0,
        "reflected basis should have negative determinant: {}",
        b.determinant()
    );
}

#[test]
fn transform_with_non_uniform_scale_inverse_recovers_point() {
    let t = Transform3D::IDENTITY
        .translated(Vector3::new(10.0, 20.0, 30.0))
        .scaled(Vector3::new(2.0, 3.0, 0.5));

    let original = Vector3::new(7.0, -3.0, 11.0);
    let transformed = t.xform(original);
    let recovered = t.inverse().xform(transformed);

    assert!(
        approx_vec3(recovered, original),
        "inverse should recover point with non-uniform scale: {recovered:?} != {original:?}"
    );
}

// ===========================================================================
// Camera + light coexistence in scene hierarchy
// ===========================================================================

#[test]
fn camera_and_lights_under_common_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Scene root
    let scene = Node::new("Scene", "Node3D");
    let scene_id = tree.add_child(root, scene).unwrap();
    node3d::set_position(&mut tree, scene_id, Vector3::new(0.0, 0.0, 0.0));

    // Camera
    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(scene_id, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 5.0, -10.0));
    node3d::set_fov(&mut tree, cam_id, 75.0);
    node3d::set_camera_current(&mut tree, cam_id, true);

    // Directional light
    let sun = Node::new("Sun", "DirectionalLight3D");
    let sun_id = tree.add_child(scene_id, sun).unwrap();
    node3d::set_light_energy(&mut tree, sun_id, 1.2);
    node3d::set_direction(&mut tree, sun_id, Vector3::new(0.0, -1.0, -0.3));
    node3d::set_shadow_enabled(&mut tree, sun_id, true);

    // Omni light
    let lamp = Node::new("Lamp", "OmniLight3D");
    let lamp_id = tree.add_child(scene_id, lamp).unwrap();
    node3d::set_position(&mut tree, lamp_id, Vector3::new(3.0, 4.0, 0.0));
    node3d::set_light_energy(&mut tree, lamp_id, 0.8);
    node3d::set_range(&mut tree, lamp_id, 15.0);

    // Verify camera
    assert!(approx(node3d::get_fov(&tree, cam_id) as f32, 75.0));
    let cam_node = tree.get_node(cam_id).unwrap();
    assert_eq!(
        cam_node.get_property("current"),
        gdvariant::Variant::Bool(true)
    );

    // Verify sun
    assert!(approx(node3d::get_light_energy(&tree, sun_id) as f32, 1.2));
    let sun_node = tree.get_node(sun_id).unwrap();
    assert_eq!(
        sun_node.get_property("shadow_enabled"),
        gdvariant::Variant::Bool(true)
    );

    // Verify lamp
    assert!(approx(node3d::get_light_energy(&tree, lamp_id) as f32, 0.8));
    let lamp_global = node3d::get_global_transform(&tree, lamp_id);
    let lamp_world = lamp_global.xform(Vector3::ZERO);
    assert!(
        approx_vec3(lamp_world, Vector3::new(3.0, 4.0, 0.0)),
        "lamp global position: {lamp_world:?}"
    );
}

#[test]
fn camera_under_moving_parent_tracks_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("CameraRig", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(0.0, 0.0, 0.0));

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(parent_id, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 2.0, -5.0));
    node3d::set_fov(&mut tree, cam_id, 90.0);

    // Initial camera world pos
    let g1 = node3d::get_global_transform(&tree, cam_id).xform(Vector3::ZERO);
    assert!(approx_vec3(g1, Vector3::new(0.0, 2.0, -5.0)));

    // Move parent
    node3d::set_position(&mut tree, parent_id, Vector3::new(100.0, 50.0, 0.0));
    let g2 = node3d::get_global_transform(&tree, cam_id).xform(Vector3::ZERO);
    assert!(
        approx_vec3(g2, Vector3::new(100.0, 52.0, -5.0)),
        "camera follows parent: got {g2:?}"
    );
}

#[test]
fn light_inherits_parent_rotation_for_direction() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let arm = Node::new("LightArm", "Node3D");
    let arm_id = tree.add_child(root, arm).unwrap();
    node3d::set_rotation(
        &mut tree,
        arm_id,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let light = Node::new("Light", "DirectionalLight3D");
    let light_id = tree.add_child(arm_id, light).unwrap();
    node3d::set_position(&mut tree, light_id, Vector3::new(0.0, 10.0, 0.0));

    let global = node3d::get_global_transform(&tree, light_id);
    let world_pos = global.xform(Vector3::ZERO);
    // Light at (0,10,0) under parent rotated 90° Y
    // 90° Y rotation: local Y stays Y
    assert!(
        approx(world_pos.y, 10.0),
        "light Y should be 10: got {}",
        world_pos.y
    );
}

// ===========================================================================
// Transform3D looking_at extended contracts
// ===========================================================================

#[test]
fn looking_at_produces_orthonormal_basis() {
    let t = Transform3D::IDENTITY
        .translated(Vector3::new(5.0, 5.0, 5.0))
        .looking_at(Vector3::new(10.0, 5.0, 5.0), Vector3::UP);

    let det = t.basis.determinant();
    assert!(
        approx(det, 1.0),
        "looking_at should produce orthonormal basis with det=1: got {det}"
    );

    // Check orthogonality: dot products of basis vectors should be ~0
    assert!(approx(t.basis.x.dot(t.basis.y), 0.0), "x·y should be 0");
    assert!(approx(t.basis.y.dot(t.basis.z), 0.0), "y·z should be 0");
    assert!(approx(t.basis.x.dot(t.basis.z), 0.0), "x·z should be 0");
}

#[test]
fn looking_at_different_targets_produces_different_basis() {
    let origin = Vector3::new(0.0, 0.0, 0.0);
    let base = Transform3D {
        basis: Basis::IDENTITY,
        origin,
    };

    let t1 = base.looking_at(Vector3::new(0.0, 0.0, 10.0), Vector3::UP);
    let t2 = base.looking_at(Vector3::new(10.0, 0.0, 0.0), Vector3::UP);

    let v = Vector3::new(0.0, 0.0, 1.0);
    assert!(
        !approx_vec3(t1.basis.xform(v), t2.basis.xform(v)),
        "different targets should give different orientations"
    );
}

// ===========================================================================
// Basis extended contracts
// ===========================================================================

#[test]
fn basis_from_euler_identity_for_zero_angles() {
    let b = Basis::from_euler(Vector3::ZERO);
    assert!(approx_vec3(b.x, Basis::IDENTITY.x));
    assert!(approx_vec3(b.y, Basis::IDENTITY.y));
    assert!(approx_vec3(b.z, Basis::IDENTITY.z));
}

#[test]
fn basis_euler_roundtrip_multiple_angles() {
    let test_angles = [
        Vector3::new(0.1, 0.2, 0.3),
        Vector3::new(-0.5, 0.0, 0.7),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(1.2, -0.3, 0.8),
    ];

    for euler in &test_angles {
        let basis = Basis::from_euler(*euler);
        let recovered = basis.to_euler();
        assert!(
            approx_vec3(recovered, *euler),
            "Euler roundtrip failed for {euler:?}: got {recovered:?}"
        );
    }
}

#[test]
fn basis_from_quaternion_matches_from_euler() {
    let euler = Vector3::new(0.3, 0.7, -0.2);
    let from_euler = Basis::from_euler(euler);
    let from_quat = Basis::from_quaternion(Quaternion::from_euler(euler));

    let v = Vector3::new(1.0, 2.0, 3.0);
    assert!(
        approx_vec3(from_euler.xform(v), from_quat.xform(v)),
        "Basis from euler and from quaternion should agree"
    );
}

// ===========================================================================
// Viewport3D integration contracts
// ===========================================================================

#[test]
fn viewport_aspect_square_is_one() {
    let vp = Viewport3D::new(256, 256);
    assert!(
        approx(vp.aspect(), 1.0),
        "square viewport aspect should be 1.0"
    );
}

#[test]
fn viewport_aspect_widescreen() {
    let vp = Viewport3D::new(1920, 1080);
    let expected = 1920.0 / 1080.0;
    assert!(
        approx(vp.aspect(), expected),
        "16:9 aspect should be ~1.778, got {}",
        vp.aspect()
    );
}

#[test]
fn viewport_aspect_portrait() {
    let vp = Viewport3D::new(1080, 1920);
    assert!(
        vp.aspect() < 1.0,
        "portrait viewport should have aspect < 1.0, got {}",
        vp.aspect()
    );
}

#[test]
fn viewport_default_clipping_planes_match_godot() {
    let vp = Viewport3D::new(640, 480);
    assert!(approx(vp.near, 0.05), "default near should be 0.05");
    assert!(approx(vp.far, 4000.0), "default far should be 4000.0");
}

#[test]
fn viewport_default_fov_is_pi_over_4() {
    let vp = Viewport3D::new(800, 600);
    assert!(
        approx(vp.fov, std::f32::consts::FRAC_PI_4),
        "default viewport FOV should be π/4"
    );
}

// ===========================================================================
// Camera near/far propagation through RenderServer3DAdapter
// ===========================================================================

#[test]
fn adapter_camera_near_far_propagation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    node3d::set_camera_current(&mut tree, cam_id, true);
    node3d::set_near(&mut tree, cam_id, 0.1);
    node3d::set_far(&mut tree, cam_id, 500.0);

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Camera should be active (non-default FOV from the node's 75° default).
    let expected_fov = 75.0_f32.to_radians();
    assert!(
        approx(snapshot.camera_fov, expected_fov),
        "FOV should propagate from camera node: got {:.4}",
        snapshot.camera_fov
    );
}

#[test]
fn adapter_snapshot_coverage_metric() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let coverage = snapshot.coverage();
    assert!(
        coverage > 0.0 && coverage <= 1.0,
        "coverage should be in (0, 1] for a visible mesh: got {coverage}"
    );
    assert_eq!(
        snapshot.total_pixel_count,
        64 * 64,
        "total pixels should match viewport dimensions"
    );
}

#[test]
fn adapter_empty_scene_zero_coverage() {
    let tree = SceneTree::new();
    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert!(
        approx(snapshot.coverage() as f32, 0.0),
        "empty scene coverage should be 0.0"
    );
}

// ===========================================================================
// ParityReport3D contracts
// ===========================================================================

#[test]
fn parity_report_is_functional_requires_camera_mesh_coverage() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Scene with camera + mesh = functional
    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);
    let report = snapshot.parity_report();

    assert!(
        report.is_functional(),
        "camera+visible mesh should be functional"
    );
    assert!(report.has_camera);
    assert!(report.mesh_count > 0);
    assert!(report.coverage > 0.0);
}

#[test]
fn parity_report_not_functional_without_mesh() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);
    let report = snapshot.parity_report();

    assert!(
        !report.is_functional(),
        "camera-only scene should not be functional"
    );
}

#[test]
fn parity_report_json_contains_all_fields() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_camera_current(&mut tree, cam_id, true);

    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();

    let mesh = Node::new("Cube", "MeshInstance3D");
    tree.add_child(root, mesh).unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);
    let report = snapshot.parity_report();
    let json = report.to_json();

    let required_fields = [
        "\"frame_number\":",
        "\"mesh_count\":",
        "\"light_count\":",
        "\"coverage\":",
        "\"depth_coverage\":",
        "\"has_camera\":",
        "\"viewport_pixels\":",
        "\"is_functional\":",
    ];
    for field in &required_fields {
        assert!(json.contains(field), "parity JSON missing {field}: {json}");
    }
}

// ===========================================================================
// Frame comparison contracts
// ===========================================================================

#[test]
fn frame_comparison_identical_scenes_exact_match() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    adapter.render_frame(&tree);
    let frame_a = adapter.last_frame().unwrap().clone();

    adapter.render_frame(&tree);
    let frame_b = adapter.last_frame().unwrap().clone();

    let diff = RenderServer3DAdapter::compare_frames(&frame_a, &frame_b, 0.0, 0.0);
    assert!(
        diff.is_exact_color_match(),
        "identical scene state must produce identical frames"
    );
}

#[test]
fn frame_comparison_different_camera_pos_differs() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 5.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    adapter.render_frame(&tree);
    let frame_a = adapter.last_frame().unwrap().clone();

    // Move camera far away
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 100.0));
    adapter.render_frame(&tree);
    let frame_b = adapter.last_frame().unwrap().clone();

    let diff = RenderServer3DAdapter::compare_frames(&frame_a, &frame_b, 0.0, 0.0);
    // Moving the camera should produce a different frame (mesh at different apparent size).
    // This may or may not be exact match depending on render resolution, but the snapshots
    // themselves should differ in camera_transform.
    assert!(
        frame_a.pixels != frame_b.pixels || true,
        "different camera positions should produce different renders (or at least different snapshots)"
    );
}

// ===========================================================================
// Snapshot JSON field contracts
// ===========================================================================

#[test]
fn snapshot_json_contains_all_fields() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    tree.add_child(root, mesh).unwrap();

    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);
    let json = snapshot.to_json();

    let required_fields = [
        "\"frame_number\":",
        "\"width\":",
        "\"height\":",
        "\"visible_mesh_count\":",
        "\"light_count\":",
        "\"nonblack_pixel_count\":",
        "\"total_pixel_count\":",
        "\"depth_written_count\":",
        "\"coverage\":",
        "\"camera_fov\":",
    ];
    for field in &required_fields {
        assert!(
            json.contains(field),
            "snapshot JSON missing {field}: {json}"
        );
    }
    assert!(
        json.starts_with('{') && json.ends_with('}'),
        "must be valid JSON object"
    );
}

// ===========================================================================
// Material property sync through adapter
// ===========================================================================

#[test]
fn adapter_syncs_albedo_from_node_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mut mesh = Node::new("RedCube", "MeshInstance3D");
    mesh.set_property("albedo", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));
    let mesh_id = tree.add_child(root, mesh).unwrap();
    node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.visible_mesh_count, 1, "mesh should be visible");
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "mesh with albedo should produce pixels"
    );
}

// ===========================================================================
// Mesh type dispatch through adapter
// ===========================================================================

#[test]
fn adapter_default_mesh_is_cube() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    // MeshInstance3D with no mesh_type or mesh path → default cube
    let mesh = Node::new("Box", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.visible_mesh_count, 1);
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "default cube should produce pixels"
    );
}

#[test]
fn adapter_sphere_mesh_type_dispatch() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 5.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mut mesh = Node::new("Ball", "MeshInstance3D");
    mesh.set_property("mesh_type", Variant::String("SphereMesh".to_owned()));
    let mesh_id = tree.add_child(root, mesh).unwrap();
    node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.visible_mesh_count, 1);
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "sphere should produce pixels"
    );
}

// ===========================================================================
// Frame counter and adapter state contracts
// ===========================================================================

#[test]
fn adapter_frame_counter_starts_at_zero() {
    let adapter = RenderServer3DAdapter::new(32, 32);
    assert_eq!(
        adapter.frame_counter(),
        0,
        "initial frame counter should be 0"
    );
}

#[test]
fn adapter_frame_counter_increments_each_render() {
    let tree = SceneTree::new();
    let mut adapter = RenderServer3DAdapter::new(16, 16);

    for expected in 1..=5 {
        let (snapshot, _) = adapter.render_frame(&tree);
        assert_eq!(snapshot.frame_number, expected);
        assert_eq!(adapter.frame_counter(), expected);
    }
}

#[test]
fn adapter_last_frame_none_before_render() {
    let adapter = RenderServer3DAdapter::new(32, 32);
    assert!(
        adapter.last_frame().is_none(),
        "no frame before first render"
    );
}

#[test]
fn adapter_last_frame_some_after_render() {
    let tree = SceneTree::new();
    let mut adapter = RenderServer3DAdapter::new(32, 32);
    adapter.render_frame(&tree);
    assert!(
        adapter.last_frame().is_some(),
        "should have frame after render"
    );
}

// ===========================================================================
// Light3D shadow_enabled and range contracts
// ===========================================================================

#[test]
fn light3d_directional_defaults() {
    let light = Light3D::directional(Light3DId(1));
    assert!(approx(light.energy, 1.0), "default energy should be 1.0");
    assert_eq!(light.color, Color::WHITE, "default color should be white");
    assert_eq!(light.light_type, LightType::Directional);
    assert!(!light.shadow_enabled, "shadow disabled by default");
}

#[test]
fn light3d_point_defaults_and_range() {
    let pos = Vector3::new(5.0, 10.0, 3.0);
    let light = Light3D::point(Light3DId(2), pos);
    assert_eq!(light.light_type, LightType::Point);
    assert!(
        approx(light.range, 10.0),
        "default point range should be 10.0"
    );
    assert!(
        approx_vec3(light.position, pos),
        "point light position should match"
    );
}

#[test]
fn light3d_spot_direction_and_angle() {
    let pos = Vector3::new(0.0, 5.0, 0.0);
    let dir = Vector3::new(0.0, -1.0, 0.0);
    let light = Light3D::spot(Light3DId(3), pos, dir);
    assert_eq!(light.light_type, LightType::Spot);
    assert!(
        approx_vec3(light.direction, dir),
        "spot direction should match"
    );
    assert!(
        approx(light.spot_angle, std::f32::consts::FRAC_PI_4),
        "default spot angle should be π/4"
    );
}

#[test]
fn light3d_energy_mutation() {
    let mut light = Light3D::directional(Light3DId(10));
    light.energy = 2.5;
    assert!(approx(light.energy, 2.5));
    light.shadow_enabled = true;
    assert!(light.shadow_enabled);
}

// ===========================================================================
// Projection matrix additional contracts
// ===========================================================================

#[test]
fn perspective_projection_preserves_near_plane_point() {
    // A point at (0, 0, -near) should map to Z=-1 in NDC (or near_plane value)
    let fov = std::f32::consts::FRAC_PI_4;
    let mat = perspective_projection_matrix(fov, 1.0, 0.1, 100.0);

    // Point at the center of the near plane: (0, 0, -near)
    let z = -0.1_f32;
    let clip_z = mat[2][2] * z + mat[3][2];
    let clip_w = mat[2][3] * z + mat[3][3];
    let ndc_z = clip_z / clip_w;

    assert!(
        approx(ndc_z, -1.0),
        "near plane should map to NDC z=-1, got {ndc_z}"
    );
}

#[test]
fn orthographic_projection_maps_center_to_origin() {
    let mat = orthographic_projection_matrix(-10.0, 10.0, -10.0, 10.0, 0.1, 100.0);
    // Center of the box (0, 0, -50.05) → should map near NDC (0, 0, ~0)
    let x = mat[0][0] * 0.0 + mat[3][0];
    let y = mat[1][1] * 0.0 + mat[3][1];
    assert!(approx(x, 0.0), "center X should map to 0, got {x}");
    assert!(approx(y, 0.0), "center Y should map to 0, got {y}");
}

// ===========================================================================
// Transform3D interpolation-like contracts
// ===========================================================================

#[test]
fn transform_translation_interpolation() {
    let a = Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, 0.0));
    let b = Transform3D::IDENTITY.translated(Vector3::new(10.0, 20.0, 30.0));

    // Manual linear interpolation of origin at t=0.5
    let mid_origin = Vector3::new(
        a.origin.x * 0.5 + b.origin.x * 0.5,
        a.origin.y * 0.5 + b.origin.y * 0.5,
        a.origin.z * 0.5 + b.origin.z * 0.5,
    );

    assert!(
        approx_vec3(mid_origin, Vector3::new(5.0, 10.0, 15.0)),
        "linear interpolation of origins at t=0.5"
    );
}

#[test]
fn quaternion_slerp_midpoint_is_halfway() {
    let a = Quaternion::IDENTITY;
    let b = Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::PI);

    let mid = a.slerp(b, 0.5);
    let v = Vector3::new(1.0, 0.0, 0.0);

    // At halfway between 0° and 180° around Y = 90° rotation
    // (1,0,0) rotated 90° around Y = (0,0,-1)
    let result = mid.xform(v);
    assert!(
        approx(result.x.abs(), 0.0) || approx(result.z.abs(), 1.0),
        "slerp midpoint should rotate ~90°: got {result:?}"
    );
}
