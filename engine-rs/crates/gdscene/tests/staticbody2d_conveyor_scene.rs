//! pat-s3rrv.101 acceptance: an authored `StaticBody2D` conveyor carries what
//! rests on it, end to end — parse → instance → `register_physics_bodies` →
//! `MainLoop::step` → the solver's surface-velocity term moves the rider.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/static_body_2d.cpp`.
//! `constant_linear_velocity` / `constant_angular_velocity` drag contacting
//! bodies by friction **without moving the static body itself** — conveyor belts
//! and turntables. `doc/classes/StaticBody2D.xml` documents both defaulting to
//! zero, which is exactly "no conveyor", so every other body class is unaffected
//! by reading them.
//!
//! ⚠️ **The solver already did all the work.** `PhysicsBody2D` has carried
//! `constant_linear_velocity` for a long time and `ContactSolver2D` folds it
//! into the tangential relative velocity, with a green unit test
//! (`solver.rs::constant_linear_velocity_drags_a_resting_body`). What was
//! missing was the one hop that makes it reachable from a scene: the bridge in
//! `gdscene::physics_server` never read the property off the node, so an
//! authored belt sat inert while every Rust-level test passed. That is why this
//! test loads a `.tscn` instead of building `PhysicsBody2D` structs — a
//! struct-level test cannot fail for the bug this bead is about.
//!
//! ⚠️ **The fixture mirrors the proven solver-level one**
//! (`gdphysics2d/tests/physics2d_solver_stacking_conveyor_sleep.rs`), and every
//! part of it is load-bearing rather than decorative:
//!
//!   * `friction = 1.0` via an authored `PhysicsMaterial` on BOTH bodies — the
//!     default 0.5 drags, but weakly enough that the assertion would be
//!     measuring solver tuning instead of wiring;
//!   * `can_sleep = false` on the rider — a body that settles for a second goes
//!     to sleep, and a sleeping box is not dragged however correct the belt is.
//!     This is the failure mode most likely to read as "the feature is broken";
//!   * `lock_rotation = true` — otherwise the surface impulse tips the box, and
//!     the test starts measuring how far a block topples rather than how far the
//!     belt carries it.
//!
//! ⚠️ **What this deliberately does NOT assert:** that the rider reaches the
//! belt's ~100 px/s. The rider approaches belt speed asymptotically, so its
//! exact velocity after one second is a solver-tuning artifact, not a parity
//! fact. Pinning a number would turn any future friction or warm-start change
//! into a spurious failure in a test about property wiring. The load-bearing
//! claim is the DIFFERENTIAL: the same scene with and without the property
//! behaves differently.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Belt top surface is y = 90 (centre 100, half-height 10); a 16×16 box of
/// half-height 8 therefore rests centred at y = 82.
///
/// A **box**, not a ball: a circle on a moving belt spins up under friction, so
/// part of the surface impulse goes into angular velocity and the translation
/// becomes a rolling question rather than a conveyor question. The solver's own
/// conveyor test uses a box for the same reason.
///
/// The `AnimatableBody2D` is parked well below everything else so it takes part
/// in registration without colliding with the belt or the rider — this test only
/// asks whether it became a real body, which is the half of the acceptance that
/// pat-s3rrv.93 already satisfied.
fn scene(belt_velocity: Option<&str>) -> String {
    let belt_line = match belt_velocity {
        Some(v) => format!("constant_linear_velocity = {v}\n"),
        // Omitted entirely rather than written as zero: that also exercises the
        // default path, proving an ordinary StaticBody2D with no conveyor
        // authored on it is untouched by the new read.
        None => String::new(),
    };
    format!(
        r#"[gd_scene load_steps=4 format=3]

[sub_resource type="RectangleShape2D" id="Rect_belt"]
size = Vector2(400, 20)

[sub_resource type="RectangleShape2D" id="Rect_box"]
size = Vector2(16, 16)

[sub_resource type="PhysicsMaterial" id="Grippy"]
friction = 1.0

[node name="World" type="Node2D"]

[node name="Belt" type="StaticBody2D" parent="."]
position = Vector2(0, 100)
physics_material_override = SubResource("Grippy")
{belt_line}
[node name="BeltShape" type="CollisionShape2D" parent="Belt"]
shape = SubResource("Rect_belt")

[node name="Box" type="RigidBody2D" parent="."]
position = Vector2(0, 0)
physics_material_override = SubResource("Grippy")
can_sleep = false
lock_rotation = true

[node name="BoxShape" type="CollisionShape2D" parent="Box"]
shape = SubResource("Rect_box")

[node name="Platform" type="AnimatableBody2D" parent="."]
position = Vector2(0, 400)

[node name="PlatformShape" type="CollisionShape2D" parent="Platform"]
shape = SubResource("Rect_belt")
"#
    )
}

fn load(tscn: &str) -> (MainLoop, gdscene::NodeId) {
    gdobject::class_db::register_all_classes();
    let parsed = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &parsed).expect("scene instances");
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, scene_root)
}

fn node_pos(ml: &MainLoop, scene_root: gdscene::NodeId, name: &str) -> Vector2 {
    let id = ml
        .tree()
        .get_node_or_null(scene_root, name)
        .unwrap_or_else(|| panic!("node {name} exists"));
    match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("expected a Vector2 position for {name}, got {other:?}"),
    }
}

