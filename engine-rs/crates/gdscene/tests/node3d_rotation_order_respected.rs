//! pat-mgixk.29 item 2: a Node3D's `rotation_order` decides how its `rotation`
//! Euler triple is composed into the rendered basis, and moving the node does
//! not rewrite the authored triple.
//!
//! Godot ground truth: `Node3D.rotation_order` selects the sequence the three
//! axis rotations multiply in (`scene/3d/node_3d.cpp`, `Basis::set_euler`), and
//! every transform property is a view of one `data.local_transform`. The
//! default is **YXZ**, not the more familiar XYZ — `Basis::get_euler()` and the
//! editor's own rotation fields both use YXZ, so a port that defaulted to XYZ
//! would disagree with the inspector.
//!
//! Patina composed `rotation` with a hardcoded YXZ and ignored `rotation_order`
//! entirely: `grep -rn rotation_order gdscene/src` found **nothing**, while
//! ClassDB has registered the property (with Godot's default of `2`) all along.
//! So a scene authored in the editor with any non-default order rendered at the
//! wrong orientation, silently and permanently.
//!
//! # What makes this test non-vacuous
//!
//! The two nodes carry the SAME `rotation` and differ only in `rotation_order`,
//! so the orientations can only diverge if the order is actually read. The
//! authored properties are asserted present first: a `.tscn` parser that
//! dropped `rotation_order`, or lowered `rotation` into a `transform` (which
//! `local_transform_of` would then prefer, bypassing composition entirely),
//! would otherwise make every assertion below pass for the wrong reason.
//!
//! The expected bases are computed with `Basis::from_euler_order`, which is the
//! math this wiring newly reaches — NOT re-derived here. That keeps the test
//! about the wiring rather than re-implementing the composition beside it.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, EulerOrder};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, NodeId};
use gdvariant::Variant;

/// The angles the bead names. Both nodes get exactly these; only the order
/// differs. Non-zero on two axes on purpose — a single-axis rotation composes
/// identically under every order, which would make the comparison vacuous.
const ROT: Vector3 = Vector3::new(0.5, 0.7, 0.0);

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Default" type="MeshInstance3D" parent="."]
rotation = Vector3(0.5, 0.7, 0)
rotation_order = 2

[node name="Xyz" type="MeshInstance3D" parent="."]
rotation = Vector3(0.5, 0.7, 0)
rotation_order = 0
"#;

fn load() -> (SceneTree, NodeId, NodeId) {
    // 🔑 Registering the 3D classes is the REALISTIC path — `patina-runner`
    // does it — and it is what makes this test a witness for the other half of
    // pat-mgixk.29.
    //
    // `packed_scene::node_with_class_defaults` applies every ClassDB default
    // BEFORE the scene's own properties. Node3D registers `transform` with an
    // identity default, and `node3d::local_transform_of` PREFERS an explicit
    // `transform` — so until this bead, a seeded identity beat the authored
    // `rotation` and every node below rendered unrotated. With classes
    // registered and that seeding fixed, the authored `rotation` wins and the
    // Euler composition this test is about actually runs.
    //
    // So without the fix this test fails for TWO independent reasons: the
    // `transform`-is-Nil control below trips, and both bases collapse to
    // identity so the "orders differ" assertion has nothing to see.
    gdobject::class_db::register_3d_classes();

    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");
    let yxz = tree
        .get_node_or_null(scene_root, "Default")
        .expect("the YXZ node exists");
    let xyz = tree
        .get_node_or_null(scene_root, "Xyz")
        .expect("the XYZ node exists");
    (tree, yxz, xyz)
}

fn max_delta(a: Basis, b: Basis) -> f32 {
    let pairs = [
        (a.x.x, b.x.x),
        (a.x.y, b.x.y),
        (a.x.z, b.x.z),
        (a.y.x, b.y.x),
        (a.y.y, b.y.y),
        (a.y.z, b.y.z),
        (a.z.x, b.z.x),
        (a.z.y, b.z.y),
        (a.z.z, b.z.z),
    ];
    pairs
        .iter()
        .map(|(l, r)| (l - r).abs())
        .fold(0.0f32, f32::max)
}

