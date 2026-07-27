//! pat-0q2kx.250 — render-tick physics interpolation for 3D bodies.
//!
//! `PhysicsServer3D::sync_from_physics` snapped node transforms once per tick
//! with no previous/current state, so at 10 tps against 60 fps rendering **five
//! frames in six** showed a stale transform and motion visibly stepped.
//!
//! Godot 4.4 blends between the previous and current physics ticks using the
//! accumulator fraction, gated by `physics/common/physics_interpolation`, with
//! per-node `physics_interpolation_mode` and `reset_physics_interpolation()`
//! after teleports.
//!
//! ## Where the blend is published
//!
//! The blended transform goes to the `__interp_position_3d` property, not onto
//! the node transform, and these tests read it through
//! `interpolated_position_3d`. That is deliberate and matches the 2D path
//! shipped in pat-0q2kx.219: writing the blend onto the node would feed
//! interpolated values back into the next tick's `sync_to_physics` and corrupt
//! the simulation. The raw node transform stays authoritative for physics; the
//! published value is what a renderer draws.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::physics_interpolation::INTERP_MODE_PROP;
use gdscene::physics_interpolation_3d::interpolated_position_3d;
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, LifecycleManager};
use gdvariant::Variant;

/// The bead's numbers: 10 physics ticks per second against 60 fps rendering, so
/// a tick every sixth frame and five frames in six are pure interpolation.
const TPS: u32 = 10;
const FRAME: f64 = 1.0 / 60.0;
const FRAMES_PER_TICK: usize = 6;

/// `Root -> Ball (RigidBody3D, sphere r=0.5)` falling from `y`.
///
/// A `CollisionShape3D` child is required: `sync_to_physics` looks for one when
/// it registers the body, and without it the ball never enters the simulation
/// and every assertion below would pass vacuously against a motionless node.
fn falling_ball(interpolate: bool) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();
    let ball = tree
        .add_child(root, Node::new("Ball", "RigidBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, ball, Vector3::new(0.0, 10.0, 0.0));
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(ball, shape).unwrap();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.set_physics_ticks_per_second(TPS);
    ml.set_physics_interpolation_enabled(interpolate);
    (ml, ball)
}

/// `Root -> Mover (CharacterBody3D)` with interpolation on.
///
/// Kinematic rather than rigid because 3D only pushes the node transform into
/// the world for static/kinematic bodies — see the note on the teleport section
/// of the acceptance test. A `RigidBody3D` cannot be repositioned from its node
/// at all here.
fn teleportable_character() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();
    let mover = tree
        .add_child(root, Node::new("Mover", "CharacterBody3D"))
        .unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(mover, shape).unwrap();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.set_physics_ticks_per_second(TPS);
    ml.set_physics_interpolation_enabled(true);
    (ml, mover)
}

/// Steps one frame and reports whether a physics tick fired.
///
/// A tick is **detected**, not assumed: `1.0/60.0` has no exact binary
/// representation, so six frames may accumulate to a hair under `physics_dt`
/// and fire on the seventh. Counting frames would make these assertions depend
/// on which side of the boundary the rounding fell.
fn step_frame(ml: &mut MainLoop, ball: NodeId) -> bool {
    let before = ml.physics_interpolation_3d().tick_positions(ball).1;
    ml.step(FRAME);
    ml.physics_interpolation_3d().tick_positions(ball).1 != before
}

/// Steps until `ticks` physics ticks have fired, or panics.
fn run_until_ticks(ml: &mut MainLoop, ball: NodeId, ticks: usize) {
    let mut seen = 0;
    for _ in 0..ticks * FRAMES_PER_TICK * 4 {
        if step_frame(ml, ball) {
            seen += 1;
            if seen == ticks {
                return;
            }
        }
    }
    panic!("only {seen} of {ticks} physics ticks fired");
}

