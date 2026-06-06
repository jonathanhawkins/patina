//! pat-x52z: Deterministic physics trace goldens — integrated runtime path.
//!
//! Extends golden coverage beyond basic single-scenario traces to cover the
//! full integrated MainLoop pipeline with multiple subsystem interactions.
//!
//! New golden scenarios:
//! 1. CharacterBody2D slide along floor (20 frames)
//! 2. Mixed body types: Rigid + Static + Character in one scene (30 frames)
//! 3. Multi-rigid cascade: 3 rigid bodies chain reaction (30 frames)
//! 4. High TPS (120): gravity at double tick rate (30 frames)
//! 5. Pause-resume: trace continuity across pause boundary (20 frames)
//!
//! Each scenario has a golden JSON artifact and a determinism cross-check.
//! Acceptance: traces compare cleanly against checked-in golden artifacts.

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

// ---------------------------------------------------------------------------
// Scene builders
// ---------------------------------------------------------------------------

/// CharacterBody2D sliding right along a static floor.
fn make_character_slide() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    player.set_property("velocity", Variant::Vector2(Vector2::new(200.0, 100.0)));
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let pid = tree.add_child(root, player).unwrap();
    let mut ps = Node::new("Shape", "CollisionShape2D");
    ps.set_property("radius", Variant::Float(8.0));
    tree.add_child(pid, ps).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 30.0)));
    floor.set_property("collision_layer", Variant::Int(1));
    floor.set_property("collision_mask", Variant::Int(1));
    let fid = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("Shape", "CollisionShape2D");
    fs.set_property("size", Variant::Vector2(Vector2::new(2000.0, 20.0)));
    tree.add_child(fid, fs).unwrap();

    tree
}

/// Mixed body types: RigidBody2D falling, StaticBody2D floor, CharacterBody2D moving right.
fn make_mixed_bodies() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Rigid body falling
    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 120.0)),
    );
    ball.set_property("mass", Variant::Float(1.0));
    let bid = tree.add_child(root, ball).unwrap();
    let mut bs = Node::new("Shape", "CollisionShape2D");
    bs.set_property("radius", Variant::Float(8.0));
    tree.add_child(bid, bs).unwrap();

    // Static floor
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(100.0, 80.0)));
    let fid = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("Shape", "CollisionShape2D");
    fs.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(fid, fs).unwrap();

    // Character moving right
    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    player.set_property("velocity", Variant::Vector2(Vector2::new(150.0, 0.0)));
    let pid = tree.add_child(root, player).unwrap();
    let mut ps = Node::new("Shape", "CollisionShape2D");
    ps.set_property("radius", Variant::Float(6.0));
    tree.add_child(pid, ps).unwrap();

    tree
}

/// Three rigid bodies in a line — first pushes into second, second into third.
fn make_multi_rigid_cascade() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    a.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(200.0, 0.0)),
    );
    a.set_property("mass", Variant::Float(2.0));
    a.set_property("bounce", Variant::Float(0.8));
    let aid = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(8.0));
    tree.add_child(aid, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(25.0, 0.0)));
    b.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    b.set_property("mass", Variant::Float(1.0));
    b.set_property("bounce", Variant::Float(0.8));
    let bid = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(8.0));
    tree.add_child(bid, sb).unwrap();

    let mut c = Node::new("C", "RigidBody2D");
    c.set_property("position", Variant::Vector2(Vector2::new(50.0, 0.0)));
    c.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    c.set_property("mass", Variant::Float(1.0));
    c.set_property("bounce", Variant::Float(0.8));
    let cid = tree.add_child(root, c).unwrap();
    let mut sc = Node::new("Shape", "CollisionShape2D");
    sc.set_property("radius", Variant::Float(8.0));
    tree.add_child(cid, sc).unwrap();

    tree
}

/// Single rigid body gravity fall — same as base scenario but at 120 TPS.
fn make_high_tps_gravity() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 100.0)),
    );
    let bid = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(bid, s).unwrap();

    tree
}

