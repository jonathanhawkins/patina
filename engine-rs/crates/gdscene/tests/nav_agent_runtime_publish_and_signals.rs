//! pat-s3rrv.20, runtime side: what `process_navigation` publishes each tick,
//! and which signals it is allowed to emit.
//!
//! Two things here are easy to get wrong in ways no body-movement test can see:
//!
//! 1. **Publisher and reader can drift.** The frame loop writes the agent's
//!    state onto the node under `__nav_*` keys and the method surface reads it
//!    back. Nothing but agreement on those key names makes the getters work, so
//!    a round-trip test is the only thing standing between a typo and a getter
//!    that silently answers "empty" forever.
//! 2. **Signals can fire twice.** `NavAgentSignal` carries five milestones, but
//!    the runtime already hand-emits three of them. Draining all five would
//!    double-fire those three — and a doubled signal is invisible until a game
//!    counts them.

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdscene::navigation::{NavMesh2D, NavPolygon, NavigationAgent2D, NavigationServer2D};
use gdscene::navigation_agents::{
    call_agent_method, drain_and_emit_2d, publish_state_2d, sync_tuning_2d,
};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use std::sync::{Arc, Mutex};

fn agent_scene() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let agent = tree
        .add_child(root, Node::new("Agent", "NavigationAgent2D"))
        .unwrap();
    (tree, agent)
}

/// One 200x60 walkable rectangle.
fn corridor_server() -> NavigationServer2D {
    let mut server = NavigationServer2D::new();
    server.add_region(NavMesh2D::new(
        vec![NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(200.0, 0.0),
            Vector2::new(200.0, 60.0),
            Vector2::new(0.0, 60.0),
        ])],
        0.01,
    ));
    server
}

/// Authored tuning reaches the agent; unauthored tuning keeps Godot's default.
///
/// The second half is the real assertion. `process_navigation` read only
/// `target_position`, so every other tuning value in a `.tscn` was inert — but a
/// sync that "helpfully" writes 0.0 for absent properties is just as wrong, and
/// would silently make every agent arrive at a zero-radius tolerance.
#[test]
fn sync_tuning_2d_applies_authored_values_and_preserves_unset_defaults() {
    let (mut tree, agent_node) = agent_scene();
    tree.get_node_mut(agent_node)
        .unwrap()
        .set_property("path_desired_distance", Variant::Float(33.0));

    let mut agent = NavigationAgent2D::new();
    let default_target_distance = agent.target_desired_distance;
    assert!(
        (default_target_distance - 10.0).abs() < 0.001,
        "fixture assumption: Godot's target_desired_distance default is 10.0",
    );

    sync_tuning_2d(&tree, agent_node, &mut agent);

    assert!(
        (agent.path_desired_distance - 33.0).abs() < 0.001,
        "the authored path_desired_distance must reach the agent, got {}",
        agent.path_desired_distance,
    );
    assert!(
        (agent.target_desired_distance - default_target_distance).abs() < 0.001,
        "an unauthored property must keep its default, not be zeroed; got {}",
        agent.target_desired_distance,
    );
}

/// **Publisher and reader agree.** State written by the frame loop is exactly
/// what the method surface answers with.
#[test]
fn published_state_round_trips_through_the_method_surface() {
    let (mut tree, agent_node) = agent_scene();
    let server = corridor_server();

    let mut agent = NavigationAgent2D::new();
    agent.set_target_position(Vector2::new(180.0, 30.0), Vector2::new(10.0, 30.0), &server);
    assert!(
        !agent.path.is_empty(),
        "fixture assumption: the corridor must yield a path",
    );
    let expected_path = agent.path.clone();
    let expected_final = *expected_path.last().unwrap();

    publish_state_2d(&mut tree, agent_node, &agent);

    match call_agent_method(&mut tree, agent_node, "get_current_navigation_path", &[]) {
        Some(Variant::PackedVector2Array(p)) => assert_eq!(
            p, expected_path,
            "the published path must be exactly what the getter answers — a key \
             mismatch between publisher and reader shows up here and nowhere else",
        ),
        other => panic!("expected a PackedVector2Array, got {other:?}"),
    }
    assert_eq!(
        call_agent_method(&mut tree, agent_node, "get_final_position", &[]),
        Some(Variant::Vector2(expected_final)),
    );
    assert_eq!(
        call_agent_method(&mut tree, agent_node, "is_target_reachable", &[]),
        Some(Variant::Bool(true)),
        "a computed path means the target is reachable",
    );
    match call_agent_method(&mut tree, agent_node, "get_path_length", &[]) {
        Some(Variant::Float(len)) => assert!(
            len > 100.0,
            "a path across a 200-wide corridor must be longer than 100, got {len}",
        ),
        other => panic!("expected a float, got {other:?}"),
    }
}

