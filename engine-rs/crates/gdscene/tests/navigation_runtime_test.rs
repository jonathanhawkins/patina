//! pat-u3l8c.5 — Navigation stepped at runtime (agents path in a running game).
//!
//! Before this bead the navigation logic in `navigation.rs` was never driven
//! from the node/runtime layer: a `NavigationAgent2D` stayed frozen at its
//! defaults even with a target set, because nothing advanced it against a scene
//! node each frame. These app-level tests exercise the new runtime drivers
//! (`step_navigation_agent_2d` / `_3d`), which a game's process loop calls per
//! frame to move an agent along its path.
//!
//! ACCEPT: an agent with a target_position returns successive
//! `get_next_path_position` values across frames — i.e. it advances through
//! multiple waypoints as it walks the path, and arrives at the goal.

use gdcore::math::{Vector2, Vector3};
use gdscene::navigation::{
    step_navigation_agent_2d, step_navigation_agent_3d, NavMesh2D, NavPolygon, NavigationAgent2D,
    NavigationAgent3D, NavigationServer2D,
};
use gdscene::node::Node;
use gdscene::node2d;
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;

/// Three 10x10 squares in a row along +x — `[0,10]`, `[10,20]`, `[20,30]` —
/// each sharing an edge with the next, so A* produces a path with an
/// intermediate waypoint (start -> centroid -> goal) rather than one hop.
fn strip_mesh() -> NavMesh2D {
    let p0 = NavPolygon::new(vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(10.0, 10.0),
        Vector2::new(0.0, 10.0),
    ]);
    let p1 = NavPolygon::new(vec![
        Vector2::new(10.0, 0.0),
        Vector2::new(20.0, 0.0),
        Vector2::new(20.0, 10.0),
        Vector2::new(10.0, 10.0),
    ]);
    let p2 = NavPolygon::new(vec![
        Vector2::new(20.0, 0.0),
        Vector2::new(30.0, 0.0),
        Vector2::new(30.0, 10.0),
        Vector2::new(20.0, 10.0),
    ]);
    NavMesh2D::new(vec![p0, p1, p2], 0.01)
}

#[test]
fn agent_2d_paths_across_frames_to_target() {
    // Running-game setup: a scene node whose position the agent drives.
    let mut scene = SceneTree::new();
    let root = scene.root_id();
    let node = scene
        .add_child(root, Node::new("Agent", "NavigationAgent2D"))
        .unwrap();
    let start = Vector2::new(5.0, 5.0);
    node2d::set_position(&mut scene, node, start);

    // Multi-polygon navmesh => the computed path has an intermediate waypoint,
    // which is what makes "successive get_next_path_position" meaningful.
    let mut server = NavigationServer2D::new();
    server.add_region(strip_mesh());

    let target = Vector2::new(25.0, 5.0);
    let mut agent = NavigationAgent2D::new();
    agent.set_target_position(target, start, &server);

    assert!(
        agent.path.len() >= 3,
        "expected a multi-waypoint path, got {:?}",
        agent.path
    );
    assert!(!agent.is_navigation_finished());

    // Drive the agent frame by frame like a process loop, recording each
    // distinct waypoint it heads toward.
    let dt = 0.02_f32;
    let mut targets: Vec<Vector2> = Vec::new();
    let mut steps = 0;
    while !agent.is_navigation_finished() && steps < 10_000 {
        let cur = node2d::get_position(&scene, node);
        // Re-reading with the same current position is idempotent, so it does
        // not perturb the step() call inside the driver below.
        let wp = agent.get_next_path_position(cur);
        let changed = match targets.last() {
            Some(prev) => (prev.x - wp.x).abs() > 0.001 || (prev.y - wp.y).abs() > 0.001,
            None => true,
        };
        if changed {
            targets.push(wp);
        }
        step_navigation_agent_2d(&mut scene, node, &mut agent, dt, &[]);
        steps += 1;
    }

    // It finished by arriving, not by hitting the step guard.
    assert!(agent.is_navigation_finished(), "agent never finished");
    assert!(steps < 10_000, "agent did not converge in time");

    // Successive get_next_path_position values: at least two distinct,
    // forward-progressing waypoints (intermediate, then the goal).
    assert!(
        targets.len() >= 2,
        "expected successive waypoints across frames, got {targets:?}"
    );
    for w in targets.windows(2) {
        assert!(
            w[1].x > w[0].x,
            "waypoints must progress toward the goal: {targets:?}"
        );
    }

    // The node actually moved through the scene from start to near the target.
    let final_pos = node2d::get_position(&scene, node);
    assert!(
        final_pos.x > start.x + 10.0,
        "node barely moved: {final_pos:?}"
    );
    assert!(
        (final_pos - target).length() <= agent.path_desired_distance + 5.0,
        "node did not arrive near target: {final_pos:?}"
    );
}

#[test]
fn agent_3d_steps_toward_target_across_frames() {
    let mut scene = SceneTree::new();
    let root = scene.root_id();
    let node = scene
        .add_child(root, Node::new("Agent3D", "NavigationAgent3D"))
        .unwrap();
    let start = Vector3::new(0.0, 0.0, 0.0);
    node3d::set_position(&mut scene, node, start);

    let mut agent = NavigationAgent3D::new();
    // A precomputed 3-waypoint path along +x (as a NavigationServer3D would
    // produce); the target is the final waypoint.
    let target = Vector3::new(20.0, 0.0, 0.0);
    agent.target_position = target;
    agent.path = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
        target,
    ];
    agent.path_index = 1;
    agent.path_desired_distance = 1.0;

    assert!(!agent.is_navigation_finished());

    // max_speed default 10.0 * dt 0.1 = 1.0 unit/frame (clean convergence).
    let dt = 0.1_f32;
    let mut waypoints: Vec<Vector3> = Vec::new();
    let mut steps = 0;
    while !agent.is_navigation_finished() && steps < 10_000 {
        let cur = node3d::get_position(&scene, node);
        let wp = agent.get_next_path_position(cur);
        let changed = match waypoints.last() {
            Some(prev) => (prev.x - wp.x).abs() > 0.001,
            None => true,
        };
        if changed {
            waypoints.push(wp);
        }
        step_navigation_agent_3d(&mut scene, node, &mut agent, dt, &[]);
        steps += 1;
    }

    assert!(agent.is_navigation_finished(), "3D agent never finished");
    assert!(steps < 10_000, "3D agent did not converge in time");
    assert!(
        waypoints.len() >= 2,
        "expected successive 3D waypoints across frames, got {waypoints:?}"
    );
    let final_pos = node3d::get_position(&scene, node);
    assert!(
        final_pos.x > start.x + 8.0,
        "3D node barely moved: {final_pos:?}"
    );
    assert!(
        (final_pos - target).length() <= agent.path_desired_distance + 5.0,
        "3D node did not arrive near target: {final_pos:?}"
    );
}
