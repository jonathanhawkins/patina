//! pat-mgixk.27 acceptance: avoidance solves each agent at its PARENT BODY's
//! global position, not at the agent node's own (absent) local position.
//!
//! # What was wrong
//!
//! Godot's `NavigationAgent2D` feeds the solver its parent's position every
//! physics frame —
//! `agent_set_position(agent, agent_parent->get_global_position())`
//! (`scene/2d/navigation/navigation_agent_2d.cpp`,
//! `NOTIFICATION_INTERNAL_PHYSICS_PROCESS`) — and the 3D agent and both
//! obstacle classes do the same with `get_global_position()`.
//!
//! Patina's avoidance pass read `node2d::get_position(agent)`, i.e. the agent
//! node's OWN `position` property. A `NavigationAgent2D` extends `Node`, so in
//! the standard scene — agent as a child of a `CharacterBody2D` — it has no
//! `position` at all and `get_position` reports `Vector2::ZERO`. **Every agent
//! in the scene was therefore solved at the origin, stacked on top of each
//! other**, so ORCA returned garbage and `velocity_computed` was meaningless.
//!
//! `process_navigation` had already been switched to the global position
//! (pat-s3rrv.20); only the avoidance pass was left behind, so the two passes
//! disagreed about where every agent was.
//!
//! # Why it was green
//!
//! `navigation_avoidance_3d_velocity_computed` sets `position` **directly on the
//! NavigationAgent3D node** and parents it to the root. A Godot scene never does
//! that — an agent carries no position — so the fixture supplied the one thing
//! that made the broken read work, and the bug could not be seen. This file is
//! deliberately built the way a real scene is, and asserts that no agent node
//! carries a `position` property.
//!
//! # The path driven
//!
//! ```text
//! CharacterBody2D (position) -> child NavigationAgent2D (avoidance_enabled)
//!   -> MainLoop::step -> process_navigation_avoidance
//!     -> ORCA -> `safe_velocity` on the agent
//!       -> the test moves the BODY with it, as a game's
//!          velocity_computed handler feeds move_and_slide
//! ```

use gdcore::math::{Vector2, Vector3};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// 60 Hz, matching the physics tick these solvers run on.
const DT: f32 = 1.0 / 60.0;
/// Agent radius. Two agents may not come closer than `2 * RADIUS`.
const RADIUS: f32 = 10.0;
/// Starting separation: closing, but already well inside avoidance range.
const START_GAP: f32 = 40.0;
const FRAMES: usize = 120;

/// A `CharacterBody2D` at `position` holding an avoidance-enabled
/// `NavigationAgent2D`.
///
/// ⚠️ The agent is given NO `position` property — that is the whole point. If a
/// future edit adds one, `nav_agents_carry_no_position_property` fails.
fn body_with_agent_2d(tree: &mut SceneTree, name: &str, position: Vector2) -> (NodeId, NodeId) {
    let root = tree.root_id();
    let mut body = Node::new(name, "CharacterBody2D");
    body.set_property("position", Variant::Vector2(position));
    let body_id = tree.add_child(root, body).unwrap();

    let mut agent = Node::new(&format!("{name}Agent"), "NavigationAgent2D");
    agent.set_property("avoidance_enabled", Variant::Bool(true));
    agent.set_property("radius", Variant::Float(RADIUS as f64));
    agent.set_property("max_speed", Variant::Float(100.0));
    agent.set_property("velocity", Variant::Vector2(Vector2::ZERO));
    let agent_id = tree.add_child(body_id, agent).unwrap();
    (body_id, agent_id)
}

fn body_with_agent_3d(tree: &mut SceneTree, name: &str, position: Vector3) -> (NodeId, NodeId) {
    let root = tree.root_id();
    let mut body = Node::new(name, "CharacterBody3D");
    body.set_property("position", Variant::Vector3(position));
    let body_id = tree.add_child(root, body).unwrap();

    let mut agent = Node::new(&format!("{name}Agent"), "NavigationAgent3D");
    agent.set_property("avoidance_enabled", Variant::Bool(true));
    agent.set_property("radius", Variant::Float(RADIUS as f64));
    agent.set_property("height", Variant::Float(2.0));
    agent.set_property("max_speed", Variant::Float(100.0));
    agent.set_property("velocity", Variant::Vector3(Vector3::ZERO));
    let agent_id = tree.add_child(body_id, agent).unwrap();
    (body_id, agent_id)
}

fn body_pos_2d(ml: &MainLoop, id: NodeId) -> Vector2 {
    match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("body position should be a Vector2, got {other:?}"),
    }
}

fn body_pos_3d(ml: &MainLoop, id: NodeId) -> Vector3 {
    match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector3(v) => v,
        other => panic!("body position should be a Vector3, got {other:?}"),
    }
}

fn set_prop(ml: &mut MainLoop, id: NodeId, prop: &str, value: Variant) {
    ml.tree_mut()
        .get_node_mut(id)
        .expect("node is in the tree")
        .set_property(prop, value);
}

fn safe_velocity_2d(ml: &MainLoop, agent: NodeId) -> Vector2 {
    match ml
        .tree()
        .get_node(agent)
        .unwrap()
        .get_property("safe_velocity")
    {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    }
}

fn safe_velocity_3d(ml: &MainLoop, agent: NodeId) -> Vector3 {
    match ml
        .tree()
        .get_node(agent)
        .unwrap()
        .get_property("safe_velocity")
    {
        Variant::Vector3(v) => v,
        _ => Vector3::ZERO,
    }
}

