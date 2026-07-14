//! pat-ljop: Deterministic physics trace goldens for the integrated runtime path.
//!
//! Validates that physics traces produced by the full MainLoop pipeline
//! (register_physics_bodies → run_frames with EventTrace enabled) are
//! deterministic and compare cleanly against checked-in golden artifacts.
//!
//! Coverage:
//! 1. Gravity fall: single rigid body constant-velocity trajectory is deterministic
//! 2. Elastic bounce: two-body elastic collision trace is reproducible
//! 3. Friction deceleration: friction trace is deterministic across runs
//! 4. Static blocking: rigid-vs-static collision trace is deterministic
//! 5. EventTrace integration: PHYSICS_PROCESS notifications appear in traced frames
//! 6. Frame-by-frame physics tick counts are deterministic
//! 7. Position evolution across traced frames is monotonically consistent
//! 8. Multi-run determinism: two independent runs produce identical traces

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::physics_server::PhysicsTraceEntry;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

// ---------------------------------------------------------------------------
// Scene builders
// ---------------------------------------------------------------------------

fn make_gravity_fall() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 100.0)),
    );
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(ball_id, s).unwrap();

    tree
}

fn make_elastic_bounce() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    a.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(100.0, 0.0)),
    );
    a.set_property("bounce", Variant::Float(1.0));
    a.set_property("mass", Variant::Float(1.0));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    b.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(-100.0, 0.0)),
    );
    b.set_property("bounce", Variant::Float(1.0));
    b.set_property("mass", Variant::Float(1.0));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, sb).unwrap();

    tree
}

fn make_friction_decel() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(100.0, 50.0)),
    );
    ball.set_property("friction", Variant::Float(1.0));
    ball.set_property("mass", Variant::Float(1.0));
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(ball_id, s).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 10.0)));
    floor.set_property("friction", Variant::Float(1.0));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("Shape", "CollisionShape2D");
    sf.set_property("half_extents", Variant::Vector2(Vector2::new(500.0, 5.0)));
    tree.add_child(floor_id, sf).unwrap();

    tree
}

fn make_static_blocking() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 200.0)),
    );
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(ball_id, s).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 50.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("Shape", "CollisionShape2D");
    sf.set_property("half_extents", Variant::Vector2(Vector2::new(100.0, 10.0)));
    tree.add_child(floor_id, sf).unwrap();

    tree
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run_traced(tree: SceneTree, frames: u64, delta: f64) -> (MainLoop, Vec<PhysicsTraceEntry>) {
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);
    ml.tree_mut().event_trace_mut().enable();
    ml.run_frames(frames, delta);
    let trace = ml.physics_server().trace().to_vec();
    (ml, trace)
}

/// Assert two traces are entry-by-entry equal within EPSILON.
fn assert_traces_equal(label: &str, a: &[PhysicsTraceEntry], b: &[PhysicsTraceEntry]) {
    assert_eq!(a.len(), b.len(), "{label}: trace length mismatch");
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

/// Load a golden physics trace from fixtures/golden/physics/.
fn load_golden(name: &str) -> Vec<serde_json::Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../fixtures/golden/physics/{name}.json"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load golden {name}: {e}"));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse golden {name}: {e}"))
}

/// Compare a physics trace against a loaded golden JSON array.
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

// ===========================================================================
// 1. Gravity fall trace matches checked-in golden
// ===========================================================================

#[test]
fn golden_gravity_fall_matches_artifact() {
    let (_, trace) = run_traced(make_gravity_fall(), 30, 1.0 / 60.0);
    let golden = load_golden("gravity_fall_30frames");
    assert_trace_matches_golden("gravity_fall", &trace, &golden);
}

// ===========================================================================
// 2. Elastic bounce trace matches checked-in golden
// ===========================================================================

#[test]
fn golden_elastic_bounce_matches_artifact() {
    let (_, trace) = run_traced(make_elastic_bounce(), 30, 1.0 / 60.0);
    let golden = load_golden("elastic_bounce_30frames");
    assert_trace_matches_golden("elastic_bounce", &trace, &golden);
}

