//! pat-s3rrv.20 acceptance: a GDScript drives its **parent body** along a
//! navigation path using the `NavigationAgent2D` method surface.
//!
//! This is the idiom every Godot navigation tutorial shows, and none of it
//! worked in Patina. Two independent defects met here:
//!
//! 1. **No methods existed.** `NavigationAgent2D` was registered with properties
//!    and signals but zero `.method(...)` entries, and no node-method arms
//!    backed them, so `agent.get_next_path_position()` raised
//!    `UndefinedFunction("ObjectId.get_next_path_position")` — the script died on
//!    its first line and every later statement silently never ran.
//! 2. **The agent moved itself.** `process_navigation` simulated the agent
//!    walking its own path and wrote the result onto the agent node with
//!    `node2d::set_position`. Godot's agent inherits `Node`, has no transform,
//!    and moves *nothing*: it is a passive child that publishes where the parent
//!    should head next. So Patina moved the wrong object and left the body — the
//!    thing the player sees — standing still.
//!
//! The test is written so those two failures are distinguishable rather than
//! both collapsing into "it did not move": the body's travel and the agent's
//! stillness are asserted separately.

use gdcore::math::{Vector2, Vector3};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscene::{node2d, node3d};
use gdvariant::Variant;

/// Verbatim Godot: read the agent, steer the *body* toward the published point.
///
/// The body is moved by assigning `position`, not `move_and_slide()`, so this
/// test isolates the navigation contract from the physics registration step —
/// a body that never registers stays at its exact spawn point and would make a
/// navigation failure indistinguishable from a physics one.
///
/// `to_next.length()` guards the first frame: no physics frame has published a
/// point yet, so `get_next_path_position()` answers with the agent's own
/// position and the delta is zero. Normalising a zero vector would poison
/// `position` with NaN and every later assertion would read as "did not move".
const WALKER_GD: &str = "\
extends CharacterBody2D
var steps = 0
func _physics_process(delta):
    var agent = get_node(\"Agent\")
    var next = agent.get_next_path_position()
    var to_next = next - position
    if agent.distance_to_target() > 4.0:
        if to_next.length() > 0.001:
            position = position + to_next.normalized() * 150.0 * delta
            self.steps = self.steps + 1
";

/// The start and goal, in the same open corridor.
const START: Vector2 = Vector2::new(10.0, 30.0);
const GOAL: Vector2 = Vector2::new(180.0, 30.0);

/// A body with an agent child inside one wide navigable rectangle.
///
/// The region is a single convex polygon: `navmesh_polygon_indices` treats bare
/// `__navmesh_vertices__` as one polygon of every vertex, which is the same
/// shape a baked `NavigationRegion2D` feeds `process_navigation`.
fn corridor_scene() -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut region = Node::new("Region", "NavigationRegion2D");
    region.set_property(
        "__navmesh_vertices__",
        Variant::PackedVector2Array(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(200.0, 0.0),
            Vector2::new(200.0, 60.0),
            Vector2::new(0.0, 60.0),
        ]),
    );
    tree.add_child(root, region).unwrap();

    let mut body = Node::new("Body", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(START));
    let body_id = tree.add_child(root, body).unwrap();

    // The agent sits at the body's origin, exactly as Godot's docs set it up.
    // Its LOCAL position must stay (0,0) forever; its GLOBAL position therefore
    // tracks the body, which is what makes path progress follow the body.
    let mut agent = Node::new("Agent", "NavigationAgent2D");
    agent.set_property("position", Variant::Vector2(Vector2::ZERO));
    agent.set_property("target_position", Variant::Vector2(GOAL));
    let agent_id = tree.add_child(body_id, agent).unwrap();

    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(WALKER_GD, body_id).unwrap()),
    );

    (MainLoop::new(tree), body_id, agent_id)
}

fn local_position(ml: &MainLoop, node: NodeId) -> Vector2 {
    node2d::get_position(ml.tree(), node)
}

