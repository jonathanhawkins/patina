//! pat-0q2kx.239 — `CopyTransformModifier3D` and `ConvertTransformModifier3D`
//! run in `Skeleton3D`'s modifier pass.
//!
//! Godot ground truth: `scene/3d/copy_transform_modifier_3d.cpp` (Copy),
//! `scene/3d/convert_transform_modifier_3d.cpp` (Convert), over
//! `scene/3d/bone_constraint_3d.cpp`'s indexed `settings/<N>/…` array.
//!
//! Before this, `process_modifiers` knew only the IK solvers plus
//! `LookAtModifier3D`; both constraint classes were unregistered (a bare comment
//! in `class_db.rs`), so a configured constraint did nothing at all.
//!
//! ⚠️ The file is `bone_constraints` while the acceptance function is
//! `bone_constraint_modifiers_apply` — the verifier truncates a reported test
//! command at 58 chars, and `--test bone_constraint_modifiers` overruns it. The
//! acceptance name is a function name.
//!
//! ⚠️ These modifiers are *pose-pure*, which is why they can be tested by
//! calling `process_modifiers` directly: it takes no `delta` and keeps no state
//! between calls (a time-integrating modifier could not be tested this way, or
//! implemented at all against this signature).

use gdcore::math::Vector3;
use gdcore::math3d::{Quaternion, Transform3D};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{
    add_bone, get_bone_pose, get_bone_pose_position, get_bone_pose_scale, process_modifiers,
    set_bone_pose_position, set_bone_pose_rotation, set_bone_pose_scale, set_bone_rest,
};
use gdvariant::Variant;

/// A skeleton with two independent bones: `a` is the reference, `b` the target.
fn two_bones(tree: &mut SceneTree) -> (NodeId, usize, usize) {
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skeleton", "Skeleton3D"))
        .expect("skeleton added");
    let a = add_bone(tree, skel, "a");
    let b = add_bone(tree, skel, "b");
    set_bone_rest(tree, skel, a, Transform3D::IDENTITY);
    set_bone_rest(tree, skel, b, Transform3D::IDENTITY);
    (skel, a, b)
}

/// Attaches a constraint modifier with one configured setting.
fn add_constraint(
    tree: &mut SceneTree,
    skel: NodeId,
    class: &str,
    apply_bone: usize,
    reference_bone: usize,
    amount: f32,
    extra: &[(&str, Variant)],
) -> NodeId {
    let mut m = Node::new("Constraint", class);
    m.set_property("setting_count", Variant::Int(1));
    m.set_property("settings/0/apply_bone", Variant::Int(apply_bone as i64));
    m.set_property(
        "settings/0/reference_bone",
        Variant::Int(reference_bone as i64),
    );
    m.set_property("settings/0/amount", Variant::Float(amount as f64));
    for (k, v) in extra {
        m.set_property(k, v.clone());
    }
    tree.add_child(skel, m).expect("modifier added")
}

/// **The acceptance test.** ClassDB knows both classes under
/// `BoneConstraint3D`; a configured Copy writes the reference bone's transform
/// onto the apply bone during the modifier pass; Convert maps a source axis
/// component onto the configured target.
#[test]
fn bone_constraint_modifiers_apply() {
    gdobject::class_db::register_3d_classes();

    // -- ClassDB clause -----------------------------------------------------
    for class in ["CopyTransformModifier3D", "ConvertTransformModifier3D"] {
        assert!(
            gdobject::class_db::class_exists(class),
            "{class} must be registered"
        );
        assert!(
            gdobject::class_db::is_parent_class(class, "BoneConstraint3D"),
            "{class} must inherit BoneConstraint3D"
        );
        // And therefore reach SkeletonModifier3D through it, which is what makes
        // Skeleton3D's modifier pass consider it at all.
        assert!(
            gdobject::class_db::is_parent_class(class, "SkeletonModifier3D"),
            "{class} must reach SkeletonModifier3D through BoneConstraint3D"
        );
    }

    // -- Copy clause --------------------------------------------------------
    let mut tree = SceneTree::new();
    let (skel, a, b) = two_bones(&mut tree);
    // Reference bone `a` is moved somewhere distinctive; `b` starts elsewhere so
    // an implementation that simply leaves `b` alone cannot pass.
    set_bone_pose_position(&mut tree, skel, a, Vector3::new(3.0, 4.0, 5.0));
    set_bone_pose_scale(&mut tree, skel, a, Vector3::new(2.0, 2.0, 2.0));
    set_bone_pose_position(&mut tree, skel, b, Vector3::new(-9.0, -9.0, -9.0));
    add_constraint(&mut tree, skel, "CopyTransformModifier3D", b, a, 1.0, &[]);

    process_modifiers(&mut tree, skel);

    let pos = get_bone_pose_position(&tree, skel, b);
    assert!(
        (pos.x - 3.0).abs() < 1e-4 && (pos.y - 4.0).abs() < 1e-4 && (pos.z - 5.0).abs() < 1e-4,
        "amount=1 must copy the reference position exactly, got {pos:?}"
    );
    let scl = get_bone_pose_scale(&tree, skel, b);
    assert!(
        (scl.x - 2.0).abs() < 1e-3,
        "scale must be copied too (copy_flags defaults to ALL), got {scl:?}"
    );

    // -- Convert clause -----------------------------------------------------
    // Map reference position X (range 0..10) onto apply position Y (range 0..1).
    // A reference X of 5 is halfway, so Y must land at 0.5.
    let mut tree = SceneTree::new();
    let (skel, a, b) = two_bones(&mut tree);
    set_bone_pose_position(&mut tree, skel, a, Vector3::new(5.0, 0.0, 0.0));
    add_constraint(
        &mut tree,
        skel,
        "ConvertTransformModifier3D",
        b,
        a,
        1.0,
        &[
            ("settings/0/reference_transform_mode", Variant::Int(0)),
            ("settings/0/reference_axis", Variant::Int(0)),
            ("settings/0/reference_range_min", Variant::Float(0.0)),
            ("settings/0/reference_range_max", Variant::Float(10.0)),
            ("settings/0/apply_transform_mode", Variant::Int(0)),
            ("settings/0/apply_axis", Variant::Int(1)),
            ("settings/0/apply_range_min", Variant::Float(0.0)),
            ("settings/0/apply_range_max", Variant::Float(1.0)),
            // Absolute, so the expected value is the mapped number itself.
            ("settings/0/relative", Variant::Bool(false)),
        ],
    );

    process_modifiers(&mut tree, skel);

    let pos = get_bone_pose_position(&tree, skel, b);
    assert!(
        (pos.y - 0.5).abs() < 1e-4,
        "reference X=5 in 0..10 must map to apply Y=0.5 in 0..1, got {pos:?}"
    );
}

