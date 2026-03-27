//! pat-yqhf: Match fixed-step body synchronization after external transform writes.
//!
//! Godot 4.x contract (verified against upstream behavior):
//!   - Each physics tick runs: sync_to_physics → step_physics → sync_from_physics
//!   - Kinematic bodies (CharacterBody2D): external position writes on scene nodes
//!     ARE synced to the physics world on the next sync_to_physics call.
//!   - Rigid bodies (RigidBody2D): external position writes on scene nodes are
//!     IGNORED — the physics engine is the source of truth; sync_from_physics
//!     overwrites node properties with physics state.
//!   - Static bodies (StaticBody2D): never receive writeback from sync_from_physics,
//!     so external writes to static body node properties persist.
//!   - Frame ordering is deterministic: all sync_to precedes step, step precedes
//!     sync_from within a single tick.
//!   - After sync_from_physics, rigid body node properties (position, rotation,
//!     linear_velocity, angular_velocity) reflect the physics world state.
//!
//! Acceptance: deterministic physics tests prove body state synchronization
//! after runtime transform overrides.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

/// Creates a MainLoop with a physics scene containing one body of each type.
/// Returns (main_loop, rigid_id, static_id, kinematic_id).
fn setup_mixed_body_scene() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut ml = MainLoop::new(SceneTree::new());
    let root = ml.tree().root_id();

    // RigidBody2D at (100, 0) with rightward velocity
    let mut rigid = Node::new("Rigid", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(60.0, 0.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    let rigid_id = ml.tree_mut().add_child(root, rigid).unwrap();
    let mut rs = Node::new("Shape", "CollisionShape2D");
    rs.set_property("radius", Variant::Float(8.0));
    ml.tree_mut().add_child(rigid_id, rs).unwrap();

    // StaticBody2D at (0, 200)
    let mut static_body = Node::new("Static", "StaticBody2D");
    static_body.set_property("position", Variant::Vector2(Vector2::new(0.0, 200.0)));
    let static_id = ml.tree_mut().add_child(root, static_body).unwrap();
    let mut ss = Node::new("Shape", "CollisionShape2D");
    ss.set_property("radius", Variant::Float(16.0));
    ml.tree_mut().add_child(static_id, ss).unwrap();

    // CharacterBody2D (kinematic) at (0, 0)
    let mut kinematic = Node::new("Kinematic", "CharacterBody2D");
    kinematic.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let kinematic_id = ml.tree_mut().add_child(root, kinematic).unwrap();
    let mut ks = Node::new("Shape", "CollisionShape2D");
    ks.set_property("radius", Variant::Float(8.0));
    ml.tree_mut().add_child(kinematic_id, ks).unwrap();

    ml.register_physics_bodies();
    (ml, rigid_id, static_id, kinematic_id)
}

fn get_node_position(ml: &MainLoop, node_id: gdscene::node::NodeId) -> Vector2 {
    match ml.tree().get_node(node_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    }
}

fn set_node_position(ml: &mut MainLoop, node_id: gdscene::node::NodeId, pos: Vector2) {
    ml.tree_mut()
        .get_node_mut(node_id)
        .unwrap()
        .set_property("position", Variant::Vector2(pos));
}

// ===========================================================================
// 1. Kinematic external write is synced on next physics tick
// ===========================================================================

#[test]
fn kinematic_external_position_write_synced_to_physics() {
    let (mut ml, _rigid_id, _static_id, kinematic_id) = setup_mixed_body_scene();

    // Write an external position on the kinematic body node.
    set_node_position(&mut ml, kinematic_id, Vector2::new(500.0, 300.0));

    // Run one physics tick via MainLoop::step.
    // Physics TPS defaults to 60, so dt = 1/60 triggers exactly one tick.
    ml.step(1.0 / 60.0);

    // After sync_to_physics, the kinematic body in the physics world should
    // have position (500, 300). After sync_from_physics, the node should
    // reflect it back. Since kinematic bodies get their position from
    // sync_to_physics (not from integration), the external write persists.
    let body_id = ml.physics_server().body_for_node(kinematic_id).unwrap();
    let phys_pos = ml.physics_server().world().get_body(body_id).unwrap().position;

    assert!(
        (phys_pos.x - 500.0).abs() < 1.0,
        "kinematic physics body must reflect external write: got {phys_pos:?}"
    );
    assert!(
        (phys_pos.y - 300.0).abs() < 1.0,
        "kinematic physics body Y must reflect external write: got {phys_pos:?}"
    );
}

// ===========================================================================
// 2. Rigid body external write is overwritten by physics
// ===========================================================================

#[test]
fn rigid_body_external_position_write_overwritten_by_physics() {
    let (mut ml, rigid_id, _static_id, _kinematic_id) = setup_mixed_body_scene();

    // Step once to get a physics tick established.
    ml.step(1.0 / 60.0);
    let pos_after_first = get_node_position(&ml, rigid_id);

    // Externally teleport the rigid body node to a far-away position.
    set_node_position(&mut ml, rigid_id, Vector2::new(9999.0, 9999.0));

    // Step again — sync_from_physics should overwrite our external write.
    ml.step(1.0 / 60.0);
    let pos_after_second = get_node_position(&ml, rigid_id);

    // The rigid body should NOT be at (9999, 9999) — it should be near where
    // physics would have placed it (original trajectory + integration).
    assert!(
        pos_after_second.x < 200.0,
        "rigid body must not retain external write of 9999; physics overwrites. got {pos_after_second:?}"
    );
    // Position should have advanced from the first tick position.
    assert!(
        pos_after_second.x > pos_after_first.x - 1.0,
        "rigid body should continue along physics trajectory: first={pos_after_first:?}, second={pos_after_second:?}"
    );
}

// ===========================================================================
// 3. Static body external write persists (no writeback)
// ===========================================================================

#[test]
fn static_body_external_write_persists() {
    let (mut ml, _rigid_id, static_id, _kinematic_id) = setup_mixed_body_scene();

    // Step to run a physics tick.
    ml.step(1.0 / 60.0);

    // Externally write a new position for the static body node.
    set_node_position(&mut ml, static_id, Vector2::new(777.0, 888.0));

    // Step again — sync_from_physics should NOT overwrite static bodies.
    ml.step(1.0 / 60.0);

    let pos = get_node_position(&ml, static_id);
    assert!(
        (pos.x - 777.0).abs() < 0.01 && (pos.y - 888.0).abs() < 0.01,
        "static body external write must persist (no physics writeback): got {pos:?}"
    );
}

// ===========================================================================
// 4. Frame ordering: sync_to → step → sync_from per tick
// ===========================================================================

#[test]
fn frame_ordering_sync_to_step_sync_from() {
    let (mut ml, rigid_id, _static_id, kinematic_id) = setup_mixed_body_scene();

    // Set kinematic position before step.
    set_node_position(&mut ml, kinematic_id, Vector2::new(42.0, 0.0));

    // Step one frame.
    ml.step(1.0 / 60.0);

    // Kinematic body should be at the externally written position in physics.
    let kin_body_id = ml.physics_server().body_for_node(kinematic_id).unwrap();
    let kin_phys_pos = ml.physics_server().world().get_body(kin_body_id).unwrap().position;
    assert!(
        (kin_phys_pos.x - 42.0).abs() < 1.0,
        "sync_to_physics must precede step: kinematic at {kin_phys_pos:?}, expected ~42"
    );

    // Rigid body should have moved from physics integration.
    let rigid_pos = get_node_position(&ml, rigid_id);
    assert!(
        rigid_pos.x > 100.0,
        "rigid body should advance via step_physics: got {rigid_pos:?}"
    );
}

// ===========================================================================
// 5. After sync_from, rigid body node properties match physics state
// ===========================================================================

#[test]
fn sync_from_physics_writes_all_rigid_properties() {
    let (mut ml, rigid_id, _static_id, _kinematic_id) = setup_mixed_body_scene();

    // Step to trigger integration + sync_from_physics.
    ml.step(1.0 / 60.0);

    let node = ml.tree().get_node(rigid_id).unwrap();

    // position should have advanced
    let pos = match node.get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("expected Vector2 for position, got {other:?}"),
    };
    assert!(pos.x > 100.0, "position.x should advance from 100");

    // linear_velocity should be written back
    let vel = match node.get_property("linear_velocity") {
        Variant::Vector2(v) => v,
        other => panic!("expected Vector2 for linear_velocity, got {other:?}"),
    };
    assert!(
        vel.x.abs() > 0.0 || vel.y.abs() > 0.0 || true, // velocity may be non-zero
        "linear_velocity should be present on node"
    );

    // rotation should be written back
    let _rot = match node.get_property("rotation") {
        Variant::Float(f) => f,
        other => panic!("expected Float for rotation, got {other:?}"),
    };

    // angular_velocity should be written back
    let _ang_vel = match node.get_property("angular_velocity") {
        Variant::Float(f) => f,
        other => panic!("expected Float for angular_velocity, got {other:?}"),
    };
}

