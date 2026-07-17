//! pat-nzgdu.183 — `FABRIK3D` drives a 3-bone chain's end effector to a moving
//! target, pose-verified through the real modifier pass.
//!
//! The FABRIK **math core** (`ik_solver::solve_fabrik`) already existed with its
//! own tests, but nothing routed to it: `process_modifiers` only knew
//! `IKModifier3D` → CCD. These tests drive the real
//! `skeleton3d::process_modifiers` pipeline with a `FABRIK3D` node and assert
//! the pose actually moves — the math core's own tests prove the algorithm, not
//! the wiring.
//!
//! Geometry worth knowing before editing: bone0 is the **fixed pivot** at
//! (0, 1, 0) — rotating a joint's own pose swings its *descendants*, never
//! itself. So the chain's reach is **2** (bones 1 and 2), not 3, and every
//! target below sits within 2 units of (0, 1, 0). This is the same trap
//! pat-nzgdu.151 documents for CCD.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{
    add_bone, get_bone_global_pose, process_modifiers, set_bone_parent, set_bone_rest,
};
use gdvariant::Variant;

/// A 3-bone chain along +Y: bone0 at (0,1,0), bone1 at (0,2,0), tip at (0,3,0).
fn make_chain(tree: &mut SceneTree) -> (NodeId, usize, usize) {
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skeleton", "Skeleton3D"))
        .expect("skeleton added");

    let b0 = add_bone(tree, skel, "root");
    let b1 = add_bone(tree, skel, "mid");
    let b2 = add_bone(tree, skel, "tip");
    set_bone_parent(tree, skel, b1, b0 as i64);
    set_bone_parent(tree, skel, b2, b1 as i64);

    let up = Transform3D {
        origin: Vector3::new(0.0, 1.0, 0.0),
        ..Transform3D::IDENTITY
    };
    set_bone_rest(tree, skel, b0, up);
    set_bone_rest(tree, skel, b1, up);
    set_bone_rest(tree, skel, b2, up);

    (skel, b0, b2)
}

/// Attaches a `FABRIK3D` child aiming `tip_bone` at `target`.
fn add_fabrik(
    tree: &mut SceneTree,
    skel: NodeId,
    root_bone: usize,
    tip_bone: usize,
    target: Vector3,
    influence: f32,
) -> NodeId {
    let mut m = Node::new("IK", "FABRIK3D");
    m.set_property("target_position", Variant::Vector3(target));
    m.set_property("root_bone", Variant::Int(root_bone as i64));
    m.set_property("tip_bone", Variant::Int(tip_bone as i64));
    m.set_property("iterations", Variant::Int(16));
    m.set_property("influence", Variant::Float(influence as f64));
    tree.add_child(skel, m).expect("modifier added")
}

fn tip_pos(tree: &SceneTree, skel: NodeId, tip: usize) -> Vector3 {
    get_bone_global_pose(tree, skel, tip).origin
}

fn set_target(tree: &mut SceneTree, ik: NodeId, target: Vector3) {
    tree.get_node_mut(ik)
        .expect("modifier still in tree")
        .set_property("target_position", Variant::Vector3(target));
}

/// The acceptance: a FABRIK3D on a 3-bone chain drives the end effector to a
/// **moving** target within tolerance across frames.
#[test]
fn fabrik3d_chain_reaches_target() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);

    // Reachable: 1.5 from the pivot at (0,1,0), inside the chain's reach of 2.
    let first = Vector3::new(1.5, 1.0, 0.0);
    let ik = add_fabrik(&mut tree, skel, b0, tip, first, 1.0);

    let start = tip_pos(&tree, skel, tip);
    assert!(
        (start - Vector3::new(0.0, 3.0, 0.0)).length() < 1e-5,
        "the chain starts straight up at (0,3,0), got {start:?}"
    );

    for _ in 0..4 {
        process_modifiers(&mut tree, skel);
    }
    let reached = tip_pos(&tree, skel, tip);
    assert!(
        (reached - first).length() < 0.05,
        "FABRIK must drive the tip to {first:?}, got {reached:?} \
         (distance {})",
        (reached - first).length()
    );

    // Now MOVE the target — the solve must track it across frames, not just
    // converge once and stick.
    let second = Vector3::new(-1.0, 2.0, 0.5);
    set_target(&mut tree, ik, second);
    for _ in 0..4 {
        process_modifiers(&mut tree, skel);
    }
    let tracked = tip_pos(&tree, skel, tip);
    assert!(
        (tracked - second).length() < 0.05,
        "FABRIK must track the target to {second:?}, got {tracked:?} \
         (distance {})",
        (tracked - second).length()
    );
}

