//! pat-s3rrv.222 — `SkeletonProfileHumanoid`'s 56-bone table, and retargeting a
//! pose through a `BoneMap` onto a differently-named rig.
//!
//! ⚠️ **The bead's "Patina today" is stale.** It states that nothing exists and
//! that neither class is among the ClassDB registrations. In fact
//! `pat-s3rrv.134` had already registered `SkeletonProfile`,
//! `SkeletonProfileHumanoid` and `BoneMap` and built `gdscene/src/bone_map.rs`
//! with the profile/map plumbing. What was genuinely missing is what this file
//! pins: the **humanoid preset itself** (the module had zero bone names in it,
//! so `SkeletonProfileHumanoid` named a table nobody could populate) and any
//! **retargeting** step at all.
//!
//! ⚠️ **One acceptance clause is wrong and these tests deliberately contradict
//! it.** It asks for "root bone `Hips`". Godot's humanoid profile sets
//! `root_bone = "Root"` and `scale_base_bone = "Hips"` — two different fields
//! (`scene/resources/skeleton_profile.cpp`). Asserting `Hips` as the root would
//! pin the retarget origin one bone too low, so the tests assert Godot's values
//! and name both.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::bone_map::{retarget_pose, retarget_poses, BoneMap, SkeletonProfile};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{add_bone, get_bone_global_pose, set_bone_parent, set_bone_rest};

// ---------------------------------------------------------------------------
// The humanoid table
// ---------------------------------------------------------------------------

#[test]
fn the_humanoid_profile_carries_godots_fifty_six_bones() {
    let p = SkeletonProfile::humanoid();
    assert_eq!(
        p.bone_size(),
        56,
        "Godot's SkeletonProfileHumanoid defines exactly 56 bones"
    );
    assert_eq!(
        p.bone_name(0),
        Some("Root"),
        "profile order is load-bearing — `find_bone` returns an index the \
         importers key on, so Root must stay first"
    );
}

#[test]
fn the_root_bone_is_root_and_the_scale_base_is_hips() {
    // The clause the bead got wrong. These are two distinct fields in Godot.
    let p = SkeletonProfile::humanoid();
    assert_eq!(p.root_bone(), "Root");
    assert_eq!(p.scale_base_bone(), "Hips");
    assert_ne!(
        p.root_bone(),
        p.scale_base_bone(),
        "conflating root_bone with scale_base_bone puts the retarget origin one \
         bone too low"
    );
}

#[test]
fn find_bone_resolves_a_limb_and_rejects_a_non_bone() {
    let p = SkeletonProfile::humanoid();
    let arm = p
        .find_bone("LeftUpperArm")
        .expect("LeftUpperArm is a humanoid bone");
    assert_eq!(p.bone_name(arm), Some("LeftUpperArm"));
    assert_eq!(
        p.bone_parent(arm),
        Some("LeftShoulder"),
        "the arm hangs off the shoulder — a flat table with no parents would \
         still pass every name assertion above"
    );
    assert_eq!(
        p.find_bone("LeftElbow"),
        None,
        "a plausible-but-absent name must not resolve"
    );
}

#[test]
fn the_required_bones_are_godots_seventeen() {
    let p = SkeletonProfile::humanoid();
    let required: Vec<&str> = (0..p.bone_size())
        .filter(|&i| p.is_required(i))
        .filter_map(|i| p.bone_name(i))
        .collect();
    assert_eq!(
        required.len(),
        17,
        "Godot marks 17 of the 56 humanoid bones required, got {required:?}"
    );
    for name in ["Hips", "Spine", "Head", "LeftHand", "RightFoot"] {
        assert!(required.contains(&name), "{name} must be required");
    }
    // A finger is optional — a rig without fingers is still a valid humanoid.
    for name in ["LeftThumbDistal", "LeftEye", "Jaw"] {
        assert!(
            !required.contains(&name),
            "{name} must NOT be required — otherwise every fingerless rig fails \
             validation"
        );
    }
}

#[test]
fn a_name_only_profile_reports_nothing_required() {
    // The pre-existing `SkeletonProfile::new` path must keep working, and must
    // not pretend its bones are mandatory just because the field now exists.
    let p = SkeletonProfile::new(["a", "b"]);
    assert_eq!(p.bone_size(), 2);
    assert!(!p.is_required(0));
    assert_eq!(p.bone_parent(0), None);
    assert_eq!(p.root_bone(), "");
}

// ---------------------------------------------------------------------------
// Retargeting onto a differently-named rig
// ---------------------------------------------------------------------------

