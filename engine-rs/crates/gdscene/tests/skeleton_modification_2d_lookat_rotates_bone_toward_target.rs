//! pat-s3rrv.221 — the `SkeletonModification2D` family is registered, and
//! `LookAt` actually rotates a bone through the SceneTree tick.
//!
//! ⚠️ **The bead's "Patina today" is stale and this file documents what was
//! really there.** It claims no 2D skeleton module exists and that
//! `SkeletonModification` has zero ClassDB hits. In fact pat-s3rrv.185 had
//! already built `gdscene/src/skeleton2d.rs` — the stack machinery, the
//! `__skelmod_*` keys, and a working `TwoBoneIK` solver — and registered
//! `SkeletonModification2D`, `SkeletonModificationStack2D` and
//! `SkeletonModification2DTwoBoneIK`. What was genuinely missing:
//!
//! * the **six** other subclasses, absent from ClassDB entirely; and
//! * any solver other than TwoBoneIK — `execute_modifications` declined every
//!   other class, so a `LookAt` entry in a rig was silently inert.
//!
//! ⚠️ And `skeleton2d.rs` had **no tests at all**, so the TwoBoneIK path shipped
//! unverified. This is the first test to drive that module.

use gdcore::math::{Transform2D, Vector2};
use gdobject::class_db::{class_exists, is_parent_class, register_all_classes};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The seven concrete modifications Godot ships.
const SUBCLASSES: [&str; 7] = [
    "SkeletonModification2DCCDIK",
    "SkeletonModification2DFABRIK",
    "SkeletonModification2DJiggle",
    "SkeletonModification2DLookAt",
    "SkeletonModification2DPhysicalBones",
    "SkeletonModification2DStackHolder",
    "SkeletonModification2DTwoBoneIK",
];

#[test]
fn the_modification_family_is_registered_under_the_right_bases() {
    register_all_classes();

    assert!(
        class_exists("SkeletonModification2D"),
        "the abstract base must exist — every subclass names it as parent"
    );
    assert!(
        is_parent_class("SkeletonModification2D", "Resource"),
        "Godot's SkeletonModification2D inherits Resource"
    );
    assert!(
        class_exists("SkeletonModificationStack2D"),
        "the stack resource a Skeleton2D holds must exist"
    );
    assert!(
        is_parent_class("SkeletonModificationStack2D", "Resource"),
        "the stack is a Resource too"
    );

    for name in SUBCLASSES {
        assert!(class_exists(name), "{name} must be registered");
        assert!(
            is_parent_class(name, "SkeletonModification2D"),
            "{name} must inherit SkeletonModification2D — a wrong parent still \
             satisfies class_exists, so only the chain proves it"
        );
        // Inheritance is transitive: the chain must reach Resource.
        assert!(
            is_parent_class(name, "Resource"),
            "{name} must reach Resource through its base"
        );
    }
}

/// `root > Skel(Skeleton2D) > { Bone1 > Bone2, Target }`.
///
/// The target sits straight up from the bone, so a correct LookAt lands the
/// bone's global rotation on +90°, and a rest pose of identity means "unrotated"
/// is 0° — the two are far apart, which is what makes the assertions meaningful.
fn rig() -> (SceneTree, NodeId, NodeId) {
    register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skel", "Skeleton2D"))
        .expect("skeleton added");
    let bone1 = tree
        .add_child(skel, Node::new("Bone1", "Bone2D"))
        .expect("bone1 added");
    tree.add_child(bone1, Node::new("Bone2", "Bone2D"))
        .expect("bone2 added");
    let target = tree
        .add_child(skel, Node::new("Target", "Node2D"))
        .expect("target added");

    {
        let n = tree.get_node_mut(bone1).unwrap();
        // A rest of identity: rest direction +X, rest rotation 0.
        n.set_property("rest", Variant::Transform2D(Transform2D::IDENTITY));
        n.set_property("length", Variant::Float(10.0));
    }
    tree.get_node_mut(target)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(0.0, 10.0)));

    // One LookAt entry aiming Bone1 at Target. Paths are relative to the
    // SKELETON, not the scene root.
    {
        let n = tree.get_node_mut(skel).unwrap();
        n.set_property("__skelmod_count__", Variant::Int(1));
        n.set_property(
            "__skelmod_0_class__",
            Variant::String("SkeletonModification2DLookAt".into()),
        );
        n.set_property("__skelmod_0_bone2d_node__", Variant::String("Bone1".into()));
        n.set_property(
            "__skelmod_0_target_nodepath__",
            Variant::String("Target".into()),
        );
    }
    (tree, skel, bone1)
}