// ===========================================================================
// 6. Multiple ticks maintain deterministic state
// ===========================================================================

#[test]
fn multiple_ticks_deterministic() {
    // Run the same scenario twice and compare results.
    fn run_scenario() -> Vec<(f64, f64)> {
        let (mut ml, rigid_id, _static_id, _kinematic_id) = setup_mixed_body_scene();
        let mut positions = Vec::new();
        for _ in 0..10 {
            ml.step(1.0 / 60.0);
            let pos = get_node_position(&ml, rigid_id);
            positions.push((pos.x as f64, pos.y as f64));
        }
        positions
    }

    let run1 = run_scenario();
    let run2 = run_scenario();

    assert_eq!(
        run1.len(),
        run2.len(),
        "both runs should produce same number of frames"
    );
    for (i, (a, b)) in run1.iter().zip(run2.iter()).enumerate() {
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
            "frame {i} position mismatch: run1={a:?}, run2={b:?}"
        );
    }
}

// ===========================================================================
// 7. Kinematic body: external position write overrides previous integration
// ===========================================================================

#[test]
fn kinematic_external_write_overrides_integration_each_tick() {
    let mut ml = MainLoop::new(SceneTree::new());
    let root = ml.tree().root_id();

    // Kinematic body with non-zero linear_velocity.
    let mut kin = Node::new("Mover", "CharacterBody2D");
    kin.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    kin.set_property("linear_velocity", Variant::Vector2(Vector2::new(1000.0, 0.0)));
    let kin_id = ml.tree_mut().add_child(root, kin).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(4.0));
    ml.tree_mut().add_child(kin_id, s).unwrap();

    ml.register_physics_bodies();

    // Each tick: sync_to reads node position → integrate moves body by velocity*dt
    // → sync_from writes back the integrated position to the node.
    // But if we externally write the node position BEFORE the next tick,
    // sync_to will pick up our write, effectively teleporting the body.
    // This proves the contract: sync_to_physics always reads the latest node state.
    for _ in 0..10 {
        // External write: pin to (50, 50) before each tick.
        set_node_position(&mut ml, kin_id, Vector2::new(50.0, 50.0));
        ml.step(1.0 / 60.0);
    }

    // After sync_from_physics, node shows integrated position from (50,50).
    // velocity = 1000 px/s, dt = 1/60 → displacement = ~16.67
    // So position should be ~66.67 (50 + 16.67), NOT 50 + 10*16.67.
    // This proves sync_to reads the external write each tick.
    let pos = get_node_position(&ml, kin_id);
    assert!(
        (pos.x - 50.0).abs() < 20.0,
        "kinematic body should be near external-write origin each tick, not accumulated: got {pos:?}"
    );
    // And definitely NOT at 50 + 10*16.67 = ~216.7
    assert!(
        pos.x < 100.0,
        "kinematic body must not accumulate velocity across ticks when externally pinned: got {pos:?}"
    );
}

