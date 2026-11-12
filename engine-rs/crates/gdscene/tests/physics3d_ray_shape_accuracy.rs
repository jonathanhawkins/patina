//! pat-0q2kx.120 — 3D ray queries hit the *real* shape, not a bounding fake.
//!
//! Godot's `GodotPhysicsDirectSpaceState3D::intersect_ray` transforms the
//! segment into each shape's local frame and dispatches to that shape's
//! `intersect_segment` (godot_shape_3d.cpp): an oriented slab test for boxes,
//! cap/side split for cylinders, plane clipping for convex hulls, per-triangle
//! for trimeshes.
//!
//! Patina's `gdphysics3d::query::ray_test_body` was exact only for spheres and
//! capsules. Boxes went through an *axis-aligned* `ray_aabb` that dropped
//! `body.rotation` entirely, and cylinders, convex hulls, trimeshes and world
//! boundaries all fell through to a bounding-*sphere* approximation — so a
//! `RayCast3D` over trimesh terrain reported a point floating far above the
//! surface, and a rotated box was hit on its unrotated bounding box.
//!
//! This drives the real `.tscn` → `PackedScene` → `PhysicsServer3D` →
//! `MainLoop::process_queries_3d` path, with no manual `force_raycast_update`
//! anywhere, and asserts the reported points/normals against the exact
//! geometry. Every expectation below differs from what the old approximation
//! produced, so this test fails against it.

use gdcore::math::Vector3;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{raycast3d, MainLoop, NodeId, SceneTree};

/// Fixtures are spread far apart along X and separated by collision layers so
/// each ray can only ever see its own body:
///
/// - layer 1 @ x=0     — two-triangle trimesh floor on the y=0 plane.
/// - layer 2 @ x=50    — 2×2×2 box rotated 45° about Z (a diamond in XY).
/// - layer 4 @ x=100   — cylinder, radius 1, height 2.
/// - layer 8 @ x=150   — 2×2×2 box with a ray starting at its centre.
const SCENE: &str = r#"[gd_scene load_steps=5 format=3]

[sub_resource type="ConcavePolygonShape3D" id="Floor_1"]
data = PackedVector3Array(-5, 0, -5, -5, 0, 5, 5, 0, -5, 5, 0, -5, -5, 0, 5, 5, 0, 5)

[sub_resource type="BoxShape3D" id="Box_1"]
size = Vector3(2, 2, 2)

[sub_resource type="CylinderShape3D" id="Cyl_1"]
radius = 1.0
height = 2.0

[sub_resource type="BoxShape3D" id="Box_2"]
size = Vector3(2, 2, 2)

[node name="Root" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]
position = Vector3(0, 0, 0)
collision_layer = 1

[node name="FloorShape" type="CollisionShape3D" parent="Floor"]
shape = SubResource("Floor_1")

[node name="RotatedBox" type="StaticBody3D" parent="."]
position = Vector3(50, 0, 0)
rotation = Vector3(0, 0, 0.7853982)
collision_layer = 2

[node name="RotatedBoxShape" type="CollisionShape3D" parent="RotatedBox"]
shape = SubResource("Box_1")

[node name="Cylinder" type="StaticBody3D" parent="."]
position = Vector3(100, 0, 0)
collision_layer = 4

[node name="CylinderShape" type="CollisionShape3D" parent="Cylinder"]
shape = SubResource("Cyl_1")

[node name="InsideBox" type="StaticBody3D" parent="."]
position = Vector3(150, 0, 0)
collision_layer = 8

[node name="InsideBoxShape" type="CollisionShape3D" parent="InsideBox"]
shape = SubResource("Box_2")

[node name="RayFloor" type="RayCast3D" parent="."]
position = Vector3(-1, 5, -1)
target_position = Vector3(0, -10, 0)
collision_mask = 1

[node name="RayBoxFace" type="RayCast3D" parent="."]
position = Vector3(50.3, 5, 0)
target_position = Vector3(0, -10, 0)
collision_mask = 2

[node name="RayBoxBeyondAabb" type="RayCast3D" parent="."]
position = Vector3(51.2, 5, 0)
target_position = Vector3(0, -10, 0)
collision_mask = 2

[node name="RayCylinderCap" type="RayCast3D" parent="."]
position = Vector3(100.7, 5, 0)
target_position = Vector3(0, -10, 0)
collision_mask = 4

