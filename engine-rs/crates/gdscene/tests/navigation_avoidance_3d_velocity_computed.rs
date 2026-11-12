//! pat-0q2kx.97 — 3D avoidance runs in the physics tick.
//!
//! Patina had both 3D solvers (`rvo_avoidance_velocity_3d`, and the ORCA solver
//! the 2D path uses) and **no caller for either**:
//! `process_navigation_avoidance` matched only `NavigationAgent2D` /
//! `NavigationObstacle2D`, 3D agents path-stepped with a hardcoded empty
//! neighbour list, `velocity_computed` never fired for a 3D agent, and
//! `NavigationObstacle3D` did not exist as a class at all.
//!
//! Godot runs RVO for 3D agents every physics frame in two modes
//! (doc/classes/NavigationAgent3D.xml): the default **2D-layered** solve on the
//! XZ plane, where agents are cylinders that interact only when their vertical
//! extents overlap, and full 3D spheres under `use_3d_avoidance`. These tests
//! drive real `MainLoop` physics ticks and assert on what a scene-connected
//! script observes.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Records every `velocity_computed` payload the agent receives, so the test
/// asserts on the signal a game actually connects to rather than on internal
/// state.
const LISTENER: &str = "\
extends Node
var got = 0
var last_x = 0.0
var last_y = 0.0
var last_z = 0.0
func _on_velocity(v):
    self.got = self.got + 1
    self.last_x = v.x
    self.last_y = v.y
    self.last_z = v.z
";

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn float_prop(tree: &SceneTree, node: NodeId, prop: &str) -> f32 {
    match script_prop(tree, node, prop) {
        Variant::Float(f) => f as f32,
        Variant::Int(i) => i as f32,
        other => panic!("{prop} should be a number, got {other:?}"),
    }
}

/// An avoidance-enabled `NavigationAgent3D` at `position` pushing toward
/// `velocity`, with a listener script connected to `velocity_computed`.
fn add_agent(
    tree: &mut SceneTree,
    name: &str,
    position: Vector3,
    velocity: Vector3,
    radius: f32,
    height: f32,
) -> NodeId {
    let root = tree.root_id();
    let mut node = Node::new(name, "NavigationAgent3D");
    node.set_property("position", Variant::Vector3(position));
    node.set_property("avoidance_enabled", Variant::Bool(true));
    node.set_property("velocity", Variant::Vector3(velocity));
    node.set_property("radius", Variant::Float(radius as f64));
    node.set_property("height", Variant::Float(height as f64));
    node.set_property("max_speed", Variant::Float(10.0));
    let id = tree.add_child(root, node).unwrap();
    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(LISTENER, id).expect("listener parses")),
    );
    tree.connect_signal(
        id,
        "velocity_computed",
        gdobject::signal::Connection::new(id.object_id(), "_on_velocity"),
    );
    id
}

fn agent_position(ml: &MainLoop, id: NodeId) -> Vector3 {
    match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector3(v) => v,
        other => panic!("position should be a Vector3, got {other:?}"),
    }
}

/// Integrates the solved velocity back onto the node, the way a game's
/// `velocity_computed` handler feeds `move_and_slide`. The engine solves; moving
/// is the game's job, so the test has to close that loop itself.
fn integrate(ml: &mut MainLoop, ids: &[NodeId], dt: f32) {
    for &id in ids {
        let safe = match ml
            .tree()
            .get_node(id)
            .unwrap()
            .get_property("safe_velocity")
        {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
        };
        let p = agent_position(ml, id);
        let moved = Vector3::new(p.x + safe.x * dt, p.y + safe.y * dt, p.z + safe.z * dt);
        ml.tree_mut()
            .get_node_mut(id)
            .unwrap()
            .set_property("position", Variant::Vector3(moved));
    }
}

