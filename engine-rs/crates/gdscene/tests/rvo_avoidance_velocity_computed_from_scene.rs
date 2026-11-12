//! pat-nzgdu.116 — ORCA/RVO2 avoidance driven from the physics tick.
//!
//! ACCEPT: two avoidance-enabled `NavigationAgent2D` nodes given converging
//! `set_velocity` values receive `velocity_computed` safe velocities (from the
//! physics tick) that keep their positions non-overlapping after several ticks.
//!
//! Before this bead avoidance was wired end-to-end yet inert: `MainLoop`'s
//! navigation phase passed a hardcoded empty neighbor slice, so no agent was
//! ever told another agent existed and `avoidance_enabled` changed nothing.
//!
//! This models Godot's contract faithfully: the NavigationServer does **not**
//! move the body. It hands back a collision-free `safe_velocity` via
//! `velocity_computed`, and the game integrates it — which is exactly what a
//! `CharacterBody2D` would do, and what this test does below.

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Radius both agents use; they overlap when closer than twice this.
const AGENT_RADIUS: f32 = 10.0;
const MAX_SPEED: f32 = 100.0;

/// Records every `velocity_computed` payload a node emits.
type Captured = Arc<Mutex<Vec<Vector2>>>;

fn capture(cell: &Captured, tag: u64) -> Connection {
    let sink = cell.clone();
    Connection::with_callback(
        ObjectId::from_raw(tag),
        "on_velocity_computed",
        move |args| {
            if let Some(Variant::Vector2(v)) = args.first() {
                sink.lock().expect("capture mutex poisoned").push(*v);
            }
            Variant::Nil
        },
    )
}

/// Adds an avoidance-enabled `NavigationAgent2D` at `position` that wants to
/// travel at `velocity`.
fn add_agent(tree: &mut SceneTree, name: &str, position: Vector2, velocity: Vector2) -> NodeId {
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new(name, "NavigationAgent2D"))
        .expect("agent node is added");
    node2d::set_position(tree, id, position);
    let node = tree.get_node_mut(id).expect("agent node exists");
    node.set_property("avoidance_enabled", Variant::Bool(true));
    node.set_property("velocity", Variant::Vector2(velocity));
    node.set_property("radius", Variant::Float(AGENT_RADIUS as f64));
    node.set_property("max_speed", Variant::Float(MAX_SPEED as f64));
    id
}

