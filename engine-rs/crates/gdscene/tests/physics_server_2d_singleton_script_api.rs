//! pat-0q2kx.200 — the `PhysicsServer2D` server-side API: RID-addressed shapes
//! and bodies that share the scene's world, plus swept motion tests.
//!
//! Godot ground truth: `servers/physics_server_2d.cpp` — `*_shape_create` +
//! `shape_set_data`, `body_create` / `body_set_space` / `body_add_shape` /
//! `body_set_state`, and `body_test_motion`, all addressed by RID so a script
//! can drive physics without nodes.
//!
//! Patina exposed none of it: there is no `PhysicsServer2D` arm anywhere in the
//! interpreter's singleton dispatch, and no RID registry behind it.
//!
//! Two layers are covered, because a break in either one is a different bug:
//! the **server** level (RID registry over the shared world) and the **script**
//! level (`PhysicsServer2D.circle_shape_create()` reaching it from GDScript
//! through the interpreter's singleton dispatch).

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::physics_server::PhysicsServer;
use gdscene::GDScriptNodeInstance;
use gdscene::SceneTree;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DT: f32 = 1.0 / 60.0;

/// A scene with a `StaticBody2D` floor spanning `y ∈ [100, 116]`, registered
/// into the server the ordinary (node) way.
fn server_with_node_floor() -> PhysicsServer {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let floor = tree
        .add_child(root, Node::new("Floor", "StaticBody2D"))
        .unwrap();
    {
        let node = tree.get_node_mut(floor).unwrap();
        node.set_property("position", Variant::Vector2(Vector2::new(0.0, 108.0)));
    }
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(400.0, 8.0)));
    tree.add_child(floor, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);
    server
}

/// **Server side.** A body created purely through the server API falls under
/// gravity and is stopped by a node-created floor — proving both live in the
/// same world. The script-level acceptance below rides on this.
#[test]
fn physics_server_2d_server_api_shares_the_scene_world() {
    let mut server = server_with_node_floor();

    // circle_shape_create + shape_set_data(radius)
    let shape = server.server_circle_shape_create();
    assert!(
        server.server_shape_set_data(shape, 8.0, None),
        "shape_set_data must accept a radius for a circle shape RID"
    );

    // body_create + body_set_space + body_add_shape + body_set_state(TRANSFORM)
    let body = server.server_body_create();
    assert!(
        server.server_body_id(body).is_none(),
        "a body with no space must not be in the world yet — Godot's spaceless \
         body is inert"
    );
    assert!(server.server_body_set_space(body, 1));
    assert!(
        server.server_body_id(body).is_some(),
        "body_set_space must admit the body to the server's world"
    );
    assert!(server.server_body_add_shape(body, shape));
    // TRANSFORM = 0: start well above the floor.
    assert!(server.server_body_set_state_vec2(body, 0, Vector2::new(0.0, 20.0)));

    let start = server
        .server_body_position(body)
        .expect("body is in the world");
    assert!(
        (start.y - 20.0).abs() < 1e-4,
        "set_state must place the body"
    );

    // Fall. The floor's top surface is y = 100 and the circle radius is 8, so a
    // body that is stopped rests near y = 92; one that is NOT stopped keeps
    // going past the floor entirely.
    for _ in 0..180 {
        server.step_physics(DT);
    }
    let resting = server
        .server_body_position(body)
        .expect("still in the world");
    assert!(
        resting.y > start.y,
        "the server-created body must fall under gravity: {} -> {}",
        start.y,
        resting.y
    );
    assert!(
        resting.y < 108.0,
        "the node-created StaticBody2D floor must stop the server-created body \
         — they have to share one world. Body ended at y = {}",
        resting.y
    );
}

/// `body_test_motion` reports a blocked sweep with a travel shorter than the
/// motion asked for.
#[test]
fn body_test_motion_reports_blocked_travel_against_a_wall() {
    let mut server = server_with_node_floor();

    let shape = server.server_circle_shape_create();
    server.server_shape_set_data(shape, 8.0, None);
    let body = server.server_body_create();
    server.server_body_set_space(body, 1);
    server.server_body_add_shape(body, shape);
    // Just above the floor, so a downward sweep runs into it.
    server.server_body_set_state_vec2(body, 0, Vector2::new(0.0, 60.0));

    let motion = Vector2::new(0.0, 80.0);
    let (collided, travel) = server.server_body_test_motion(body, motion);
    assert!(
        collided,
        "a downward sweep into the floor must report a collision"
    );
    assert!(
        travel.length() < motion.length(),
        "a blocked sweep must travel less than it asked for: {} vs {}",
        travel.length(),
        motion.length()
    );
    assert!(
        travel.y >= 0.0,
        "travel must be along the motion, not backwards: {travel:?}"
    );

    // The control: the same sweep upward, away from the floor, is unobstructed
    // and reports the full motion.
    let (up_collided, up_travel) = server.server_body_test_motion(body, Vector2::new(0.0, -20.0));
    assert!(!up_collided, "an unobstructed sweep must not report a hit");
    assert!(
        (up_travel.y + 20.0).abs() < 1e-3,
        "an unobstructed sweep returns the full motion, got {up_travel:?}"
    );
}