/// An agent with no path publishes "unreachable", so a script can tell the
/// difference between "not there yet" and "cannot get there".
#[test]
fn an_unreachable_target_publishes_reachable_false() {
    let (mut tree, agent_node) = agent_scene();
    // No regions at all => no path can be found.
    let server = NavigationServer2D::new();
    let mut agent = NavigationAgent2D::new();
    agent.set_target_position(Vector2::new(180.0, 30.0), Vector2::new(10.0, 30.0), &server);

    publish_state_2d(&mut tree, agent_node, &agent);

    assert_eq!(
        call_agent_method(&mut tree, agent_node, "is_target_reachable", &[]),
        Some(Variant::Bool(false)),
        "with no navmesh there is no path, so the target is not reachable",
    );
}

/// **The double-emit guard.** `drain_and_emit_2d` emits the milestones that had
/// no producer, and stays silent on the three the runtime already hand-emits.
///
/// Without this, the natural implementation — drain everything — fires
/// `target_reached`, `navigation_finished` and `velocity_computed` twice per
/// frame, which no existing test would notice.
#[test]
fn drain_emits_only_the_signals_the_runtime_does_not_already_emit() {
    let (mut tree, agent_node) = agent_scene();
    let server = corridor_server();

    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    for name in [
        "path_changed",
        "waypoint_reached",
        "target_reached",
        "navigation_finished",
        "velocity_computed",
    ] {
        let log = Arc::clone(&log);
        let recorded = name.to_string();
        tree.connect_signal(
            agent_node,
            name,
            Connection::with_callback(ObjectId::from_raw(1), name, move |_args| {
                log.lock().unwrap().push(recorded.clone());
                Variant::Nil
            }),
        );
    }

    // Drive the agent to its goal so it queues the full milestone set: the path
    // computation pushes PathChanged, and arriving pushes WaypointReached plus
    // TargetReached / NavigationFinished.
    let start = Vector2::new(10.0, 30.0);
    let goal = Vector2::new(180.0, 30.0);
    let mut agent = NavigationAgent2D::new();
    agent.set_target_position(goal, start, &server);
    let _ = agent.get_next_path_position(start);
    let _ = agent.get_next_path_position(goal);
    let queued = agent.pending_signals().to_vec();
    assert!(
        !queued.is_empty(),
        "fixture assumption: the agent must have queued milestones to drain",
    );

    drain_and_emit_2d(&mut tree, agent_node, &mut agent);

    let fired = log.lock().unwrap().clone();
    assert!(
        fired.iter().any(|s| s == "path_changed"),
        "path_changed had no producer before this bead and must now fire; got {fired:?}",
    );
    for already_emitted in ["target_reached", "navigation_finished", "velocity_computed"] {
        assert!(
            !fired.iter().any(|s| s == already_emitted),
            "`{already_emitted}` is already hand-emitted by the runtime; draining it \
             here too fires it TWICE per frame. Fired: {fired:?}",
        );
    }
    assert!(
        agent.pending_signals().is_empty(),
        "draining must consume the queue, or the same milestones re-fire next frame",
    );
}
