//! Integration tests for 3D viewport node selection and gizmo interaction.
//!
//! Exercises the `viewport_3d` module's ray casting, node picking, gizmo axis
//! detection, selection state management, and drag lifecycle.

use gdcore::math::Vector3;
use gdeditor::viewport_3d::{GizmoAxis, GizmoMode3D, Ray3D, Selection3D, Viewport3D};

fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() < eps
}

fn vec3_approx_eq(a: Vector3, b: Vector3, eps: f32) -> bool {
    approx_eq(a.x, b.x, eps) && approx_eq(a.y, b.y, eps) && approx_eq(a.z, b.z, eps)
}

// ===========================================================================
// Ray3D
// ===========================================================================

#[test]
fn ray_at_parameter() {
    let ray = Ray3D::new(Vector3::new(1.0, 2.0, 3.0), Vector3::new(0.0, 0.0, -1.0));
    let p = ray.at(4.0);
    assert!(approx_eq(p.x, 1.0, 1e-6));
    assert!(approx_eq(p.y, 2.0, 1e-6));
    assert!(approx_eq(p.z, -1.0, 1e-6));
}

#[test]
fn ray_direction_is_normalized() {
    let ray = Ray3D::new(Vector3::ZERO, Vector3::new(3.0, 4.0, 0.0));
    let len = ray.direction.length();
    assert!(
        approx_eq(len, 1.0, 1e-5),
        "Direction should be normalized, len={len}"
    );
}

#[test]
fn ray_sphere_head_on_hit() {
    let ray = Ray3D::new(Vector3::new(0.0, 0.0, 20.0), Vector3::new(0.0, 0.0, -1.0));
    let t = ray.intersect_sphere(Vector3::ZERO, 2.0);
    assert!(t.is_some());
    let t = t.unwrap();
    assert!(approx_eq(t, 18.0, 0.01), "Expected t=18, got {t}");
}

#[test]
fn ray_sphere_tangent_miss() {
    // Ray passes just outside the sphere
    let ray = Ray3D::new(Vector3::new(2.01, 0.0, 10.0), Vector3::new(0.0, 0.0, -1.0));
    let t = ray.intersect_sphere(Vector3::ZERO, 2.0);
    assert!(t.is_none(), "Ray tangent to sphere should miss");
}

#[test]
fn ray_sphere_behind_camera_misses() {
    // Sphere is behind the ray origin
    let ray = Ray3D::new(Vector3::new(0.0, 0.0, 10.0), Vector3::new(0.0, 0.0, 1.0));
    let t = ray.intersect_sphere(Vector3::ZERO, 1.0);
    assert!(t.is_none(), "Sphere behind camera should not be hit");
}

#[test]
fn ray_sphere_inside_returns_exit_point() {
    let ray = Ray3D::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    let t = ray.intersect_sphere(Vector3::ZERO, 5.0);
    assert!(t.is_some(), "Ray inside sphere should hit the exit");
    let t = t.unwrap();
    assert!(
        approx_eq(t, 5.0, 0.01),
        "Exit should be at t=radius, got {t}"
    );
}

// ===========================================================================
// Selection3D
// ===========================================================================

#[test]
fn selection_single_select() {
    let mut sel = Selection3D::default();
    sel.select(42);
    assert_eq!(sel.primary(), Some(42));
    assert!(sel.is_selected(42));
    assert!(!sel.is_selected(99));
}

#[test]
fn selection_single_select_replaces_previous() {
    let mut sel = Selection3D::default();
    sel.select(1);
    sel.select(2);
    assert_eq!(sel.selected_nodes.len(), 1);
    assert_eq!(sel.primary(), Some(2));
}

#[test]
fn selection_multi_select() {
    let mut sel = Selection3D::default();
    sel.add_to_selection(10);
    sel.add_to_selection(20);
    sel.add_to_selection(30);
    assert_eq!(sel.selected_nodes.len(), 3);
    assert!(sel.is_selected(10));
    assert!(sel.is_selected(20));
    assert!(sel.is_selected(30));
}

#[test]
fn selection_no_duplicates() {
    let mut sel = Selection3D::default();
    sel.add_to_selection(1);
    sel.add_to_selection(1);
    sel.add_to_selection(1);
    assert_eq!(sel.selected_nodes.len(), 1);
}