/// `test_move` on a node body: true when the motion would be blocked.
///
/// This is `PhysicsBody2D.test_move`'s contract, addressed by scene node rather
/// than RID, over the same swept helper.
#[test]
fn node_test_move_reports_blocked_before_a_wall() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // A wall at x = 60, and a character just to its left.
    let wall = tree
        .add_child(root, Node::new("Wall", "StaticBody2D"))
        .unwrap();
    {
        let node = tree.get_node_mut(wall).unwrap();
        node.set_property("position", Variant::Vector2(Vector2::new(60.0, 0.0)));
    }
    let mut wall_shape = Node::new("Shape", "CollisionShape2D");
    wall_shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    wall_shape.set_property("size", Variant::Vector2(Vector2::new(8.0, 200.0)));
    tree.add_child(wall, wall_shape).unwrap();

    let character = tree
        .add_child(root, Node::new("Player", "CharacterBody2D"))
        .unwrap();
    {
        let node = tree.get_node_mut(character).unwrap();
        node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    }
    let mut body_shape = Node::new("Shape", "CollisionShape2D");
    body_shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    body_shape.set_property("size", Variant::Vector2(Vector2::new(16.0, 16.0)));
    tree.add_child(character, body_shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    assert!(
        server.node_test_move(character, Vector2::new(100.0, 0.0)),
        "moving 100px right into a wall at x = 60 must report blocked"
    );
    assert!(
        !server.node_test_move(character, Vector2::new(-100.0, 0.0)),
        "moving away from the wall must report clear"
    );
}