/// **The acceptance test.** The script moves the body along the path, and the
/// agent node itself never moves.
#[test]
fn nav_agent_script_api_moves_parent_body() {
    let (mut ml, body, agent) = corridor_scene();

    for _ in 0..240 {
        ml.step(1.0 / 60.0);
    }

    let body_end = local_position(&ml, body);
    let agent_local = local_position(&ml, agent);

    // Link 1 — the script ran at all. If the method surface is missing, the
    // handler aborts on `get_next_path_position()` with UndefinedFunction and
    // `steps` is never incremented. Asserting this FIRST means a missing method
    // reports as "the script never ran", not as "navigation is broken".
    let steps = match ml.tree().get_node(body).unwrap().get_property("steps") {
        Variant::Int(n) => n,
        other => panic!("`steps` must be an int counter, got {other:?}"),
    };
    assert!(
        steps > 0,
        "the script never completed a step — `agent.get_next_path_position()` or \
         `agent.distance_to_target()` most likely raised UndefinedFunction, which \
         aborts the handler at the first unknown call",
    );

    // Link 2 — the BODY travelled toward the goal. This is the whole point of a
    // passive agent: the game moves the body.
    assert!(
        body_end.x > START.x + 100.0,
        "the body must travel down the corridor toward {GOAL:?}; it reached \
         {body_end:?} from {START:?}",
    );
    assert!(
        (body_end - GOAL).length() <= 6.0,
        "the body must arrive within its stop threshold of the goal, got \
         {body_end:?} (goal {GOAL:?})",
    );

    // Link 3 — the AGENT never moved itself. This is the half of the bead that a
    // travel assertion alone cannot see: before the fix the agent teleported
    // along the path under its own simulation while the body stood still.
    assert_eq!(
        agent_local,
        Vector2::ZERO,
        "the NavigationAgent2D must never write its own transform — Godot's agent \
         inherits Node and has no transform at all. A non-zero local position \
         means the runtime is still moving the agent instead of the body",
    );

    // Link 4 — and because it stayed put locally, its GLOBAL position rides the
    // body. That is what lets `get_next_path_position` consume waypoints against
    // the body's real progress rather than a private simulation.
    let agent_global = node2d::get_global_position(ml.tree(), agent);
    assert!(
        (agent_global - body_end).length() < 0.001,
        "the agent's global position must track its parent body: agent \
         {agent_global:?} vs body {body_end:?}",
    );
}

/// The identical script pattern on `CharacterBody3D` / `NavigationAgent3D`.
///
/// The 3D path is a separate code path end to end — its own agent map, its own
/// publish loop, its own `set_position` removal — so "2D works" is no evidence
/// for it. The script is the same shape on purpose: what is being proved is
/// that one idiom serves both, which is the Godot contract.
const WALKER_3D_GD: &str = "\
extends CharacterBody3D
var steps = 0
func _physics_process(delta):
    var agent = get_node(\"Agent\")
    var next = agent.get_next_path_position()
    var to_next = next - position
    if agent.distance_to_target() > 4.0:
        if to_next.length() > 0.001:
            position = position + to_next.normalized() * 150.0 * delta
            self.steps = self.steps + 1
";

const START_3D: Vector3 = Vector3::new(10.0, 0.0, 30.0);
const GOAL_3D: Vector3 = Vector3::new(180.0, 0.0, 30.0);

#[test]
fn nav_agent_script_api_moves_parent_body_3d() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // One flat walkable quad in the XZ plane, matching the 2D corridor.
    let mut region = Node::new("Region", "NavigationRegion3D");
    region.set_property(
        "__navmesh_vertices__",
        Variant::PackedVector3Array(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(200.0, 0.0, 0.0),
            Vector3::new(200.0, 0.0, 60.0),
            Vector3::new(0.0, 0.0, 60.0),
        ]),
    );
    tree.add_child(root, region).unwrap();

    let mut body = Node::new("Body", "CharacterBody3D");
    body.set_property("position", Variant::Vector3(START_3D));
    let body_id = tree.add_child(root, body).unwrap();

    let mut agent = Node::new("Agent", "NavigationAgent3D");
    agent.set_property("position", Variant::Vector3(Vector3::ZERO));
    agent.set_property("target_position", Variant::Vector3(GOAL_3D));
    let agent_id = tree.add_child(body_id, agent).unwrap();

    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(WALKER_3D_GD, body_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    for _ in 0..240 {
        ml.step(1.0 / 60.0);
    }

    let steps = match ml.tree().get_node(body_id).unwrap().get_property("steps") {
        Variant::Int(n) => n,
        other => panic!("`steps` must be an int counter, got {other:?}"),
    };
    assert!(
        steps > 0,
        "the 3D script never completed a step — the agent method surface is \
         unreachable for NavigationAgent3D",
    );

    let body_end = node3d::get_position(ml.tree(), body_id);
    assert!(
        (body_end - GOAL_3D).length() <= 6.0,
        "the 3D body must arrive near {GOAL_3D:?}, got {body_end:?}",
    );
    assert_eq!(
        node3d::get_position(ml.tree(), agent_id),
        Vector3::ZERO,
        "the NavigationAgent3D must never write its own transform either",
    );
}

/// The agent is inert without a script driving it: nothing in the engine moves
/// the body on its own.
///
/// Without this control, link 2 above could pass on an engine that quietly moved
/// the body itself — which is the *same* class of bug as the agent moving
/// itself, just relocated.
#[test]
fn an_unscripted_body_is_never_moved_by_its_agent() {
    let (mut ml, body, agent) = corridor_scene();
    // Same scene, minus the script.
    let _ = ml.tree_mut().detach_script(body);

    for _ in 0..240 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        local_position(&ml, body),
        START,
        "no script means no movement — navigation must never move a body on its own",
    );
    assert_eq!(
        local_position(&ml, agent),
        Vector2::ZERO,
        "and the agent must still not move itself",
    );
}