#[test]
fn selection_remove() {
    let mut sel = Selection3D::default();
    sel.add_to_selection(1);
    sel.add_to_selection(2);
    sel.remove_from_selection(1);
    assert!(!sel.is_selected(1));
    assert!(sel.is_selected(2));
}

#[test]
fn selection_toggle_on_off() {
    let mut sel = Selection3D::default();
    sel.toggle_selection(5);
    assert!(sel.is_selected(5));
    sel.toggle_selection(5);
    assert!(!sel.is_selected(5));
}

#[test]
fn selection_clear_empties_all() {
    let mut sel = Selection3D::default();
    sel.add_to_selection(1);
    sel.add_to_selection(2);
    sel.add_to_selection(3);
    sel.clear();
    assert!(sel.selected_nodes.is_empty());
    assert_eq!(sel.primary(), None);
}

// ===========================================================================
// GizmoMode3D
// ===========================================================================

#[test]
fn gizmo_mode_default_is_select() {
    let sel = Selection3D::default();
    assert_eq!(sel.gizmo_mode, GizmoMode3D::Select);
}

#[test]
fn gizmo_mode_switch() {
    let mut sel = Selection3D::default();
    sel.set_gizmo_mode(GizmoMode3D::Move);
    assert_eq!(sel.gizmo_mode, GizmoMode3D::Move);
    sel.set_gizmo_mode(GizmoMode3D::Rotate);
    assert_eq!(sel.gizmo_mode, GizmoMode3D::Rotate);
    sel.set_gizmo_mode(GizmoMode3D::Scale);
    assert_eq!(sel.gizmo_mode, GizmoMode3D::Scale);
}

#[test]
fn gizmo_mode_switch_cancels_active_drag() {
    let mut sel = Selection3D::default();
    sel.set_gizmo_mode(GizmoMode3D::Move);
    sel.begin_drag(GizmoAxis::X, Vector3::new(1.0, 0.0, 0.0));
    assert!(sel.dragging);
    sel.set_gizmo_mode(GizmoMode3D::Rotate);
    assert!(!sel.dragging, "Switching mode should cancel drag");
    assert_eq!(sel.active_axis, GizmoAxis::None);
}

// ===========================================================================
// Drag lifecycle
// ===========================================================================

#[test]
fn drag_begin_sets_state() {
    let mut sel = Selection3D::default();
    sel.begin_drag(GizmoAxis::Y, Vector3::new(0.0, 5.0, 0.0));
    assert!(sel.dragging);
    assert_eq!(sel.active_axis, GizmoAxis::Y);
    assert!(vec3_approx_eq(
        sel.drag_start,
        Vector3::new(0.0, 5.0, 0.0),
        1e-6
    ));
}

#[test]
fn drag_update_accumulates() {
    let mut sel = Selection3D::default();
    sel.begin_drag(GizmoAxis::Z, Vector3::ZERO);
    sel.update_drag(Vector3::new(0.0, 0.0, 3.0));
    assert!(vec3_approx_eq(
        sel.drag_delta,
        Vector3::new(0.0, 0.0, 3.0),
        1e-6
    ));
    sel.update_drag(Vector3::new(0.0, 0.0, 7.0));
    assert!(vec3_approx_eq(
        sel.drag_delta,
        Vector3::new(0.0, 0.0, 7.0),
        1e-6
    ));
}

#[test]
fn drag_update_ignored_when_not_dragging() {
    let mut sel = Selection3D::default();
    sel.update_drag(Vector3::new(10.0, 10.0, 10.0));
    assert!(vec3_approx_eq(sel.drag_delta, Vector3::ZERO, 1e-6));
}

#[test]
fn drag_end_returns_delta_and_resets() {
    let mut sel = Selection3D::default();
    sel.begin_drag(GizmoAxis::X, Vector3::ZERO);
    sel.update_drag(Vector3::new(5.5, 0.0, 0.0));
    let delta = sel.end_drag();
    assert!(vec3_approx_eq(delta, Vector3::new(5.5, 0.0, 0.0), 1e-6));
    assert!(!sel.dragging);
    assert_eq!(sel.active_axis, GizmoAxis::None);
}

#[test]
fn drag_cancel_zeroes_everything() {
    let mut sel = Selection3D::default();
    sel.begin_drag(GizmoAxis::Y, Vector3::new(0.0, 1.0, 0.0));
    sel.update_drag(Vector3::new(0.0, 99.0, 0.0));
    sel.cancel_drag();
    assert!(!sel.dragging);
    assert_eq!(sel.active_axis, GizmoAxis::None);
    assert!(vec3_approx_eq(sel.drag_delta, Vector3::ZERO, 1e-6));
    assert!(vec3_approx_eq(sel.drag_start, Vector3::ZERO, 1e-6));
}