fn vec3_of(tree: &SceneTree, node: NodeId, prop: &str) -> Vector3 {
    match tree.get_node(node).unwrap().get_property(prop) {
        Variant::Vector3(v) => v,
        other => panic!("`{prop}` should be a Vector3, got {other:?}"),
    }
}

/// **The acceptance test.** Same Euler angles, different `rotation_order`,
/// different rendered basis — each matching that order's composition.
#[test]
fn node3d_rotation_order_respected() {
    let (mut tree, yxz, xyz) = load();

    // -- controls: the fixture really authored what the test assumes ---------
    //
    // Without these, a parser that dropped `rotation_order` would leave both
    // nodes on the default order and "the bases differ" would simply fail,
    // while a parser that lowered `rotation` into a `transform` would make the
    // comparisons pass without composition ever running.
    assert_eq!(
        vec3_of(&tree, yxz, "rotation"),
        ROT,
        "the fixture must author `rotation` as a Vector3 on the YXZ node",
    );
    assert_eq!(
        vec3_of(&tree, xyz, "rotation"),
        ROT,
        "...and the same angles on the XYZ node, so order is the ONLY difference",
    );
    assert_eq!(
        tree.get_node(xyz).unwrap().get_property("rotation_order"),
        Variant::Int(0),
        "the fixture must author `rotation_order = 0`; if the parser dropped it \
         this node silently uses the default and the test proves nothing",
    );
    assert!(
        tree.get_node(xyz)
            .unwrap()
            .get_property("transform")
            .is_nil(),
        "the fixture must NOT carry an explicit `transform` — `local_transform_of` \
         prefers one, which would bypass Euler composition entirely",
    );

    // -- the orders produce genuinely different orientations ----------------
    let b_yxz = node3d::get_global_transform(&tree, yxz).basis;
    let b_xyz = node3d::get_global_transform(&tree, xyz).basis;
    let spread = max_delta(b_yxz, b_xyz);
    assert!(
        spread > 1e-3,
        "two nodes with the same `rotation` and different `rotation_order` must \
         render at different orientations; the bases agree to {spread}, which \
         means `rotation_order` was never read",
    );

    // -- ...and each matches ITS OWN order's composition ---------------------
    //
    // This is what rules out "the orders differ, but both are wrong".
    let want_yxz = Basis::from_euler_order(ROT, EulerOrder::Yxz);
    let want_xyz = Basis::from_euler_order(ROT, EulerOrder::Xyz);
    assert!(
        max_delta(b_yxz, want_yxz) < 1e-5,
        "the default-order node must match from_euler_order(.., Yxz); got \
         {b_yxz:?} want {want_yxz:?}",
    );
    assert!(
        max_delta(b_xyz, want_xyz) < 1e-5,
        "the `rotation_order = 0` node must match from_euler_order(.., Xyz); \
         got {b_xyz:?} want {want_xyz:?}",
    );

    // -- moving the node leaves the authored triple alone -------------------
    //
    // `set_position` reads the transform back through `local_transform_of`
    // (composing with the node's order) and writes every view through
    // `store_local_transform` (decomposing). Decomposing with a DIFFERENT order
    // than the composition used would rewrite `rotation` into another triple
    // naming the same basis — so simply moving a node would change its authored
    // rotation numbers in the inspector.
    node3d::set_position(&mut tree, xyz, Vector3::new(1.0, 2.0, 3.0));
    let after = vec3_of(&tree, xyz, "rotation");
    let drift = (after.x - ROT.x)
        .abs()
        .max((after.y - ROT.y).abs())
        .max((after.z - ROT.z).abs());
    assert!(
        drift < 1e-4,
        "`rotation` must read back unchanged after set_position; drifted by \
         {drift} to {after:?}",
    );
    // ...and the move itself landed, so the assertion above is not passing
    // because `set_position` did nothing at all.
    assert_eq!(
        node3d::get_global_transform(&tree, xyz).origin,
        Vector3::new(1.0, 2.0, 3.0),
        "set_position must still move the node",
    );
    // The orientation survived the move too, not just the property view.
    assert!(
        max_delta(node3d::get_global_transform(&tree, xyz).basis, want_xyz) < 1e-5,
        "moving the node must not re-compose its basis under a different order",
    );
}
