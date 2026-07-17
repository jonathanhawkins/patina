//! pat-nzgdu.151 — `IKModifier3D` drives a bone chain's tip toward its target.
//!
//! Before this bead `Skeleton3D` had no modifier stack at all: bone poses were
//! whatever the animation wrote, and nothing could layer IK on top. These tests
//! drive the real `skeleton3d::process_modifiers` pipeline and assert the
//! acceptance's three properties — the tip moves **toward the target**, the
//! movement is **blended by influence**, and intermediate bones **do not
//! detach** (the chain stays rigid: each bone keeps its parent-relative offset).
//!
//! The skeleton is a straight 3-bone chain along +Y with the tip at (0, 3, 0);
//! targets sit out on +X, so a correct solve has to swing the chain.
//!
//! Geometry worth knowing before editing these: bone0 is the **fixed pivot** at
//! (0, 1, 0) — rotating a joint's own pose swings its *descendants*, never
//! itself — so the chain's reach is **2** (bones 1 and 2), not 3.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{
    add_bone, get_bone_global_pose, process_modifiers, set_bone_parent, set_bone_rest,
};
use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::node::NodeId;
use gdvariant::Variant;

/// A skeleton whose bones each sit 1 unit above their parent, so bone 3's
/// global origin starts at (0, 3, 0).
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

    // Each child rests 1 unit up from its parent.
    let up = Transform3D {
        origin: Vector3::new(0.0, 1.0, 0.0),
        ..Transform3D::IDENTITY
    };
    set_bone_rest(tree, skel, b0, up);
    set_bone_rest(tree, skel, b1, up);
    set_bone_rest(tree, skel, b2, up);

    (skel, b0, b2)
}

/// Attaches an IKModifier3D child aiming `tip` at `target`.
fn add_ik(
    tree: &mut SceneTree,
    skel: NodeId,
    root_bone: usize,
    tip_bone: usize,
    target: Vector3,
    influence: f32,
) -> NodeId {
    let mut m = Node::new("IK", "IKModifier3D");
    m.set_property("target_position", Variant::Vector3(target));
    m.set_property("root_bone", Variant::Int(root_bone as i64));
    m.set_property("tip_bone", Variant::Int(tip_bone as i64));
    m.set_property("iterations", Variant::Int(16));
    m.set_property("influence", Variant::Float(influence as f64));
    tree.add_child(skel, m).expect("modifier added")
}

#[test]
fn ik_modifier3d_drives_bone_chain_tip_toward_target() {
    let mut tree = SceneTree::new();
    let (skel, root_bone, tip_bone) = make_chain(&mut tree);

    // Straight up the +Y axis to start.
    let start = get_bone_global_pose(&tree, skel, tip_bone).origin;
    assert!((start - Vector3::new(0.0, 3.0, 0.0)).length() < 1e-4, "{start:?}");

    // Target out on +X, inside the chain's reachable ball.
    //
    // NB the reach is 2, not 3: bone0 is the fixed pivot at (0, 1, 0) — a
    // joint's own pose rotation swings its *descendants*, never itself — so
    // only bones 1 and 2 (1 unit each) contribute reach. (0, 1, 0) is 1.0 away
    // from that pivot, comfortably reachable.
    let target = Vector3::new(1.0, 1.0, 0.0);
    let before = start.distance_to(target);
    add_ik(&mut tree, skel, root_bone, tip_bone, target, 1.0);

    process_modifiers(&mut tree, skel);

    let after_pos = get_bone_global_pose(&tree, skel, tip_bone).origin;
    let after = after_pos.distance_to(target);
    assert!(
        after < before,
        "tip must move TOWARD the target: {before} -> {after} (tip now {after_pos:?})",
    );
    // A reachable target: the solve should get genuinely close, not just nudge.
    assert!(after < 0.1, "tip should about reach the target, got {after}");
}

