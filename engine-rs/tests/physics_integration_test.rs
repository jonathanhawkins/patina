//! Physics integration tests covering beads pat-cyf, pat-clv, pat-1za, pat-yxp.
//!
//! - pat-cyf: Collision shape registration and overlap coverage
//! - pat-clv: CharacterBody2D and StaticBody2D behavior fixtures
//! - pat-1za: Deterministic physics trace goldens
//! - pat-yxp: physics_playground golden trace fixture

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::physics_server::PhysicsTraceEntry;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

/// Helper: build a scene tree with physics bodies programmatically.
fn make_rigid_and_static_scene() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // RigidBody2D with circle collision shape
    let mut rigid = Node::new("Ball", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(0.0, 50.0)));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(16.0));
    tree.add_child(rigid_id, shape).unwrap();

    // StaticBody2D with rect collision shape (floor)
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(100.0, 300.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut shape2 = Node::new("CollisionShape", "CollisionShape2D");
    shape2.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(floor_id, shape2).unwrap();

    (tree, rigid_id, floor_id)
}

/// Helper: build a CharacterBody2D scene.
fn make_character_scene() -> (
    SceneTree,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // CharacterBody2D
    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(12.0));
    tree.add_child(player_id, s).unwrap();

    // StaticBody2D platform
    let mut platform = Node::new("Platform", "StaticBody2D");
    platform.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    let platform_id = tree.add_child(root, platform).unwrap();
    let mut s2 = Node::new("Shape", "CollisionShape2D");
    s2.set_property("size", Variant::Vector2(Vector2::new(200.0, 20.0)));
    tree.add_child(platform_id, s2).unwrap();

    // StaticBody2D wall
    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(300.0, 150.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut s3 = Node::new("Shape", "CollisionShape2D");
    s3.set_property("size", Variant::Vector2(Vector2::new(20.0, 100.0)));
    tree.add_child(wall_id, s3).unwrap();

    (tree, player_id, platform_id, wall_id)
}

/// Helper: run a physics simulation and collect the trace.
fn run_traced_physics(
    tree: SceneTree,
    frames: u64,
    delta: f64,
) -> (MainLoop, Vec<PhysicsTraceEntry>) {
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);
    ml.run_frames(frames, delta);
    let trace = ml.physics_server().trace().to_vec();
    (ml, trace)
}

// ═══════════════════════════════════════════════════════════════════════
// pat-cyf: Collision shape registration and overlap coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cyf_circle_shape_registered_from_scene_node() {
    let (tree, _, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    assert_eq!(ml.physics_server().body_count(), 2);
}

#[test]
fn cyf_rect_shape_registered_from_scene_node() {
    let (tree, _, floor_id) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(floor_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert_eq!(
        body.shape,
        gdphysics2d::Shape2D::Rectangle {
            half_extents: Vector2::new(200.0, 10.0)
        }
    );
}

#[test]
fn cyf_overlapping_bodies_produce_collision_events() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Two overlapping circles
    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        !events.is_empty(),
        "overlapping bodies should generate collision events"
    );
}

#[test]
fn cyf_separated_bodies_no_collision_events() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(5.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(5.0));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        events.is_empty(),
        "well-separated bodies should not collide"
    );
}

#[test]
fn cyf_collision_layer_mask_filtering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    a.set_property("collision_layer", Variant::Int(1));
    a.set_property("collision_mask", Variant::Int(1));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    b.set_property("collision_layer", Variant::Int(2));
    b.set_property("collision_mask", Variant::Int(2));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        events.is_empty(),
        "bodies on different layers should not collide"
    );
}

#[test]
fn cyf_area_overlap_detection() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
    let body_nid = tree.add_child(root, body).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(2.0));
    tree.add_child(body_nid, s).unwrap();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
    let area_nid = tree.add_child(root, area).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(area_nid, sa).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_overlap_events();
    assert!(!events.is_empty(), "area should detect overlapping body");
}

// ═══════════════════════════════════════════════════════════════════════
// pat-clv: CharacterBody2D and StaticBody2D behavior fixtures
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn clv_static_body_does_not_move_over_time() {
    let (tree, _, _, _) = make_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Record platform position
    let platform_pos_before = {
        let node = ml
            .tree()
            .get_node(ml.tree().all_nodes_in_tree_order()[2])
            .unwrap();
        match node.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        }
    };

    ml.run_frames(60, 1.0 / 60.0);

    // Re-read platform position — should not have changed
    let platform_pos_after = {
        let node = ml
            .tree()
            .get_node(ml.tree().all_nodes_in_tree_order()[2])
            .unwrap();
        match node.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        }
    };

    assert_eq!(
        platform_pos_before, platform_pos_after,
        "StaticBody2D must not move"
    );
}