[node name="RayCylinderSide" type="RayCast3D" parent="."]
position = Vector3(97, 0.8, 0)
target_position = Vector3(10, 0, 0)
collision_mask = 4

[node name="RayFromInside" type="RayCast3D" parent="."]
position = Vector3(150, 0, 0)
target_position = Vector3(0, -10, 0)
collision_mask = 8
hit_from_inside = true

[node name="RayFromInsideDisabled" type="RayCast3D" parent="."]
position = Vector3(150, 0, 0)
target_position = Vector3(0, -10, 0)
collision_mask = 8
"#;

/// √2 — the half-diagonal of the 2×2×2 box, i.e. the height of the 45°-rotated
/// box's top vertex above its centre.
const SQRT2: f32 = std::f32::consts::SQRT_2;

/// Loads the scene and runs a few `MainLoop` frames so the bodies register and
/// the 3D query phase updates every `RayCast3D` — no manual `force_*` call.
fn stepped_scene() -> (MainLoop, NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let mut ml = MainLoop::new(tree);
    // Frame one registers the bodies; later frames run the query phase against a
    // populated world.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    (ml, scene_root)
}

fn ray(ml: &MainLoop, scene_root: NodeId, name: &str) -> NodeId {
    ml.tree()
        .get_node_or_null(scene_root, name)
        .unwrap_or_else(|| panic!("{name} present in the instanced scene"))
}

fn assert_close(actual: f32, expected: f32, tol: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: expected {expected}, got {actual} (tolerance {tol})",
    );
}

