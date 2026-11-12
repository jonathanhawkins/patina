//! pat-s3rrv.93 acceptance: `AnimatableBody2D` / `AnimatableBody3D` moving
//! platforms register a real collider and carry riders.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/animatable_body_2d.cpp`
//! and `scene/3d/physics/animatable_body_3d.cpp`. Both are a StaticBody with one
//! property, `sync_to_physics`, defaulting to **true**: the node's per-frame
//! transform is pushed into the physics body, and the body's velocity is derived
//! from that motion so bodies resting on it are carried.
//!
//! ⚠️ Neither class existed at ANY layer before this bead — not in ClassDB, and
//! not in `body_type_for_class` / `body_type_for_class_3d`. A `.tscn` platform
//! therefore registered no body at all, so it had no collider and a rider fell
//! straight through it. That makes "the rider does not fall" as much a part of
//! the acceptance as "the rider is carried".

use gdcore::math::{Vector2, Vector3};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Platform top surface is y = 90 (centre 100, half-height 10); a rider of
/// half-height 8 therefore rests centred at y = 82.
///
/// The rider starts ABOVE the platform with a downward velocity rather than
/// already resting on it. That is not decoration: `process_character_movement`
/// skips a body whose own velocity AND stored platform velocity are both zero,
/// so a rider spawned at rest would never run `move_and_slide`, never detect the
/// floor, and never acquire the platform velocity that keeps it being processed.
/// Falling onto the platform is what bootstraps the carry — exactly as gravity
/// does in a real scene.
fn scene_2d(sync_to_physics: bool) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="RectangleShape2D" id="Rect_plat"]
size = Vector2(200, 20)

[sub_resource type="RectangleShape2D" id="Rect_rider"]
size = Vector2(16, 16)

[node name="World" type="Node2D"]

[node name="Platform" type="AnimatableBody2D" parent="."]
position = Vector2(0, 100)
sync_to_physics = {sync_to_physics}

[node name="PlatformShape" type="CollisionShape2D" parent="Platform"]
shape = SubResource("Rect_plat")

[node name="Rider" type="CharacterBody2D" parent="."]
position = Vector2(0, 60)
velocity = Vector2(0, 120)

[node name="RiderShape" type="CollisionShape2D" parent="Rider"]
shape = SubResource("Rect_rider")
"#
    )
}

fn load(tscn: &str) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml
}

fn pos_2d(ml: &MainLoop, name: &str) -> Vector2 {
    let root = ml.tree().root_id();
    let id = ml
        .tree()
        .find_child(root, name, true)
        .unwrap_or_else(|| panic!("{name} exists"));
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v,
        other => panic!("{name}.position should be a Vector2, got {other:?}"),
    }
}

/// Nudges the platform right by `dx` — what an AnimationPlayer track would do.
fn move_platform(ml: &mut MainLoop, dx: f32) {
    let root = ml.tree().root_id();
    let id = ml
        .tree()
        .find_child(root, "Platform", true)
        .expect("Platform");
    let current = pos_2d(ml, "Platform");
    if let Some(node) = ml.tree_mut().get_node_mut(id) {
        node.set_property(
            "position",
            Variant::Vector2(Vector2::new(current.x + dx, current.y)),
        );
    }
}

/// Re-applies the rider's downward velocity, which is what a real scene's
/// `_physics_process` does with gravity every tick.
///
/// ⚠️ **Without this the rider falls asleep and the carry never starts.**
/// `process_character_movement` skips a body whose own velocity AND stored
/// platform velocity are both ~0 (`physics_server.rs:2340`). Landing zeroes the
/// rider's velocity, and while the platform is still stationary there is no
/// stored platform velocity either — so the rider goes idle during the settle
/// loop and nothing ever wakes it, however far the platform then travels. The
/// symptom reads exactly like "the carry is not implemented", which is why this
/// is called out rather than quietly folded into the loop.
fn arm_rider(ml: &mut MainLoop) {
    let root = ml.tree().root_id();
    let id = ml.tree().find_child(root, "Rider", true).expect("Rider");
    if let Some(node) = ml.tree_mut().get_node_mut(id) {
        node.set_property("velocity", Variant::Vector2(Vector2::new(0.0, 120.0)));
    }
}