/// Single rigid body for pause-resume trace.
fn make_pause_resume() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(60.0, 90.0)),
    );
    let bid = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(bid, s).unwrap();

    tree
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run_traced(tree: SceneTree, frames: u64, delta: f64) -> (MainLoop, Vec<PhysicsTraceEntry>) {
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);
    ml.run_frames(frames, delta);
    let trace = ml.physics_server().trace().to_vec();
    (ml, trace)
}

fn run_traced_high_tps(
    tree: SceneTree,
    frames: u64,
    delta: f64,
    tps: u32,
) -> (MainLoop, Vec<PhysicsTraceEntry>) {
    let mut ml = MainLoop::new(tree);
    ml.set_physics_ticks_per_second(tps);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);
    ml.run_frames(frames, delta);
    let trace = ml.physics_server().trace().to_vec();
    (ml, trace)
}

fn run_traced_with_pause(
    tree: SceneTree,
    pre_pause_frames: u64,
    paused_frames: u64,
    post_pause_frames: u64,
    delta: f64,
) -> (MainLoop, Vec<PhysicsTraceEntry>) {
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);

    // Run pre-pause
    ml.run_frames(pre_pause_frames, delta);

    // Pause and run
    ml.set_paused(true);
    ml.run_frames(paused_frames, delta);

    // Unpause and run
    ml.set_paused(false);
    ml.run_frames(post_pause_frames, delta);

    let trace = ml.physics_server().trace().to_vec();
    (ml, trace)
}

/// Assert two traces are entry-by-entry equal within EPSILON.
fn assert_traces_equal(label: &str, a: &[PhysicsTraceEntry], b: &[PhysicsTraceEntry]) {
    assert_eq!(
        a.len(),
        b.len(),
        "{label}: trace length mismatch ({} vs {})",
        a.len(),
        b.len()
    );
    for (i, (ga, gb)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(ga.name, gb.name, "{label} entry {i}: name");
        assert_eq!(ga.frame, gb.frame, "{label} entry {i}: frame");
        assert!(
            approx_eq(ga.position.x, gb.position.x) && approx_eq(ga.position.y, gb.position.y),
            "{label} entry {i}: position drift: {:?} vs {:?}",
            ga.position,
            gb.position,
        );
        assert!(
            approx_eq(ga.velocity.x, gb.velocity.x) && approx_eq(ga.velocity.y, gb.velocity.y),
            "{label} entry {i}: velocity drift: {:?} vs {:?}",
            ga.velocity,
            gb.velocity,
        );
    }
}

fn load_golden(name: &str) -> Vec<serde_json::Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../fixtures/golden/physics/{name}.json"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load golden {name}: {e}"));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse golden {name}: {e}"))
}

fn assert_trace_matches_golden(
    label: &str,
    trace: &[PhysicsTraceEntry],
    golden: &[serde_json::Value],
) {
    assert_eq!(
        trace.len(),
        golden.len(),
        "{label}: trace length {} != golden length {}",
        trace.len(),
        golden.len()
    );
    for (i, (entry, gval)) in trace.iter().zip(golden.iter()).enumerate() {
        let g_name = gval["name"].as_str().unwrap();
        let g_frame = gval["frame"].as_u64().unwrap();
        let g_px = gval["px"].as_f64().unwrap() as f32;
        let g_py = gval["py"].as_f64().unwrap() as f32;
        let g_vx = gval["vx"].as_f64().unwrap() as f32;
        let g_vy = gval["vy"].as_f64().unwrap() as f32;

        assert_eq!(entry.name, g_name, "{label} entry {i}: name");
        assert_eq!(entry.frame, g_frame, "{label} entry {i}: frame");
        assert!(
            approx_eq(entry.position.x, g_px) && approx_eq(entry.position.y, g_py),
            "{label} entry {i}: position ({},{}) vs golden ({},{})",
            entry.position.x,
            entry.position.y,
            g_px,
            g_py
        );
        assert!(
            approx_eq(entry.velocity.x, g_vx) && approx_eq(entry.velocity.y, g_vy),
            "{label} entry {i}: velocity ({},{}) vs golden ({},{})",
            entry.velocity.x,
            entry.velocity.y,
            g_vx,
            g_vy
        );
    }
}

