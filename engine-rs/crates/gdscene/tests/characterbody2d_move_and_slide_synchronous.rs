//! pat-mgixk.37 acceptance: `move_and_slide()` does the whole slide BEFORE it
//! returns, so the next line of the same `_physics_process` sees the result.
//!
//! In Godot `CharacterBody2D::move_and_slide`
//! (`scene/2d/physics/character_body_2d.cpp`) moves the body, updates
//! `velocity`, the floor/wall/ceiling flags, the floor and wall normals and the
//! slide-collision list, and only then returns whether it collided. Everything
//! after the call in the same callback reads the new state.
//!
//! Patina's dispatch arm used to set a `_move_and_slide_requested` flag and let
//! `PhysicsServer::process_character_movement` do the real work after EVERY
//! script had run. Four things were wrong at once:
//!
//!   * `position`, `velocity`, `is_on_floor()` and `get_slide_collision*()`
//!     read right after the call were one tick stale;
//!   * `move_and_slide(); velocity = Vector2.ZERO` moved by the ZEROED
//!     velocity, because the deferred pass read `velocity` afterwards;
//!   * two calls in one tick moved the body once;
//!   * the return value was the PREVIOUS tick's flags.
//!
//! # What makes these tests non-vacuous
//!
//! Every assertion is a WITHIN-CALL difference, measured by the script itself:
//! it records a value before the call and the same value after, in the same
//! callback. Under the deferred implementation both reads returned the same
//! number, so each test fails without the change for the right reason — not
//! merely "the body ended up somewhere else".
//!
//! The `entered`/`ticks` counter pair is the no-script-errors assertion: a
//! handler that throws part-way increments the first and never the second. It
//! matters more than usual here, because a mis-typed native call would leave
//! every sentinel at its initial value and look like a clean failure.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Records `position.y` either side of the `move_and_slide()` call on the FIRST
/// tick, so the assertion needs no landing and no tuning.
///
/// `velocity.y += …` rather than an assignment because that is the form the
/// green reference test (`characterbody2d_move_and_slide_script_dispatch`)
/// proves resolves against an unauthored engine property.
const SYNC_GD: &str = "\
extends CharacterBody2D
var gravity = 980.0
var entered = 0
var ticks = 0
var before_y = -1.0
var after_y = -1.0
var hit_first = true
func _physics_process(delta):
    self.entered = self.entered + 1
    velocity.y += gravity * delta
    if self.ticks == 0:
        self.before_y = position.y
        self.hit_first = move_and_slide()
        self.after_y = position.y
    else:
        move_and_slide()
    self.ticks = self.ticks + 1
";

/// The ordering case: the script zeroes `velocity` AFTER asking to move, which
/// is ordinary Godot and used to cancel the move entirely.
const ZERO_AFTER_GD: &str = "\
extends CharacterBody2D
var entered = 0
var ticks = 0
var moved = -1.0
func _physics_process(delta):
    self.entered = self.entered + 1
    if self.ticks == 0:
        velocity.y += 600.0
        var before = position.y
        move_and_slide()
        velocity.y = 0.0
        self.moved = position.y - before
    self.ticks = self.ticks + 1
";

/// Reads the floor normal and angle back INSIDE the callback that landed.
const NORMALS_GD: &str = "\
extends CharacterBody2D
var gravity = 980.0
var entered = 0
var ticks = 0
var floor_n = 0
var wall_n = 0
var angle = -1.0
var landed = false
func _physics_process(delta):
    self.entered = self.entered + 1
    velocity.y += gravity * delta
    move_and_slide()
    if is_on_floor():
        self.landed = true
        self.floor_n = get_floor_normal()
        self.wall_n = get_wall_normal()
        self.angle = get_floor_angle()
    self.ticks = self.ticks + 1
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn script_vec2(ml: &MainLoop, node: NodeId, name: &str) -> Vector2 {
    match script_var(ml, node, name) {
        Some(Variant::Vector2(v)) => v,
        // The sentinels start as Ints on purpose: an Int here means the native
        // call never dispatched and returned nothing to assign.
        other => panic!("script var `{name}` should be a Vector2, got {other:?}"),
    }
}

fn script_f32(ml: &MainLoop, node: NodeId, name: &str) -> f32 {
    match script_var(ml, node, name) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        other => panic!("script var `{name}` should be a number, got {other:?}"),
    }
}

/// Every handler entry reached its last statement, so nothing in between threw.
///
/// Checked in each test: a native call that failed to dispatch would leave the
/// sentinels untouched, which is indistinguishable from the stale behaviour
/// this bead removes.
fn assert_no_script_errors(ml: &MainLoop, node: NodeId) {
    let entered = script_var(ml, node, "entered");
    let ticks = script_var(ml, node, "ticks");
    assert!(
        matches!(ticks, Some(Variant::Int(n)) if n > 0),
        "_physics_process must have run at all, got {ticks:?}",
    );
    assert_eq!(
        entered, ticks,
        "every _physics_process entry must reach its last statement — a mismatch \
         means a statement threw mid-handler, so the sentinels below prove nothing",
    );
}