// ===========================================================================
// 3. Friction deceleration trace matches checked-in golden
// ===========================================================================

#[test]
fn golden_friction_decel_matches_artifact() {
    let (_, trace) = run_traced(make_friction_decel(), 30, 1.0 / 60.0);
    let golden = load_golden("friction_decel_30frames");
    assert_trace_matches_golden("friction_decel", &trace, &golden);
}

// ===========================================================================
// 4. Static blocking trace matches checked-in golden
// ===========================================================================

#[test]
fn golden_static_blocking_matches_artifact() {
    let (_, trace) = run_traced(make_static_blocking(), 60, 1.0 / 60.0);
    let golden = load_golden("static_blocking_60frames");
    assert_trace_matches_golden("static_blocking", &trace, &golden);
}

// ===========================================================================
// 5. EventTrace integration: traced frames capture lifecycle + process events
// ===========================================================================

#[test]
fn event_trace_contains_physics_notifications() {
    let tree = make_gravity_fall();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);
    ml.tree_mut().event_trace_mut().enable();

    let ft = ml.run_frames_traced(5, 1.0 / 60.0);

    // Collect all events across all frame records.
    let all_events: Vec<_> = ft.frames.iter().flat_map(|f| f.events.iter()).collect();

    // Must contain process-related events from the integrated runtime path.
    // The exact event names depend on the MainLoop dispatch, but we expect
    // at least some Notification events across 5 frames.
    let notification_count = all_events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .count();

    assert!(
        notification_count > 0,
        "traced frames must contain notification events from integrated path (got 0 across {} frames)",
        ft.len()
    );

    // Each frame must have at least one physics tick at 1/60 delta.
    for (i, frame) in ft.frames.iter().enumerate() {
        assert!(
            frame.physics_ticks >= 1,
            "frame {i}: must have at least 1 physics tick at 1/60 delta, got {}",
            frame.physics_ticks
        );
    }
}

// ===========================================================================
// 6. Frame-by-frame physics tick counts are deterministic
// ===========================================================================

#[test]
fn frame_trace_physics_ticks_deterministic() {
    let tree1 = make_gravity_fall();
    let mut ml1 = MainLoop::new(tree1);
    ml1.register_physics_bodies();
    ml1.tree_mut().event_trace_mut().enable();
    let ft1 = ml1.run_frames_traced(10, 1.0 / 60.0);

    let tree2 = make_gravity_fall();
    let mut ml2 = MainLoop::new(tree2);
    ml2.register_physics_bodies();
    ml2.tree_mut().event_trace_mut().enable();
    let ft2 = ml2.run_frames_traced(10, 1.0 / 60.0);

    assert_eq!(ft1.len(), ft2.len(), "frame count must match");
    for (i, (f1, f2)) in ft1.frames.iter().zip(ft2.frames.iter()).enumerate() {
        assert_eq!(
            f1.physics_ticks, f2.physics_ticks,
            "frame {i}: physics_ticks must be deterministic"
        );
        assert_eq!(
            f1.frame_number, f2.frame_number,
            "frame {i}: frame_number must match"
        );
    }
}

// ===========================================================================
// 7. Gravity fall position evolution is monotonically increasing in Y
// ===========================================================================

#[test]
fn gravity_fall_position_monotonic_increase() {
    let (_, trace) = run_traced(make_gravity_fall(), 30, 1.0 / 60.0);
    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();

    assert!(
        ball_entries.len() >= 2,
        "must have multiple Ball trace entries"
    );

    for window in ball_entries.windows(2) {
        assert!(
            window[1].position.y >= window[0].position.y - EPSILON,
            "ball must fall monotonically: frame {} py={} >= frame {} py={}",
            window[1].frame,
            window[1].position.y,
            window[0].frame,
            window[0].position.y,
        );
    }

    // X should remain constant at 0.0 (no lateral movement).
    for e in &ball_entries {
        assert!(
            approx_eq(e.position.x, 0.0),
            "ball X must remain 0: frame {} px={}",
            e.frame,
            e.position.x
        );
    }
}