const STEP: f64 = 1.0 / 60.0;

/// What one run of the scene observed.
struct Run {
    /// Where the box settled, to prove the fixture itself works before any
    /// conclusion is drawn from its horizontal travel.
    settled: Vector2,
    /// Horizontal travel over the second AFTER settling.
    carried_dx: f32,
    /// Whether the `AnimatableBody2D` became a real physics body.
    platform_registered: bool,
}

fn run(belt_velocity: Option<&str>) -> Run {
    let (mut ml, scene_root) = load(&scene(belt_velocity));

    // One second to fall the ~82px and come to rest, matching the timing the
    // sibling test `physics2d_tscn_subresource_shapes_drive_collision` already
    // proves is enough for this geometry.
    for _ in 0..60 {
        ml.step(STEP);
    }
    let settled = node_pos(&ml, scene_root, "Box");

    // A second second, this time measuring horizontal travel.
    for _ in 0..60 {
        ml.step(STEP);
    }
    let after = node_pos(&ml, scene_root, "Box");

    let platform_id = ml
        .tree()
        .get_node_or_null(scene_root, "Platform")
        .expect("Platform exists");
    let platform_registered = ml.physics_server().body_for_node(platform_id).is_some();

    Run {
        settled,
        carried_dx: after.x - settled.x,
        platform_registered,
    }
}

/// **The acceptance test.** An authored conveyor carries a resting rigid body;
/// the same scene without the property does not.
#[test]
fn staticbody2d_conveyor_and_animatablebody_scene() {
    let belt = run(Some("Vector2(100, 0)"));
    let plain = run(None);

    // ⚠️ Fixture check FIRST. Every horizontal claim below is meaningless if
    // the box tunnelled through the belt or never landed — and a fallen-through
    // box would report a perfectly stable `carried_dx` of ~0, which reads as
    // "the conveyor is not wired" no matter what the bridge does.
    assert!(
        (belt.settled.y - 82.0).abs() < 2.0,
        "the box must come to rest ON the belt (y≈82); y={} — it never landed, \
         so nothing this test measures afterwards means anything",
        belt.settled.y
    );
    assert!(
        (plain.settled.y - 82.0).abs() < 2.0,
        "the control box must rest on the belt too (y≈82); y={}",
        plain.settled.y
    );

    // The control: with no `constant_linear_velocity` authored, a static floor
    // is just a floor.
    assert!(
        plain.carried_dx.abs() < 1.0,
        "a StaticBody2D with no conveyor authored must not push anything \
         sideways; the box drifted {}px",
        plain.carried_dx
    );

    // The feature: the belt drags the box along +x.
    //
    // ⚠️ Threshold deliberately far below the belt's 100px/s: the box
    // approaches belt speed rather than snapping to it, and the point of the
    // assertion is the DIRECTION and the difference from the control — not a
    // tuned distance. The proven solver-level fixture clears 10px in the same
    // 60 ticks, so 5px is a floor, not a target.
    assert!(
        belt.carried_dx > 5.0,
        "the conveyor must carry the box along +x; it moved {}px against the \
         control's {}px. ~0 here means `constant_linear_velocity` never made it \
         from the node onto the body — the solver term itself is covered by \
         `solver.rs::constant_linear_velocity_drags_a_resting_body`",
        belt.carried_dx,
        plain.carried_dx
    );

    // The other half of the acceptance, satisfied by pat-s3rrv.93 and asserted
    // here so a regression in `body_type_for_class` is caught by this bead's
    // test too.
    assert!(
        belt.platform_registered,
        "an AnimatableBody2D must register a real physics body, or it has no \
         collider at all and riders fall straight through it"
    );
}

/// A turntable's `constant_angular_velocity` reaches the body as well.
///
/// ⚠️ Asserted at the BODY, not through the solver. A rotating surface's effect
/// on a rider depends on the contact point's lever arm, so a scene-level
/// assertion would be measuring the solver's angular surface term rather than
/// the property wiring this bead adds. The wiring is the claim; the body's field
/// is where that claim is decided.
#[test]
fn staticbody2d_constant_angular_velocity_reaches_the_body() {
    let tscn = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="RectangleShape2D" id="Rect_table"]
size = Vector2(100, 20)

[node name="World" type="Node2D"]

[node name="Table" type="StaticBody2D" parent="."]
position = Vector2(0, 100)
constant_angular_velocity = 2.5

[node name="TableShape" type="CollisionShape2D" parent="Table"]
shape = SubResource("Rect_table")
"#;
    let (ml, scene_root) = load(tscn);
    let id = ml
        .tree()
        .get_node_or_null(scene_root, "Table")
        .expect("Table exists");
    let body_id = ml
        .physics_server()
        .body_for_node(id)
        .expect("the turntable registers a body");
    let body = ml
        .physics_server()
        .world()
        .get_body(body_id)
        .expect("body exists");

    assert!(
        (body.constant_angular_velocity - 2.5).abs() < 1e-6,
        "the authored turntable speed must reach the body, got {}",
        body.constant_angular_velocity
    );
    // Positive control for the pair: the linear field stays at Godot's
    // documented default rather than picking up stray state.
    assert_eq!(
        body.constant_linear_velocity,
        Vector2::ZERO,
        "an unauthored constant_linear_velocity must stay at Godot's Vector2(0, 0)"
    );
}