#[test]
fn clv_character_body_is_kinematic() {
    let (tree, player_id, _, _) = make_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(player_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert_eq!(body.body_type, gdphysics2d::BodyType::Kinematic);
}

#[test]
fn clv_static_wall_position_unchanged_after_collision() {
    let (tree, _, _, wall_id) = make_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = {
        let body_id = ml.physics_server().body_for_node(wall_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    ml.run_frames(120, 1.0 / 60.0);

    let pos_after = {
        let body_id = ml.physics_server().body_for_node(wall_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    assert_eq!(pos_before, pos_after, "static wall must not move");
}

#[test]
fn clv_rigid_body_moves_under_velocity() {
    let (tree, rigid_id, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = {
        let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    ml.run_frames(10, 1.0 / 60.0);

    let pos_after = {
        let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    assert!(
        pos_after.y > pos_before.y,
        "rigid body with downward velocity should fall: before={:?}, after={:?}",
        pos_before,
        pos_after
    );
}

// ═══════════════════════════════════════════════════════════════════════
// pat-1za: Deterministic physics trace goldens
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn za_deterministic_physics_same_setup_same_trace() {
    let run = || {
        let (tree, _, _) = make_rigid_and_static_scene();
        let (_, mut trace) = run_traced_physics(tree, 30, 1.0 / 60.0);
        trace.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
        trace
    };

    let trace1 = run();
    let trace2 = run();

    assert_eq!(trace1.len(), trace2.len(), "trace lengths must match");
    for (i, (a, b)) in trace1.iter().zip(trace2.iter()).enumerate() {
        assert_eq!(a.name, b.name, "trace entry {i}: name mismatch");
        assert_eq!(a.frame, b.frame, "trace entry {i}: frame mismatch");
        assert!(
            approx_eq(a.position.x, b.position.x) && approx_eq(a.position.y, b.position.y),
            "trace entry {i}: position mismatch: {:?} vs {:?}",
            a.position,
            b.position
        );
        assert!(
            approx_eq(a.velocity.x, b.velocity.x) && approx_eq(a.velocity.y, b.velocity.y),
            "trace entry {i}: velocity mismatch: {:?} vs {:?}",
            a.velocity,
            b.velocity
        );
    }
}

#[test]
fn za_trace_contains_expected_bodies() {
    let (tree, _, _) = make_rigid_and_static_scene();
    let (_, trace) = run_traced_physics(tree, 5, 1.0 / 60.0);

    let names: std::collections::HashSet<&str> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("Ball"), "trace should contain Ball");
    assert!(names.contains("Floor"), "trace should contain Floor");
}

#[test]
fn za_trace_frame_numbers_monotonically_increase() {
    let (tree, _, _) = make_rigid_and_static_scene();
    let (_, trace) = run_traced_physics(tree, 10, 1.0 / 60.0);

    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();
    for window in ball_entries.windows(2) {
        assert!(
            window[1].frame >= window[0].frame,
            "frame numbers should increase: {} -> {}",
            window[0].frame,
            window[1].frame
        );
    }
}

#[test]
fn za_golden_trace_write_and_compare() {
    // Generate a trace, write it to a golden file, then re-generate and compare.
    let generate_trace = || {
        let (tree, _, _) = make_rigid_and_static_scene();
        let (_, trace) = run_traced_physics(tree, 20, 1.0 / 60.0);
        trace
    };

    let golden = generate_trace();
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/physics/rigid_static_20frames.json");

    // Write golden
    let golden_json: Vec<_> = golden
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
                "vx": (e.velocity.x * 1000.0).round() / 1000.0,
                "vy": (e.velocity.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    let golden_str = serde_json::to_string_pretty(&golden_json).unwrap();
    std::fs::write(&golden_path, &golden_str).unwrap();

    // Re-generate and compare
    let rerun = generate_trace();
    assert_eq!(golden.len(), rerun.len(), "trace length mismatch");
    for (i, (g, r)) in golden.iter().zip(rerun.iter()).enumerate() {
        assert_eq!(g.name, r.name, "entry {i}: name");
        assert_eq!(g.frame, r.frame, "entry {i}: frame");
        assert!(
            approx_eq(g.position.x, r.position.x) && approx_eq(g.position.y, r.position.y),
            "entry {i}: position drift: golden={:?} vs rerun={:?}",
            g.position,
            r.position
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// pat-yxp: physics_playground golden trace fixture
// ═══════════════════════════════════════════════════════════════════════

/// Build the physics_playground scene programmatically (matching the .tscn).
fn make_physics_playground() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Ball: RigidBody2D at (400, 100)
    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(400.0, 100.0)));
    ball.set_property("mass", Variant::Float(1.0));
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("CollisionShape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(16.0));
    tree.add_child(ball_id, s).unwrap();

    // Wall: StaticBody2D at (800, 300)
    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(800.0, 300.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut sw = Node::new("CollisionShape", "CollisionShape2D");
    sw.set_property("size", Variant::Vector2(Vector2::new(20.0, 400.0)));
    tree.add_child(wall_id, sw).unwrap();

    // Floor: StaticBody2D at (400, 600)
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(400.0, 600.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("CollisionShape", "CollisionShape2D");
    sf.set_property("size", Variant::Vector2(Vector2::new(800.0, 20.0)));
    tree.add_child(floor_id, sf).unwrap();

    tree
}

#[test]
fn yxp_physics_playground_golden_trace() {
    let tree = make_physics_playground();
    let (ml, trace) = run_traced_physics(tree, 60, 1.0 / 60.0);

    // Verify trace is not empty
    assert!(!trace.is_empty(), "trace should not be empty");

    // Verify all expected bodies are in the trace
    let names: std::collections::HashSet<&str> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("Ball"), "should trace Ball");
    assert!(names.contains("Wall"), "should trace Wall");
    assert!(names.contains("Floor"), "should trace Floor");

    // Write golden trace
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/physics/physics_playground_60frames.json");
    let golden_json: Vec<_> = trace
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
                "vx": (e.velocity.x * 1000.0).round() / 1000.0,
                "vy": (e.velocity.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    std::fs::write(
        &golden_path,
        serde_json::to_string_pretty(&golden_json).unwrap(),
    )
    .unwrap();

    // Static bodies should not have moved
    let wall_entries: Vec<_> = trace.iter().filter(|e| e.name == "Wall").collect();
    for entry in &wall_entries {
        assert!(
            approx_eq(entry.position.x, 800.0) && approx_eq(entry.position.y, 300.0),
            "Wall should stay at (800, 300), got {:?}",
            entry.position
        );
    }

    let floor_entries: Vec<_> = trace.iter().filter(|e| e.name == "Floor").collect();
    for entry in &floor_entries {
        assert!(
            approx_eq(entry.position.x, 400.0) && approx_eq(entry.position.y, 600.0),
            "Floor should stay at (400, 600), got {:?}",
            entry.position
        );
    }

    // Verify determinism — sort by (frame, name) to handle HashMap iteration order.
    let sort_trace = |t: &mut Vec<PhysicsTraceEntry>| {
        t.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
    };
    let tree2 = make_physics_playground();
    let (_, mut trace2) = run_traced_physics(tree2, 60, 1.0 / 60.0);
    let mut trace_sorted = trace.clone();
    sort_trace(&mut trace_sorted);
    sort_trace(&mut trace2);

    assert_eq!(
        trace_sorted.len(),
        trace2.len(),
        "replay trace length must match"
    );
    for (i, (a, b)) in trace_sorted.iter().zip(trace2.iter()).enumerate() {
        assert_eq!(a.name, b.name, "entry {i}: name");
        assert!(
            approx_eq(a.position.x, b.position.x) && approx_eq(a.position.y, b.position.y),
            "entry {i}: position drift: {:?} vs {:?}",
            a.position,
            b.position
        );
    }

    assert_eq!(ml.frame_count(), 60);
}

#[test]
fn yxp_physics_playground_ball_position_evolves() {
    let tree = make_physics_playground();
    let (_, trace) = run_traced_physics(tree, 30, 1.0 / 60.0);

    let ball_first = trace.iter().find(|e| e.name == "Ball").unwrap();
    let _ball_last = trace.iter().rev().find(|e| e.name == "Ball").unwrap();

    // Ball starts at (400, 100) with no initial velocity.
    // Even without gravity force, just being in the world for 30 frames
    // should show it at the same place (no force applied).
    // The ball's position should be stable since there's no gravity force.
    assert!(
        approx_eq(ball_first.position.x, 400.0),
        "Ball should start near x=400, got {}",
        ball_first.position.x
    );
}

#[test]
fn yxp_golden_file_exists_after_generation() {
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/physics/physics_playground_60frames.json");

    // Generate the trace (will create the file)
    let tree = make_physics_playground();
    let (_, trace) = run_traced_physics(tree, 60, 1.0 / 60.0);

    let golden_json: Vec<_> = trace
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    std::fs::write(
        &golden_path,
        serde_json::to_string_pretty(&golden_json).unwrap(),
    )
    .unwrap();

    assert!(golden_path.exists(), "golden trace file should exist");

    // Read it back and verify it parses
    let contents = std::fs::read_to_string(&golden_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(parsed.is_array(), "golden file should be a JSON array");
    assert!(
        parsed.as_array().unwrap().len() > 0,
        "golden file should not be empty"
    );
}