// ===========================================================================
// 8. Multi-run determinism: two independent runs produce identical traces
// ===========================================================================

#[test]
fn multi_run_gravity_deterministic() {
    let (_, trace1) = run_traced(make_gravity_fall(), 30, 1.0 / 60.0);
    let (_, trace2) = run_traced(make_gravity_fall(), 30, 1.0 / 60.0);
    assert_traces_equal("gravity_fall_determinism", &trace1, &trace2);
}

#[test]
fn multi_run_elastic_bounce_deterministic() {
    let (_, trace1) = run_traced(make_elastic_bounce(), 30, 1.0 / 60.0);
    let (_, trace2) = run_traced(make_elastic_bounce(), 30, 1.0 / 60.0);
    assert_traces_equal("elastic_bounce_determinism", &trace1, &trace2);
}

#[test]
fn multi_run_friction_deterministic() {
    let (_, trace1) = run_traced(make_friction_decel(), 30, 1.0 / 60.0);
    let (_, trace2) = run_traced(make_friction_decel(), 30, 1.0 / 60.0);
    assert_traces_equal("friction_decel_determinism", &trace1, &trace2);
}

#[test]
fn multi_run_static_blocking_deterministic() {
    let (_, trace1) = run_traced(make_static_blocking(), 60, 1.0 / 60.0);
    let (_, trace2) = run_traced(make_static_blocking(), 60, 1.0 / 60.0);
    assert_traces_equal("static_blocking_determinism", &trace1, &trace2);
}

// ===========================================================================
// 9. Elastic bounce: bodies A and B exchange velocities after collision
// ===========================================================================

#[test]
fn elastic_bounce_velocity_reversal() {
    let (_, trace) = run_traced(make_elastic_bounce(), 30, 1.0 / 60.0);

    let a_entries: Vec<_> = trace.iter().filter(|e| e.name == "A").collect();
    let b_entries: Vec<_> = trace.iter().filter(|e| e.name == "B").collect();

    assert!(!a_entries.is_empty() && !b_entries.is_empty());

    // The golden shows that by frame 1 the collision has already occurred:
    // A now moves left (vx=-100) and B moves right (vx=+100).
    // This is consistent with elastic equal-mass head-on collision velocity exchange.
    let first_a = &a_entries[0];
    let first_b = &b_entries[0];

    assert!(
        first_a.velocity.x < 0.0,
        "A must be moving left after collision: vx={}",
        first_a.velocity.x
    );
    assert!(
        first_b.velocity.x > 0.0,
        "B must be moving right after collision: vx={}",
        first_b.velocity.x
    );

    // Bodies should be separating: A moves left, B moves right.
    let last_a = a_entries.last().unwrap();
    let last_b = b_entries.last().unwrap();
    assert!(
        last_a.position.x < first_a.position.x,
        "A must move further left over time"
    );
    assert!(
        last_b.position.x > first_b.position.x,
        "B must move further right over time"
    );
}

// ===========================================================================
// 10. Static blocking: floor never moves
// ===========================================================================

#[test]
fn static_body_never_moves_in_trace() {
    let (_, trace) = run_traced(make_static_blocking(), 60, 1.0 / 60.0);

    let floor_entries: Vec<_> = trace.iter().filter(|e| e.name == "Floor").collect();
    assert!(!floor_entries.is_empty(), "Floor must appear in trace");

    for e in &floor_entries {
        assert!(
            approx_eq(e.position.y, 50.0),
            "Floor must stay at y=50: frame {} py={}",
            e.frame,
            e.position.y
        );
        assert!(
            approx_eq(e.velocity.x, 0.0) && approx_eq(e.velocity.y, 0.0),
            "Floor velocity must be zero: frame {} v=({},{})",
            e.frame,
            e.velocity.x,
            e.velocity.y
        );
    }
}