/// `amount` blends rather than snapping: at 0.5 the apply bone lands halfway
/// between where it was and the reference. Without this, an implementation that
/// ignored `amount` entirely would still pass the acceptance above.
#[test]
fn copy_amount_blends_toward_the_reference() {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let (skel, a, b) = two_bones(&mut tree);
    set_bone_pose_position(&mut tree, skel, a, Vector3::new(10.0, 0.0, 0.0));
    set_bone_pose_position(&mut tree, skel, b, Vector3::new(0.0, 0.0, 0.0));
    add_constraint(&mut tree, skel, "CopyTransformModifier3D", b, a, 0.5, &[]);

    process_modifiers(&mut tree, skel);

    let pos = get_bone_pose_position(&tree, skel, b);
    assert!(
        (pos.x - 5.0).abs() < 1e-3,
        "amount=0.5 must land halfway (5.0), got {pos:?}"
    );
}

/// `copy_flags` selects which components are written. Copying rotation only must
/// leave the apply bone's position untouched — and this is the assertion that
/// catches reading the flag default as enum-zero, which would mask everything
/// and write nothing at all.
#[test]
fn copy_flags_select_components() {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let (skel, a, b) = two_bones(&mut tree);
    set_bone_pose_position(&mut tree, skel, a, Vector3::new(7.0, 7.0, 7.0));
    set_bone_pose_rotation(
        &mut tree,
        skel,
        a,
        Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), 1.0),
    );
    set_bone_pose_position(&mut tree, skel, b, Vector3::new(1.0, 2.0, 3.0));
    add_constraint(
        &mut tree,
        skel,
        "CopyTransformModifier3D",
        b,
        a,
        1.0,
        // TRANSFORM_FLAG_ROTATION only.
        &[("settings/0/copy_flags", Variant::Int(2))],
    );

    process_modifiers(&mut tree, skel);

    let pos = get_bone_pose_position(&tree, skel, b);
    assert!(
        (pos.x - 1.0).abs() < 1e-4 && (pos.y - 2.0).abs() < 1e-4 && (pos.z - 3.0).abs() < 1e-4,
        "rotation-only copy must not move the bone, got {pos:?}"
    );
    let rot = get_bone_pose(&tree, skel, b)
        .basis
        .get_rotation_quaternion();
    assert!(
        rot.y.abs() > 1e-3,
        "rotation-only copy must still rotate the bone, got {rot:?}"
    );
}

/// An inactive modifier does nothing — the control that keeps the assertions
/// above from passing for an implementation that ignores `active`.
#[test]
fn inactive_constraint_is_skipped() {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let (skel, a, b) = two_bones(&mut tree);
    set_bone_pose_position(&mut tree, skel, a, Vector3::new(3.0, 4.0, 5.0));
    set_bone_pose_position(&mut tree, skel, b, Vector3::new(-1.0, -1.0, -1.0));
    let m = add_constraint(&mut tree, skel, "CopyTransformModifier3D", b, a, 1.0, &[]);
    tree.get_node_mut(m)
        .unwrap()
        .set_property("active", Variant::Bool(false));

    process_modifiers(&mut tree, skel);

    let pos = get_bone_pose_position(&tree, skel, b);
    assert!(
        (pos.x + 1.0).abs() < 1e-4,
        "an inactive modifier must not write, got {pos:?}"
    );
}