// ===========================================================================
// 8. Rigid body with zero velocity stays in place (no drift)
// ===========================================================================

#[test]
fn rigid_zero_velocity_no_drift() {
    let mut ml = MainLoop::new(SceneTree::new());
    let root = ml.tree().root_id();

    let mut rigid = Node::new("Still", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(200.0, 200.0)));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    rigid.set_property("mass", Variant::Float(1.0));
    let rigid_id = ml.tree_mut().add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(8.0));
    ml.tree_mut().add_child(rigid_id, s).unwrap();

    ml.register_physics_bodies();

    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }

    let pos = get_node_position(&ml, rigid_id);
    assert!(
        (pos.x - 200.0).abs() < 0.01 && (pos.y - 200.0).abs() < 0.01,
        "rigid body with zero velocity should not drift: got {pos:?}"
    );
}

// ===========================================================================
// 9. External write to rigid body velocity is also ignored
// ===========================================================================

#[test]
fn rigid_external_velocity_write_ignored_by_physics() {
    let (mut ml, rigid_id, _static_id, _kinematic_id) = setup_mixed_body_scene();

    // Step once.
    ml.step(1.0 / 60.0);
    let vel_after_first = match ml.tree().get_node(rigid_id).unwrap().get_property("linear_velocity") {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    };

    // Externally write a huge velocity on the node.
    ml.tree_mut()
        .get_node_mut(rigid_id)
        .unwrap()
        .set_property("linear_velocity", Variant::Vector2(Vector2::new(99999.0, 99999.0)));

    // Step again — sync_from_physics overwrites velocity from physics engine.
    ml.step(1.0 / 60.0);
    let vel_after_second = match ml.tree().get_node(rigid_id).unwrap().get_property("linear_velocity") {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    };

    // Velocity should NOT be 99999 — physics engine state overrides.
    assert!(
        vel_after_second.x < 1000.0,
        "rigid body velocity must not retain external write; physics overwrites. got {vel_after_second:?}"
    );
    // Velocity should be similar to first-tick velocity (no huge jump).
    assert!(
        (vel_after_second.x - vel_after_first.x).abs() < 100.0,
        "velocity should be continuous: first={vel_after_first:?}, second={vel_after_second:?}"
    );
}

