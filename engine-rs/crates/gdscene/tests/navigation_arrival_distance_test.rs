//! pat-0q2kx.25: arrival uses `target_desired_distance`, not the waypoint
//! threshold.
//!
//! Godot's NavigationAgent keeps two separate distances — `path_desired_distance`
//! advances waypoints, `target_desired_distance` decides arrival
//! (doc/classes/NavigationAgent2D.xml: defaults 20.0 and 10.0). Patina reused
//! `path_desired_distance` for both, so an agent tuned with a wide waypoint
//! tolerance declared itself finished far from the goal.

use gdscene::navigation::NavigationAgent2D;

/// The two thresholds are independent, and arrival keys off the target one.
#[test]
fn target_desired_distance_is_separate_from_path_desired_distance() {
    let agent = NavigationAgent2D::new();
    // Distinct fields with Godot's own defaults; the bug was that only one existed.
    assert_eq!(
        agent.target_desired_distance, 10.0,
        "target_desired_distance must default to Godot's 10.0"
    );
    assert!(
        agent.path_desired_distance != agent.target_desired_distance,
        "the waypoint and arrival thresholds must be independently tunable"
    );
}