const STEP: f64 = 1.0 / 60.0;
/// 2 px per tick at 60 fps = 120 px/s.
const PLATFORM_DX: f32 = 2.0;
const CARRY_TICKS: usize = 30;

/// Runs the scene: settle the rider onto the platform, then drive the platform
/// right for `CARRY_TICKS`. Returns the rider's position before and after.
fn run_2d(sync_to_physics: bool) -> Run2D {
    let mut ml = load(&scene_2d(sync_to_physics));
    // Settle: the rider falls the 22px onto the platform and lands.
    for _ in 0..30 {
        arm_rider(&mut ml);
        ml.step(STEP);
    }
    let settled = pos_2d(&ml, "Rider");
    let platform_before = pos_2d(&ml, "Platform");
    for _ in 0..CARRY_TICKS {
        arm_rider(&mut ml);
        move_platform(&mut ml, PLATFORM_DX);
        ml.step(STEP);
    }
    Run2D {
        settled,
        carried: pos_2d(&ml, "Rider"),
        platform_travel: pos_2d(&ml, "Platform").x - platform_before.x,
    }
}

/// What one 2D run observed.
struct Run2D {
    settled: Vector2,
    carried: Vector2,
    /// How far the platform ACTUALLY moved, measured rather than assumed.
    platform_travel: f32,
}

/// **The acceptance test.**
#[test]
fn animatable_body_carries_riders() {
    let run = run_2d(true);
    let (settled, carried) = (run.settled, run.carried);

    // --- (0) The fixture itself moved. Every assertion below compares the
    //     rider against a platform travel this test only ASSUMED until now:
    //     the expected distance is a computed constant, so if the platform had
    //     silently failed to move, "rider moved 0px" would have been reported
    //     against a platform that also moved 0px and read as an engine bug.
    let travel = PLATFORM_DX * CARRY_TICKS as f32;
    assert!(
        (run.platform_travel - travel).abs() < 1.0,
        "the fixture must actually drive the platform {travel}px before the \
         rider can be expected to follow; it moved {}px",
        run.platform_travel,
    );

    // --- (a) It has a COLLIDER. Before this bead the platform registered no
    //     body at all, so the rider fell forever. y is the assertion that
    //     catches that, and it has to come first: a rider in free fall also
    //     fails to be carried, and "not carried" would misdescribe the bug.
    assert!(
        settled.y < 95.0,
        "the rider must land ON the platform (top surface y = 90), not fall \
         through it; settled at y = {}",
        settled.y
    );
    assert!(
        (carried.y - settled.y).abs() < 2.0,
        "a carried rider must stay on the platform, not sink or fall; y moved \
         from {} to {}",
        settled.y,
        carried.y
    );

    // --- (b) It CARRIES. The platform travels 60px over these ticks; the
    //     rider inherits the platform velocity recorded on the previous tick,
    //     so it lags by about one tick and cannot reach the full distance.
    //     Two thirds is comfortably above that lag and far above zero.
    let advanced = carried.x - settled.x;
    assert!(
        advanced > travel * 0.66,
        "a rider standing on a moving AnimatableBody2D must travel with it: \
         platform moved {travel}px, rider moved {advanced}px"
    );
    assert!(
        advanced <= travel + 1.0,
        "the rider must not OUTRUN the platform; moved {advanced}px against \
         the platform's {travel}px"
    );
}

