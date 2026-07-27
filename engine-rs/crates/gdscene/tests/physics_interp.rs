//! pat-0q2kx.219 — render-tick physics interpolation.
//!
//! Physics ran at its own rate and `sync_from_physics` wrote the raw last-tick
//! transform, so at 20 tps against 60 fps rendering two frames in three showed a
//! *stale* position and the motion visibly stepped. `physics_interpolation` was
//! zero hits across the whole runtime.
//!
//! Godot blends between the previous and current physics ticks using the
//! accumulator fraction (`Main::iteration` passes `physics_interpolation_
//! fraction`), with `Node.physics_interpolation_mode` opting nodes in or out and
//! `reset_physics_interpolation()` snapping after teleports.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::node2d;
use gdscene::physics_interpolation::{interpolated_position, INTERP_MODE_PROP};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// 20 physics ticks per second against 60 fps rendering: a tick every third
/// frame, so two frames in three are pure interpolation.
const TPS: u32 = 20;
const FRAME: f64 = 1.0 / 60.0;

/// A falling RigidBody2D, with interpolation optionally on.
fn falling_body(interpolate: bool) -> (MainLoop, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let body = tree
        .add_child(root, Node::new("Body", "RigidBody2D"))
        .unwrap();
    node2d::set_position(&mut tree, body, Vector2::ZERO);

    let mut ml = MainLoop::new(tree);
    ml.set_physics_ticks_per_second(TPS);
    ml.set_physics_interpolation_enabled(interpolate);
    // MainLoop does not register 2D bodies on its own — without this the body
    // never moves and every assertion below would pass vacuously.
    ml.register_physics_bodies();
    (ml, body)
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** Interpolated frames move and sit between the last
/// two ticks; with interpolation off they do not; a reset kills the streak.
#[test]
fn physics_interpolation_toggle() {
    // -- with interpolation ON -----------------------------------------------
    let (mut ml, body) = falling_body(true);

    // Six frames = two physics ticks, which is the minimum before a blend is
    // even definable: you cannot interpolate until there is a *previous* tick.
    for _ in 0..6 {
        ml.step(FRAME);
    }
    let (prev, curr) = ml.physics_interpolation().tick_positions(body);
    let prev = prev.expect("two ticks have run, so a previous snapshot exists");
    let curr = curr.expect("current snapshot exists");
    assert!(
        curr.y > prev.y,
        "gravity must have moved the body between ticks: {prev:?} -> {curr:?}",
    );

    // The next two frames run NO physics — they are the ones that used to show
    // a stale transform.
    ml.step(FRAME);
    let a = interpolated_position(ml.tree(), body).expect("frame publishes a blend");
    ml.step(FRAME);
    let b = interpolated_position(ml.tree(), body).expect("frame publishes a blend");

    assert!(
        a.y != b.y,
        "a rendered frame with no physics step must still advance: {a:?} vs {b:?}",
    );
    for (label, p) in [("first", a), ("second", b)] {
        assert!(
            p.y >= prev.y && p.y <= curr.y,
            "the {label} interpolated value must lie between the last two tick \
             positions ({} .. {}), got {}",
            prev.y,
            curr.y,
            p.y,
        );
    }
    assert!(
        b.y > a.y,
        "the blend must advance monotonically across the tick interval",
    );

    // -- with interpolation OFF ----------------------------------------------
    let (mut off, off_body) = falling_body(false);
    for _ in 0..6 {
        off.step(FRAME);
    }
    let settled = node2d::get_position(off.tree(), off_body);

    // Two frames with no physics step: the raw transform must not move.
    off.step(FRAME);
    off.step(FRAME);
    assert_eq!(
        node2d::get_position(off.tree(), off_body),
        settled,
        "with interpolation disabled the transform only changes on tick frames",
    );
    assert!(
        interpolated_position(off.tree(), off_body).is_none(),
        "nothing should be published when interpolation is disabled",
    );

    // -- reset after a teleport prevents blending through the gap ------------
    let (mut tp, tp_body) = falling_body(true);
    for _ in 0..6 {
        tp.step(FRAME);
    }

    // Teleport the *body*, not just the node. Writing `position` on the node
    // alone does not survive: the next tick's `sync_from_physics` rewrites the
    // node from the body, so the "teleport" silently evaporates and the
    // assertion below would be measuring ordinary falling.
    let bid = tp
        .physics_server_mut()
        .body_for_node(tp_body)
        .expect("the body is registered");
    tp.physics_server_mut()
        .world_mut()
        .get_body_mut(bid)
        .expect("body exists")
        .position = Vector2::new(0.0, 10_000.0);

    tp.reset_physics_interpolation(tp_body);
    // Three frames guarantee at least one physics tick at 20 tps, which is what
    // re-captures the post-teleport transform. A single frame runs no tick at
    // all, so nothing would be published and this would assert on a stale value.
    for _ in 0..3 {
        tp.step(FRAME);
    }

    let after = interpolated_position(tp.tree(), tp_body).expect("publishes");
    assert!(
        after.y > 9_000.0,
        "after reset_physics_interpolation the body must render at its new \
         location, not streak back toward the old one; got {after:?}",
    );
}

// ===========================================================================
// Per-node modes
// ===========================================================================

/// `physics_interpolation_mode = OFF` (2) opts a single node out even while the
/// tree toggle is on.
#[test]
fn per_node_mode_off_opts_out() {
    let (mut ml, body) = falling_body(true);
    ml.tree_mut()
        .get_node_mut(body)
        .unwrap()
        .set_property(INTERP_MODE_PROP, Variant::Int(2));

    for _ in 0..8 {
        ml.step(FRAME);
    }
    assert!(
        interpolated_position(ml.tree(), body).is_none(),
        "a node with mode OFF must not receive a published blend",
    );
}

/// A node inherits its parent's mode, so opting a subtree out works from the
/// top — the reason `INHERIT` is the default.
#[test]
fn mode_inherits_from_the_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let holder = tree.add_child(root, Node::new("Holder", "Node2D")).unwrap();
    let body = tree
        .add_child(holder, Node::new("Body", "RigidBody2D"))
        .unwrap();
    // The BODY stays INHERIT; the parent is what turns it off.
    tree.get_node_mut(holder)
        .unwrap()
        .set_property(INTERP_MODE_PROP, Variant::Int(2));

    let mut ml = MainLoop::new(tree);
    ml.set_physics_ticks_per_second(TPS);
    ml.set_physics_interpolation_enabled(true);
    ml.register_physics_bodies();
    for _ in 0..8 {
        ml.step(FRAME);
    }

    assert!(
        interpolated_position(ml.tree(), body).is_none(),
        "an INHERIT node under an OFF parent must not be interpolated",
    );
}

/// Disabling the tree toggle clears the buffers, so re-enabling cannot blend a
/// body from wherever it happened to be when it was switched off.
#[test]
fn disabling_clears_the_buffers() {
    let (mut ml, body) = falling_body(true);
    for _ in 0..6 {
        ml.step(FRAME);
    }
    assert!(ml.physics_interpolation().tick_positions(body).1.is_some());

    ml.set_physics_interpolation_enabled(false);
    let (prev, curr) = ml.physics_interpolation().tick_positions(body);
    assert!(
        prev.is_none() && curr.is_none(),
        "turning interpolation off must drop the snapshots, not keep them stale",
    );
    assert!(!ml.physics_interpolation_enabled());
}