/// **The acceptance test.** Two bodies driving into each other never overlap.
///
/// Each frame points its agent's desired `velocity` straight at the other body,
/// steps the loop, then moves the BODY by the solved `safe_velocity` — the loop
/// a game closes in its `velocity_computed` handler via `move_and_slide`.
#[test]
fn nav_avoidance_uses_parent_global_position() {
    let mut tree = SceneTree::new();
    let (a_body, a_agent) = body_with_agent_2d(&mut tree, "A", Vector2::new(0.0, 0.0));
    let (b_body, b_agent) = body_with_agent_2d(&mut tree, "B", Vector2::new(START_GAP, 0.0));
    let mut ml = MainLoop::new(tree);

    // Fixture check: the bodies start further apart than the no-overlap bound,
    // so any violation below is something the solve did, not the setup.
    let start = (body_pos_2d(&ml, b_body) - body_pos_2d(&ml, a_body)).length();
    assert!(
        start > 2.0 * RADIUS,
        "the bodies must start non-overlapping; gap was {start}"
    );

    let mut closest = f32::INFINITY;
    for frame in 0..FRAMES {
        let a = body_pos_2d(&ml, a_body);
        let b = body_pos_2d(&ml, b_body);

        // Desired velocity: straight at the other body, full speed.
        let to_b = (b - a).normalized() * 60.0;
        let to_a = (a - b).normalized() * 60.0;
        set_prop(&mut ml, a_agent, "velocity", Variant::Vector2(to_b));
        set_prop(&mut ml, b_agent, "velocity", Variant::Vector2(to_a));

        ml.step(DT as f64);

        // Move each BODY by what the solver returned for its agent.
        let sa = safe_velocity_2d(&ml, a_agent);
        let sb = safe_velocity_2d(&ml, b_agent);
        let na = Vector2::new(a.x + sa.x * DT, a.y + sa.y * DT);
        let nb = Vector2::new(b.x + sb.x * DT, b.y + sb.y * DT);
        set_prop(&mut ml, a_body, "position", Variant::Vector2(na));
        set_prop(&mut ml, b_body, "position", Variant::Vector2(nb));

        let gap = (nb - na).length();
        closest = closest.min(gap);
        assert!(
            gap > 2.0 * RADIUS - 0.5,
            "frame {frame}: the bodies closed to {gap}, inside 2*radius ({}). \
             Before this bead the avoidance pass read the AGENT's own `position` \
             — which a NavigationAgent2D does not have — so both agents solved \
             at (0,0), ORCA saw them already coincident, and the safe velocities \
             it returned did not separate anything.",
            2.0 * RADIUS
        );
    }

    // The agents must actually have been pushed apart, not simply left alone:
    // if the solver never engaged, `closest` would still be the start gap.
    assert!(
        closest < START_GAP,
        "the bodies never approached at all (closest {closest}), so this test \
         did not exercise avoidance"
    );
}

/// The 3D twin, same claim.
#[test]
fn nav_avoidance_uses_parent_global_position_3d() {
    let mut tree = SceneTree::new();
    let (a_body, a_agent) = body_with_agent_3d(&mut tree, "A", Vector3::new(0.0, 0.0, 0.0));
    let (b_body, b_agent) = body_with_agent_3d(&mut tree, "B", Vector3::new(START_GAP, 0.0, 0.0));
    let mut ml = MainLoop::new(tree);

    let mut closest = f32::INFINITY;
    for frame in 0..FRAMES {
        let a = body_pos_3d(&ml, a_body);
        let b = body_pos_3d(&ml, b_body);
        let to_b = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z).normalized();
        let to_a = Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z).normalized();
        set_prop(
            &mut ml,
            a_agent,
            "velocity",
            Variant::Vector3(Vector3::new(to_b.x * 60.0, to_b.y * 60.0, to_b.z * 60.0)),
        );
        set_prop(
            &mut ml,
            b_agent,
            "velocity",
            Variant::Vector3(Vector3::new(to_a.x * 60.0, to_a.y * 60.0, to_a.z * 60.0)),
        );

        ml.step(DT as f64);

        let sa = safe_velocity_3d(&ml, a_agent);
        let sb = safe_velocity_3d(&ml, b_agent);
        let na = Vector3::new(a.x + sa.x * DT, a.y + sa.y * DT, a.z + sa.z * DT);
        let nb = Vector3::new(b.x + sb.x * DT, b.y + sb.y * DT, b.z + sb.z * DT);
        set_prop(&mut ml, a_body, "position", Variant::Vector3(na));
        set_prop(&mut ml, b_body, "position", Variant::Vector3(nb));

        let d = Vector3::new(nb.x - na.x, nb.y - na.y, nb.z - na.z);
        let gap = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
        closest = closest.min(gap);
        assert!(
            gap > 2.0 * RADIUS - 0.5,
            "frame {frame}: 3D bodies closed to {gap}, inside 2*radius ({})",
            2.0 * RADIUS
        );
    }
    assert!(
        closest < START_GAP,
        "the 3D bodies never approached (closest {closest})"
    );
}

/// The fixture guard. A Godot `NavigationAgent2D`/`3D` extends `Node` and has no
/// `position`; the moment a test gives one, the broken local-position read works
/// again and the bug becomes invisible — which is exactly how it survived.
#[test]
fn nav_agents_carry_no_position_property() {
    let mut tree = SceneTree::new();
    let (_a_body, a_agent) = body_with_agent_2d(&mut tree, "A", Vector2::new(0.0, 0.0));
    let (_b_body, b_agent) = body_with_agent_3d(&mut tree, "B", Vector3::new(0.0, 0.0, 0.0));

    for (agent, label) in [
        (a_agent, "NavigationAgent2D"),
        (b_agent, "NavigationAgent3D"),
    ] {
        let node = tree.get_node(agent).expect("agent is in the tree");
        assert!(
            !node.has_property("position"),
            "{label} must carry no `position` property — a Godot scene never \
             sets one, and supplying it is what hid pat-mgixk.27"
        );
    }
}