fn rotation_of(tree: &SceneTree, bone: NodeId) -> f32 {
    gdscene::node2d::get_global_rotation(tree, bone)
}

#[test]
fn lookat_rotates_the_bone_toward_the_target() {
    let (mut tree, _skel, bone1) = rig();
    assert!(
        rotation_of(&tree, bone1).abs() < 1e-5,
        "the rig must start at its rest rotation, or the assertion below is \
         measuring nothing"
    );

    gdscene::skeleton2d::process_skeletons_2d(&mut tree);

    let got = rotation_of(&tree, bone1);
    assert!(
        (got - std::f32::consts::FRAC_PI_2).abs() < 1e-4,
        "the bone must point at a target directly above it (+90°), got {got} rad"
    );
}

#[test]
fn a_disabled_stack_returns_the_bone_to_rest() {
    let (mut tree, skel, bone1) = rig();
    gdscene::skeleton2d::process_skeletons_2d(&mut tree);
    assert!(
        (rotation_of(&tree, bone1) - std::f32::consts::FRAC_PI_2).abs() < 1e-4,
        "precondition: the bone is solved away from rest"
    );

    tree.get_node_mut(skel)
        .unwrap()
        .set_property("__skelmod_enabled__", Variant::Bool(false));
    gdscene::skeleton2d::process_skeletons_2d(&mut tree);

    let got = rotation_of(&tree, bone1);
    assert!(
        got.abs() < 1e-5,
        "a disabled stack must return the rig to its rest pose, not freeze the \
         last solved pose — got {got} rad"
    );
}

#[test]
fn a_disabled_entry_is_skipped_but_the_stack_still_runs() {
    // Per-entry `enabled` is a different switch from the stack's own, and the
    // existing machinery already honoured it; this pins that the new stack-level
    // flag did not break it.
    let (mut tree, skel, bone1) = rig();
    tree.get_node_mut(skel)
        .unwrap()
        .set_property("__skelmod_0_enabled__", Variant::Bool(false));

    gdscene::skeleton2d::process_skeletons_2d(&mut tree);

    assert!(
        rotation_of(&tree, bone1).abs() < 1e-5,
        "a disabled LookAt entry must leave the bone alone"
    );
}

#[test]
fn an_unknown_modification_class_leaves_the_pose_untouched() {
    // The decline path: an entry naming a class with no solver must be inert
    // rather than corrupting a pose it does not understand. Six of the seven
    // subclasses are registered-but-unsolved, so this is their current contract.
    let (mut tree, skel, bone1) = rig();
    tree.get_node_mut(skel).unwrap().set_property(
        "__skelmod_0_class__",
        Variant::String("SkeletonModification2DJiggle".into()),
    );

    gdscene::skeleton2d::process_skeletons_2d(&mut tree);

    assert!(
        rotation_of(&tree, bone1).abs() < 1e-5,
        "an unsolved modification class must not move the bone"
    );
}

#[test]
fn the_solver_runs_from_a_real_mainloop_frame() {
    // The acceptance says the pass must run "from the SceneTree tick". The tests
    // above call `process_skeletons_2d` directly — which *is* that pass — but
    // this one drives a whole `MainLoop::step`, so a regression that unhooked
    // the 2D skeleton phase from the frame loop would fail here and nowhere
    // else. Routing, not just the solver.
    let (tree, _skel, bone1) = rig();
    let mut ml = MainLoop::new(tree);
    assert!(
        rotation_of(ml.tree(), bone1).abs() < 1e-5,
        "precondition: the bone starts at rest"
    );

    ml.step(1.0 / 60.0);

    let got = rotation_of(ml.tree(), bone1);
    assert!(
        (got - std::f32::consts::FRAC_PI_2).abs() < 1e-4,
        "one MainLoop frame must solve the LookAt, got {got} rad"
    );
}