/// **Diagnostic.** Prints the per-tick rider delta so the carry PATTERN is
/// visible, not just the total (pat-s3rrv.93).
///
/// The total alone cannot distinguish two very different bugs that both land on
/// 30 of 60 px:
///
/// * a **uniform half** — every tick carries 1px, i.e. the platform velocity
///   reaching the rider is 60 px/s instead of 120, a scaling error; versus
/// * an **alternating** 2,0,2,0 — every other tick carries nothing, i.e. the
///   platform is being FORGOTTEN between ticks, a state error.
///
/// Those need opposite fixes, and a passing/failing total points at neither.
/// The assertion below is written so its failure message carries the sequence.
#[test]
fn carry_is_uniform_per_tick() {
    let mut ml = load(&scene_2d(true));
    for _ in 0..30 {
        arm_rider(&mut ml);
        ml.step(STEP);
    }

    let mut deltas: Vec<f32> = Vec::new();
    let mut stored: Vec<f32> = Vec::new();
    let mut floors: Vec<bool> = Vec::new();
    let mut steps: Vec<u32> = Vec::new();
    let mut prev = pos_2d(&ml, "Rider").x;
    for _ in 0..CARRY_TICKS {
        arm_rider(&mut ml);
        move_platform(&mut ml, PLATFORM_DX);
        // ⚠️ How many FIXED physics ticks this one `step` ran. The fixture moves
        // the platform ONCE per `step`, so if this is ever >1 the extra ticks
        // see a stationary platform and derive a zero velocity from it.
        steps.push(ml.step(STEP).physics_steps);
        let now = pos_2d(&ml, "Rider").x;
        deltas.push(now - prev);
        prev = now;
        // What the rider RECORDED as the platform under it. The carry reads
        // this on the next tick, so if it alternates, so does the carry — and
        // the question becomes why the floor lookup missed.
        let root = ml.tree().root_id();
        let rid = ml.tree().find_child(root, "Rider", true).expect("Rider");
        let pv = match ml
            .tree()
            .get_node(rid)
            .map(|n| n.get_property("platform_velocity"))
        {
            Some(Variant::Vector2(v)) => v.x,
            _ => f32::NAN,
        };
        let on_floor = matches!(
            ml.tree()
                .get_node(rid)
                .map(|n| n.get_property("is_on_floor")),
            Some(Variant::Bool(true))
        );
        stored.push(pv);
        floors.push(on_floor);
    }

    // Ignore the first tick: the carry reads the platform velocity recorded on
    // the PREVIOUS tick, and the platform was stationary through the settle
    // loop, so tick 1 legitimately carries nothing.
    let steady = &deltas[1..];
    let zeros = steady.iter().filter(|d| d.abs() < 0.5).count();
    let full = steady
        .iter()
        .filter(|d| (**d - PLATFORM_DX).abs() < 0.5)
        .count();

    assert_eq!(
        zeros,
        0,
        "every steady-state tick must carry the rider. {zeros} of {} ticks \
         carried ~0px and {full} carried the full {PLATFORM_DX}px. A run of \
         alternating 2,0,2,0 means the platform is forgotten between ticks (a \
         state bug); a uniform 1px means the velocity reaching the rider is \
         halved (a scaling bug). Sequence: {steady:?}\n\
         rider-recorded platform_velocity.x per tick: {stored:?}\n\
         rider is_on_floor per tick: {floors:?}\n\
         physics_steps per step() call: {steps:?}",
        steady.len()
    );
}

/// ⚠️ The control, and the reason the test above cannot pass by accident.
///
/// With `sync_to_physics = false` an AnimatableBody2D is a plain static body:
/// it still collides, so the rider still lands on it, but it imparts no
/// velocity and the platform slides out from under the rider. Without this
/// case, a bug that carried riders regardless of the flag — or one that moved
/// the rider for some unrelated reason — would look identical to success.
#[test]
fn sync_to_physics_false_reverts_to_a_plain_static_body() {
    let run = run_2d(false);
    let (settled, after) = (run.settled, run.carried);

    // The control is only meaningful if the platform moved here too — an
    // unmoved platform leaves the rider behind for the wrong reason.
    let travel = PLATFORM_DX * CARRY_TICKS as f32;
    assert!(
        (run.platform_travel - travel).abs() < 1.0,
        "the control's platform must move too, or 'the rider was left behind' \
         proves nothing; it moved {}px",
        run.platform_travel,
    );

    assert!(
        settled.y < 95.0,
        "the platform must still COLLIDE with sync_to_physics off; the rider \
         settled at y = {}",
        settled.y
    );
    let advanced = after.x - settled.x;
    assert!(
        advanced.abs() < 2.0,
        "with sync_to_physics = false the rider must be left behind, not \
         carried; it moved {advanced}px"
    );
}

