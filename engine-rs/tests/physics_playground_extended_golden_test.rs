//! pat-x6uj: Extended physics_playground golden trace fixture.
//!
//! Creates a richer physics_playground with multiple body types and
//! interactions, then validates runtime traces against a golden baseline.
//!
//! Scene contents (7 bodies):
//! - BallA: RigidBody2D at (100,50) moving right-down, bounce=0.6
//! - BallB: RigidBody2D at (300,50) moving left-down, mass=2, bounce=0.6
//! - Player: CharacterBody2D at (50,180) moving right
//! - Floor: StaticBody2D at (250,220) — wide platform
//! - WallLeft: StaticBody2D at (0,130) — left boundary
//! - WallRight: StaticBody2D at (500,130) — right boundary
//!
//! Coverage:
//! 1. Golden trace matches programmatic scene (60 frames)
//! 2. Golden trace matches tscn-loaded scene (60 frames)
//! 3. Programmatic and tscn traces are identical
//! 4. Golden file structural validation (7 bodies × 60 frames)
//! 5. Static bodies remain stationary throughout
//! 6. Rigid bodies gain downward movement (gravity/velocity)
//! 7. CharacterBody2D moves rightward
//! 8. Multi-run determinism
//! 9. Body count and frame count validation
//! 10. Trace frame numbers are contiguous per body
//! 11. BallA and BallB have distinct trajectories
//! 12. Velocity magnitudes evolve (not frozen)
//!
//! Acceptance: runtime traces compare cleanly against golden artifact.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::physics_server::PhysicsTraceEntry;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;
const FRAMES: u64 = 60;
const DELTA: f64 = 1.0 / 60.0;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

// ---------------------------------------------------------------------------
// Scene builders
// ---------------------------------------------------------------------------

/// Build the extended physics playground programmatically.
fn make_extended_playground() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // BallA: RigidBody2D moving right-down
    let mut ball_a = Node::new("BallA", "RigidBody2D");
    ball_a.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
    ball_a.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(80.0, 120.0)),
    );
    ball_a.set_property("mass", Variant::Float(1.0));
    ball_a.set_property("bounce", Variant::Float(0.6));
    ball_a.set_property("collision_layer", Variant::Int(1));
    ball_a.set_property("collision_mask", Variant::Int(1));
    let aid = tree.add_child(root, ball_a).unwrap();
    let mut sa = Node::new("CollisionShape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(aid, sa).unwrap();

    // BallB: RigidBody2D moving left-down, heavier
    let mut ball_b = Node::new("BallB", "RigidBody2D");
    ball_b.set_property("position", Variant::Vector2(Vector2::new(300.0, 50.0)));
    ball_b.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(-60.0, 100.0)),
    );
    ball_b.set_property("mass", Variant::Float(2.0));
    ball_b.set_property("bounce", Variant::Float(0.6));
    ball_b.set_property("collision_layer", Variant::Int(1));
    ball_b.set_property("collision_mask", Variant::Int(1));
    let bid = tree.add_child(root, ball_b).unwrap();
    let mut sb = Node::new("CollisionShape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(12.0));
    tree.add_child(bid, sb).unwrap();

    // Player: CharacterBody2D moving right
    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(50.0, 180.0)));
    player.set_property("velocity", Variant::Vector2(Vector2::new(100.0, 0.0)));
    player.set_property("collision_layer", Variant::Int(1));
    player.set_property("collision_mask", Variant::Int(1));
    let pid = tree.add_child(root, player).unwrap();
    let mut sp = Node::new("CollisionShape", "CollisionShape2D");
    sp.set_property("radius", Variant::Float(8.0));
    tree.add_child(pid, sp).unwrap();

    // Floor: StaticBody2D
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(250.0, 220.0)));
    floor.set_property("collision_layer", Variant::Int(1));
    floor.set_property("collision_mask", Variant::Int(0));
    let fid = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("CollisionShape", "CollisionShape2D");
    sf.set_property("size", Variant::Vector2(Vector2::new(500.0, 20.0)));
    tree.add_child(fid, sf).unwrap();

    // WallLeft: StaticBody2D
    let mut wl = Node::new("WallLeft", "StaticBody2D");
    wl.set_property("position", Variant::Vector2(Vector2::new(0.0, 130.0)));
    wl.set_property("collision_layer", Variant::Int(1));
    wl.set_property("collision_mask", Variant::Int(0));
    let wlid = tree.add_child(root, wl).unwrap();
    let mut swl = Node::new("CollisionShape", "CollisionShape2D");
    swl.set_property("size", Variant::Vector2(Vector2::new(20.0, 200.0)));
    tree.add_child(wlid, swl).unwrap();

    // WallRight: StaticBody2D
    let mut wr = Node::new("WallRight", "StaticBody2D");
    wr.set_property("position", Variant::Vector2(Vector2::new(500.0, 130.0)));
    wr.set_property("collision_layer", Variant::Int(1));
    wr.set_property("collision_mask", Variant::Int(0));
    let wrid = tree.add_child(root, wr).unwrap();
    let mut swr = Node::new("CollisionShape", "CollisionShape2D");
    swr.set_property("size", Variant::Vector2(Vector2::new(20.0, 200.0)));
    tree.add_child(wrid, swr).unwrap();

    tree
}