fn xz_distance(a: Vector3, b: Vector3) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core wiring test.** Two converging agents each receive
/// `velocity_computed`, and the velocities they are handed keep them apart.
#[test]
fn navigation_avoidance_3d_velocity_computed() {
    const DT: f32 = 1.0 / 60.0;
    const RADIUS: f32 = 1.0;

    let mut tree = SceneTree::new();
    // Head-on along X at the same height, so the layered solve must resolve it.
    let a = add_agent(
        &mut tree,
        "A",
        Vector3::new(-6.0, 0.0, 0.0),
        Vector3::new(4.0, 0.0, 0.0),
        RADIUS,
        1.0,
    );
    let b = add_agent(
        &mut tree,
        "B",
        Vector3::new(6.0, 0.0, 0.0),
        Vector3::new(-4.0, 0.0, 0.0),
        RADIUS,
        1.0,
    );
    let mut ml = MainLoop::new(tree);

    let mut min_separation = f32::INFINITY;
    for _ in 0..60 {
        ml.step(DT as f64);
        integrate(&mut ml, &[a, b], DT);
        let sep = xz_distance(agent_position(&ml, a), agent_position(&ml, b));
        min_separation = min_separation.min(sep);
    }

    // The signal fired — this is the wiring the bead is about.
    assert!(
        float_prop(ml.tree(), a, "got") >= 60.0,
        "agent A must receive velocity_computed every physics tick, got {}",
        float_prop(ml.tree(), a, "got")
    );
    assert!(
        float_prop(ml.tree(), b, "got") >= 60.0,
        "agent B must receive velocity_computed every physics tick"
    );

    // …and the velocities it handed out actually avoided. Head-on agents that
    // ignored each other would pass through, driving separation to ~0.
    assert!(
        min_separation >= RADIUS * 2.0 * 0.9,
        "converging agents must stay roughly a radius-sum apart in XZ; \
         closest approach was {min_separation}, radius sum {}",
        RADIUS * 2.0
    );
}

// ---------------------------------------------------------------------------
// Layered vs full-3D mode
// ---------------------------------------------------------------------------

/// Vertically separated agents ignore each other in the default layered mode —
/// a crowd on a bridge does not negotiate with the crowd underneath it.
#[test]
fn layered_mode_ignores_vertically_disjoint_agents() {
    const DT: f32 = 1.0 / 60.0;

    let mut tree = SceneTree::new();
    // Same XZ line, 10 units apart vertically, each only 1 tall: the height
    // ranges [0,1) and [10,11) do not overlap.
    let a = add_agent(
        &mut tree,
        "Low",
        Vector3::new(-3.0, 0.0, 0.0),
        Vector3::new(4.0, 0.0, 0.0),
        1.0,
        1.0,
    );
    let b = add_agent(
        &mut tree,
        "High",
        Vector3::new(3.0, 10.0, 0.0),
        Vector3::new(-4.0, 0.0, 0.0),
        1.0,
        1.0,
    );
    let mut ml = MainLoop::new(tree);

    for _ in 0..30 {
        ml.step(DT as f64);
        integrate(&mut ml, &[a, b], DT);
    }

    // The solve must actually have run — otherwise "did not deflect" is true
    // for the uninteresting reason that nothing ever solved. This is not
    // hypothetical: the first version of the 3D pass hung off the end of the 2D
    // one, which returns early when there are no 2D agents, so a 3D-only scene
    // like this one skipped avoidance entirely and this test passed vacuously.
    assert!(
        float_prop(ml.tree(), a, "got") >= 30.0,
        "the solve must run every tick for this to mean anything; \
         velocity_computed fired {} times",
        float_prop(ml.tree(), a, "got")
    );

    // Neither was deflected: each kept its straight-line Z of 0.
    assert!(
        agent_position(&ml, a).z.abs() < 1e-3,
        "the low agent must not deflect around one it cannot collide with, z = {}",
        agent_position(&ml, a).z
    );
    assert!(
        agent_position(&ml, b).z.abs() < 1e-3,
        "the high agent must not deflect either, z = {}",
        agent_position(&ml, b).z
    );
}

