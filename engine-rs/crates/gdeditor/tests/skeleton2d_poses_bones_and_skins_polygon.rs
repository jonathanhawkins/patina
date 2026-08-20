//! pat-s3rrv.185 acceptance: a `Skeleton2D` poses its `Bone2D` chain and a bound
//! `Polygon2D` is drawn deformed by it.
//!
//! Every piece existed and nothing ran. `gdrender2d::polygon_raster` has carried
//! `Bone2D`, `Skeleton2D::skinning_transforms` and `deform_polygon_vertices`
//! with **no caller outside its own unit tests**; `ik_solver_2d` had exactly one
//! workspace reference — its own `pub mod` line; and pat-s3rrv.135 registered
//! the modification classes while stating in its own header that "the stack is
//! still never *executed*". This pins the phase that joins them.
//!
//! ⚠️ Fixtures are `.tscn` text, not injected properties. Two of the three
//! defects this bead fixes live at load boundaries — the `modification_stack`
//! sub-resource chain and `Polygon2D.bones` — and injecting node properties
//! would skip exactly the code that was broken.
//!
//! Ported from `upstream/godot/scene/2d/skeleton_2d.cpp` and `polygon_2d.cpp`.

use gdcore::math::Vector2;
use gdeditor::viewport_2d_render::{build_runtime_render_list, Render2DKind};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A two-bone chain along +X with a quad skinned one bone per end.
///
/// Bone1 sits at the origin, Bone2 50px along it. The polygon's left edge is
/// weighted entirely to Bone1 and its right edge entirely to Bone2, so each
/// bone's transform shows up on its own pair of vertices and a mix-up between
/// them is visible rather than averaged away.
const RIG: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="Skel" type="Skeleton2D" parent="."]

[node name="Bone1" type="Bone2D" parent="Skel"]
rest = Transform2D(1, 0, 0, 1, 0, 0)
length = 50.0

[node name="Bone2" type="Bone2D" parent="Skel/Bone1"]
position = Vector2(50, 0)
rest = Transform2D(1, 0, 0, 1, 50, 0)
length = 50.0

[node name="Poly" type="Polygon2D" parent="."]
polygon = PackedVector2Array(0, -10, 0, 10, 100, -10, 100, 10)
skeleton = NodePath("../Skel")
bones = ["Bone1", PackedFloat32Array(1, 1, 0, 0), "Bone1/Bone2", PackedFloat32Array(0, 0, 1, 1)]
"#;

/// The same chain driven by a TwoBoneIK modification aimed at a Marker2D.
///
/// Reach is 50 + 50 = 100 and the target sits exactly 100 away, so the chain is
/// at full extension — the one distance where the solver's clamping and its
/// bend-direction choice both have to be right for the tip to land.
const IK_RIG: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="SkeletonModification2DTwoBoneIK" id="IK_1"]
target_nodepath = NodePath("Target")
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

[node name="Target" type="Marker2D" parent="Skel"]
position = Vector2(0, 100)
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

/// The polygon's emitted points, as the rasterizer receives them.
fn polygon_points(tree: &SceneTree) -> Vec<Vector2> {
    build_runtime_render_list(tree)
        .into_iter()
        .find_map(|item| match item.kind {
            Render2DKind::Polygon { points, .. } => Some(points),
            _ => None,
        })
        .expect("the Polygon2D must produce a Polygon render item")
}

fn approx(a: Vector2, b: Vector2, eps: f32) -> bool {
    (a - b).length() <= eps
}