#[test]
fn physics3d_ray_shape_accuracy() {
    let (ml, scene_root) = stepped_scene();
    let tree = ml.tree();

    // -- Trimesh: the ray must stop on the triangle plane ------------------
    //
    // The floor's faces span x,z in [-5, 5] at y = 0, so the bounding-sphere
    // approximation had an effective radius of 5 and reported y = 5.0 — the ray
    // "hit" the instant it left its own origin.
    let floor_ray = ray(&ml, scene_root, "RayFloor");
    assert!(
        raycast3d::is_colliding(tree, floor_ray),
        "downward ray over the trimesh floor must report a hit",
    );
    let p = raycast3d::get_collision_point(tree, floor_ray);
    assert_close(p.y, 0.0, 1e-3, "trimesh hit height");
    // Aimed at (-1, -1) so the hit lands in the interior of the first triangle
    // rather than on the diagonal the two triangles share.
    assert_close(p.x, -1.0, 1e-3, "trimesh hit x");
    assert_close(p.z, -1.0, 1e-3, "trimesh hit z");
    let n = raycast3d::get_collision_normal(tree, floor_ray);
    assert_close(n.y, 1.0, 1e-3, "trimesh face normal points up");

    // -- Oriented box: the rotated face, not the AABB ----------------------
    //
    // A 2×2×2 box rotated 45° about Z is a diamond in XY whose surface obeys
    // |x| + |y| = √2. At x = 0.3 from the centre the true surface sits at
    // y = √2 - 0.3 ≈ 1.114; the rotation-ignoring AABB reported a flat 1.0.
    let box_ray = ray(&ml, scene_root, "RayBoxFace");
    assert!(
        raycast3d::is_colliding(tree, box_ray),
        "ray over the rotated box must report a hit",
    );
    let p = raycast3d::get_collision_point(tree, box_ray);
    assert_close(p.y, SQRT2 - 0.3, 1e-3, "rotated box face height");
    // The hit is on the diamond's upper-right face, whose normal is the box's
    // local +X rotated 45°.
    let n = raycast3d::get_collision_normal(tree, box_ray);
    assert_close(n.x, SQRT2 / 2.0, 1e-3, "rotated face normal x");
    assert_close(n.y, SQRT2 / 2.0, 1e-3, "rotated face normal y");

    // Beyond the *unrotated* half-extent of 1.0 the old axis-aligned test
    // missed entirely; the rotated box genuinely reaches out to √2 ≈ 1.414.
    let beyond_ray = ray(&ml, scene_root, "RayBoxBeyondAabb");
    assert!(
        raycast3d::is_colliding(tree, beyond_ray),
        "ray at x=+1.2 from the box centre is outside the AABB but inside the rotated box",
    );
    let p = raycast3d::get_collision_point(tree, beyond_ray);
    assert_close(p.y, SQRT2 - 1.2, 1e-3, "rotated box hit beyond the AABB");

    // -- Cylinder: flat cap and curved side, both exact --------------------
    //
    // Radius 1, height 2. The cap is flat, so a ray 0.7 off-axis still lands at
    // y = 1.0; the bounding sphere curved away to y ≈ 0.714.
    let cap_ray = ray(&ml, scene_root, "RayCylinderCap");
    assert!(
        raycast3d::is_colliding(tree, cap_ray),
        "ray onto the cylinder cap must report a hit",
    );
    let p = raycast3d::get_collision_point(tree, cap_ray);
    assert_close(p.y, 1.0, 1e-3, "cylinder cap is flat");
    let n = raycast3d::get_collision_normal(tree, cap_ray);
    assert_close(n.y, 1.0, 1e-3, "cylinder cap normal points up");

    // The side is a straight tube: a horizontal ray at y = 0.8 (still within the
    // half-height of 1.0) hits at exactly one radius out, x = 99.0. The bounding
    // sphere pulled that in to x ≈ 99.4.
    let side_ray = ray(&ml, scene_root, "RayCylinderSide");
    assert!(
        raycast3d::is_colliding(tree, side_ray),
        "horizontal ray at the cylinder side must report a hit",
    );
    let p = raycast3d::get_collision_point(tree, side_ray);
    assert_close(p.x, 99.0, 1e-3, "cylinder side is a straight tube");
    assert_close(p.y, 0.8, 1e-3, "cylinder side hit keeps the ray's height");
    let n = raycast3d::get_collision_normal(tree, side_ray);
    assert_close(n.x, -1.0, 1e-3, "cylinder side normal faces the ray");

    // -- hit_from_inside ---------------------------------------------------
    //
    // Godot reports the ray's own origin with a zero normal when the ray starts
    // inside a shape and `hit_from_inside` is set, and skips the shape entirely
    // when it is not (it never reports the far surface).
    let inside_ray = ray(&ml, scene_root, "RayFromInside");
    assert!(
        raycast3d::is_colliding(tree, inside_ray),
        "hit_from_inside=true must report the shape the ray starts inside of",
    );
    let p = raycast3d::get_collision_point(tree, inside_ray);
    assert_close(p.x, 150.0, 1e-4, "inside hit reports the ray origin x");
    assert_close(p.y, 0.0, 1e-4, "inside hit reports the ray origin y");
    let n = raycast3d::get_collision_normal(tree, inside_ray);
    assert_eq!(
        n,
        Vector3::ZERO,
        "a hit from inside has no surface normal in Godot",
    );

    let inside_off = ray(&ml, scene_root, "RayFromInsideDisabled");
    assert!(
        !raycast3d::is_colliding(tree, inside_off),
        "hit_from_inside defaults to false, which skips the enclosing shape entirely",
    );
}

/// The `.tscn` sub-resource geometry must actually reach the physics body.
///
/// `shape_from_node_3d` only understood the hand-built convention (the shape's
/// *class name* in the `shape` property), so the parser's real output
/// `shape = "SubResource:<id>"` matched no arm and every authored 3D shape
/// silently became a radius-0.5 sphere. That alone would make the assertions
/// above meaningless, so it is pinned separately.
#[test]
fn physics3d_ray_shape_accuracy_uses_authored_subresources() {
    let (ml, scene_root) = stepped_scene();

    // A default 0.5-radius sphere at the cylinder's origin would be hit at
    // y ≈ 0.5 by an on-axis ray and missed entirely by the 0.7-off-axis one.
    let cap_ray = ray(&ml, scene_root, "RayCylinderCap");
    assert!(
        raycast3d::is_colliding(ml.tree(), cap_ray),
        "a 0.7-off-axis ray only hits if the authored radius-1 cylinder reached the body",
    );

    // The floor is authored as a 10×10 trimesh; the fallback sphere (radius 0.5)
    // would put the hit at y = 0.5 instead of on the plane.
    let floor_ray = ray(&ml, scene_root, "RayFloor");
    let p = raycast3d::get_collision_point(ml.tree(), floor_ray);
    assert_close(p.y, 0.0, 1e-3, "authored trimesh floor plane");
}
