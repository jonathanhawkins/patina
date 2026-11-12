//! pat-hunm: physics_playground golden trace fixture comparison.
//!
//! Loads the `physics_playground.tscn` scene, runs it through the MainLoop
//! for 60 frames at 60 TPS, and compares the resulting physics trace
//! directly against the golden baseline at
//! `fixtures/golden/physics/physics_playground_60frames.json`.
//!
//! This is the definitive regression test for the physics playground:
//! any change to physics stepping, body integration, or sync logic that
//! alters trace output will fail this test.

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

// ===========================================================================
// Scene builders
// ===========================================================================

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

/// Load physics_playground.tscn from disk and instance into a SceneTree.
fn load_physics_playground_from_tscn() -> SceneTree {
    let tscn_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/scenes/physics_playground.tscn");
    let source = std::fs::read_to_string(&tscn_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", tscn_path.display()));
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&source).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    tree
}

/// Run a physics simulation and collect the sorted trace.
fn run_traced_physics(
    tree: SceneTree,
    frames: u64,
    delta: f64,
) -> (MainLoop, Vec<PhysicsTraceEntry>) {
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);
    ml.run_frames(frames, delta);
    let mut trace = ml.physics_server().trace().to_vec();
    trace.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
    (ml, trace)
}

/// Golden entry parsed from JSON.
#[derive(Debug)]
struct GoldenEntry {
    name: String,
    frame: u64,
    px: f32,
    py: f32,
    vx: f32,
    vy: f32,
}

/// Load the golden trace JSON from disk, sorted by (frame, name).
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

// ===========================================================================
// 1. Golden baseline comparison — programmatic scene
// ===========================================================================

/// Core acceptance test: runtime trace from the programmatic scene matches
/// the golden baseline fixture exactly (within floating-point tolerance).
#[test]
fn golden_trace_matches_programmatic_scene() {
    let golden = load_golden("physics_playground_60frames.json");
    let (_, trace) = run_traced_physics(make_physics_playground(), FRAMES, DELTA);

    assert_eq!(
        trace.len(),
        golden.len(),
        "trace length {} != golden length {}",
        trace.len(),
        golden.len()
    );

    for (i, (t, g)) in trace.iter().zip(golden.iter()).enumerate() {
        assert_eq!(
            t.name, g.name,
            "entry {i}: name mismatch: '{}' vs '{}'",
            t.name, g.name
        );
        assert_eq!(
            t.frame, g.frame,
            "entry {i} ({}): frame mismatch: {} vs {}",
            t.name, t.frame, g.frame
        );
        assert!(
            approx_eq(t.position.x, g.px) && approx_eq(t.position.y, g.py),
            "entry {i} ({}, frame {}): position drift: ({}, {}) vs golden ({}, {})",
            t.name,
            t.frame,
            t.position.x,
            t.position.y,
            g.px,
            g.py
        );
        assert!(
            approx_eq(t.velocity.x, g.vx) && approx_eq(t.velocity.y, g.vy),
            "entry {i} ({}, frame {}): velocity drift: ({}, {}) vs golden ({}, {})",
            t.name,
            t.frame,
            t.velocity.x,
            t.velocity.y,
            g.vx,
            g.vy
        );
    }
}

// ===========================================================================
// 2. Golden baseline comparison — tscn-loaded scene
// ===========================================================================

/// The .tscn-loaded scene must produce the same trace as the golden baseline.
#[test]
fn golden_trace_matches_tscn_scene() {
    let golden = load_golden("physics_playground_60frames.json");
    let (_, trace) = run_traced_physics(load_physics_playground_from_tscn(), FRAMES, DELTA);

    assert_eq!(
        trace.len(),
        golden.len(),
        "tscn trace length {} != golden length {}",
        trace.len(),
        golden.len()
    );

    for (i, (t, g)) in trace.iter().zip(golden.iter()).enumerate() {
        assert_eq!(t.name, g.name, "entry {i}: name mismatch");
        assert_eq!(t.frame, g.frame, "entry {i} ({}): frame mismatch", t.name);
        assert!(
            approx_eq(t.position.x, g.px) && approx_eq(t.position.y, g.py),
            "entry {i} ({}, frame {}): position drift: ({}, {}) vs golden ({}, {})",
            t.name,
            t.frame,
            t.position.x,
            t.position.y,
            g.px,
            g.py
        );
        assert!(
            approx_eq(t.velocity.x, g.vx) && approx_eq(t.velocity.y, g.vy),
            "entry {i} ({}, frame {}): velocity drift: ({}, {}) vs golden ({}, {})",
            t.name,
            t.frame,
            t.velocity.x,
            t.velocity.y,
            g.vx,
            g.vy
        );
    }
}