/// Overlapping height ranges *do* interact in layered mode — the same geometry
/// as above, but with tall enough agents that their cylinders meet.
#[test]
fn layered_mode_interacts_when_height_ranges_overlap() {
    const DT: f32 = 1.0 / 60.0;

    let mut tree = SceneTree::new();
    let a = add_agent(
        &mut tree,
        "Low",
        Vector3::new(-3.0, 0.0, 0.0),
        Vector3::new(4.0, 0.0, 0.0),
        1.0,
        // 12 tall: [0,12) overlaps the other's [10,11).
        12.0,
    );
    let b = add_agent(
        &mut tree,
        "High",
        Vector3::new(3.0, 10.0, 0.0),
        Vector3::new(-4.0, 0.0, 0.0),
        1.0,
        1.0,
    );
    let mut ml = MainLoop::new(tree);

    let mut min_xz = f32::INFINITY;
    for _ in 0..60 {
        ml.step(DT as f64);
        integrate(&mut ml, &[a, b], DT);
        min_xz = min_xz.min(xz_distance(agent_position(&ml, a), agent_position(&ml, b)));
    }

    assert!(
        min_xz >= 1.8,
        "overlapping cylinders must avoid in XZ even though their origins are \
         10 units apart vertically; closest approach {min_xz}"
    );
}

/// `keep_y_velocity` (Godot's default) means the layered solve never cancels
/// the agent's own vertical motion — a falling agent keeps falling while it
/// steers around someone.
#[test]
fn layered_mode_keeps_vertical_velocity() {
    const DT: f32 = 1.0 / 60.0;

    let mut tree = SceneTree::new();
    let a = add_agent(
        &mut tree,
        "Faller",
        Vector3::new(-2.0, 0.0, 0.0),
        // Moving +X and falling -Y at the same time.
        Vector3::new(4.0, -3.0, 0.0),
        1.0,
        1.0,
    );
    let _b = add_agent(
        &mut tree,
        "Blocker",
        Vector3::new(2.0, 0.0, 0.0),
        Vector3::new(-4.0, 0.0, 0.0),
        1.0,
        1.0,
    );
    let mut ml = MainLoop::new(tree);
    ml.step(DT as f64);

    match ml.tree().get_node(a).unwrap().get_property("safe_velocity") {
        Variant::Vector3(v) => assert!(
            (v.y - -3.0).abs() < 1e-4,
            "keep_y_velocity must pass the agent's own Y through the XZ solve, got {}",
            v.y
        ),
        other => panic!("safe_velocity should be a Vector3, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Obstacles
// ---------------------------------------------------------------------------

/// A `NavigationObstacle3D` between two agents deflects them — the class is
/// registered, gathered by the solve, and contributes to the constraints.
#[test]
fn navigation_obstacle_3d_deflects_agents() {
    const DT: f32 = 1.0 / 60.0;

    gdobject::class_db::register_3d_classes();
    assert!(
        gdobject::class_db::class_exists("NavigationObstacle3D"),
        "NavigationObstacle3D must be a registered class"
    );

    let mut tree = SceneTree::new();
    let a = add_agent(
        &mut tree,
        "A",
        Vector3::new(-6.0, 0.0, 0.0),
        Vector3::new(4.0, 0.0, 0.0),
        0.5,
        1.0,
    );
    {
        let root = tree.root_id();
        let mut obstacle = Node::new("Rock", "NavigationObstacle3D");
        obstacle.set_property("position", Variant::Vector3(Vector3::ZERO));
        obstacle.set_property("radius", Variant::Float(2.0));
        obstacle.set_property("height", Variant::Float(2.0));
        tree.add_child(root, obstacle).unwrap();
    }
    let mut ml = MainLoop::new(tree);

    let mut min_xz = f32::INFINITY;
    for _ in 0..60 {
        ml.step(DT as f64);
        integrate(&mut ml, &[a], DT);
        min_xz = min_xz.min(xz_distance(agent_position(&ml, a), Vector3::ZERO));
    }

    assert!(
        min_xz >= 1.5,
        "the agent must steer around a radius-2 obstacle rather than through \
         it; closest approach {min_xz}"
    );
}