/// A source rig whose bones are named nothing like Godot's profile — which is
/// the whole point of a BoneMap.
fn source_rig() -> (SceneTree, NodeId, usize) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skel", "Skeleton3D"))
        .expect("skeleton added");

    let hips = add_bone(&mut tree, skel, "bassin");
    let shoulder = add_bone(&mut tree, skel, "epaule_G");
    let arm = add_bone(&mut tree, skel, "bras_G");
    set_bone_parent(&mut tree, skel, shoulder, hips as i64);
    set_bone_parent(&mut tree, skel, arm, shoulder as i64);

    let up = Transform3D {
        origin: Vector3::new(0.0, 1.0, 0.0),
        ..Transform3D::IDENTITY
    };
    set_bone_rest(&mut tree, skel, hips, up);
    set_bone_rest(&mut tree, skel, shoulder, up);
    set_bone_rest(&mut tree, skel, arm, up);
    (tree, skel, arm)
}

fn humanoid_map() -> BoneMap {
    let mut map = BoneMap::with_profile(SkeletonProfile::humanoid());
    assert!(map.set_skeleton_bone_name("Hips", "bassin"));
    assert!(map.set_skeleton_bone_name("LeftShoulder", "epaule_G"));
    assert!(map.set_skeleton_bone_name("LeftUpperArm", "bras_G"));
    map
}

#[test]
fn a_pose_retargets_onto_the_mapped_source_bone() {
    let (mut tree, skel, arm) = source_rig();
    let map = humanoid_map();

    let before = get_bone_global_pose(&tree, skel, arm).origin;
    let pose = Transform3D {
        origin: Vector3::new(0.5, 0.0, 0.0),
        ..Transform3D::IDENTITY
    };
    assert!(
        retarget_pose(&mut tree, skel, &map, "LeftUpperArm", pose),
        "the profile bone is mapped, so the retarget must apply"
    );

    let after = get_bone_global_pose(&tree, skel, arm).origin;
    assert_ne!(
        before, after,
        "the mapped bone must actually move — an unmapped no-op would leave it \
         at rest and every assertion below would be vacuous"
    );
    // ⚠️ y was 3.0 before pat-mgixk.3, and this comment used to say "y = 3,
    // not 2". Godot 4 composes `parent_global * pose`
    // (`skeleton_3d.cpp:1108-1116`): the pose IS the bone's local transform and
    // REPLACES its rest. So the chain is hips(0,1,0) * shoulder(0,1,0) *
    // arm_pose(0.5,0,0) — the arm's own (0,1,0) rest is overridden by the
    // retargeted pose — putting the arm at y = 2. Hips and shoulder still
    // contribute their rests because an unposed bone's pose DEFAULTS to its
    // rest, which is what keeps the chain from collapsing.
    assert!(
        (after.x - 0.5).abs() < 1e-4 && (after.y - 2.0).abs() < 1e-4,
        "the pose must land in the source bone's own chain and replace that \
         bone's rest, got {after:?}"
    );
}

#[test]
fn an_unmapped_or_unknown_profile_bone_is_refused_not_silent() {
    let (mut tree, skel, _) = source_rig();
    let map = humanoid_map();
    let pose = Transform3D::IDENTITY;

    assert!(
        !retarget_pose(&mut tree, skel, &map, "RightUpperArm", pose),
        "a profile bone with no mapping must report failure, not pretend to \
         retarget"
    );
    assert!(
        !retarget_pose(&mut tree, skel, &map, "NotABone", pose),
        "a name outside the profile must be refused"
    );
}

#[test]
fn a_mapping_to_a_bone_the_rig_lacks_is_refused() {
    // The typo case: the map names a bone the source skeleton does not have.
    let (mut tree, skel, _) = source_rig();
    let mut map = humanoid_map();
    assert!(map.set_skeleton_bone_name("Head", "tete"));
    assert!(
        !retarget_pose(&mut tree, skel, &map, "Head", Transform3D::IDENTITY),
        "the rig has no `tete` bone, so this must fail rather than silently \
         doing nothing"
    );
}

#[test]
fn a_batch_retarget_counts_only_what_landed() {
    let (mut tree, skel, _) = source_rig();
    let map = humanoid_map();
    let pose = Transform3D::IDENTITY;
    let applied = retarget_poses(
        &mut tree,
        skel,
        &map,
        &[
            ("Hips", pose),
            ("LeftUpperArm", pose),
            ("RightUpperArm", pose),   // unmapped
            ("LeftThumbDistal", pose), // unmapped
        ],
    );
    assert_eq!(
        applied, 2,
        "a partial map is normal — a rig without fingers still retargets its \
         limbs, and the count is how a caller tells how much landed"
    );
}