// ===========================================================================
// Axis direction
// ===========================================================================

#[test]
fn axis_direction_for_each_axis() {
    let mut sel = Selection3D::default();
    sel.active_axis = GizmoAxis::X;
    assert!(vec3_approx_eq(
        sel.axis_direction(),
        Vector3::new(1.0, 0.0, 0.0),
        1e-6
    ));
    sel.active_axis = GizmoAxis::Y;
    assert!(vec3_approx_eq(
        sel.axis_direction(),
        Vector3::new(0.0, 1.0, 0.0),
        1e-6
    ));
    sel.active_axis = GizmoAxis::Z;
    assert!(vec3_approx_eq(
        sel.axis_direction(),
        Vector3::new(0.0, 0.0, 1.0),
        1e-6
    ));
    sel.active_axis = GizmoAxis::None;
    assert!(vec3_approx_eq(sel.axis_direction(), Vector3::ZERO, 1e-6));
}

// ===========================================================================
// Viewport3D screen-to-ray
// ===========================================================================

#[test]
fn screen_to_ray_center_is_forward() {
    let vp = Viewport3D::new(800, 600);
    let ray = vp.screen_to_ray(400.0, 300.0);
    let fwd = vp.camera.orbit_direction();
    let dot = ray.direction.dot(fwd);
    assert!(
        dot > 0.99,
        "Center pixel ray should align with camera forward, dot={dot}"
    );
}

#[test]
fn screen_to_ray_origin_is_camera_position() {
    let vp = Viewport3D::new(800, 600);
    let ray = vp.screen_to_ray(400.0, 300.0);
    let cam_pos = vp.camera.position();
    assert!(
        vec3_approx_eq(ray.origin, cam_pos, 1e-4),
        "Ray origin should be camera position"
    );
}

#[test]
fn screen_to_ray_top_left_vs_bottom_right_diverge() {
    let vp = Viewport3D::new(800, 600);
    let ray_tl = vp.screen_to_ray(0.0, 0.0);
    let ray_br = vp.screen_to_ray(800.0, 600.0);
    let dot = ray_tl.direction.dot(ray_br.direction);
    assert!(dot < 0.98, "Corner rays should diverge, dot={dot}");
}

#[test]
fn screen_to_ray_with_different_fov() {
    let mut vp = Viewport3D::new(800, 600);
    let ray_70 = vp.screen_to_ray(0.0, 300.0);

    vp.camera.fov_degrees = 120.0;
    let ray_120 = vp.screen_to_ray(0.0, 300.0);

    // Wider FOV should produce a more divergent ray for the same corner pixel
    let fwd = vp.camera.orbit_direction();
    let dot_70 = ray_70.direction.dot(fwd);
    let dot_120 = ray_120.direction.dot(fwd);
    assert!(
        dot_120 < dot_70,
        "Wider FOV should produce more divergent corner rays: 70°={dot_70}, 120°={dot_120}"
    );
}

// ===========================================================================
// Viewport3D pick_node
// ===========================================================================

#[test]
fn pick_node_closest_wins() {
    let mut vp = Viewport3D::new(800, 600);
    vp.camera.focus_point = Vector3::ZERO;
    vp.camera.distance = 10.0;
    vp.camera.yaw = 0.0;
    vp.camera.pitch = 0.0;

    let nodes = vec![
        (100, Vector3::new(0.0, 0.0, -5.0)), // far from camera
        (200, Vector3::new(0.0, 0.0, 0.0)),  // closer to camera
    ];

    let result = vp.pick_node(400.0, 300.0, &nodes, 1.5);
    assert!(result.is_some());
    assert_eq!(result.unwrap().node_id, 200, "Closer node should be picked");
}

#[test]
fn pick_node_empty_list_returns_none() {
    let vp = Viewport3D::new(800, 600);
    let result = vp.pick_node(400.0, 300.0, &[], 1.0);
    assert!(result.is_none());
}

#[test]
fn pick_node_miss_returns_none() {
    let vp = Viewport3D::new(800, 600);
    let nodes = vec![(1, Vector3::new(500.0, 500.0, 500.0))];
    let result = vp.pick_node(400.0, 300.0, &nodes, 0.5);
    assert!(result.is_none());
}