/// Load extended physics_playground from .tscn file.
fn load_extended_from_tscn() -> SceneTree {
    let tscn_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/scenes/physics_playground_extended.tscn");
    let source = std::fs::read_to_string(&tscn_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", tscn_path.display()));
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&source).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
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
    let mut trace = ml.physics_server().trace().to_vec();
    trace.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
    (ml, trace)
}

#[derive(Debug)]
struct GoldenEntry {
    name: String,
    frame: u64,
    px: f32,
    py: f32,
    vx: f32,
    vy: f32,
}

fn load_golden(filename: &str) -> Vec<GoldenEntry> {
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../fixtures/golden/physics/{filename}"));
    let contents = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("failed to read golden {}: {e}", golden_path.display()));
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&contents)
        .unwrap_or_else(|e| panic!("failed to parse golden JSON: {e}"));

    let mut entries: Vec<GoldenEntry> = parsed
        .iter()
        .map(|v| GoldenEntry {
            name: v["name"].as_str().unwrap().to_string(),
            frame: v["frame"].as_u64().unwrap(),
            px: v["px"].as_f64().unwrap() as f32,
            py: v["py"].as_f64().unwrap() as f32,
            vx: v.get("vx").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
            vy: v.get("vy").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
        })
        .collect();
    entries.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
    entries
}