#[test]
fn influence_blends_the_correction() {
    // Deliberately OUT of reach (the pivot is (0,1,0) and the reach is 2, so
    // (3,0,0) at ~3.16 cannot be attained). That is the point: against a
    // reachable target both 0.5 and 1.0 would converge to ~0 and the strict
    // `full < half` ordering would tie and fail. An unreachable target keeps
    // the influences separated so the blend is actually observable.
    let target = Vector3::new(3.0, 0.0, 0.0);

    let dist_with = |influence: f32| {
        let mut tree = SceneTree::new();
        let (skel, root_bone, tip_bone) = make_chain(&mut tree);
        add_ik(&mut tree, skel, root_bone, tip_bone, target, influence);
        process_modifiers(&mut tree, skel);
        get_bone_global_pose(&tree, skel, tip_bone)
            .origin
            .distance_to(target)
    };

    // influence 0 leaves the pose untouched — the tip stays where it started.
    let none = dist_with(0.0);
    let start_dist = Vector3::new(0.0, 3.0, 0.0).distance_to(target);
    assert!(
        (none - start_dist).abs() < 1e-4,
        "influence 0 must not move the tip: {none} vs {start_dist}",
    );

    // Full influence gets closer than a partial one, which gets closer than none.
    let half = dist_with(0.5);
    let full = dist_with(1.0);
    assert!(full < half, "influence 1 must beat 0.5: {full} vs {half}");
    assert!(half < none, "influence 0.5 must beat 0: {half} vs {none}");
}

#[test]
fn intermediate_bones_do_not_detach() {
    let mut tree = SceneTree::new();
    let (skel, root_bone, tip_bone) = make_chain(&mut tree);
    add_ik(
        &mut tree,
        skel,
        root_bone,
        tip_bone,
        Vector3::new(3.0, 0.0, 0.0),
        1.0,
    );
    process_modifiers(&mut tree, skel);

    // The solver only writes rotations, so every bone must still sit exactly one
    // unit from its parent — the chain bends, it does not stretch or come apart.
    for (child, parent) in [(1usize, 0usize), (2usize, 1usize)] {
        let c = get_bone_global_pose(&tree, skel, child).origin;
        let p = get_bone_global_pose(&tree, skel, parent).origin;
        let len = c.distance_to(p);
        assert!(
            (len - 1.0).abs() < 1e-3,
            "bone {child} detached from {parent}: length {len}, expected 1.0",
        );
    }
}

#[test]
fn an_inactive_modifier_is_skipped() {
    let mut tree = SceneTree::new();
    let (skel, root_bone, tip_bone) = make_chain(&mut tree);
    let m = add_ik(
        &mut tree,
        skel,
        root_bone,
        tip_bone,
        Vector3::new(3.0, 0.0, 0.0),
        1.0,
    );
    tree.get_node_mut(m)
        .unwrap()
        .set_property("active", Variant::Bool(false));

    process_modifiers(&mut tree, skel);

    let tip = get_bone_global_pose(&tree, skel, tip_bone).origin;
    assert!(
        (tip - Vector3::new(0.0, 3.0, 0.0)).length() < 1e-4,
        "an inactive modifier must not touch the pose, got {tip:?}",
    );
}

#[test]
fn a_chain_whose_tip_does_not_descend_from_root_is_a_no_op() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skeleton", "Skeleton3D"))
        .expect("skeleton added");

    // Two unrelated roots: bone 1 is NOT a descendant of bone 0.
    let b0 = add_bone(&mut tree, skel, "a");
    let b1 = add_bone(&mut tree, skel, "b");
    let up = Transform3D {
        origin: Vector3::new(0.0, 1.0, 0.0),
        ..Transform3D::IDENTITY
    };
    set_bone_rest(&mut tree, skel, b0, up);
    set_bone_rest(&mut tree, skel, b1, up);

    add_ik(&mut tree, skel, b0, b1, Vector3::new(3.0, 0.0, 0.0), 1.0);
    process_modifiers(&mut tree, skel);

    // A mis-wired modifier must not mangle unrelated bones.
    let p = get_bone_global_pose(&tree, skel, b1).origin;
    assert!((p - Vector3::new(0.0, 1.0, 0.0)).length() < 1e-4, "{p:?}");
}