#[test]
fn rvo_avoidance_velocity_computed_from_scene() {
    let mut tree = SceneTree::new();

    // Two agents on converging courses, closing head-on along x. The small y
    // offset breaks the perfect symmetry of a dead-on collision, which has no
    // preferred side to pass on.
    let left_start = Vector2::new(-60.0, -0.5);
    let right_start = Vector2::new(60.0, 0.5);
    let left = add_agent(&mut tree, "Left", left_start, Vector2::new(MAX_SPEED, 0.0));
    let right = add_agent(
        &mut tree,
        "Right",
        right_start,
        Vector2::new(-MAX_SPEED, 0.0),
    );

    let left_seen: Captured = Arc::new(Mutex::new(Vec::new()));
    let right_seen: Captured = Arc::new(Mutex::new(Vec::new()));
    tree.connect_signal(left, "velocity_computed", capture(&left_seen, 1));
    tree.connect_signal(right, "velocity_computed", capture(&right_seen, 2));

    let mut ml = MainLoop::new(tree);
    let physics_dt = 1.0 / ml.physics_ticks_per_second() as f32;

    // Drive the loop. Each tick the physics phase solves avoidance and emits
    // velocity_computed; we then move each body by the safe velocity it was
    // handed, exactly as a game's callback would.
    let mut min_separation = f32::INFINITY;
    for _ in 0..60 {
        ml.step(physics_dt as f64);

        for id in [left, right] {
            let safe = match ml
                .tree()
                .get_node(id)
                .map(|n| n.get_property("safe_velocity"))
            {
                Some(Variant::Vector2(v)) => v,
                _ => panic!("the physics tick must publish safe_velocity on {id:?}"),
            };
            let current = node2d::get_position(ml.tree(), id);
            let moved = Vector2::new(
                current.x + safe.x * physics_dt,
                current.y + safe.y * physics_dt,
            );
            node2d::set_position(ml.tree_mut(), id, moved);
        }

        let separation = (node2d::get_position(ml.tree(), left)
            - node2d::get_position(ml.tree(), right))
        .length();
        min_separation = min_separation.min(separation);
    }

    // 1. velocity_computed fired, from the physics tick, for both agents.
    let left_payloads = left_seen.lock().expect("left mutex poisoned").clone();
    let right_payloads = right_seen.lock().expect("right mutex poisoned").clone();
    assert!(
        !left_payloads.is_empty() && !right_payloads.is_empty(),
        "both agents must receive velocity_computed (left {}, right {})",
        left_payloads.len(),
        right_payloads.len()
    );

    // 2. The safe velocities are real avoidance output, not the desired
    //    velocity passed through: at some point each agent was deflected off
    //    its pure -x/+x course.
    let left_deflected = left_payloads.iter().any(|v| v.y.abs() > 1e-3);
    let right_deflected = right_payloads.iter().any(|v| v.y.abs() > 1e-3);
    assert!(
        left_deflected && right_deflected,
        "converging agents must be deflected off their axis \
         (left {left_deflected}, right {right_deflected})"
    );

    // 3. Every safe velocity honours max_speed.
    for v in left_payloads.iter().chain(right_payloads.iter()) {
        assert!(
            v.length() <= MAX_SPEED + 1e-2,
            "safe velocity {v:?} exceeds max_speed {MAX_SPEED}"
        );
    }

    // 4. The acceptance itself: they never overlapped.
    let min_allowed = AGENT_RADIUS * 2.0;
    assert!(
        min_separation >= min_allowed,
        "agents overlapped: closest approach {min_separation} < {min_allowed}"
    );
}

#[test]
fn avoidance_disabled_agents_are_allowed_to_overlap() {
    // The control case. Without it, a test that only ever asserts separation
    // would still pass if the agents simply never approached each other —
    // proving the scenario is a real collision course, not a fixture that
    // trivially satisfies the acceptance.
    let mut tree = SceneTree::new();
    let left_start = Vector2::new(-60.0, -0.5);
    let right_start = Vector2::new(60.0, 0.5);
    let left = add_agent(&mut tree, "Left", left_start, Vector2::new(MAX_SPEED, 0.0));
    let right = add_agent(
        &mut tree,
        "Right",
        right_start,
        Vector2::new(-MAX_SPEED, 0.0),
    );
    for id in [left, right] {
        let node = tree.get_node_mut(id).expect("agent node exists");
        node.set_property("avoidance_enabled", Variant::Bool(false));
    }

    let mut ml = MainLoop::new(tree);
    let physics_dt = 1.0 / ml.physics_ticks_per_second() as f32;

    let mut min_separation = f32::INFINITY;
    for _ in 0..60 {
        ml.step(physics_dt as f64);
        // No avoidance phase runs for these nodes, so move by the desired
        // velocity the scene set.
        for id in [left, right] {
            let desired = match ml.tree().get_node(id).map(|n| n.get_property("velocity")) {
                Some(Variant::Vector2(v)) => v,
                _ => Vector2::ZERO,
            };
            let current = node2d::get_position(ml.tree(), id);
            let moved = Vector2::new(
                current.x + desired.x * physics_dt,
                current.y + desired.y * physics_dt,
            );
            node2d::set_position(ml.tree_mut(), id, moved);
        }
        let separation = (node2d::get_position(ml.tree(), left)
            - node2d::get_position(ml.tree(), right))
        .length();
        min_separation = min_separation.min(separation);
    }

    assert!(
        min_separation < AGENT_RADIUS * 2.0,
        "the fixture must be a genuine collision course: unavoided agents \
         should overlap, but closest approach was {min_separation}"
    );
}