/// `Main -> {Floor (StaticBody2D at y=200, 1000x20), Player (CharacterBody2D at
/// the origin, radius 10, scripted)}`.
///
/// The call sequence is copied from the green reference test for this
/// subsystem: collision geometry comes from a child `CollisionShape2D`, which
/// is what the physics server's `find_collision_shape` reads, and
/// `register_physics_bodies` registers the scene before the first `step` the
/// way `patina-runner` does. `velocity` is deliberately not authored.
fn platformer(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 200.0)));
    let floor_id = tree.add_child(main, floor).unwrap();
    let mut floor_shape = Node::new("CollisionShape", "CollisionShape2D");
    floor_shape.set_property("size", Variant::Vector2(Vector2::new(1000.0, 20.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    let mut body = Node::new("Player", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(main, body).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(body_id, shape).unwrap();
    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(src, body_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, body_id)
}

/// **The acceptance test.** The body has already moved by the time
/// `move_and_slide()` returns, measured inside the same callback.
#[test]
fn characterbody2d_move_and_slide_synchronous() {
    let (mut ml, player) = platformer(SYNC_GD);
    ml.step(1.0 / 60.0);
    assert_no_script_errors(&ml, player);

    let before = script_f32(&ml, player, "before_y");
    let after = script_f32(&ml, player, "after_y");

    // The control: the script really did reach the recording branch, so the
    // comparison below is between two readings and not two initial values.
    assert!(
        before > -1.0 || after > -1.0,
        "the script never reached its recording branch (before_y={before}, \
         after_y={after})",
    );
    assert!(
        after > before,
        "`position.y` read on the line AFTER move_and_slide() must already show \
         the move: got before={before}, after={after}. Equal values are the \
         deferred implementation — the slide had not happened yet when the \
         script read back",
    );

    // Gravity over one 60 Hz tick is ~16.3 px/s, so the move is ~0.27 px. Only
    // its sign is asserted; the magnitude is the solver's business.
    assert!(
        after - before > 0.0,
        "the body must move DOWN under gravity, got {}",
        after - before,
    );

    // The return value describes THIS call. Falling through open air collides
    // with nothing, so it must be false — and it starts as `true`, so a
    // sentinel that was never written cannot pass.
    assert_eq!(
        script_var(&ml, player, "hit_first"),
        Some(Variant::Bool(false)),
        "move_and_slide() must return whether THIS call collided; a free fall \
         through open air collides with nothing. `true` here is the old \
         behaviour, which returned the previous tick's floor/wall/ceiling flags",
    );
}

/// `move_and_slide(); velocity = 0` moves by the PRE-zero velocity, because the
/// slide is already finished when the assignment runs.
#[test]
fn velocity_zeroed_after_the_call_does_not_cancel_the_move() {
    let (mut ml, player) = platformer(ZERO_AFTER_GD);
    ml.step(1.0 / 60.0);
    assert_no_script_errors(&ml, player);

    // 600 px/s for one 60 Hz tick is 10 px, well clear of the floor at y=200.
    let moved = script_f32(&ml, player, "moved");
    assert!(
        moved > 1.0,
        "`velocity.y += 600; move_and_slide(); velocity.y = 0` must move by the \
         velocity that was in force AT THE CALL — expected ~10 px, got {moved}. \
         Zero means the deferred pass read `velocity` after the script had \
         already zeroed it, which is the ordering bug this bead fixes",
    );
}

/// The floor normal, wall normal and floor angle are readable from the script,
/// in the callback that landed. None of the three had a dispatch arm at all
/// before this bead, so a script asking for one got `Nil`.
#[test]
fn floor_and_wall_normals_reach_the_script() {
    let (mut ml, player) = platformer(NORMALS_GD);
    // Two seconds: long enough to fall 200 px onto the floor and land.
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }
    assert_no_script_errors(&ml, player);

    // The control: without a landing the three sentinels below keep their
    // initial values and every assertion would be vacuous.
    assert_eq!(
        script_var(&ml, player, "landed"),
        Some(Variant::Bool(true)),
        "precondition: the body must land on the floor at y=200 within 2s",
    );

    // Flat ground with the default `up_direction` of (0, -1): the floor normal
    // points up the screen, which is NEGATIVE y in Godot's 2D coordinates.
    //
    // ⚠️ Asserted with a tolerance, not as an exact `Vector2::new(0.0, -1.0)`.
    // The normal is solver-computed and normalised, and the solver's own unit
    // test (`get_floor_normal_returns_correct_value`) only claims `y < -0.5` —
    // so an exact comparison would be asserting a precision nothing promises.
    // The tolerance is still far tighter than any wrong answer: a missing
    // dispatch gives ZERO, a wall normal gives x = ±1, and the sign of y is
    // what distinguishes up from down.
    let floor_n = script_vec2(&ml, player, "floor_n");
    assert!(
        floor_n.x.abs() < 1e-3 && (floor_n.y + 1.0).abs() < 1e-3,
        "get_floor_normal() must answer ~(0, -1) on flat ground, got {floor_n:?}",
    );
    // Nothing to the sides, so Godot answers the zero vector rather than null —
    // a script may chain `.dot()` on it without a guard. Exact here because
    // this value is the solver's initialiser, not a computed one.
    assert_eq!(
        script_vec2(&ml, player, "wall_n"),
        Vector2::ZERO,
        "get_wall_normal() must answer Vector2.ZERO when not on a wall, not Nil",
    );
    // acos(normal · up) with both pointing the same way is acos(1) = 0. The
    // bound is ~3°, which the same normalisation argument above requires: a
    // real slope in this fixture would be 0, and a WRONG answer is either
    // acos(0) = 90° (normal not published) or acos(-1) = 180° (sign flipped).
    let angle = script_f32(&ml, player, "angle");
    assert!(
        angle.abs() < 0.05,
        "get_floor_angle() must be ~0 rad on flat ground, got {angle}",
    );
}