/// Runs whole frames without tick detection, for the interpolation-off path
/// where no snapshots exist to observe.
fn run_frames(ml: &mut MainLoop, frames: usize) {
    for _ in 0..frames {
        ml.step(FRAME);
    }
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** Interpolated frames advance monotonically and sit
/// between the last two ticks; with interpolation off nothing moves between
/// ticks; a reset after a teleport snaps instead of blending.
#[test]
fn physics3d_interpolation() {
    // -- interpolation ON ----------------------------------------------------
    let (mut ml, ball) = falling_ball(true);

    // Two whole ticks: you cannot blend until there IS a previous tick.
    run_until_ticks(&mut ml, ball, 2);
    let (prev, curr) = ml.physics_interpolation_3d().tick_positions(ball);
    let prev = prev.expect("two ticks have run, so a previous snapshot exists");
    let curr = curr.expect("current snapshot exists");
    assert!(
        curr.y < prev.y,
        "gravity must have moved the body down between ticks: {prev:?} -> {curr:?}",
    );

    // Frames until the NEXT tick fires run no physics at all — these are the
    // ones that used to show a stale transform. Collecting up to the tick
    // rather than a fixed count keeps `prev`/`curr` valid for every sample.
    let mut blended = Vec::new();
    for _ in 0..(FRAMES_PER_TICK * 2) {
        if step_frame(&mut ml, ball) {
            break;
        }
        blended
            .push(interpolated_position_3d(ml.tree(), ball).expect("each frame publishes a blend"));
    }
    assert!(
        blended.len() >= 2,
        "at 10 tps against 60 fps there must be several non-physics frames per \
         tick to interpolate across, got {}",
        blended.len(),
    );

    for (i, p) in blended.iter().enumerate() {
        assert!(
            p.y <= prev.y && p.y >= curr.y,
            "frame {i} must lie between the last two tick positions ({} .. {}), got {}",
            curr.y,
            prev.y,
            p.y,
        );
    }
    for pair in blended.windows(2) {
        assert!(
            pair[1].y < pair[0].y,
            "the blend must advance monotonically across the tick interval: {:?}",
            blended,
        );
    }
    assert!(
        blended.first().unwrap().y != blended.last().unwrap().y,
        "a rendered frame with no physics step must still advance",
    );

    // -- interpolation OFF ---------------------------------------------------
    //
    // No snapshots exist here, so a tick cannot be detected through the buffers.
    // Instead: count how many of a fixed run of frames moved the node at all.
    // With physics at 10 tps and rendering at 60 fps that must be a small
    // minority — if the transform advanced every frame, something is
    // interpolating when it was told not to.
    let (mut off, off_ball) = falling_ball(false);
    run_frames(&mut off, FRAMES_PER_TICK * 2);

    let mut moved_frames = 0;
    let total = FRAMES_PER_TICK * 2;
    let mut last = node3d::get_position(off.tree(), off_ball);
    for _ in 0..total {
        off.step(FRAME);
        let now = node3d::get_position(off.tree(), off_ball);
        if now != last {
            moved_frames += 1;
            last = now;
        }
    }
    assert!(
        moved_frames > 0,
        "the body must still be simulated with interpolation off",
    );
    assert!(
        moved_frames * 2 < total,
        "with interpolation disabled the transform must change only on tick \
         boundaries, but {moved_frames} of {total} frames moved it",
    );
    assert!(
        interpolated_position_3d(off.tree(), off_ball).is_none(),
        "nothing should be published when interpolation is disabled",
    );

    // -- reset after a teleport ---------------------------------------------
    //
    // ⚠️ The teleport uses a CharacterBody3D, NOT the falling RigidBody3D, and
    // the 3D rule here is the OPPOSITE of the 2D one:
    //
    //   2D  `sync_to_physics` pushes node -> body for Rigid bodies too, so a
    //       teleport must move the NODE (pat-0q2kx.219).
    //   3D  `sync_to_physics` sends Rigid bodies to `sync_rigid_body`, which
    //       reads velocities, forces and flags but **never position**
    //       (physics_server_3d.rs:1049). Only static/kinematic bodies get their
    //       node transform pushed into the world. So the body is authoritative
    //       for a RigidBody3D, and writing its node position is silently
    //       reverted by the next `sync_from_physics`.
    //
    // A kinematic body is therefore the only thing in 3D a test can actually
    // teleport — and it is also the honest subject, since teleporting is a
    // manual move rather than a simulated one.
    let (mut tp, mover) = teleportable_character();

    // Two ticks at different positions, so a real previous->current blend exists
    // to be smeared across if the reset were ignored.
    node3d::set_position(tp.tree_mut(), mover, Vector3::new(0.0, 10.0, 0.0));
    run_until_ticks(&mut tp, mover, 1);
    node3d::set_position(tp.tree_mut(), mover, Vector3::new(0.0, 9.0, 0.0));
    run_until_ticks(&mut tp, mover, 1);
    let (before_prev, before_curr) = tp.physics_interpolation_3d().tick_positions(mover);
    assert_eq!(before_prev.map(|p| p.y), Some(10.0));
    assert_eq!(before_curr.map(|p| p.y), Some(9.0));

    node3d::set_position(tp.tree_mut(), mover, Vector3::new(0.0, -500.0, 0.0));
    tp.reset_physics_interpolation(mover);
    run_until_ticks(&mut tp, mover, 1);

    let after = interpolated_position_3d(tp.tree(), mover).expect("publishes");
    assert!(
        after.y < -400.0,
        "after reset_physics_interpolation the body must render at its new \
         location, not streak back toward the old one; got {after:?}",
    );

    // Control: the SAME teleport without the reset must blend, which is what
    // proves the assertion above is testing `reset_physics_interpolation` and
    // not merely that a teleport moves something.
    let (mut ctl, ctl_mover) = teleportable_character();
    node3d::set_position(ctl.tree_mut(), ctl_mover, Vector3::new(0.0, 10.0, 0.0));
    run_until_ticks(&mut ctl, ctl_mover, 1);
    node3d::set_position(ctl.tree_mut(), ctl_mover, Vector3::new(0.0, 9.0, 0.0));
    run_until_ticks(&mut ctl, ctl_mover, 1);
    node3d::set_position(ctl.tree_mut(), ctl_mover, Vector3::new(0.0, -500.0, 0.0));
    run_until_ticks(&mut ctl, ctl_mover, 1);
    // No reset: the next frame inside the interval blends from y=9 toward -500.
    ctl.step(FRAME);
    let smeared = interpolated_position_3d(ctl.tree(), ctl_mover).expect("publishes");
    assert!(
        smeared.y > -400.0,
        "without a reset the blend must still be streaking across the teleport, \
         otherwise the reset assertion above proves nothing; got {smeared:?}",
    );
}

// ===========================================================================
// Per-node modes and buffer hygiene
// ===========================================================================

/// `physics_interpolation_mode = OFF` (2) opts a single node out while the
/// tree-wide toggle stays on.
#[test]
fn per_node_mode_off_opts_out_3d() {
    let (mut ml, ball) = falling_ball(true);
    ml.tree_mut()
        .get_node_mut(ball)
        .unwrap()
        .set_property(INTERP_MODE_PROP, Variant::Int(2));

    run_frames(&mut ml, FRAMES_PER_TICK * 3);
    assert!(
        interpolated_position_3d(ml.tree(), ball).is_none(),
        "a node with mode OFF must not receive a published blend",
    );
}

/// A node inherits its parent's mode, so opting a subtree out works from the
/// top — the reason `INHERIT` is the default.
#[test]
fn mode_inherits_from_the_parent_3d() {
    let (mut ml, ball) = falling_ball(true);
    let parent = ml.tree().get_node(ball).unwrap().parent().unwrap();
    // The BALL stays INHERIT; the parent is what turns it off.
    ml.tree_mut()
        .get_node_mut(parent)
        .unwrap()
        .set_property(INTERP_MODE_PROP, Variant::Int(2));

    run_frames(&mut ml, FRAMES_PER_TICK * 3);
    assert!(
        interpolated_position_3d(ml.tree(), ball).is_none(),
        "an INHERIT node under an OFF parent must not be interpolated",
    );
}

/// Disabling the tree toggle clears the buffers, so re-enabling cannot blend a
/// body from wherever it happened to be when it was switched off.
#[test]
fn disabling_clears_the_3d_buffers() {
    let (mut ml, ball) = falling_ball(true);
    run_until_ticks(&mut ml, ball, 2);
    assert!(ml
        .physics_interpolation_3d()
        .tick_positions(ball)
        .1
        .is_some());

    ml.set_physics_interpolation_enabled(false);
    let (prev, curr) = ml.physics_interpolation_3d().tick_positions(ball);
    assert!(
        prev.is_none() && curr.is_none(),
        "turning interpolation off must drop the snapshots, not keep them stale",
    );
    assert!(!ml.physics_interpolation_enabled());
}

/// The 2D and 3D buffers are independent: a scene with only 3D bodies must not
/// populate the 2D one, and vice versa. They share a capture point, so a
/// mistake there would silently cross-populate.
#[test]
fn the_2d_buffers_stay_empty_for_a_3d_scene() {
    let (mut ml, ball) = falling_ball(true);
    run_until_ticks(&mut ml, ball, 2);

    assert!(
        ml.physics_interpolation_3d()
            .tick_positions(ball)
            .1
            .is_some(),
        "the 3D buffer must have captured the body",
    );
    let (prev_2d, curr_2d) = ml.physics_interpolation().tick_positions(ball);
    assert!(
        prev_2d.is_none() && curr_2d.is_none(),
        "a RigidBody3D must not appear in the 2D interpolation buffers",
    );
}
