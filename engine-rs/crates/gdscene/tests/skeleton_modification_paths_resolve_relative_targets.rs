//! pat-s3rrv.185 regression guard: a modification's `NodePath` members survive
//! the `.tscn` load boundary, including paths that reach OUTSIDE the skeleton.
//!
//! Two defects made every scene-authored modification stack a silent no-op, and
//! neither was visible to the existing suite:
//!
//! * `mod_string` accepted only `Variant::String`. A `.tscn` writes these keys
//!   as `NodePath("Bone1")`, and the flattener copies the parsed Variant through
//!   verbatim — so each solver returned at its FIRST guard with the stack
//!   flattened, `enabled` true, and nothing to show for it.
//! * `resolve_from_skeleton` treated `..` as an ordinary name and searched for a
//!   child literally called "..", so any rig whose target lives outside the
//!   skeleton resolved to `None` and declined.
//!
//! ⚠️ The existing `skeleton_modification_2d_lookat_*` suite cannot catch
//! either one: it INJECTS `__skelmod_*` properties as `Variant::String` on a
//! hand-built tree, which is precisely the boundary that was broken. A guard
//! for this has to come through `.tscn` text.
//!
//! ⚠️ TwoBoneIK rather than LookAt deliberately. A `.tscn`-authored LookAt still
//! cannot run at all — `packed_scene.rs`'s flatten allowlist omits its
//! `bone2d_node`/`bone_index` members, so `look_at` returns at its first guard.
//! That gap belongs to pat-s3rrv.221 and is reported separately; testing through
//! it here would assert someone else's bug instead of this fix.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The same two-bone chain as the acceptance, but the IK target is a SIBLING of
/// the skeleton — reached as `../Target`, which is how a rig that aims at
/// something in the wider scene is normally authored.
///
/// Reach is 50 + 50 = 100 and the target sits exactly 100 away, so the chain is
/// at full extension and the tip has exactly one correct landing spot.
const REL_RIG: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="SkeletonModification2DTwoBoneIK" id="IK_1"]
target_nodepath = NodePath("../Target")
joint_one_bone2d_node = NodePath("Bone1")
joint_two_bone2d_node = NodePath("Bone1/Bone2")

[sub_resource type="SkeletonModificationStack2D" id="Stack_1"]
enabled = true
modifications/0 = SubResource("IK_1")

[node name="Main" type="Node2D"]

[node name="Skel" type="Skeleton2D" parent="."]
modification_stack = SubResource("Stack_1")

[node name="Bone1" type="Bone2D" parent="Skel"]
rest = Transform2D(1, 0, 0, 1, 0, 0)
length = 50.0

[node name="Bone2" type="Bone2D" parent="Skel/Bone1"]
position = Vector2(50, 0)
rest = Transform2D(1, 0, 0, 1, 50, 0)
length = 50.0

[node name="Target" type="Marker2D" parent="."]
position = Vector2(0, 100)
"#;

/// The identical rig aimed at a node that does not exist.
const MISSING_TARGET_RIG: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="SkeletonModification2DTwoBoneIK" id="IK_1"]
target_nodepath = NodePath("../NoSuchNode")
joint_one_bone2d_node = NodePath("Bone1")
joint_two_bone2d_node = NodePath("Bone1/Bone2")

[sub_resource type="SkeletonModificationStack2D" id="Stack_1"]
enabled = true
modifications/0 = SubResource("IK_1")

[node name="Main" type="Node2D"]

[node name="Skel" type="Skeleton2D" parent="."]
modification_stack = SubResource("Stack_1")

[node name="Bone1" type="Bone2D" parent="Skel"]
rest = Transform2D(1, 0, 0, 1, 0, 0)
length = 50.0

[node name="Bone2" type="Bone2D" parent="Skel/Bone1"]
position = Vector2(50, 0)
rest = Transform2D(1, 0, 0, 1, 50, 0)
length = 50.0
"#;

fn load(tscn: &str) -> SceneTree {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

fn node(tree: &SceneTree, path: &str) -> gdscene::NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

/// The far end of Bone2: its global position plus its length along its own
/// direction. Same measurement the acceptance makes.
fn tip_of(tree: &SceneTree) -> Vector2 {
    let bone2 = node(tree, "/root/Main/Skel/Bone1/Bone2");
    let joint = gdscene::node2d::get_global_position(tree, bone2);
    let angle = gdscene::node2d::get_global_rotation(tree, bone2);
    joint + Vector2::new(angle.cos(), angle.sin()) * 50.0
}

/// The rest tip, before anything solves: straight out along +X.
const REST_TIP: Vector2 = Vector2::new(100.0, 0.0);

#[test]
fn a_two_bone_ik_reaches_a_target_outside_the_skeleton() {
    let tree = load(REL_RIG);

    // The stack has to have flattened, or the assertion below would be
    // measuring a rig that was never wired rather than one that solved.
    let skel = node(&tree, "/root/Main/Skel");
    assert!(
        matches!(
            tree.get_node(skel)
                .map(|n| n.get_property("__skelmod_count__")),
            Some(Variant::Int(1))
        ),
        "the stack's one modification must flatten onto the Skeleton2D"
    );

    // 🔑 The control. The chain starts at full extension along +X, 100px from
    // where the target is — so "the tip is on the target" cannot be true by
    // accident of the rest pose.
    assert!(
        (tip_of(&tree) - REST_TIP).length() <= 1e-3,
        "the rig must start at its rest pose, got {:?}",
        tip_of(&tree)
    );

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    let tip = tip_of(ml.tree());
    let target =
        gdscene::node2d::get_global_position(ml.tree(), node(ml.tree(), "/root/Main/Target"));
    assert!(
        (tip - target).length() <= 1.0,
        "a `../Target` path must resolve out of the skeleton and solve: tip \
         {tip:?} vs target {target:?}"
    );
}

/// An unresolvable path still declines, leaving the pose untouched.
///
/// ⚠️ This is the half that keeps the fix honest. Teaching the resolver to walk
/// `..` must not make it permissive: a path naming nothing has to yield `None`
/// and skip the modification, NOT fall through to some default that aims the
/// chain at the origin. Without this, a resolver that returned the skeleton
/// itself on failure would pass the test above and quietly break every rig with
/// a typo in it.
#[test]
fn an_unresolvable_target_leaves_the_chain_at_rest() {
    let tree = load(MISSING_TARGET_RIG);
    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    let tip = tip_of(ml.tree());
    assert!(
        (tip - REST_TIP).length() <= 1e-3,
        "a modification whose target does not resolve must be skipped, leaving \
         the chain at rest, got {tip:?}"
    );
}