fn trace_to_json(trace: &[PhysicsTraceEntry]) -> Vec<serde_json::Value> {
    trace
        .iter()
        .map(|e| {
            serde_json::json!({
                "frame": e.frame,
                "name": e.name,
                "px": e.position.x,
                "py": e.position.y,
                "vx": e.velocity.x,
                "vy": e.velocity.y,
            })
        })
        .collect()
}

// ===========================================================================
// 1. CharacterBody2D slide — golden artifact match
// ===========================================================================

#[test]
fn golden_character_slide_matches_artifact() {
    let (_, trace) = run_traced(make_character_slide(), 20, 1.0 / 60.0);
    let golden = load_golden("character_slide_20frames");
    assert_trace_matches_golden("character_slide", &trace, &golden);
}

#[test]
fn character_slide_deterministic() {
    let (_, t1) = run_traced(make_character_slide(), 20, 1.0 / 60.0);
    let (_, t2) = run_traced(make_character_slide(), 20, 1.0 / 60.0);
    assert_traces_equal("character_slide_determinism", &t1, &t2);
}

#[test]
fn character_slide_player_moves_right() {
    let (_, trace) = run_traced(make_character_slide(), 20, 1.0 / 60.0);
    let player: Vec<_> = trace.iter().filter(|e| e.name == "Player").collect();
    assert!(player.len() >= 2);
    // Player should be moving right across frames
    assert!(
        player.last().unwrap().position.x > player.first().unwrap().position.x,
        "Player must move right over time"
    );
}

#[test]
fn character_slide_floor_immobile() {
    let (_, trace) = run_traced(make_character_slide(), 20, 1.0 / 60.0);
    let floor: Vec<_> = trace.iter().filter(|e| e.name == "Floor").collect();
    for e in &floor {
        assert!(
            approx_eq(e.position.y, 30.0),
            "Floor must stay at y=30: frame {} py={}",
            e.frame,
            e.position.y
        );
    }
}

// ===========================================================================
// 2. Mixed body types — golden artifact match
// ===========================================================================

#[test]
fn golden_mixed_bodies_matches_artifact() {
    let (_, trace) = run_traced(make_mixed_bodies(), 30, 1.0 / 60.0);
    let golden = load_golden("mixed_bodies_30frames");
    assert_trace_matches_golden("mixed_bodies", &trace, &golden);
}

#[test]
fn mixed_bodies_deterministic() {
    let (_, t1) = run_traced(make_mixed_bodies(), 30, 1.0 / 60.0);
    let (_, t2) = run_traced(make_mixed_bodies(), 30, 1.0 / 60.0);
    assert_traces_equal("mixed_bodies_determinism", &t1, &t2);
}

#[test]
fn mixed_bodies_all_three_types_traced() {
    let (_, trace) = run_traced(make_mixed_bodies(), 30, 1.0 / 60.0);
    let names: std::collections::HashSet<_> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("Ball"), "RigidBody2D must appear");
    assert!(names.contains("Floor"), "StaticBody2D must appear");
    assert!(names.contains("Player"), "CharacterBody2D must appear");
}

#[test]
fn mixed_bodies_rigid_falls_toward_floor() {
    let (_, trace) = run_traced(make_mixed_bodies(), 30, 1.0 / 60.0);
    let ball: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();
    assert!(ball.len() >= 2);
    // Ball starts at y=0 moving down at vy=120 — it should move down
    assert!(
        ball.last().unwrap().position.y > ball.first().unwrap().position.y,
        "Ball must fall downward"
    );
}

