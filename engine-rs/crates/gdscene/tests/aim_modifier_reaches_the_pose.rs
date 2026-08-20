//! pat-s3rrv.132 — `AimModifier3D` turns a bone toward its reference bone.
//!
//! `process_modifiers` had no arm for this class — it fell to `_ => continue` —
//! so every knob the inspector showed did nothing. The rotation core
//! (`ik_solver::aim_rotation`) had existed with its own unit tests and **zero
//! non-test callers**.
//!
//! ⚠️ `AimModifier3D` is a per-setting class: its only direct property is
//! `setting_count`, and the real knobs live under `settings/<i>/…`
//! (`scene/3d/bone_constraint_3d.cpp:101`, `aim_modifier_3d.cpp:88`). Reading
//! flat `bone`/`forward_axis` the way `LookAtModifier3D` does finds nothing.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{
    add_bone, get_bone_global_pose, process_modifiers, set_bone_parent, set_bone_rest,
};
use gdvariant::Variant;

/// Two independent bones: the aimer at the origin, the reference out along +X.
/// Aiming the aimer's +Z at the reference is therefore a 90° turn.
fn make_pair(tree: &mut SceneTree) -> (NodeId, usize, usize) {
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skeleton", "Skeleton3D"))
        .expect("skeleton added");

    let aimer = add_bone(tree, skel, "aimer");
    let reference = add_bone(tree, skel, "reference");
    set_bone_parent(tree, skel, aimer, -1);
    set_bone_parent(tree, skel, reference, -1);

    set_bone_rest(tree, skel, aimer, Transform3D::IDENTITY);
    set_bone_rest(
        tree,
        skel,
        reference,
        Transform3D {
            origin: Vector3::new(4.0, 0.0, 0.0),
            ..Transform3D::IDENTITY
        },
    );
    (skel, aimer, reference)
}

/// Attaches an AimModifier3D with one setting. `amount` is written only when
/// `Some`, so the default-value case can be exercised.
fn add_aim(
    tree: &mut SceneTree,
    skel: NodeId,
    apply_bone: usize,
    reference_bone: usize,
    amount: Option<f64>,
) -> NodeId {
    let mut m = Node::new("Aim", "AimModifier3D");
    m.set_property("setting_count", Variant::Int(1));
    m.set_property("settings/0/apply_bone", Variant::Int(apply_bone as i64));
    m.set_property(
        "settings/0/reference_bone",
        Variant::Int(reference_bone as i64),
    );
    // 4 = BONE_AXIS_PLUS_Z, the forward default.
    m.set_property("settings/0/forward_axis", Variant::Int(4));
    if let Some(a) = amount {
        m.set_property("settings/0/amount", Variant::Float(a));
    }
    tree.add_child(skel, m).expect("modifier added")
}

/// The aimer's forward (+Z) direction in world space after the pass.
fn forward_after_pass(tree: &mut SceneTree, skel: NodeId, aimer: usize) -> Vector3 {
    process_modifiers(tree, skel);
    get_bone_global_pose(tree, skel, aimer)
        .basis
        .xform(Vector3::new(0.0, 0.0, 1.0))
}

/// **The core of the slice.** The bone's forward axis ends up pointing at the
/// reference bone.
#[test]
fn the_aimed_bone_turns_to_face_its_reference() {
    let mut tree = SceneTree::new();
    let (skel, aimer, reference) = make_pair(&mut tree);
    add_aim(&mut tree, skel, aimer, reference, Some(1.0));

    let fwd = forward_after_pass(&mut tree, skel, aimer);

    // The reference sits along +X, so a fully-applied aim points +Z there.
    assert!(
        fwd.x > 0.99,
        "forward should end up along +X toward the reference, got {fwd:?}"
    );
    assert!(fwd.z.abs() < 0.05, "and should no longer point along Z");
}

/// ⚠️ `amount` is a 0..1 blend. A half amount must turn part of the way — not
/// all of it, and not none.
#[test]
fn the_amount_blends_the_turn() {
    let mut tree = SceneTree::new();
    let (skel, aimer, reference) = make_pair(&mut tree);
    add_aim(&mut tree, skel, aimer, reference, Some(0.5));

    let fwd = forward_after_pass(&mut tree, skel, aimer);

    // Half of a 90° turn is 45°: both components positive and comparable.
    assert!(
        fwd.x > 0.6 && fwd.z > 0.6,
        "a half amount should land about halfway, got {fwd:?}"
    );
}

/// 🔑 A missing `amount` defaults to **1.0**, not 0.0.
///
/// ⚠️ Reading an absent float as `0.0` — the natural `unwrap_or_default()` — is
/// a silent disable: the inspector shows the setting configured and the bone
/// never moves. This scene omits the key entirely, so it fails for that version
/// while every other test here still passes.
#[test]
fn an_omitted_amount_means_fully_applied() {
    let mut tree = SceneTree::new();
    let (skel, aimer, reference) = make_pair(&mut tree);
    add_aim(&mut tree, skel, aimer, reference, None);

    let fwd = forward_after_pass(&mut tree, skel, aimer);
    assert!(
        fwd.x > 0.99,
        "an omitted amount must mean 1.0, not 0.0 — got {fwd:?}"
    );
}

/// The control: with no settings the pass leaves the pose alone, so adding this
/// arm cannot have disturbed skeletons that carry an unconfigured modifier.
#[test]
fn a_modifier_with_no_settings_changes_nothing() {
    let mut tree = SceneTree::new();
    let (skel, aimer, _reference) = make_pair(&mut tree);
    let mut m = Node::new("Aim", "AimModifier3D");
    m.set_property("setting_count", Variant::Int(0));
    tree.add_child(skel, m).expect("modifier added");

    let fwd = forward_after_pass(&mut tree, skel, aimer);
    assert!(
        fwd.z > 0.99,
        "an unconfigured modifier must leave forward at +Z, got {fwd:?}"
    );
}