#[test]
fn pick_node_hit_point_is_on_sphere() {
    let mut vp = Viewport3D::new(800, 600);
    vp.camera.focus_point = Vector3::ZERO;
    vp.camera.distance = 10.0;
    vp.camera.yaw = 0.0;
    vp.camera.pitch = 0.0;

    let center = Vector3::ZERO;
    let radius = 2.0;
    let nodes = vec![(1, center)];

    let result = vp.pick_node(400.0, 300.0, &nodes, radius);
    assert!(result.is_some());
    let r = result.unwrap();
    let dist_from_center = Vector3::new(
        r.hit_point.x - center.x,
        r.hit_point.y - center.y,
        r.hit_point.z - center.z,
    )
    .length();
    assert!(
        approx_eq(dist_from_center, radius, 0.1),
        "Hit point should be on sphere surface, dist={dist_from_center}"
    );
}

// ===========================================================================
// Viewport3D pick_gizmo_axis
// ===========================================================================

#[test]
fn pick_gizmo_axis_miss_returns_none() {
    let vp = Viewport3D::new(800, 600);
    let axis = vp.pick_gizmo_axis(0.0, 0.0, Vector3::ZERO, 1.0);
    assert_eq!(axis, GizmoAxis::None);
}

// ===========================================================================
// End-to-end selection + gizmo workflow
// ===========================================================================

#[test]
fn full_selection_and_move_workflow() {
    let mut vp = Viewport3D::new(800, 600);
    vp.camera.focus_point = Vector3::ZERO;
    vp.camera.distance = 10.0;
    vp.camera.yaw = 0.0;
    vp.camera.pitch = 0.0;

    let mut sel = Selection3D::default();

    // Step 1: Pick a node
    let nodes = vec![
        (1, Vector3::new(0.0, 0.0, 0.0)),
        (2, Vector3::new(5.0, 0.0, 0.0)),
    ];
    let result = vp.pick_node(400.0, 300.0, &nodes, 1.0);
    assert!(result.is_some());
    let picked = result.unwrap();
    assert_eq!(picked.node_id, 1);

    // Step 2: Select the node
    sel.select(picked.node_id);
    assert!(sel.is_selected(1));
    assert_eq!(sel.primary(), Some(1));

    // Step 3: Switch to Move gizmo
    sel.set_gizmo_mode(GizmoMode3D::Move);
    assert_eq!(sel.gizmo_mode, GizmoMode3D::Move);

    // Step 4: Begin drag on X axis
    sel.begin_drag(GizmoAxis::X, picked.hit_point);
    assert!(sel.dragging);
    assert_eq!(sel.active_axis, GizmoAxis::X);

    // Step 5: Update drag
    sel.update_drag(Vector3::new(3.0, 0.0, 0.0));

    // Step 6: End drag — apply the delta
    let delta = sel.end_drag();
    assert!(approx_eq(delta.x, 3.0, 1e-6));
    assert!(!sel.dragging);
}

#[test]
fn full_rotate_workflow() {
    let mut sel = Selection3D::default();
    sel.select(42);
    sel.set_gizmo_mode(GizmoMode3D::Rotate);
    sel.begin_drag(GizmoAxis::Y, Vector3::ZERO);
    sel.update_drag(Vector3::new(0.0, 1.57, 0.0)); // ~90 degrees
    let delta = sel.end_drag();
    assert!(approx_eq(delta.y, 1.57, 0.01));
}

#[test]
fn full_scale_workflow() {
    let mut sel = Selection3D::default();
    sel.select(7);
    sel.set_gizmo_mode(GizmoMode3D::Scale);
    sel.begin_drag(GizmoAxis::Z, Vector3::ZERO);
    sel.update_drag(Vector3::new(0.0, 0.0, 2.0));
    let delta = sel.end_drag();
    assert!(approx_eq(delta.z, 2.0, 1e-6));
}

#[test]
fn multi_select_then_clear() {
    let mut sel = Selection3D::default();
    sel.add_to_selection(1);
    sel.add_to_selection(2);
    sel.add_to_selection(3);
    assert_eq!(sel.selected_nodes.len(), 3);
    // Toggle removes from selection
    sel.toggle_selection(2);
    assert_eq!(sel.selected_nodes.len(), 2);
    assert!(!sel.is_selected(2));
    // Clear all
    sel.clear();
    assert!(sel.selected_nodes.is_empty());
}