// ===========================================================================
// 3. Programmatic and tscn produce identical traces
// ===========================================================================

#[test]
fn programmatic_and_tscn_traces_identical() {
    let (_, trace_prog) = run_traced_physics(make_physics_playground(), FRAMES, DELTA);
    let (_, trace_tscn) = run_traced_physics(load_physics_playground_from_tscn(), FRAMES, DELTA);

    assert_eq!(
        trace_prog.len(),
        trace_tscn.len(),
        "trace lengths differ: prog={} vs tscn={}",
        trace_prog.len(),
        trace_tscn.len()
    );

    for (i, (p, t)) in trace_prog.iter().zip(trace_tscn.iter()).enumerate() {
        assert_eq!(p.name, t.name, "entry {i}: name mismatch");
        assert_eq!(p.frame, t.frame, "entry {i}: frame mismatch");
        assert!(
            approx_eq(p.position.x, t.position.x) && approx_eq(p.position.y, t.position.y),
            "entry {i} ({}, frame {}): position drift: prog=({}, {}) tscn=({}, {})",
            p.name,
            p.frame,
            p.position.x,
            p.position.y,
            t.position.x,
            t.position.y
        );
        assert!(
            approx_eq(p.velocity.x, t.velocity.x) && approx_eq(p.velocity.y, t.velocity.y),
            "entry {i} ({}, frame {}): velocity drift: prog=({}, {}) tscn=({}, {})",
            p.name,
            p.frame,
            p.velocity.x,
            p.velocity.y,
            t.velocity.x,
            t.velocity.y
        );
    }
}

// ===========================================================================
// 4. Golden file structural validation
// ===========================================================================

/// Verify the golden file has the expected structure: 3 bodies × 60 frames.
#[test]
fn golden_file_structure_valid() {
    let golden = load_golden("physics_playground_60frames.json");

    // 3 bodies (Ball, Wall, Floor) × 60 frames = 180 entries.
    assert_eq!(
        golden.len(),
        180,
        "expected 180 entries (3 bodies × 60 frames)"
    );

    // All expected bodies present.
    let names: std::collections::HashSet<&str> = golden.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("Ball"), "Ball must be in golden");
    assert!(names.contains("Wall"), "Wall must be in golden");
    assert!(names.contains("Floor"), "Floor must be in golden");

    // Each body has exactly 60 entries.
    for name in ["Ball", "Wall", "Floor"] {
        let count = golden.iter().filter(|e| e.name == name).count();
        assert_eq!(count, 60, "{name} should have 60 entries, got {count}");
    }

    // Frames span 1..=60.
    let frames: std::collections::HashSet<u64> = golden.iter().map(|e| e.frame).collect();
    for f in 1..=60u64 {
        assert!(frames.contains(&f), "golden missing frame {f}");
    }
}

// ===========================================================================
// 5. Static bodies remain stationary in golden
// ===========================================================================

/// Golden fixture contract: Wall and Floor (static bodies) never move.
#[test]
fn golden_static_bodies_stationary() {
    let golden = load_golden("physics_playground_60frames.json");

    for entry in golden.iter().filter(|e| e.name == "Wall") {
        assert!(
            approx_eq(entry.px, 800.0) && approx_eq(entry.py, 300.0),
            "Wall should stay at (800, 300), got ({}, {}) at frame {}",
            entry.px,
            entry.py,
            entry.frame
        );
        assert!(
            approx_eq(entry.vx, 0.0) && approx_eq(entry.vy, 0.0),
            "Wall velocity should be zero, got ({}, {}) at frame {}",
            entry.vx,
            entry.vy,
            entry.frame
        );
    }

    for entry in golden.iter().filter(|e| e.name == "Floor") {
        assert!(
            approx_eq(entry.px, 400.0) && approx_eq(entry.py, 600.0),
            "Floor should stay at (400, 600), got ({}, {}) at frame {}",
            entry.px,
            entry.py,
            entry.frame
        );
        assert!(
            approx_eq(entry.vx, 0.0) && approx_eq(entry.vy, 0.0),
            "Floor velocity should be zero, got ({}, {}) at frame {}",
            entry.vx,
            entry.vy,
            entry.frame
        );
    }
}

