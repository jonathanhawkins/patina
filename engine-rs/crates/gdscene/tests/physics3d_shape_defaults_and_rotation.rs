//! pat-mgixk.39 — Godot's shape defaults, a real `WorldBoundaryShape3D` plane,
//! and a `CollisionShape3D`'s rotation all reach the 3D physics world.
//!
//! Godot ground truth: `CapsuleShape3D.height` and `CylinderShape3D.height`
//! default to **2.0** (`doc/classes/CapsuleShape3D.xml:14`,
//! `CylinderShape3D.xml:17`); `WorldBoundaryShape3D` carries a `plane`
//! (`Plane(0, 1, 0, 0)` by default); and a `CollisionShape3D`'s FULL local
//! transform is its shape owner's transform, rotation included
//! (`CollisionObject3D::shape_owner_set_transform`).
//!
//! # What was wrong
//!
//! 1. `physics_server_3d` built capsules and cylinders with `num("height", 1.0)`.
//!    ⚠️ A `.tscn` OMITS a property still holding its default, so an unedited
//!    `CapsuleShape3D` — the stock CharacterBody3D collider — authors no
//!    `height` line and arrived as a 1 m capsule: with radius 0.5 that is
//!    exactly a sphere. It collided as a ball while rendering as a 2 m capsule,
//!    and the gizmo path already used Godot's 2.0, so the two layers disagreed
//!    about the same node.
//! 2. The `WorldBoundaryShape3D` arm hardcoded normal `(0,1,0)` and distance
//!    `0.0` and read no property at all, so every world boundary was the ground
//!    at y = 0 however the scene authored it.
//! 3. `shapes_from_body_subtree` stored only a translation, so a cylinder laid
//!    on its side or a box turned 90 degrees into a wall collided bolt upright.
//!
//! ⚠️ Everything here is driven by `MainLoop::step` on a parsed `.tscn`, as the
//! bead's Wiring requirement asks (`.tscn load -> packed_scene __shape__* ->
//! shape_from_node_3d -> world body -> MainLoop step`). Nothing is injected into
//! the physics world directly, and every number is read back off the NODE — the
//! value a game script would see — so a pass means the whole chain works. The
//! harness is the one `physics3d_multi_shape_bodies.rs` already uses.

use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::node3d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

/// ⚠️ `f64`, because `MainLoop::step` takes `delta_secs: f64`. Declaring this
/// `f32` to match the `Vector3` maths around it is a type error, not a lint.
const STEP: f64 = 1.0 / 60.0;

fn load(tscn: &str) -> MainLoop {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = gdscene::scene_tree::SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    MainLoop::new(tree)
}

fn node_of(ml: &MainLoop, name: &str) -> NodeId {
    let root = ml.tree().root_id();
    ml.tree()
        .find_child(root, name, true)
        .unwrap_or_else(|| panic!("node {name} exists"))
}

/// The node's own world-space height.
///
/// ⚠️ Read from the NODE, not from the physics body. A 3D body registers lazily
/// inside `sync_to_physics`, which only runs from `MainLoop::step`, and reading
/// the body directly would also let the test pass on a value the fixture itself
/// wrote.
fn y_of(ml: &MainLoop, name: &str) -> f32 {
    node3d::get_global_transform(ml.tree(), node_of(ml, name))
        .origin
        .y
}

fn settle(ml: &mut MainLoop, ticks: usize) {
    for _ in 0..ticks {
        ml.step(STEP);
    }
}

// ---------------------------------------------------------------------------
// (a) an unedited CapsuleShape3D is 2 m tall
// ---------------------------------------------------------------------------

/// 🔑 The `CapsuleShape3D` sub-resource authors **only** `radius`. The missing
/// `height` line is the entire fixture: Godot omits a default-valued property,
/// so this is byte-for-byte what the editor writes for an untouched capsule, and
/// `packed_scene` copies only the keys the sub-resource authored — nothing fills
/// in 2.0 downstream.
///
/// The floor's collision box spans y in [-1, 0], so its top surface is exactly
/// y = 0 and the resting height of the body IS the capsule's half-height. That
/// makes the expected number the acceptance's "y = 1.0 above a floor" with no
/// arithmetic in between.
///
/// ⚠️ `lock_rotation = true` on the capsule. A free capsule balanced on its end
/// can topple, and a toppled capsule rests at its RADIUS (0.5) — the very number
/// the broken default produced, which would make a correct engine look broken.
const DEFAULT_CAPSULE_SCENE: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxShape3D" id="Box_floor"]
size = Vector3(20, 1, 20)

[sub_resource type="CapsuleShape3D" id="Capsule_default"]
radius = 0.5

[node name="Root" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]

[node name="FloorShape" type="CollisionShape3D" parent="Floor"]
position = Vector3(0, -0.5, 0)
shape = SubResource("Box_floor")

[node name="Faller" type="RigidBody3D" parent="."]
position = Vector3(0, 4, 0)
lock_rotation = true

[node name="FallerShape" type="CollisionShape3D" parent="Faller"]
shape = SubResource("Capsule_default")
"#;

// ---------------------------------------------------------------------------
// (b) a WorldBoundaryShape3D honours its plane
// ---------------------------------------------------------------------------

/// `Plane(0, 1, 0, 3)` is the ground raised to y = 3. The narrowphase builds the
/// world plane as `distance + normal.dot(shape_world_position)`
/// (`collision.rs::plane_world`), and both the body and its shape sit at the
/// origin here, so the plane is exactly y = 3 with no offset folded in.
const RAISED_BOUNDARY_SCENE: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="WorldBoundaryShape3D" id="Plane_raised"]
plane = Plane(0, 1, 0, 3)