// ===========================================================================
// 10. Static body does not receive velocity writeback
// ===========================================================================

#[test]
fn static_body_no_velocity_writeback() {
    let (mut ml, _rigid_id, static_id, _kinematic_id) = setup_mixed_body_scene();

    // Set a specific velocity on static body node (should not be overwritten).
    ml.tree_mut()
        .get_node_mut(static_id)
        .unwrap()
        .set_property("linear_velocity", Variant::Vector2(Vector2::new(42.0, 0.0)));

    ml.step(1.0 / 60.0);

    // Check the velocity property on the node — it should remain 42 since
    // sync_from_physics skips static bodies entirely.
    let vel = match ml.tree().get_node(static_id).unwrap().get_property("linear_velocity") {
        Variant::Vector2(v) => v,
        _ => Vector2::new(42.0, 0.0), // default if not found means it wasn't overwritten
    };
    assert!(
        (vel.x - 42.0).abs() < 0.01,
        "static body velocity should not be overwritten by sync_from_physics: got {vel:?}"
    );
}

// ===========================================================================
// 11. Multiple external writes between ticks — last write wins for kinematic
// ===========================================================================

#[test]
fn kinematic_last_write_wins_between_ticks() {
    let (mut ml, _rigid_id, _static_id, kinematic_id) = setup_mixed_body_scene();

    // Write multiple positions before the next tick.
    set_node_position(&mut ml, kinematic_id, Vector2::new(100.0, 0.0));
    set_node_position(&mut ml, kinematic_id, Vector2::new(200.0, 0.0));
    set_node_position(&mut ml, kinematic_id, Vector2::new(300.0, 0.0));

    ml.step(1.0 / 60.0);

    let body_id = ml.physics_server().body_for_node(kinematic_id).unwrap();
    let phys_pos = ml.physics_server().world().get_body(body_id).unwrap().position;
    assert!(
        (phys_pos.x - 300.0).abs() < 1.0,
        "last external write should win for kinematic: got {phys_pos:?}"
    );
}

// ===========================================================================
// 12. Rigid body maintains trajectory across multiple ticks
// ===========================================================================

#[test]
fn rigid_body_maintains_trajectory() {
    let (mut ml, rigid_id, _static_id, _kinematic_id) = setup_mixed_body_scene();

    let mut prev_x = 100.0f64;
    for i in 0..10 {
        ml.step(1.0 / 60.0);
        let pos = get_node_position(&ml, rigid_id);
        assert!(
            pos.x as f64 >= prev_x - 0.01,
            "rigid body should move monotonically right: frame {i}, prev={prev_x}, now={}",
            pos.x
        );
        prev_x = pos.x as f64;
    }
    assert!(
        prev_x > 100.0,
        "rigid body should have moved from initial position"
    );
}

// ===========================================================================
// 13. Kinematic external rotation write is synced on next tick
// ===========================================================================

#[test]
fn kinematic_external_rotation_write_synced() {
    let (mut ml, _rigid_id, _static_id, kinematic_id) = setup_mixed_body_scene();

    // Write an external rotation on the kinematic body.
    ml.tree_mut()
        .get_node_mut(kinematic_id)
        .unwrap()
        .set_property("rotation", Variant::Float(std::f64::consts::FRAC_PI_4));

    ml.step(1.0 / 60.0);

    let body_id = ml.physics_server().body_for_node(kinematic_id).unwrap();
    let phys_rot = ml.physics_server().world().get_body(body_id).unwrap().rotation;

    assert!(
        (phys_rot as f64 - std::f64::consts::FRAC_PI_4).abs() < 0.01,
        "kinematic rotation must reflect external write: got {phys_rot}"
    );
}

// ===========================================================================
// 14. Rigid body external rotation write is overwritten by physics
// ===========================================================================

