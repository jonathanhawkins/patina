//! pat-8v31: Area2D overlap signal parity coverage.
//!
//! Deterministic fixed-step tests verifying that Area2D nodes emit
//! `body_entered` / `body_exited` signals through the MainLoop when
//! physics bodies enter and exit their monitoring region.
//!
//! These tests use `MainLoop::step()` with a fixed delta (1/60 s) so
//! results are fully deterministic across runs.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a scene with one RigidBody2D and one Area2D.
/// `body_pos` — initial position of the rigid body.
/// `area_pos` — position of the area.
/// Returns `(tree, body_node_id, area_node_id)`.
fn make_body_area_scene(
    body_pos: Vector2,
    area_pos: Vector2,
) -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(body_pos));
    body.set_property("mass", Variant::Float(1.0));
    body.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(root, body).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_id, s).unwrap();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(area_pos));
    let area_id = tree.add_child(root, area).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(20.0));
    tree.add_child(area_id, sa).unwrap();

    (tree, body_id, area_id)
}

// ═══════════════════════════════════════════════════════════════════════════
// body_entered fires on overlap via MainLoop
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn body_entered_fires_through_mainloop() {
    // Body starts inside the area — should trigger body_entered on the
    // first physics step.
    let (mut tree, _body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    let count = Arc::new(AtomicUsize::new(0));
    let cnt = count.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            cnt.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "body_entered should fire exactly once when body is inside area on first step"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// body_exited fires when body leaves via MainLoop
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn body_exited_fires_through_mainloop() {
    let (mut tree, body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    let entered = Arc::new(AtomicUsize::new(0));
    let exited = Arc::new(AtomicUsize::new(0));

    let e = entered.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            e.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    let x = exited.clone();
    tree.connect_signal(
        area_id,
        "body_exited",
        Connection::with_callback(area_id.object_id(), "on_body_exited", move |_args| {
            x.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step 1 — body enters
    ml.step(DT);
    assert_eq!(entered.load(Ordering::SeqCst), 1, "entered after step 1");
    assert_eq!(exited.load(Ordering::SeqCst), 0, "no exit yet");

    // Move body far away
    ml.tree_mut()
        .get_node_mut(body_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(500.0, 0.0)));

    // Step 2 — body exits
    ml.step(DT);
    assert_eq!(
        exited.load(Ordering::SeqCst),
        1,
        "body_exited should fire after body leaves area"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// No duplicate signals while body stays inside
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_duplicate_signals_while_inside() {
    let (mut tree, _body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    let count = Arc::new(AtomicUsize::new(0));
    let cnt = count.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            cnt.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Run 10 frames with body staying inside
    for _ in 0..10 {
        ml.step(DT);
    }

    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "body_entered should fire only on the first overlap frame, not repeatedly"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Re-entry fires body_entered again
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn reentry_fires_body_entered_again() {
    let (mut tree, body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    let entered = Arc::new(AtomicUsize::new(0));
    let exited = Arc::new(AtomicUsize::new(0));

    let e = entered.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            e.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    let x = exited.clone();
    tree.connect_signal(
        area_id,
        "body_exited",
        Connection::with_callback(area_id.object_id(), "on_body_exited", move |_args| {
            x.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step 1 — enter
    ml.step(DT);
    assert_eq!(entered.load(Ordering::SeqCst), 1);

    // Move out
    ml.tree_mut()
        .get_node_mut(body_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(500.0, 0.0)));
    ml.step(DT);
    assert_eq!(exited.load(Ordering::SeqCst), 1);

    // Move back in
    ml.tree_mut()
        .get_node_mut(body_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
    ml.step(DT);
    assert_eq!(
        entered.load(Ordering::SeqCst),
        2,
        "body_entered should fire again on re-entry"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Signal argument carries the body's ObjectId
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn signal_carries_body_object_id() {
    let (mut tree, body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    let expected_oid = body_id.object_id();

    let received = Arc::new(Mutex::new(Vec::<Variant>::new()));
    let r = received.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |args| {
            r.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    let args = received.lock().unwrap();
    assert_eq!(args.len(), 1, "body_entered should pass one argument");
    match &args[0] {
        Variant::ObjectId(oid) => {
            assert_eq!(*oid, expected_oid, "argument should be body's ObjectId");
        }
        other => panic!(
            "expected Variant::ObjectId, got {:?}",
            other
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// monitoring=false suppresses signals through MainLoop
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn monitoring_false_suppresses_signals() {
    let (mut tree, _body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    // Disable monitoring on the area
    tree.get_node_mut(area_id)
        .unwrap()
        .set_property("monitoring", Variant::Bool(false));

    let count = Arc::new(AtomicUsize::new(0));
    let cnt = count.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            cnt.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "monitoring=false should suppress body_entered signals"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Layer/mask mismatch suppresses signals through MainLoop
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn layer_mask_mismatch_suppresses_signals() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body on layer 2
    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
    body.set_property("collision_layer", Variant::Int(2));
    let body_id = tree.add_child(root, body).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_id, s).unwrap();

    // Area scanning layer 4 only — should NOT match body on layer 2
    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::ZERO));
    area.set_property("collision_mask", Variant::Int(4));
    let area_id = tree.add_child(root, area).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(20.0));
    tree.add_child(area_id, sa).unwrap();

    let count = Arc::new(AtomicUsize::new(0));
    let cnt = count.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            cnt.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "layer/mask mismatch should suppress signals"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Multiple bodies entering the same area
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_bodies_trigger_separate_signals() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Two bodies, both inside the area
    for i in 0..2 {
        let mut body = Node::new(&format!("Body{}", i), "RigidBody2D");
        body.set_property(
            "position",
            Variant::Vector2(Vector2::new(3.0 + i as f32 * 2.0, 0.0)),
        );
        body.set_property("mass", Variant::Float(1.0));
        let bid = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(2.0));
        tree.add_child(bid, s).unwrap();
    }

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::ZERO));
    let area_id = tree.add_child(root, area).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(30.0));
    tree.add_child(area_id, sa).unwrap();

    let count = Arc::new(AtomicUsize::new(0));
    let cnt = count.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            cnt.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "each body should trigger its own body_entered signal"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Body starts outside, moves in over several frames
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn body_moving_in_via_velocity() {
    // Body starts far away and moves toward the area via velocity
    let (mut tree, body_id, area_id) =
        make_body_area_scene(Vector2::new(100.0, 0.0), Vector2::ZERO);

    // Give body a velocity toward the area
    tree.get_node_mut(body_id)
        .unwrap()
        .set_property(
            "linear_velocity",
            Variant::Vector2(Vector2::new(-200.0, 0.0)),
        );

    let entered = Arc::new(AtomicUsize::new(0));
    let e = entered.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            e.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // No signal on first step (body still far away)
    ml.step(DT);
    let first_frame_entered = entered.load(Ordering::SeqCst);

    // Run enough frames for the body to reach the area (100 px / (200 px/s) = 0.5s = 30 frames)
    // but the shapes overlap earlier due to radii (5 + 20 = 25 px margin)
    // so (100 - 25) / 200 = 0.375s = ~23 frames.  Run 60 frames to be safe.
    for _ in 1..60 {
        ml.step(DT);
    }

    let total_entered = entered.load(Ordering::SeqCst);
    assert!(
        total_entered >= 1,
        "body should eventually enter the area (entered={})",
        total_entered
    );
    // Should fire exactly once — not repeatedly
    assert_eq!(
        total_entered - first_frame_entered,
        1,
        "body_entered should fire exactly once even over multiple frames (if body was outside initially)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Body overlaps multiple areas simultaneously
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn body_enters_multiple_areas() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // One body at the origin
    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    body.set_property("mass", Variant::Float(1.0));
    let body_id = tree.add_child(root, body).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_id, s).unwrap();

    // Two overlapping areas centred at the origin
    let mut counts = Vec::new();
    let mut area_ids = Vec::new();
    for i in 0..2 {
        let mut area = Node::new(&format!("Zone{}", i), "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::ZERO));
        let area_id = tree.add_child(root, area).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(20.0));
        tree.add_child(area_id, sa).unwrap();
        area_ids.push(area_id);

        let count = Arc::new(AtomicUsize::new(0));
        let cnt = count.clone();
        tree.connect_signal(
            area_id,
            "body_entered",
            Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
                cnt.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );
        counts.push(count);
    }

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    for (i, count) in counts.iter().enumerate() {
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "body_entered should fire on area {} when body overlaps both",
            i
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Simultaneous enter and exit in same physics step
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn simultaneous_enter_and_exit() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body A starts inside the area
    let mut body_a = Node::new("BodyA", "RigidBody2D");
    body_a.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
    body_a.set_property("mass", Variant::Float(1.0));
    let body_a_id = tree.add_child(root, body_a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_a_id, sa).unwrap();

    // Body B starts outside
    let mut body_b = Node::new("BodyB", "RigidBody2D");
    body_b.set_property("position", Variant::Vector2(Vector2::new(500.0, 0.0)));
    body_b.set_property("mass", Variant::Float(1.0));
    let body_b_id = tree.add_child(root, body_b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_b_id, sb).unwrap();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::ZERO));
    let area_id = tree.add_child(root, area).unwrap();
    let mut sz = Node::new("Shape", "CollisionShape2D");
    sz.set_property("radius", Variant::Float(20.0));
    tree.add_child(area_id, sz).unwrap();

    let entered = Arc::new(AtomicUsize::new(0));
    let exited = Arc::new(AtomicUsize::new(0));

    let e = entered.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            e.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    let x = exited.clone();
    tree.connect_signal(
        area_id,
        "body_exited",
        Connection::with_callback(area_id.object_id(), "on_body_exited", move |_args| {
            x.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step 1 — body A enters
    ml.step(DT);
    assert_eq!(entered.load(Ordering::SeqCst), 1, "A enters");
    assert_eq!(exited.load(Ordering::SeqCst), 0);

    // In the same frame: move A out, move B in
    ml.tree_mut()
        .get_node_mut(body_a_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(500.0, 0.0)));
    ml.tree_mut()
        .get_node_mut(body_b_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));

    ml.step(DT);
    assert_eq!(
        entered.load(Ordering::SeqCst),
        2,
        "B should trigger a new body_entered"
    );
    assert_eq!(
        exited.load(Ordering::SeqCst),
        1,
        "A should trigger body_exited in the same step"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Area moves away from stationary body — body_exited fires
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn area_moves_away_triggers_exit() {
    let (mut tree, _body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    let entered = Arc::new(AtomicUsize::new(0));
    let exited = Arc::new(AtomicUsize::new(0));

    let e = entered.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            e.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    let x = exited.clone();
    tree.connect_signal(
        area_id,
        "body_exited",
        Connection::with_callback(area_id.object_id(), "on_body_exited", move |_args| {
            x.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step 1 — body is inside the area
    ml.step(DT);
    assert_eq!(entered.load(Ordering::SeqCst), 1);

    // Move the area far away
    ml.tree_mut()
        .get_node_mut(area_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(500.0, 0.0)));

    ml.step(DT);
    assert_eq!(
        exited.load(Ordering::SeqCst),
        1,
        "body_exited should fire when the area moves away from the body"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Monitoring toggled at runtime suppresses further signals
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn monitoring_toggled_at_runtime() {
    let (mut tree, body_id, area_id) =
        make_body_area_scene(Vector2::new(500.0, 0.0), Vector2::ZERO);

    let entered = Arc::new(AtomicUsize::new(0));
    let e = entered.clone();
    tree.connect_signal(
        area_id,
        "body_entered",
        Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
            e.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step 1 — body is outside
    ml.step(DT);
    assert_eq!(entered.load(Ordering::SeqCst), 0);

    // Disable monitoring, then move body inside
    ml.tree_mut()
        .get_node_mut(area_id)
        .unwrap()
        .set_property("monitoring", Variant::Bool(false));
    ml.tree_mut()
        .get_node_mut(body_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));

    ml.step(DT);
    assert_eq!(
        entered.load(Ordering::SeqCst),
        0,
        "monitoring=false at runtime should suppress body_entered"
    );

    // Re-enable monitoring — body is already inside, should fire entered
    ml.tree_mut()
        .get_node_mut(area_id)
        .unwrap()
        .set_property("monitoring", Variant::Bool(true));

    ml.step(DT);
    assert_eq!(
        entered.load(Ordering::SeqCst),
        1,
        "re-enabling monitoring should detect the already-overlapping body"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// body_exited signal also carries the body's ObjectId
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn exit_signal_carries_body_object_id() {
    let (mut tree, body_id, area_id) =
        make_body_area_scene(Vector2::new(5.0, 0.0), Vector2::ZERO);

    let expected_oid = body_id.object_id();

    let exit_args = Arc::new(Mutex::new(Vec::<Variant>::new()));
    let r = exit_args.clone();
    tree.connect_signal(
        area_id,
        "body_exited",
        Connection::with_callback(area_id.object_id(), "on_body_exited", move |args| {
            r.lock().unwrap().extend_from_slice(args);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Enter
    ml.step(DT);
    // Exit
    ml.tree_mut()
        .get_node_mut(body_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(500.0, 0.0)));
    ml.step(DT);

    let args = exit_args.lock().unwrap();
    assert_eq!(args.len(), 1, "body_exited should pass one argument");
    match &args[0] {
        Variant::ObjectId(oid) => {
            assert_eq!(*oid, expected_oid, "exit argument should be body's ObjectId");
        }
        other => panic!("expected Variant::ObjectId in body_exited, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Remaining risk: area_entered / area_exited NOT implemented
// ═══════════════════════════════════════════════════════════════════════════
//
// Godot's Area2D emits `area_entered` / `area_exited` when two Area2D nodes
// overlap. The current physics server only detects Area2D ↔ PhysicsBody2D
// overlaps — Area2D ↔ Area2D is not wired. The editor server advertises
// these signal names (editor_server.rs:2377) but they will never fire at
// runtime until `AreaStore::detect_overlaps` is extended to check
// area-vs-area collisions and the physics server emits the corresponding
// signals.
//
// Impact: any game logic relying on two trigger zones overlapping (e.g.
// damage area entering safe zone) will silently fail. This is a known
// gap tracked for future work.