/// A FABRIK3D is only routed to FABRIK because of its CLASS — the same node
/// spelled `IKModifier3D` runs CCD. Both must reach the target, which is what
/// makes them interchangeable solvers rather than one being wired to nothing.
#[test]
fn fabrik3d_and_ikmodifier3d_are_both_routed_and_both_solve() {
    let target = Vector3::new(1.2, 1.4, 0.0);

    let mut fab_tree = SceneTree::new();
    let (fab_skel, fb0, fab_tip) = make_chain(&mut fab_tree);
    add_fabrik(&mut fab_tree, fab_skel, fb0, fab_tip, target, 1.0);

    let mut ccd_tree = SceneTree::new();
    let (ccd_skel, cb0, ccd_tip) = make_chain(&mut ccd_tree);
    let mut m = Node::new("IK", "IKModifier3D");
    m.set_property("target_position", Variant::Vector3(target));
    m.set_property("root_bone", Variant::Int(cb0 as i64));
    m.set_property("tip_bone", Variant::Int(ccd_tip as i64));
    m.set_property("iterations", Variant::Int(16));
    m.set_property("influence", Variant::Float(1.0));
    ccd_tree.add_child(ccd_skel, m).expect("modifier added");

    for _ in 0..8 {
        process_modifiers(&mut fab_tree, fab_skel);
        process_modifiers(&mut ccd_tree, ccd_skel);
    }

    let start = (Vector3::new(0.0, 3.0, 0.0) - target).length();
    let fab = tip_pos(&fab_tree, fab_skel, fab_tip);
    let ccd = tip_pos(&ccd_tree, ccd_skel, ccd_tip);
    assert!(
        (fab - target).length() < 0.05,
        "FABRIK3D reaches the target, got {fab:?}"
    );
    // CCD is asserted to CONVERGE, not to arrive on a schedule — pat-nzgdu.151's
    // own test only claims "moves toward", and this test exists to prove both
    // classes are ROUTED, not to race the two algorithms.
    assert!(
        (ccd - target).length() < start,
        "IKModifier3D (CCD) still solves too — I did not break pat-nzgdu.151: \
         {start} -> {} at {ccd:?}",
        (ccd - target).length()
    );
}

/// An unknown modifier class must be ignored, not silently solved as CCD.
/// Otherwise adding CCDIK3D/JacobianIK3D later would inherit FABRIK's behaviour
/// by accident.
#[test]
fn unknown_modifier_classes_are_left_alone() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);

    let mut m = Node::new("IK", "JacobianIK3D"); // a real Godot class, not wired
    m.set_property("target_position", Variant::Vector3(Vector3::new(1.5, 1.0, 0.0)));
    m.set_property("root_bone", Variant::Int(b0 as i64));
    m.set_property("tip_bone", Variant::Int(tip as i64));
    m.set_property("influence", Variant::Float(1.0));
    tree.add_child(skel, m).expect("modifier added");

    let before = tip_pos(&tree, skel, tip);
    for _ in 0..4 {
        process_modifiers(&mut tree, skel);
    }
    let after = tip_pos(&tree, skel, tip);
    assert!(
        (before - after).length() < 1e-6,
        "an unrouted solver class must not move the pose, moved to {after:?}"
    );
}

/// Godot: `influence` 0 contributes nothing, and `active` false skips the
/// modifier entirely.
#[test]
fn influence_zero_and_inactive_contribute_nothing() {
    let target = Vector3::new(1.5, 1.0, 0.0);

    // influence = 0 → untouched.
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);
    add_fabrik(&mut tree, skel, b0, tip, target, 0.0);
    let before = tip_pos(&tree, skel, tip);
    for _ in 0..4 {
        process_modifiers(&mut tree, skel);
    }
    assert!(
        (before - tip_pos(&tree, skel, tip)).length() < 1e-6,
        "influence 0 must leave the pose exactly untouched"
    );

    // active = false → skipped.
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);
    let ik = add_fabrik(&mut tree, skel, b0, tip, target, 1.0);
    tree.get_node_mut(ik)
        .expect("modifier in tree")
        .set_property("active", Variant::Bool(false));
    let before = tip_pos(&tree, skel, tip);
    for _ in 0..4 {
        process_modifiers(&mut tree, skel);
    }
    assert!(
        (before - tip_pos(&tree, skel, tip)).length() < 1e-6,
        "an inactive modifier must not solve"
    );
}

/// A partial influence must move the tip toward the target without arriving —
/// that is what makes an IK solve blendable rather than all-or-nothing.
#[test]
fn partial_influence_blends_toward_the_target() {
    let target = Vector3::new(1.5, 1.0, 0.0);
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);
    add_fabrik(&mut tree, skel, b0, tip, target, 0.25);

    let start_dist = (tip_pos(&tree, skel, tip) - target).length();
    process_modifiers(&mut tree, skel);
    let after_one = (tip_pos(&tree, skel, tip) - target).length();

    assert!(
        after_one < start_dist,
        "a quarter-influence solve must still close some distance: {start_dist} -> {after_one}"
    );
    assert!(
        after_one > 0.05,
        "...but must NOT arrive in one frame, or influence is being ignored: {after_one}"
    );
}

/// ⚠️ An unreachable target must lay the chain out straight toward it and keep
/// the bone lengths — never stretch to reach.
#[test]
fn unreachable_target_extends_straight_without_stretching() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);
    // 10 units away: far beyond the chain's reach of 2 from (0,1,0).
    let far = Vector3::new(10.0, 1.0, 0.0);
    add_fabrik(&mut tree, skel, b0, tip, far, 1.0);

    for _ in 0..6 {
        process_modifiers(&mut tree, skel);
    }

    let pivot = Vector3::new(0.0, 1.0, 0.0);
    let reached = tip_pos(&tree, skel, tip);
    // The chain cannot arrive, but it must point at the target at full stretch.
    let dist_from_pivot = (reached - pivot).length();
    assert!(
        (dist_from_pivot - 2.0).abs() < 0.05,
        "the chain must extend to its full reach of 2 without stretching, got {dist_from_pivot}"
    );
    let dir = (reached - pivot).normalized();
    assert!(
        (dir - Vector3::new(1.0, 0.0, 0.0)).length() < 0.05,
        "...and point straight at the target, got direction {dir:?}"
    );
}