/// **The acceptance test.** Rest on frame 0, then the bones move the polygon.
#[test]
fn skeleton2d_poses_bones_and_skins_polygon() {
    let tree = load(RIG);

    // 🔑 The `.tscn` data crossed both load boundaries. `bones` was not even a
    // registered property before this bead, and a polygon with no weights skins
    // to nothing — every assertion below would then pass on the rest pose.
    let poly = node(&tree, "/root/Main/Poly");
    assert!(
        matches!(tree.get_node(poly).map(|n| n.get_property("bones")), Some(Variant::Array(v)) if v.len() == 4),
        "`bones` must load as four alternating path/weight entries"
    );

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    // -- clause 1: rest pose renders at the authored vertices ---------------
    //
    // Every bone's pose equals its rest, so each skinning transform is the
    // identity and the polygon is drawn exactly as authored. This is a real
    // assertion, not a tautology: an implementation that skinned against an
    // identity REST instead of the authored one would apply each bone's whole
    // world transform here and move the right-hand pair by 50px.
    let rest = polygon_points(ml.tree());
    let authored = [
        Vector2::new(0.0, -10.0),
        Vector2::new(0.0, 10.0),
        Vector2::new(100.0, -10.0),
        Vector2::new(100.0, 10.0),
    ];
    assert_eq!(rest.len(), 4);
    for (got, want) in rest.iter().zip(authored.iter()) {
        assert!(
            approx(*got, *want, 1e-3),
            "rest pose moved: {got:?} vs {want:?}"
        );
    }

    // -- clause 2: rotating the root bone moves the skinned vertices ---------
    let bone1 = node(ml.tree(), "/root/Main/Skel/Bone1");
    gdscene::node2d::set_rotation(ml.tree_mut(), bone1, std::f32::consts::FRAC_PI_4);
    ml.step(0.016);
    let posed = polygon_points(ml.tree());

    // Derived, not measured. Bone1's skinning transform is a 45-degree rotation
    // about the origin, so its two vertices rotate: (0, -10) -> (7.071, -7.071).
    assert!(
        approx(posed[0], Vector2::new(7.0711, -7.0711), 1e-2),
        "vertex 0 follows Bone1's 45-degree rotation, got {:?}",
        posed[0]
    );
    assert!(
        approx(posed[1], Vector2::new(-7.0711, 7.0711), 1e-2),
        "vertex 1 follows Bone1, got {:?}",
        posed[1]
    );
    // ⚠️ Bone2's vertices move DIFFERENTLY: its skinning transform is
    // `pose * rest⁻¹` with a rest translated 50px along X, so the far edge
    // swings about the elbow rather than about the origin. Asserting the same
    // rotation for all four would accept an implementation that ignored `rest`.
    assert!(
        approx(posed[2], Vector2::new(77.7817, 63.6396), 1e-2),
        "vertex 2 follows Bone2 about the elbow, got {:?}",
        posed[2]
    );
    assert!(
        approx(posed[3], Vector2::new(63.6396, 77.7817), 1e-2),
        "vertex 3 follows Bone2, got {:?}",
        posed[3]
    );
}

/// **Clause 3.** A TwoBoneIK entry bends the chain onto its target.
#[test]
fn a_two_bone_ik_modification_lands_the_tip_on_its_target() {
    let tree = load(IK_RIG);
    let skel = node(&tree, "/root/Main/Skel");

    // The two-level sub-resource chain reached the node: without the flatten,
    // `modification_stack` is an inert string and the stack never runs.
    assert!(
        matches!(
            tree.get_node(skel)
                .map(|n| n.get_property("__skelmod_count__")),
            Some(Variant::Int(1))
        ),
        "the stack's one modification must flatten onto the Skeleton2D"
    );

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    // The tip is the far end of Bone2: its global position plus its length
    // along its own direction.
    let bone2 = node(ml.tree(), "/root/Main/Skel/Bone1/Bone2");
    let joint = gdscene::node2d::get_global_position(ml.tree(), bone2);
    let angle = gdscene::node2d::get_global_rotation(ml.tree(), bone2);
    let tip = joint + Vector2::new(angle.cos(), angle.sin()) * 50.0;

    let target =
        gdscene::node2d::get_global_position(ml.tree(), node(ml.tree(), "/root/Main/Skel/Target"));
    assert!(
        approx(tip, target, 1.0),
        "the IK tip must land within 1px of the target: tip {tip:?} vs target {target:?}"
    );
}

/// A polygon with no skeleton is left exactly as authored.
///
/// Without this, "the rest pose matches" above could hold because skinning never
/// ran at all rather than because it ran and was the identity.
#[test]
fn a_polygon_without_a_skeleton_is_untouched() {
    let tree = load(
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n\n\
         [node name=\"Poly\" type=\"Polygon2D\" parent=\".\"]\n\
         polygon = PackedVector2Array(0, 0, 10, 0, 10, 10)\n",
    );
    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    let pts = polygon_points(ml.tree());
    assert_eq!(pts.len(), 3);
    assert!(approx(pts[1], Vector2::new(10.0, 0.0), 1e-4));
}