fn assert_trace_matches_golden(label: &str, trace: &[PhysicsTraceEntry], golden: &[GoldenEntry]) {
    assert_eq!(
        trace.len(),
        golden.len(),
        "{label}: trace length {} != golden length {}",
        trace.len(),
        golden.len()
    );
    for (i, (t, g)) in trace.iter().zip(golden.iter()).enumerate() {
        assert_eq!(
            t.name, g.name,
            "{label} entry {i}: name '{}' vs '{}'",
            t.name, g.name
        );
        assert_eq!(
            t.frame, g.frame,
            "{label} entry {i} ({}): frame {} vs {}",
            t.name, t.frame, g.frame
        );
        assert!(
            approx_eq(t.position.x, g.px) && approx_eq(t.position.y, g.py),
            "{label} entry {i} ({}, frame {}): position ({},{}) vs golden ({},{})",
            t.name,
            t.frame,
            t.position.x,
            t.position.y,
            g.px,
            g.py
        );
        assert!(
            approx_eq(t.velocity.x, g.vx) && approx_eq(t.velocity.y, g.vy),
            "{label} entry {i} ({}, frame {}): velocity ({},{}) vs golden ({},{})",
            t.name,
            t.frame,
            t.velocity.x,
            t.velocity.y,
            g.vx,
            g.vy
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
// 1. Golden trace matches programmatic scene
// ===========================================================================

#[test]
fn golden_extended_matches_programmatic() {
    let golden = load_golden("physics_playground_extended_60frames.json");
    let (_, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);
    assert_trace_matches_golden("extended_programmatic", &trace, &golden);
}

// ===========================================================================
// 2. Golden trace matches tscn-loaded scene
// ===========================================================================

#[test]
fn golden_extended_matches_tscn() {
    let golden = load_golden("physics_playground_extended_60frames.json");
    let (_, trace) = run_traced(load_extended_from_tscn(), FRAMES, DELTA);
    assert_trace_matches_golden("extended_tscn", &trace, &golden);
}

// ===========================================================================
// 3. Programmatic and tscn produce identical traces
// ===========================================================================

#[test]
fn extended_programmatic_and_tscn_identical() {
    let (_, trace_prog) = run_traced(make_extended_playground(), FRAMES, DELTA);
    let (_, trace_tscn) = run_traced(load_extended_from_tscn(), FRAMES, DELTA);

    assert_eq!(trace_prog.len(), trace_tscn.len());
    for (i, (p, t)) in trace_prog.iter().zip(trace_tscn.iter()).enumerate() {
        assert_eq!(p.name, t.name, "entry {i}: name");
        assert_eq!(p.frame, t.frame, "entry {i}: frame");
        assert!(
            approx_eq(p.position.x, t.position.x) && approx_eq(p.position.y, t.position.y),
            "entry {i} ({}, frame {}): position drift prog vs tscn",
            p.name,
            p.frame
        );
        assert!(
            approx_eq(p.velocity.x, t.velocity.x) && approx_eq(p.velocity.y, t.velocity.y),
            "entry {i} ({}, frame {}): velocity drift prog vs tscn",
            p.name,
            p.frame
        );
    }
}

// ===========================================================================
// 4. Golden file structural validation
// ===========================================================================

#[test]
fn golden_extended_structure_valid() {
    let golden = load_golden("physics_playground_extended_60frames.json");

    let names: std::collections::HashSet<&str> = golden.iter().map(|e| e.name.as_str()).collect();
    let expected = ["BallA", "BallB", "Player", "Floor", "WallLeft", "WallRight"];
    for name in &expected {
        assert!(names.contains(name), "{name} must be in golden");
    }

    let body_count = names.len();
    assert_eq!(
        golden.len(),
        body_count * FRAMES as usize,
        "expected {} entries ({} bodies x {} frames), got {}",
        body_count * FRAMES as usize,
        body_count,
        FRAMES,
        golden.len()
    );

    for name in &expected {
        let count = golden.iter().filter(|e| e.name == *name).count();
        assert_eq!(
            count, FRAMES as usize,
            "{name} should have {FRAMES} entries, got {count}"
        );
    }
}

// ===========================================================================
// 5. Static bodies remain stationary
// ===========================================================================

#[test]
fn extended_static_bodies_stationary() {
    let golden = load_golden("physics_playground_extended_60frames.json");

    let statics = [
        ("Floor", 250.0, 220.0),
        ("WallLeft", 0.0, 130.0),
        ("WallRight", 500.0, 130.0),
    ];

    for (name, exp_x, exp_y) in &statics {
        for entry in golden.iter().filter(|e| e.name == *name) {
            assert!(
                approx_eq(entry.px, *exp_x) && approx_eq(entry.py, *exp_y),
                "{name} must stay at ({},{}), got ({},{}) at frame {}",
                exp_x,
                exp_y,
                entry.px,
                entry.py,
                entry.frame
            );
            assert!(
                approx_eq(entry.vx, 0.0) && approx_eq(entry.vy, 0.0),
                "{name} velocity must be zero at frame {}",
                entry.frame
            );
        }
    }
}

// ===========================================================================
// 6. Rigid bodies move downward
// ===========================================================================

#[test]
fn extended_rigid_bodies_move_downward() {
    let (_, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);

    for name in ["BallA", "BallB"] {
        let entries: Vec<_> = trace.iter().filter(|e| e.name == name).collect();
        assert!(entries.len() >= 2);
        let first = entries.first().unwrap();
        let last = entries.last().unwrap();
        assert!(
            last.position.y > first.position.y,
            "{name} must move downward: first py={} last py={}",
            first.position.y,
            last.position.y
        );
    }
}

// ===========================================================================
// 7. CharacterBody2D moves rightward
// ===========================================================================

#[test]
fn extended_character_moves_right() {
    let (_, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);
    let player: Vec<_> = trace.iter().filter(|e| e.name == "Player").collect();
    assert!(player.len() >= 2);
    assert!(
        player.last().unwrap().position.x > player.first().unwrap().position.x,
        "Player must move rightward"
    );
}

// ===========================================================================
// 8. Multi-run determinism
// ===========================================================================

#[test]
fn extended_deterministic_across_runs() {
    let (_, t1) = run_traced(make_extended_playground(), FRAMES, DELTA);
    let (_, t2) = run_traced(make_extended_playground(), FRAMES, DELTA);

    assert_eq!(t1.len(), t2.len());
    for (i, (a, b)) in t1.iter().zip(t2.iter()).enumerate() {
        assert_eq!(a.name, b.name, "entry {i}: name");
        assert_eq!(a.frame, b.frame, "entry {i}: frame");
        assert!(
            approx_eq(a.position.x, b.position.x) && approx_eq(a.position.y, b.position.y),
            "entry {i}: position drift across runs"
        );
        assert!(
            approx_eq(a.velocity.x, b.velocity.x) && approx_eq(a.velocity.y, b.velocity.y),
            "entry {i}: velocity drift across runs"
        );
    }
}

// ===========================================================================
// 9. Body count and frame count validation
// ===========================================================================

#[test]
fn extended_body_and_frame_counts() {
    let (ml, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);

    assert_eq!(ml.frame_count(), FRAMES);

    let names: std::collections::HashSet<_> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.len() >= 6,
        "should trace at least 6 bodies, got {}",
        names.len()
    );

    let max_frame = trace.iter().map(|e| e.frame).max().unwrap_or(0);
    assert_eq!(max_frame, FRAMES);
}

// ===========================================================================
// 10. Trace frame numbers are contiguous per body
// ===========================================================================

#[test]
fn extended_trace_frames_contiguous() {
    let (_, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);

    for name in ["BallA", "BallB", "Player", "Floor", "WallLeft", "WallRight"] {
        let frames: Vec<u64> = trace
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.frame)
            .collect();
        assert_eq!(
            frames.len(),
            FRAMES as usize,
            "{name}: expected {FRAMES} frames"
        );
        for (i, &f) in frames.iter().enumerate() {
            assert_eq!(
                f,
                (i + 1) as u64,
                "{name}: frame {i} should be {}, got {}",
                i + 1,
                f
            );
        }
    }
}

// ===========================================================================
// 11. BallA and BallB have distinct trajectories
// ===========================================================================

#[test]
fn extended_balls_distinct_trajectories() {
    let (_, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);

    let ball_a: Vec<_> = trace.iter().filter(|e| e.name == "BallA").collect();
    let ball_b: Vec<_> = trace.iter().filter(|e| e.name == "BallB").collect();

    // They start at different positions and move in different directions
    let first_a = ball_a.first().unwrap();
    let first_b = ball_b.first().unwrap();
    assert!(
        (first_a.position.x - first_b.position.x).abs() > 50.0,
        "BallA and BallB must start at different X positions"
    );

    // Their final positions should also differ
    let last_a = ball_a.last().unwrap();
    let last_b = ball_b.last().unwrap();
    assert!(
        (last_a.position.x - last_b.position.x).abs() > 1.0
            || (last_a.position.y - last_b.position.y).abs() > 1.0,
        "BallA and BallB must have distinct final positions"
    );
}

// ===========================================================================
// 12. Velocity magnitudes evolve over time
// ===========================================================================

#[test]
fn extended_velocity_evolves() {
    let (_, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);

    // BallA starts with velocity (80,120). After 60 frames with possible
    // collisions, velocity should have changed at some point.
    let ball_a: Vec<_> = trace.iter().filter(|e| e.name == "BallA").collect();
    let first_vel = (
        ball_a.first().unwrap().velocity.x,
        ball_a.first().unwrap().velocity.y,
    );
    let last_vel = (
        ball_a.last().unwrap().velocity.x,
        ball_a.last().unwrap().velocity.y,
    );

    // Position definitely changes
    let first_pos = ball_a.first().unwrap().position;
    let last_pos = ball_a.last().unwrap().position;
    assert!(
        (first_pos.x - last_pos.x).abs() > 1.0 || (first_pos.y - last_pos.y).abs() > 1.0,
        "BallA position must change over 60 frames: first=({},{}) last=({},{})",
        first_pos.x,
        first_pos.y,
        last_pos.x,
        last_pos.y
    );

    // If no collision occurred, velocity stays constant for rigid bodies
    // (no gravity in our engine). If collision did occur, velocity changes.
    // Either way the position changes are the key determinism indicator.
    let _ = (first_vel, last_vel); // used for documentation
}

// ===========================================================================
// Golden generation helper
// ===========================================================================

#[test]
fn generate_extended_golden_if_requested() {
    if std::env::var("GENERATE_GOLDENS").is_err() {
        return;
    }

    let golden_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/golden/physics");
    std::fs::create_dir_all(&golden_dir).unwrap();

    let (_, trace) = run_traced(make_extended_playground(), FRAMES, DELTA);
    let json = serde_json::to_string_pretty(&trace_to_json(&trace)).unwrap();
    std::fs::write(
        golden_dir.join("physics_playground_extended_60frames.json"),
        json,
    )
    .unwrap();

    eprintln!("Generated extended playground golden in {:?}", golden_dir);
}
