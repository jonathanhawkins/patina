//! pat-0q2kx.33 acceptance: a **verbatim Godot** `CharacterBody2D` platformer
//! script drives the body — `velocity.y += gravity * delta`, `move_and_slide()`,
//! `is_on_floor()` — with nothing seeded on the node to help it.
//!
//! pat-0q2kx.16 wired the native method dispatch, but its own fixture had to seed
//! `velocity = Vector2(0, 0)` on the node first, and said so: the interpreter only
//! routes `velocity.y += …` to the node when that property already exists and is
//! non-Nil. A scene exported from Godot never authors `velocity` (it is runtime
//! state), so the most ordinary platformer script in existence died mid-frame with
//! "cannot assign member 'y' on Nil" and the body never fell.
//!
//! These drive real `MainLoop::step` frames and assert on **tree state**, so a
//! regression to "resolves but does nothing" fails. The `entered`/`ticks` counter
//! pair is the no-script-errors assertion: a handler that throws part-way
//! increments the first but never the second.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Verbatim Godot — this is the shape of the official CharacterBody2D template:
/// no `self.` on the engine property, no seeding, gravity applied per-delta, floor
/// state read back inside the same handler, and `velocity.y` zeroed on landing.
///
/// `entered` counts handler entries and `ticks` counts completions, so
/// `entered == ticks` proves nothing in between ever threw — without the test
/// having to predict how many physics ticks a frame produces.
const PLAYER_GD: &str = "\
extends CharacterBody2D
var gravity = 980.0
var landed = false
var entered = 0
var ticks = 0
func _physics_process(delta):
    self.entered = self.entered + 1
    velocity.y += gravity * delta
    move_and_slide()
    if is_on_floor():
        self.landed = true
        velocity.y = 0
    self.ticks = self.ticks + 1
";

/// Identical, minus the `move_and_slide()` call: Godot never moves a body that
/// does not ask to.
const IDLER_GD: &str = "\
extends CharacterBody2D
var gravity = 980.0
var ticks = 0
func _physics_process(delta):
    velocity.y += gravity * delta
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

fn node_vec2(ml: &MainLoop, node: NodeId, prop: &str) -> Vector2 {
    match ml.tree().get_node(node).unwrap().get_property(prop) {
        Variant::Vector2(v) => v,
        other => panic!("{prop} should be a Vector2, got {other:?}"),
    }
}

/// `Main -> {Floor (StaticBody2D at y=200, 1000x20), <bodies…>}`, each body a
/// `CharacterBody2D` at the origin with a radius-10 shape and an attached script.
///
/// Collision geometry comes from a child `CollisionShape2D` (`size` = rect,
/// `radius` = circle), which is what the physics server's `find_collision_shape`
/// reads. **`velocity` is deliberately not seeded** — that omission is the bead.
fn platformer_scene(scripts: &[&str]) -> (MainLoop, Vec<NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 200.0)));
    let floor_id = tree.add_child(main, floor).unwrap();
    let mut floor_shape = Node::new("CollisionShape", "CollisionShape2D");
    floor_shape.set_property("size", Variant::Vector2(Vector2::new(1000.0, 20.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    let mut ids = Vec::new();
    for (i, src) in scripts.iter().enumerate() {
        let mut body = Node::new(&format!("Player{i}"), "CharacterBody2D");
        // Side by side, not stacked: two CharacterBody2Ds at the same spot
        // OVERLAP, and Godot's `move_and_slide` recovers the mover out of its
        // twin (`test_body_motion` step 1) — it climbed 20 px instead of
        // falling once Patina ran Godot's motion query.
        body.set_property(
            "position",
            Variant::Vector2(Vector2::new(100.0 * i as f32, 0.0)),
        );
        let body_id = tree.add_child(main, body).unwrap();
        let mut shape = Node::new("CollisionShape", "CollisionShape2D");
        shape.set_property("radius", Variant::Float(10.0));
        tree.add_child(body_id, shape).unwrap();
        tree.attach_script(
            body_id,
            Box::new(GDScriptNodeInstance::from_source(src, body_id).unwrap()),
        );
        ids.push(body_id);
    }

    // The explicit boot registration, mirroring `patina-runner`. Since
    // pat-s3rrv.25 `sync_to_physics` also re-scans the tree every tick, so this
    // is no longer the *only* thing standing between the fixture and an empty
    // `node_to_body` — but it still registers the scene one step earlier, before
    // the first `step`, which is what the assertions below are written against.
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, ids)
}

/// **The acceptance test.** Each link is asserted separately so a failure names
/// the broken one rather than only reporting "it did not move".
#[test]
fn characterbody2d_move_and_slide_script_dispatch() {
    let (mut ml, ids) = platformer_scene(&[PLAYER_GD]);
    let player = ids[0];

    // Link 0 — the fixture really is verbatim Godot: nothing authored `velocity`
    // on the node. If a future change seeds it, this test stops testing the bead.
    assert!(
        ml.tree()
            .get_node(player)
            .unwrap()
            .get_property("velocity")
            .is_nil(),
        "the fixture must not author `velocity` — an exported Godot scene does not",
    );
    let start = node_vec2(&ml, player, "position");

    ml.step(1.0 / 60.0);

    // Link 1 — `velocity.y += gravity * delta` resolved the unauthored engine
    // property and its read-modify-write landed on the *node*, not in a local.
    let vel = node_vec2(&ml, player, "velocity");
    assert!(
        vel.y > 0.0,
        "link 1: `velocity.y += gravity * delta` must reach the node property, got {vel:?}",
    );

    // Link 2 — the bare `move_and_slide()` reached the native dispatch AND the
    // physics phase honoured it inside this same tick.
    //
    // ⚠️ Rewritten by pat-0q2kx.201. This used to assert the one-shot request
    // was still `true` after the step, because the character pass ran BEFORE
    // the script phase and so could not have consumed a request the script had
    // not made yet. Now that `_physics_process` runs before the step (Godot's
    // `Main::iteration` order) the request is raised and consumed in one tick,
    // which is the whole point of the reorder — so the flag reads `false` and
    // the body has already moved. Both halves are asserted: the `false` alone
    // would also hold if the call never reached dispatch at all.
    assert_eq!(
        ml.tree()
            .get_node(player)
            .unwrap()
            .get_property("_move_and_slide_requested"),
        Variant::Bool(false),
        "link 2: the one-shot request must be CONSUMED in the tick the script \
         raised it (pat-0q2kx.201 ordering)",
    );
    let after_one = node_vec2(&ml, player, "position");
    assert!(
        after_one.y > start.y,
        "link 2: a bare move_and_slide() must reach SceneAccess::call_node_method \
         and move the body within that SAME tick — under the old step-before-scripts \
         order one step moved it nowhere, {start:?} -> {after_one:?}",
    );

    // Link 3 — two seconds of frames: the body falls and is stopped by the floor
    // rather than tunnelling through it. The floor's top face is y=190 and the
    // body's shape has radius 10, so it comes to rest around y=180.
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }
    let pos = node_vec2(&ml, player, "position");
    assert!(
        pos.y > start.y + 100.0,
        "link 3: the body must fall toward the floor at y=200, {start:?} -> {pos:?}",
    );
    assert!(
        pos.y < 200.0,
        "link 3: the floor must stop it rather than let it tunnel through, got {pos:?}",
    );

    // Link 4 — `is_on_floor()` answered true *inside* the script once it landed.
    assert_eq!(
        script_var(&ml, player, "landed"),
        Some(Variant::Bool(true)),
        "link 4: is_on_floor() must report true inside _physics_process after landing",
    );

    // Link 5 — every handler entry also reached the last line, so no statement in
    // between threw. This is the "runs without script errors" clause.
    let entered = script_var(&ml, player, "entered");
    let ticks = script_var(&ml, player, "ticks");
    assert!(
        matches!(ticks, Some(Variant::Int(n)) if n > 0),
        "link 5: _physics_process must have run at all, got {ticks:?}",
    );
    assert_eq!(
        entered, ticks,
        "link 5: every _physics_process entry must reach its last statement — \
         a mismatch means the handler threw mid-frame",
    );
}