// ===========================================================================
// 3. Multi-rigid cascade — golden artifact match
// ===========================================================================

#[test]
fn golden_multi_rigid_cascade_matches_artifact() {
    let (_, trace) = run_traced(make_multi_rigid_cascade(), 30, 1.0 / 60.0);
    let golden = load_golden("multi_rigid_cascade_30frames");
    assert_trace_matches_golden("multi_rigid_cascade", &trace, &golden);
}

#[test]
fn multi_rigid_cascade_deterministic() {
    let (_, t1) = run_traced(make_multi_rigid_cascade(), 30, 1.0 / 60.0);
    let (_, t2) = run_traced(make_multi_rigid_cascade(), 30, 1.0 / 60.0);
    assert_traces_equal("multi_rigid_cascade_determinism", &t1, &t2);
}

#[test]
fn multi_rigid_cascade_all_bodies_traced() {
    let (_, trace) = run_traced(make_multi_rigid_cascade(), 30, 1.0 / 60.0);
    let names: std::collections::HashSet<_> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("A"), "body A must appear");
    assert!(names.contains("B"), "body B must appear");
    assert!(names.contains("C"), "body C must appear");
}

#[test]
fn multi_rigid_cascade_c_gains_velocity() {
    let (_, trace) = run_traced(make_multi_rigid_cascade(), 30, 1.0 / 60.0);
    let c_entries: Vec<_> = trace.iter().filter(|e| e.name == "C").collect();
    // C starts stationary — after the cascade it should be moving right
    let last_c = c_entries.last().unwrap();
    assert!(
        last_c.velocity.x > 0.0 || last_c.position.x > 50.0,
        "C must gain rightward motion from cascade: vx={} px={}",
        last_c.velocity.x,
        last_c.position.x
    );
}

// ===========================================================================
// 4. High TPS gravity — golden artifact match
// ===========================================================================

#[test]
fn golden_high_tps_gravity_matches_artifact() {
    let (_, trace) = run_traced_high_tps(make_high_tps_gravity(), 30, 1.0 / 60.0, 120);
    let golden = load_golden("high_tps_gravity_30frames");
    assert_trace_matches_golden("high_tps_gravity", &trace, &golden);
}

#[test]
fn high_tps_gravity_deterministic() {
    let (_, t1) = run_traced_high_tps(make_high_tps_gravity(), 30, 1.0 / 60.0, 120);
    let (_, t2) = run_traced_high_tps(make_high_tps_gravity(), 30, 1.0 / 60.0, 120);
    assert_traces_equal("high_tps_gravity_determinism", &t1, &t2);
}

#[test]
fn high_tps_produces_more_trace_entries_than_standard() {
    // 120 TPS at 1/60 delta = 2 physics ticks per frame → 2 trace entries per frame per body
    let (_, trace_high) = run_traced_high_tps(make_high_tps_gravity(), 30, 1.0 / 60.0, 120);
    let (_, trace_std) = run_traced(make_high_tps_gravity(), 30, 1.0 / 60.0);
    assert!(
        trace_high.len() > trace_std.len(),
        "120 TPS must produce more entries ({}) than 60 TPS ({})",
        trace_high.len(),
        trace_std.len()
    );
}

// ===========================================================================
// 5. Pause-resume — golden artifact match
// ===========================================================================

#[test]
fn golden_pause_resume_matches_artifact() {
    let (_, trace) = run_traced_with_pause(make_pause_resume(), 8, 4, 8, 1.0 / 60.0);
    let golden = load_golden("pause_resume_20frames");
    assert_trace_matches_golden("pause_resume", &trace, &golden);
}

#[test]
fn pause_resume_deterministic() {
    let (_, t1) = run_traced_with_pause(make_pause_resume(), 8, 4, 8, 1.0 / 60.0);
    let (_, t2) = run_traced_with_pause(make_pause_resume(), 8, 4, 8, 1.0 / 60.0);
    assert_traces_equal("pause_resume_determinism", &t1, &t2);
}

