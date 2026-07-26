//! pat-0q2kx.140 acceptance: `SceneTree.paused` must freeze the simulation.
//!
//! Godot's `SceneTree::set_pause` (scene/main/scene_tree.cpp:1111-1140) calls
//! `PhysicsServer2D/3D::set_active(!paused)`, so a paused game does not step
//! physics at all, and the animation/tween/particle/timer phases stop with it.
//!
//! `MainLoop::step` had **no pause guard**: it stepped 2D and 3D physics,
//! character movement, animations, tweens, particles, path-follows, navigation
//! and `Timer` nodes unconditionally. A paused Patina game kept simulating
//! everything except script callbacks — bodies fell, timers counted down,
//! animations and tweens advanced.
//!
//! These drive real `MainLoop::step` frames and assert the world holds still,
//! then resumes.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::tween::{Tween, TweenPauseMode};
use gdscene::MainLoop;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// `Root -> Faller (RigidBody2D + CollisionShape2D)` plus a repeating `Timer`.
fn world() -> (MainLoop, NodeId, NodeId) {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();

    let faller = tree
        .add_child(root, Node::new("Faller", "RigidBody2D"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(faller) {
        n.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        // A body with no mass is not simulated; the working RigidBody2D fixtures
        // in this crate all set it explicitly.
        n.set_property("mass", Variant::Float(1.0));
        n.set_property("gravity_scale", Variant::Float(1.0));
        // Sleeping would freeze the body for reasons unrelated to pause.
        n.set_property("can_sleep", Variant::Bool(false));
    }
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("CircleShape2D".to_string()));
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(faller, shape).unwrap();

    let timer = tree.add_child(root, Node::new("Tick", "Timer")).unwrap();
    if let Some(n) = tree.get_node_mut(timer) {
        n.set_property("wait_time", Variant::Float(5.0));
        n.set_property("one_shot", Variant::Bool(false));
        n.set_property("autostart", Variant::Bool(true));
    }

    let mut ml = MainLoop::new(tree);
    // `MainLoop::new` does not apply project settings, so the 2D space gravity
    // is whatever the world defaults to. Set it directly: this test is about
    // pause gating, and it must not silently depend on the settings table for
    // its control run to move at all.
    ml.physics_server_mut()
        .set_gravity(Vector2::new(0.0, 980.0));
    // ⚠️ THE thing that made this fixture inert: `MainLoop::step` does NOT
    // register physics bodies — `sync_to_physics` only iterates bodies already
    // in `node_to_body`, and `register_physics_bodies` is an explicit
    // "call after loading a scene" step. Without it the RigidBody2D is not in
    // the world at all, so it never falls and a pause test has nothing to gate.
    ml.register_physics_bodies();
    (ml, faller, timer)
}

fn run(ml: &mut MainLoop, frames: usize) {
    for _ in 0..frames {
        ml.step(DT);
    }
}

fn pos_y(ml: &MainLoop, id: NodeId) -> f32 {
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v.y,
        _ => f32::NAN,
    }
}

fn time_left(ml: &MainLoop, id: NodeId) -> f64 {
    match ml.tree().get_node(id).map(|n| n.get_property("time_left")) {
        Some(Variant::Float(f)) => f,
        _ => f64::NAN,
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** After `set_paused(true)`, 60 driven frames change
/// neither the falling body's position nor the timer's countdown; unpausing
/// resumes both.
#[test]
fn scene_tree_pause_gates_simulation() {
    let (mut ml, faller, timer) = world();

    // Run unpaused first so everything is demonstrably live — otherwise a
    // "nothing moved" assertion could pass on a world that never ran.
    run(&mut ml, 30);
    let moving_y = pos_y(&ml, faller);
    let ticking = time_left(&ml, timer);
    assert!(
        moving_y > 0.01,
        "control: gravity must move the body while unpaused, got y={moving_y}"
    );
    // 30 frames is 0.5s against a 5s period — comfortably clear of both the
    // fire boundary and the wrap, so this reads a genuine mid-countdown value
    // rather than a value that happens to be a fresh reset.
    assert!(
        ticking < 5.0 && ticking > 4.0,
        "control: the Timer must be mid-countdown while unpaused, got {ticking}"
    );

    // -- pause: 60 frames must change nothing --------------------------------
    ml.tree_mut().set_paused(true);
    let y_at_pause = pos_y(&ml, faller);
    let timer_at_pause = time_left(&ml, timer);

    run(&mut ml, 60);

    assert_eq!(
        pos_y(&ml, faller),
        y_at_pause,
        "a paused tree must not step physics — the body moved",
    );
    assert_eq!(
        time_left(&ml, timer),
        timer_at_pause,
        "a paused tree must not advance Timer nodes",
    );

    // -- unpause: the world resumes ------------------------------------------
    ml.tree_mut().set_paused(false);
    run(&mut ml, 30);

    assert!(
        pos_y(&ml, faller) > y_at_pause + 0.01,
        "unpausing must resume physics (y {} vs paused {})",
        pos_y(&ml, faller),
        y_at_pause,
    );
    assert!(
        time_left(&ml, timer) != timer_at_pause,
        "unpausing must resume the Timer countdown",
    );
}

/// Unpausing must not fast-forward. Godot's servers are inactive while paused,
/// so the paused interval is not owed to the simulation — a loop that kept
/// accumulating would run a burst of catch-up ticks on resume.
#[test]
fn unpausing_does_not_fast_forward_the_paused_interval() {
    let (mut ml, faller, _timer) = world();
    run(&mut ml, 10);

    // Baseline: how far the body falls in 10 unpaused frames from here.
    let before = pos_y(&ml, faller);
    run(&mut ml, 10);
    let ten_frames = pos_y(&ml, faller) - before;

    // Now pause for a long time, then run the same 10 frames.
    ml.tree_mut().set_paused(true);
    run(&mut ml, 120);
    ml.tree_mut().set_paused(false);
    let resumed_from = pos_y(&ml, faller);
    run(&mut ml, 10);
    let after_resume = pos_y(&ml, faller) - resumed_from;

    // Gravity accelerates, so the second window is naturally larger — but it
    // must not contain 2 seconds of banked physics.
    assert!(
        after_resume < ten_frames * 3.0,
        "resuming must not replay the paused interval: 10 frames moved \
         {after_resume} vs {ten_frames} before a 2s pause",
    );
}

/// `TweenPauseMode` decides whether a tween survives pause. This pins the
/// enum's contract at the tree level rather than only in its unit tests.
#[test]
fn tween_pause_mode_controls_advance_under_pause() {
    // Bound (Godot's default) follows the bound node.
    let bound = Tween::new();
    assert_eq!(bound.pause_mode, TweenPauseMode::Bound);
    assert!(!bound.can_process(true, None), "default stops under pause");

    // Process keeps running — what a pause-menu animation needs.
    let mut menu = Tween::new();
    menu.pause_mode = TweenPauseMode::Process;
    assert!(
        menu.can_process(true, None),
        "TweenPauseMode::Process must advance while the tree is paused",
    );
}