// ===========================================================================
// 6. Ball initial conditions match scene definition
// ===========================================================================

#[test]
fn golden_ball_initial_conditions() {
    let golden = load_golden("physics_playground_60frames.json");

    let ball_frame1 = golden
        .iter()
        .find(|e| e.name == "Ball" && e.frame == 1)
        .expect("Ball at frame 1");

    // Ball starts at X=400 per the .tscn. Y has already moved slightly from
    // gravity after one physics step (frame 1 is post-step, not pre-step).
    assert!(
        approx_eq(ball_frame1.px, 400.0),
        "Ball initial X should be 400, got {}",
        ball_frame1.px
    );
    // Y should be near 100 — may be exactly 100 if gravity hasn't displaced
    // yet at frame 1, or slightly above after one integration step.
    assert!(
        ball_frame1.py >= 100.0 && ball_frame1.py < 110.0,
        "Ball Y at frame 1 should be near 100 (post-step), got {}",
        ball_frame1.py
    );
}

// ===========================================================================
// 7. Determinism: two fresh runs produce identical traces
// ===========================================================================

#[test]
fn golden_comparison_deterministic() {
    let (_, trace1) = run_traced_physics(make_physics_playground(), FRAMES, DELTA);
    let (_, trace2) = run_traced_physics(make_physics_playground(), FRAMES, DELTA);

    assert_eq!(trace1.len(), trace2.len());
    for (i, (a, b)) in trace1.iter().zip(trace2.iter()).enumerate() {
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
// 8. Tscn regression golden comparison
// ===========================================================================

/// Compare runtime tscn trace against the dedicated tscn regression golden.
#[test]
fn tscn_regression_golden_matches() {
    let golden = load_golden("physics_playground_tscn_regression.json");
    let (_, trace) = run_traced_physics(load_physics_playground_from_tscn(), FRAMES, DELTA);

    assert_eq!(
        trace.len(),
        golden.len(),
        "tscn regression: trace length {} != golden {}",
        trace.len(),
        golden.len()
    );

    for (i, (t, g)) in trace.iter().zip(golden.iter()).enumerate() {
        assert_eq!(t.name, g.name, "entry {i}: name");
        assert_eq!(t.frame, g.frame, "entry {i}: frame");
        assert!(
            approx_eq(t.position.x, g.px) && approx_eq(t.position.y, g.py),
            "entry {i} ({}, frame {}): position mismatch vs tscn regression golden",
            t.name,
            t.frame
        );
    }
}

// ===========================================================================
// 9. Frame count and body count validation
// ===========================================================================

#[test]
fn playground_frame_count_and_body_registration() {
    let (ml, trace) = run_traced_physics(make_physics_playground(), FRAMES, DELTA);

    assert_eq!(ml.frame_count(), FRAMES, "should run exactly 60 frames");
    assert_eq!(
        ml.physics_server().body_count(),
        3,
        "should register 3 bodies (Ball, Wall, Floor)"
    );

    // Trace should have entries for all 60 frames.
    let max_frame = trace.iter().map(|e| e.frame).max().unwrap_or(0);
    assert_eq!(max_frame, FRAMES, "trace should cover frame {FRAMES}");
}

// ===========================================================================
// 10. Trace frame numbers are contiguous
// ===========================================================================

#[test]
fn trace_frame_numbers_contiguous() {
    let (_, trace) = run_traced_physics(make_physics_playground(), FRAMES, DELTA);

    let ball_frames: Vec<u64> = trace
        .iter()
        .filter(|e| e.name == "Ball")
        .map(|e| e.frame)
        .collect();

    assert_eq!(ball_frames.len(), FRAMES as usize);
    for (i, &f) in ball_frames.iter().enumerate() {
        assert_eq!(
            f,
            (i + 1) as u64,
            "Ball frame {} should be {}, got {}",
            i,
            i + 1,
            f
        );
    }
}