[sub_resource type="SphereShape3D" id="Sphere_ball"]
radius = 0.5

[node name="Root" type="Node3D"]

[node name="Ground" type="StaticBody3D" parent="."]

[node name="GroundShape" type="CollisionShape3D" parent="Ground"]
shape = SubResource("Plane_raised")

[node name="Ball" type="RigidBody3D" parent="."]
position = Vector3(0, 8, 0)
lock_rotation = true

[node name="BallShape" type="CollisionShape3D" parent="Ball"]
shape = SubResource("Sphere_ball")
"#;

// ---------------------------------------------------------------------------
// (c) a rotated CollisionShape3D collides rotated
// ---------------------------------------------------------------------------

/// A thin slab turned into a vertical wall by rotating its `CollisionShape3D`
/// 90 degrees about Z.
///
/// The slab is `4 x 0.2 x 4`, so local half-extents are `(2, 0.1, 2)`. A
/// quarter-turn about Z maps the local X axis onto world Y and the local Y axis
/// onto world X, so the placed wall is thin in **X** (±0.1) and tall in **Y**
/// (±2) — its top surface is y = 2. A 1 m box (half-extent 0.5) landing on that
/// top rests at y = 2.5.
///
/// ⚠️ Rotation is authored as `rotation`, in RADIANS, and NOT alongside a
/// `transform` line: `node3d::local_transform_of` returns an explicit
/// `transform` property verbatim if one is present, so authoring both would make
/// the rotation dead weight and the wall would stay flat.
///
/// `drop_x` selects the discriminating pair: 0.0 lands on the wall, 2.0 is the
/// control that must miss it.
fn rotated_wall_scene(drop_x: f32) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxShape3D" id="Box_slab"]
size = Vector3(4, 0.2, 4)

[sub_resource type="BoxShape3D" id="Box_faller"]
size = Vector3(1, 1, 1)

[node name="Root" type="Node3D"]

[node name="Wall" type="StaticBody3D" parent="."]

[node name="WallShape" type="CollisionShape3D" parent="Wall"]
rotation = Vector3(0, 0, 1.5707964)
shape = SubResource("Box_slab")

[node name="Faller" type="RigidBody3D" parent="."]
position = Vector3({drop_x}, 6, 0)
lock_rotation = true

[node name="FallerShape" type="CollisionShape3D" parent="Faller"]
shape = SubResource("Box_faller")
"#
    )
}

/// **The acceptance clause.** All three fidelity gaps, each through a `.tscn`
/// loaded and stepped by a real `MainLoop`.
#[test]
fn physics3d_shape_defaults_and_rotation() {
    // -- (a) the unedited capsule is 2 m, so it rests at 1.0 ---------------
    let mut ml = load(DEFAULT_CAPSULE_SCENE);
    settle(&mut ml, 240);
    let capsule_y = y_of(&ml, "Faller");
    assert!(
        (capsule_y - 1.0).abs() < 0.25,
        "a CapsuleShape3D that omits `height` must default to 2.0, so with the \
         floor's top surface at y = 0 the body rests at y ~= 1.0 (its \
         half-height). Got y = {capsule_y}. A value near 0.5 means `height` \
         defaulted to 1.0, which with radius 0.5 is a sphere — the stock \
         CharacterBody3D collider colliding at half its drawn size."
    );

    // -- (b) the world boundary sits where the plane says ------------------
    let mut ml = load(RAISED_BOUNDARY_SCENE);
    settle(&mut ml, 300);
    let ball_y = y_of(&ml, "Ball");
    assert!(
        (ball_y - 3.5).abs() < 0.3,
        "a RigidBody3D dropped onto a WorldBoundaryShape3D with \
         plane = Plane(0, 1, 0, 3) must come to rest ON that plane: the sphere's \
         radius is 0.5, so its centre sits at y ~= 3.5 and its underside at the \
         plane's y = 3. Got y = {ball_y}. A value near 0.5 means the plane was \
         still hardcoded to y = 0 and the authored `plane` was ignored."
    );

    // -- (c) the rotated slab collides as a wall ---------------------------
    let mut ml = load(&rotated_wall_scene(0.0));
    settle(&mut ml, 300);
    let on_wall = y_of(&ml, "Faller");
    assert!(
        (on_wall - 2.5).abs() < 0.3,
        "a box falling onto a thin BoxShape3D rotated 90 degrees about Z must \
         land on the ROTATED extent. The slab is 4 x 0.2 x 4, so the quarter \
         turn puts its top surface at y = 2 and a 1 m box rests at y ~= 2.5. \
         Got y = {on_wall}. A value near 0.6 means the shape collided \
         unrotated (its flat top is y = 0.1); a large negative value means the \
         body fell straight through, which is what happens when the rotation \
         reaches the narrowphase but `world_aabb` still reports the UNROTATED \
         box and the broadphase prunes the pair before any test runs."
    );

    // 🔑 The control, and the reason clause (c) cannot be satisfied by a shape
    // that merely got BIGGER. The rotated wall is only ±0.1 thick in X, so a box
    // dropped at x = 2 must miss it entirely and keep falling. Without this, an
    // implementation that replaced the slab with its own bounding sphere — or
    // that grew the broadphase box without rotating the shape — would satisfy
    // every assertion above and still be wrong.
    let mut ml = load(&rotated_wall_scene(2.0));
    settle(&mut ml, 300);
    let past_wall = y_of(&ml, "Faller");
    assert!(
        past_wall < -2.0,
        "a box dropped at x = 2, clear of a wall that is only ±0.1 thick in X, \
         must fall PAST it, got y = {past_wall}. If this body stopped, the \
         rotated shape is colliding over its unrotated 4-wide X extent rather \
         than its rotated 0.2-wide one."
    );
}