// ---------------------------------------------------------------------------
// 3D
// ---------------------------------------------------------------------------

/// The 3D platform, driven the same way an AnimationPlayer track would drive it.
fn scene_3d(sync_to_physics: bool) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="BoxShape3D" id="Box_plat"]
size = Vector3(20, 2, 20)

[node name="Root" type="Node3D"]

[node name="Platform3D" type="AnimatableBody3D" parent="."]
position = Vector3(0, 0, 0)
sync_to_physics = {sync_to_physics}

[node name="PlatformShape" type="CollisionShape3D" parent="Platform3D"]
shape = SubResource("Box_plat")
"#
    )
}

/// Registers the scene, moves the platform by `dx` on x for one tick, and
/// returns `(body_was_registered, the body's linear velocity after the step)`.
///
/// ⚠️ Asserted on the BODY rather than on a rider, deliberately. The 2D test
/// above proves the end-to-end carry; what is 3D-specific here is the two
/// things this bead actually added — that an AnimatableBody3D registers a body
/// at all (it previously registered NONE, which is the "no collider" bug), and
/// that its per-tick motion becomes the `linear_velocity` that
/// `CharacterBody3D`'s carry reads. Driving a 3D rider through a full landing
/// would additionally depend on 3D resting behaviour this bead does not touch.
fn platform_velocity_3d(sync_to_physics: bool, dx: f32) -> (bool, Vector3) {
    let mut ml = load(&scene_3d(sync_to_physics));
    ml.step(STEP);
    let root = ml.tree().root_id();
    let id = ml
        .tree()
        .find_child(root, "Platform3D", true)
        .expect("Platform3D exists");

    // Move the platform, then step so the server converts the displacement.
    //
    // ⚠️ **`set_property("position", ..)` is a NO-OP on a Node3D here.**
    // `node3d::local_transform_of` returns the explicit `transform` property
    // when one exists and ignores `position` entirely (`node3d.rs:69-72`), and
    // the first step above writes one. A raw position write is then silently
    // discarded, the platform never moves, and the derived velocity is zero —
    // which reads as "the velocity derivation is broken" rather than as a
    // fixture bug. `node3d::set_position` rebuilds the local transform through
    // `store_local_transform`, keeping both representations in step.
    gdscene::node3d::set_position(ml.tree_mut(), id, Vector3::new(dx, 0.0, 0.0));
    ml.step(STEP);

    let server = ml.physics_server_3d();
    match server.body_for_node(id) {
        Some(bid) => match server.world().get_body(bid) {
            Some(b) => (true, b.linear_velocity),
            None => (true, Vector3::ZERO),
        },
        None => (false, Vector3::ZERO),
    }
}

/// An `AnimatableBody3D` registers a real body and turns its motion into the
/// velocity a rider is carried by.
#[test]
fn animatable_body_3d_registers_a_body_and_derives_its_velocity() {
    let dx = 0.05_f32;
    let (registered, vel) = platform_velocity_3d(true, dx);

    // The original defect: `body_type_for_class_3d` did not know the class, so
    // NO body was registered and the platform had no collider at all.
    assert!(
        registered,
        "an AnimatableBody3D must register a physics body; with none it has no          collider and anything standing on it falls through"
    );

    // The displacement is converted with the same `dt` the step used, so the
    // expected speed is dx / STEP regardless of the tick length.
    let expected = dx / STEP as f32;
    assert!(
        (vel.x - expected).abs() < expected * 0.05,
        "the platform's per-tick motion must become its linear velocity:          expected about {expected}, got {:?}",
        vel
    );
}

/// The 3D control: `sync_to_physics = false` still registers the collider but
/// imparts no velocity.
#[test]
fn animatable_body_3d_sync_to_physics_false_imparts_no_velocity() {
    let (registered, vel) = platform_velocity_3d(false, 0.05);
    assert!(
        registered,
        "the body must still exist, it just does not carry"
    );
    assert!(
        vel.length() < 1e-3,
        "with sync_to_physics = false the platform must impart no velocity,          got {vel:?}"
    );
}