#[test]
fn pause_resume_trace_covers_all_frames() {
    let (_, trace) = run_traced_with_pause(make_pause_resume(), 8, 4, 8, 1.0 / 60.0);

    // Total entries = 20 (8 pre + 4 paused + 8 post). The trace records body
    // state every frame regardless of pause state — consistent with Godot
    // behavior where physics traces capture snapshots even when paused.
    assert_eq!(
        trace.len(),
        20,
        "pause-resume: all 20 frames produce trace entries, got {}",
        trace.len()
    );

    // Frame numbers must be continuous 1..20
    for (i, entry) in trace.iter().enumerate() {
        assert_eq!(
            entry.frame,
            (i + 1) as u64,
            "frame numbers must be sequential"
        );
    }
}

#[test]
fn pause_resume_velocity_x_constant_y_accelerates() {
    // Rigid body with no collisions: x-velocity stays constant (no horizontal
    // forces), y-velocity increases due to world gravity (980 px/s² down).
    let (_, trace) = run_traced_with_pause(make_pause_resume(), 8, 4, 8, 1.0 / 60.0);

    for entry in &trace {
        assert!(
            approx_eq(entry.velocity.x, 60.0),
            "x-velocity must remain constant: frame {} vx={}",
            entry.frame,
            entry.velocity.x,
        );
    }

    // y-velocity should be monotonically non-decreasing (gravity accelerates)
    for w in trace.windows(2) {
        assert!(
            w[1].velocity.y >= w[0].velocity.y - EPSILON,
            "y-velocity should increase under gravity: frame {} vy={} >= frame {} vy={}",
            w[1].frame,
            w[1].velocity.y,
            w[0].frame,
            w[0].velocity.y,
        );
    }
}

// ===========================================================================
// Golden generation helper (run with GENERATE_GOLDENS=1 cargo test ...)
// ===========================================================================

#[test]
fn generate_goldens_if_requested() {
    if std::env::var("GENERATE_GOLDENS").is_err() {
        return; // Skip unless explicitly requested
    }

    let golden_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/golden/physics");
    std::fs::create_dir_all(&golden_dir).unwrap();

    // 1. Character slide
    let (_, trace) = run_traced(make_character_slide(), 20, 1.0 / 60.0);
    let json = serde_json::to_string_pretty(&trace_to_json(&trace)).unwrap();
    std::fs::write(golden_dir.join("character_slide_20frames.json"), json).unwrap();

    // 2. Mixed bodies
    let (_, trace) = run_traced(make_mixed_bodies(), 30, 1.0 / 60.0);
    let json = serde_json::to_string_pretty(&trace_to_json(&trace)).unwrap();
    std::fs::write(golden_dir.join("mixed_bodies_30frames.json"), json).unwrap();

    // 3. Multi-rigid cascade
    let (_, trace) = run_traced(make_multi_rigid_cascade(), 30, 1.0 / 60.0);
    let json = serde_json::to_string_pretty(&trace_to_json(&trace)).unwrap();
    std::fs::write(golden_dir.join("multi_rigid_cascade_30frames.json"), json).unwrap();

    // 4. High TPS gravity
    let (_, trace) = run_traced_high_tps(make_high_tps_gravity(), 30, 1.0 / 60.0, 120);
    let json = serde_json::to_string_pretty(&trace_to_json(&trace)).unwrap();
    std::fs::write(golden_dir.join("high_tps_gravity_30frames.json"), json).unwrap();

    // 5. Pause-resume
    let (_, trace) = run_traced_with_pause(make_pause_resume(), 8, 4, 8, 1.0 / 60.0);
    let json = serde_json::to_string_pretty(&trace_to_json(&trace)).unwrap();
    std::fs::write(golden_dir.join("pause_resume_20frames.json"), json).unwrap();

    eprintln!("Generated 5 golden files in {:?}", golden_dir);
}