/// Verbatim-shaped Godot: a script that drives physics through the
/// `PhysicsServer2D` singleton with no node of its own for the body.
///
/// `made` gates creation to one tick, then every later tick reads the body back,
/// so `last_y` tracks a body the MainLoop is stepping. `ticks` counts handler
/// completions — a handler that throws part-way (an unwired method is a
/// `RuntimeError`) stops incrementing it, which is the "no script errors"
/// assertion.
const SERVER_DRIVER_GD: &str = "\
extends Node2D
var shape_rid = 0
var body_rid = 0
var made = false
var start_y = 0.0
var last_y = 0.0
var motion_collided = false
var motion_travel_y = -1.0
var ticks = 0
func _physics_process(delta):
    if made == false:
        made = true
        shape_rid = PhysicsServer2D.circle_shape_create()
        PhysicsServer2D.shape_set_data(shape_rid, 8.0)
        body_rid = PhysicsServer2D.body_create()
        PhysicsServer2D.body_set_space(body_rid, 1)
        PhysicsServer2D.body_add_shape(body_rid, shape_rid)
        PhysicsServer2D.body_set_state(body_rid, 0, Vector2(0, 20))
        var placed = PhysicsServer2D.body_get_state(body_rid, 0)
        start_y = placed.y
        var res = PhysicsServer2D.body_test_motion(body_rid, Vector2(0, 200))
        motion_collided = res[\"collided\"]
        var travel = res[\"travel\"]
        motion_travel_y = travel.y
    else:
        var here = PhysicsServer2D.body_get_state(body_rid, 0)
        last_y = here.y
    ticks = ticks + 1
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn script_float(ml: &MainLoop, node: NodeId, name: &str) -> f64 {
    match script_var(ml, node, name) {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("{name} should be a number, got {other:?}"),
    }
}

fn script_int(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Some(Variant::Int(i)) => i,
        Some(Variant::Float(f)) => f as i64,
        other => panic!("{name} should be an int, got {other:?}"),
    }
}

/// **The acceptance test.** A GDScript reaches `PhysicsServer2D`, creates a
/// shape and a body through it, and that body — owned by no node — falls under
/// the MainLoop's own physics ticks and is stopped by a scene `StaticBody2D`.
///
/// Every link is asserted separately so a failure names the broken one instead of
/// only reporting "it did not move".
#[test]
fn physics_server_2d_singleton_script_api() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // Floor spanning y ∈ [190, 210]: a node body, registered the ordinary way.
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 200.0)));
    let floor_id = tree.add_child(main, floor).unwrap();
    let mut floor_shape = Node::new("CollisionShape", "CollisionShape2D");
    floor_shape.set_property("size", Variant::Vector2(Vector2::new(1000.0, 20.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    // The driver is a plain Node2D — it is not the body, it only issues server
    // calls, which is the point of the singleton.
    let driver = tree.add_child(main, Node::new("Driver", "Node2D")).unwrap();
    tree.attach_script(
        driver,
        Box::new(GDScriptNodeInstance::from_source(SERVER_DRIVER_GD, driver).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // Only `register_physics_bodies` populates `node_to_body`; without it the
    // floor is not in the world and nothing would stop the falling body.
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    // Link 1 — the dispatch arm exists and reached the server: Godot's invalid
    // RID is 0, so a real handle proves the call was not a silent no-op.
    let shape_rid = script_int(&ml, driver, "shape_rid");
    let body_rid = script_int(&ml, driver, "body_rid");
    assert!(
        shape_rid > 0,
        "link 1: PhysicsServer2D.circle_shape_create() must return a live RID, got {shape_rid}"
    );
    assert!(
        body_rid > 0,
        "link 1: PhysicsServer2D.body_create() must return a live RID, got {body_rid}"
    );

    // Link 2 — body_set_space + body_set_state placed the body in the world, and
    // body_get_state read it back through the singleton.
    let start_y = script_float(&ml, driver, "start_y");
    assert!(
        (start_y - 20.0).abs() < 1e-3,
        "link 2: body_set_state(TRANSFORM) then body_get_state must round-trip the \
         position, got {start_y}"
    );

    // Link 3 — body_test_motion swept against the node floor and reported a
    // blocked travel shorter than the 200px asked for.
    assert_eq!(
        script_var(&ml, driver, "motion_collided"),
        Some(Variant::Bool(true)),
        "link 3: a 200px downward sweep from y=20 into the floor at y=190 must collide"
    );
    let travel_y = script_float(&ml, driver, "motion_travel_y");
    assert!(
        (0.0..200.0).contains(&travel_y),
        "link 3: a blocked sweep must travel forward but less than the motion, got {travel_y}"
    );

    // Link 4 — the script-created body is simulated by the MainLoop's own ticks
    // and is stopped by the node-created floor. 150 frames is well past the
    // ~35 ticks a 980px/s² fall from y=20 to the floor needs.
    for _ in 0..150 {
        ml.step(1.0 / 60.0);
    }
    let last_y = script_float(&ml, driver, "last_y");
    assert!(
        last_y > start_y,
        "link 4: the server-created body must fall under the MainLoop's gravity: \
         {start_y} -> {last_y}"
    );
    assert!(
        last_y < 200.0,
        "link 4: the node-created StaticBody2D floor must stop the script-created \
         body — they have to share one world. Body ended at y = {last_y}"
    );

    // Link 5 — no handler ever threw. An unwired singleton method raises a
    // RuntimeError, which would abort the handler before `ticks` was bumped.
    let ticks = script_int(&ml, driver, "ticks");
    assert!(
        ticks > 100,
        "link 5: every _physics_process must have run to completion; a script error \
         would stop the count early. Got {ticks} after 151 frames"
    );
}

/// `PhysicsBody2D.test_move()` from GDScript, which routes through the same
/// swept helper as the server's `body_test_motion`.
///
/// The two directions are asserted from one script so a `test_move` that
/// resolves but always answers the same thing fails: `blocked_right` and
/// `clear_left` cannot both be right unless the sweep really ran.
const TEST_MOVE_GD: &str = "\
extends CharacterBody2D
var blocked_right = false
var clear_left = true
var asked = false
func _physics_process(delta):
    if asked == false:
        asked = true
        blocked_right = test_move(Vector2(100, 0))
        clear_left = test_move(Vector2(-100, 0))
";

/// A script calling `test_move()` on its own `CharacterBody2D` sees the wall.
#[test]
fn script_test_move_reports_blocked_before_a_wall() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // Wall at x = 60, 8px wide: its near face is x = 56.
    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(60.0, 0.0)));
    let wall_id = tree.add_child(main, wall).unwrap();
    let mut wall_shape = Node::new("CollisionShape", "CollisionShape2D");
    wall_shape.set_property("size", Variant::Vector2(Vector2::new(8.0, 200.0)));
    tree.add_child(wall_id, wall_shape).unwrap();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::ZERO));
    let player_id = tree.add_child(main, player).unwrap();
    let mut player_shape = Node::new("CollisionShape", "CollisionShape2D");
    player_shape.set_property("size", Variant::Vector2(Vector2::new(16.0, 16.0)));
    tree.add_child(player_id, player_shape).unwrap();
    tree.attach_script(
        player_id,
        Box::new(GDScriptNodeInstance::from_source(TEST_MOVE_GD, player_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, player_id, "asked"),
        Some(Variant::Bool(true)),
        "the handler must have run at all"
    );
    assert_eq!(
        script_var(&ml, player_id, "blocked_right"),
        Some(Variant::Bool(true)),
        "test_move(Vector2(100, 0)) into a wall 56px away must report blocked"
    );
    assert_eq!(
        script_var(&ml, player_id, "clear_left"),
        Some(Variant::Bool(false)),
        "test_move(Vector2(-100, 0)) away from the wall must report clear"
    );
}