/// The other half of Godot's contract: a body whose script never calls
/// `move_and_slide()` does not move, however much velocity it accumulates.
#[test]
fn a_script_that_never_calls_move_and_slide_leaves_the_body_stationary() {
    let (mut ml, ids) = platformer_scene(&[PLAYER_GD, IDLER_GD]);
    let (mover, idler) = (ids[0], ids[1]);
    let start = node_vec2(&ml, idler, "position");

    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }

    // The idler accumulated velocity — so it is not stationary for want of
    // gravity, it is stationary because it never asked to move.
    let idle_vel = node_vec2(&ml, idler, "velocity");
    assert!(
        idle_vel.y > 0.0,
        "the idler's script must still be applying gravity, got {idle_vel:?}",
    );
    let idle_pos = node_vec2(&ml, idler, "position");
    assert!(
        (idle_pos.y - start.y).abs() < 1e-3,
        "a body whose script never calls move_and_slide() must not move, got {idle_pos:?}",
    );

    // …while its twin, in the same scene and the same frames, did move.
    let moved = node_vec2(&ml, mover, "position");
    assert!(
        moved.y > start.y + 1.0,
        "the body that called move_and_slide() must have moved, got {moved:?}",
    );
}

/// The vector getters answer with vectors from the first frame, before any physics
/// tick has published state — Godot's return `Vector2.ZERO`, never null, and a
/// script chaining `.length()` on them must not die.
#[test]
fn character_vector_getters_answer_before_the_first_physics_tick() {
    // Sentinels are Ints, so "still an Int" means the getter never assigned.
    let probe = "\
extends CharacterBody2D
var seen_velocity = 0
var real_v = 0
var plat_v = 0
var speed = -1.0
func _physics_process(delta):
    self.seen_velocity = velocity
    self.real_v = get_real_velocity()
    self.plat_v = get_platform_velocity()
    self.speed = get_real_velocity().length()
";
    let (mut ml, ids) = platformer_scene(&[probe]);
    let player = ids[0];

    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, player, "seen_velocity"),
        Some(Variant::Vector2(Vector2::ZERO)),
        "an unauthored `velocity` must read back as Vector2.ZERO, not null",
    );
    assert!(
        matches!(script_var(&ml, player, "real_v"), Some(Variant::Vector2(_))),
        "get_real_velocity() must return a Vector2, got {:?}",
        script_var(&ml, player, "real_v"),
    );
    assert!(
        matches!(script_var(&ml, player, "plat_v"), Some(Variant::Vector2(_))),
        "get_platform_velocity() must return a Vector2, got {:?}",
        script_var(&ml, player, "plat_v"),
    );
    // Reached only if `.length()` on the getter's result did not throw.
    assert!(
        matches!(script_var(&ml, player, "speed"), Some(Variant::Float(f)) if f >= 0.0),
        "get_real_velocity().length() must be callable, got {:?}",
        script_var(&ml, player, "speed"),
    );
}
