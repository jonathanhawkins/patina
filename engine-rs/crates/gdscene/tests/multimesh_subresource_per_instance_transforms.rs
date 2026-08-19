//! pat-s3rrv.112 — a MultiMeshInstance3D's inline `MultiMesh` sub-resource
//! reaches the per-instance transform lookup.
//!
//! `sync_multimesh_instances` already expanded a MultiMeshInstance3D into N
//! render instances, but `node3d::get_multimesh_instance_transform` took an
//! `_index` it discarded and returned the node's single `instance_transform`
//! property. Every one of the N copies therefore rendered at the same place —
//! a MultiMesh that draws one visible object.
//!
//! 🔑 **The buffer is row-major and our `Basis` is column-major**, so the decode
//! strides rather than reading each column contiguously. A pure-translation
//! fixture cannot tell the two apart — both put the origin in the right place —
//! so the fixture below carries an asymmetric basis whose transpose is a
//! different matrix.

use gdcore::math::{Color, Vector3};
use gdscene::node::NodeId;
use gdscene::node3d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Three instances. Godot packs each as a row-major 3x4 matrix — three basis
/// rows, each followed by one origin component.
///
/// - instance 0: identity basis, origin `(10, 0, 0)`
/// - instance 1: rows `(1,2,3) (4,5,6) (7,8,9)`, origin `(0, 20, 0)`
/// - instance 2: identity basis, origin `(0, 0, 30)`
///
/// ⚠️ Instance 1's basis is deliberately not a rotation and not symmetric. A
/// symmetric matrix, an identity, or a pure translation would all decode
/// identically under the transposed reading, so none of them can fail this
/// test. `1..9` makes the correct answer and the transposed answer disjoint.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="MultiMesh" id="MultiMesh_1"]
transform_format = 1
instance_count = 3
buffer = PackedFloat32Array(1, 0, 0, 10, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 3, 0, 4, 5, 6, 20, 7, 8, 9, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 30)

[node name="Root" type="Node3D"]

[node name="MM" type="MultiMeshInstance3D" parent="."]
multimesh = SubResource("MultiMesh_1")
"#;

/// Two instances with `use_colors`, so each instance's stride grows by the
/// 4-float color block that follows its transform.
///
/// - instance 0: origin `(1, 0, 0)`, color `(1, 0, 0, 1)`
/// - instance 1: origin `(2, 0, 0)`, color `(0, 0, 1, 1)`
const COLOR_SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="MultiMesh" id="MultiMesh_2"]
transform_format = 1
use_colors = true
instance_count = 2
buffer = PackedFloat32Array(1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 2, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1)

[node name="Root" type="Node3D"]

[node name="MM" type="MultiMeshInstance3D" parent="."]
multimesh = SubResource("MultiMesh_2")
"#;

/// A MultiMeshInstance3D with no sub-resource at all, exercising the property
/// path that existed before this change.
const LEGACY_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="MM" type="MultiMeshInstance3D" parent="."]
instance_count = 2
instance_transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 6, 7)
"#;

fn load(src: &str) -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mm = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == "MM")
                .unwrap_or(false)
        })
        .expect("no node named MM");
    (tree, mm)
}

#[test]
fn multimesh_sub_resource_reaches_the_node() {
    let (tree, mm) = load(SCENE);

    // Positive control: without the flattening, every assertion below would be
    // reading the fallback path and passing for the wrong reason.
    assert!(
        matches!(
            tree.get_node(mm).unwrap().get_property("__multimesh__buffer"),
            Variant::PackedFloat32Array(ref v) if v.len() == 36
        ),
        "the .tscn MultiMesh must reach the node as __multimesh__* companions"
    );
    assert_eq!(
        node3d::get_multimesh_instance_count(&tree, mm),
        3,
        "instance_count comes from the sub-resource, not the node"
    );
}

#[test]
fn each_instance_gets_its_own_transform() {
    let (tree, mm) = load(SCENE);

    let origins: Vec<Vector3> = (0..3)
        .map(|i| node3d::get_multimesh_instance_transform(&tree, mm, i).origin)
        .collect();

    // 🔑 The regression this bead names: the index used to be discarded, so all
    // three of these were the same value and the MultiMesh drew one object.
    assert_eq!(origins[0], Vector3::new(10.0, 0.0, 0.0));
    assert_eq!(origins[1], Vector3::new(0.0, 20.0, 0.0));
    assert_eq!(origins[2], Vector3::new(0.0, 0.0, 30.0));
}

#[test]
fn the_basis_is_not_transposed() {
    let (tree, mm) = load(SCENE);
    let basis = node3d::get_multimesh_instance_transform(&tree, mm, 1).basis;

    // Godot wrote the ROWS (1,2,3), (4,5,6), (7,8,9). Our `Basis` holds
    // COLUMNS, so the columns are (1,4,7), (2,5,8), (3,6,9).
    //
    // ⚠️ Reading the buffer three floats at a time — the obvious decode — gives
    // columns (1,2,3), (4,5,6), (7,8,9) instead. That is the transpose, and it
    // renders every rotated instance mirrored about the diagonal.
    assert_eq!(
        basis.x,
        Vector3::new(1.0, 4.0, 7.0),
        "basis.x is buffer[0], [4], [8] — the first element of each ROW, not \
         the first three floats"
    );
    assert_eq!(basis.y, Vector3::new(2.0, 5.0, 8.0));
    assert_eq!(basis.z, Vector3::new(3.0, 6.0, 9.0));
}

#[test]
fn use_colors_widens_the_stride_and_yields_per_instance_colors() {
    let (tree, mm) = load(COLOR_SCENE);

    // The color block sits after the transform, so instance 1 starts 16 floats
    // in, not 12. Getting the stride wrong reads instance 1's transform out of
    // instance 0's color block — which is why the origins are checked here too.
    assert_eq!(
        node3d::get_multimesh_instance_transform(&tree, mm, 0).origin,
        Vector3::new(1.0, 0.0, 0.0)
    );
    assert_eq!(
        node3d::get_multimesh_instance_transform(&tree, mm, 1).origin,
        Vector3::new(2.0, 0.0, 0.0),
        "instance 1 begins at float 16 once use_colors widens the stride"
    );

    assert_eq!(
        node3d::get_multimesh_instance_color(&tree, mm, 0),
        Color::new(1.0, 0.0, 0.0, 1.0)
    );
    assert_eq!(
        node3d::get_multimesh_instance_color(&tree, mm, 1),
        Color::new(0.0, 0.0, 1.0, 1.0),
        "the two colors must differ; a stride that ignored use_colors would \
         return the same block twice"
    );
}

#[test]
fn a_node_without_a_sub_resource_still_uses_its_own_properties() {
    let (tree, mm) = load(LEGACY_SCENE);

    // The pre-existing path, kept working. Both instances share the node's one
    // transform here — correct, because that is all the scene authored.
    assert_eq!(node3d::get_multimesh_instance_count(&tree, mm), 2);
    for i in 0..2 {
        assert_eq!(
            node3d::get_multimesh_instance_transform(&tree, mm, i).origin,
            Vector3::new(5.0, 6.0, 7.0),
            "with no buffer authored, every instance falls back to \
             instance_transform"
        );
    }
    assert_eq!(
        node3d::get_multimesh_instance_color(&tree, mm, 0),
        Color::WHITE,
        "and to WHITE for color"
    );
}