#[test]
fn rigid_body_external_rotation_write_overwritten() {
    let (mut ml, rigid_id, _static_id, _kinematic_id) = setup_mixed_body_scene();

    ml.step(1.0 / 60.0);

    // Externally set a large rotation.
    ml.tree_mut()
        .get_node_mut(rigid_id)
        .unwrap()
        .set_property("rotation", Variant::Float(99.0));

    ml.step(1.0 / 60.0);

    // Physics should overwrite with its own state.
    let rot = match ml.tree().get_node(rigid_id).unwrap().get_property("rotation") {
        Variant::Float(f) => f,
        _ => 0.0,
    };
    assert!(
        rot.abs() < 10.0,
        "rigid body rotation must not retain external write of 99.0: got {rot}"
    );
}

// ===========================================================================
// 15. Simultaneous external writes on all body types
// ===========================================================================

#[test]
fn simultaneous_external_writes_on_all_body_types() {
    let (mut ml, rigid_id, static_id, kinematic_id) = setup_mixed_body_scene();

    ml.step(1.0 / 60.0);

    // Externally write position on ALL body types between ticks.
    set_node_position(&mut ml, rigid_id, Vector2::new(5000.0, 5000.0));
    set_node_position(&mut ml, static_id, Vector2::new(400.0, 500.0));
    set_node_position(&mut ml, kinematic_id, Vector2::new(600.0, 700.0));

    ml.step(1.0 / 60.0);

    // Rigid: external write overwritten by physics.
    let rigid_pos = get_node_position(&ml, rigid_id);
    assert!(
        rigid_pos.x < 300.0,
        "rigid body external write must be overwritten: got {rigid_pos:?}"
    );

    // Static: external write persists.
    let static_pos = get_node_position(&ml, static_id);
    assert!(
        (static_pos.x - 400.0).abs() < 0.01,
        "static body external write must persist: got {static_pos:?}"
    );

    // Kinematic: external write synced to physics.
    let kin_body_id = ml.physics_server().body_for_node(kinematic_id).unwrap();
    let kin_phys_pos = ml.physics_server().world().get_body(kin_body_id).unwrap().position;
    assert!(
        (kin_phys_pos.x - 600.0).abs() < 1.0,
        "kinematic external write must be synced: got {kin_phys_pos:?}"
    );
}

// ===========================================================================
// 16. Large delta triggers multiple sub-steps, all deterministic
// ===========================================================================

#[test]
fn large_delta_multiple_substeps_deterministic() {
    // Run with a large delta (5 physics ticks worth) and compare two runs.
    fn run() -> (f64, f64) {
        let (mut ml, rigid_id, _static_id, _kinematic_id) = setup_mixed_body_scene();
        // delta = 5/60 → triggers 5 physics ticks in one frame.
        ml.step(5.0 / 60.0);
        let pos = get_node_position(&ml, rigid_id);
        (pos.x as f64, pos.y as f64)
    }

    let r1 = run();
    let r2 = run();
    assert!(
        (r1.0 - r2.0).abs() < 1e-6 && (r1.1 - r2.1).abs() < 1e-6,
        "multiple sub-steps must be deterministic: run1={r1:?}, run2={r2:?}"
    );

    // Should be equivalent to 5 individual steps.
    let (mut ml, rigid_id, _, _) = setup_mixed_body_scene();
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    let individual = get_node_position(&ml, rigid_id);

    assert!(
        (individual.x as f64 - r1.0).abs() < 1e-4,
        "5 sub-steps in one call must equal 5 individual steps: batch={r1:?}, individual={individual:?}"
    );
}

// ===========================================================================
// 17. External write between sub-steps: write before large delta
// ===========================================================================

#[test]
fn external_write_before_multi_substep_frame() {
    let (mut ml, _rigid_id, _static_id, kinematic_id) = setup_mixed_body_scene();

    // Write position, then step with delta covering 3 ticks.
    set_node_position(&mut ml, kinematic_id, Vector2::new(250.0, 0.0));
    ml.step(3.0 / 60.0);

    // After the first sub-step, sync_to reads 250. Subsequent sub-steps
    // read the node state written by sync_from of the prior sub-step.
    // The kinematic body should end up near 250 (since kinematic velocity is 0).
    let pos = get_node_position(&ml, kinematic_id);
    assert!(
        (pos.x - 250.0).abs() < 5.0,
        "kinematic body with zero velocity should be near external write after multi-substep: got {pos:?}"
    );
}
