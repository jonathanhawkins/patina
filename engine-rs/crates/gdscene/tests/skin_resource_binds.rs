//! pat-s3rrv.200 — a `MeshInstance3D`'s `skin` resource drives the deformation
//! instead of the skeleton's rest pose.
//!
//! Godot ground truth: `upstream/godot/scene/3d/mesh_instance_3d.cpp` registers
//! the instance's `skin` with the skeleton, and only when that property is null
//! does the skeleton supply `create_skin_from_rest_transforms()`. Our
//! `resolve_skin` took the *skeleton alone* — the mesh node was not even a
//! parameter — so it always synthesized the rest skin and a rigged model's
//! authored bind poses were unreachable.
//!
//! 🔑 **The rest fallback is a plausible answer, not a blank one.** A mesh
//! skinned by the rest-derived skin still renders, still follows the bones, and
//! still looks like a working rig; it is only *wrong* where the authored binds
//! differ from the rest inverses. So every assertion here is differential: the
//! fixture's bind poses are deliberately NOT the inverse of the bone rests, and
//! each test that asserts the authored answer is paired with a control that
//! pins the rest-fallback answer on the same rig. Without the control, a test
//! reading `y == 0` cannot tell "the skin was honored" from "nothing moved".

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::deformed_mesh_for_node;
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{add_bone, set_bone_parent, set_bone_rest};
use gdserver3d::mesh::Mesh3D;
use gdvariant::Variant;

fn translation(x: f32, y: f32, z: f32) -> Transform3D {
    Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(x, y, z),
    }
}

/// One vertex at the origin, fully weighted to **bind 0**.
///
/// A single vertex on purpose: the assertions are about which matrix was
/// applied, and one vertex makes that readable as a coordinate rather than
/// inferred from a shape.
fn base_mesh() -> Mesh3D {
    Mesh3D {
        vertices: vec![Vector3::new(0.0, 0.0, 0.0)],
        normals: vec![Vector3::new(0.0, 0.0, 1.0)],
        bone_weights: vec![[1.0, 0.0, 0.0, 0.0]],
        bone_indices: vec![[0, 0, 0, 0]],
        ..Default::default()
    }
}

/// Builds `Root -> Skeleton3D -> MeshInstance3D` with two sibling bones and no
/// pose overrides.
///
/// ⚠️ **The bone rests must not be identity.** With an identity rest the
/// authored bind pose and the rest inverse are the same matrix, so the rig sits
/// on the coincidence point where the bug and the fix agree and no assertion
/// can separate them. `b0` rests 3 units up in Y and `b1` 5 units along X, and
/// every expected value below is one of those two displacements or zero.
fn rig() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skel", "Skeleton3D"))
        .expect("skeleton attaches");

    let b0 = add_bone(&mut tree, skel, "b0");
    set_bone_parent(&mut tree, skel, b0, -1);
    set_bone_rest(&mut tree, skel, b0, translation(0.0, 3.0, 0.0));

    let b1 = add_bone(&mut tree, skel, "b1");
    set_bone_parent(&mut tree, skel, b1, -1);
    set_bone_rest(&mut tree, skel, b1, translation(5.0, 0.0, 0.0));

    // No `skeleton` property: Godot's default for it is `".."`, so parenting
    // the mesh directly under the Skeleton3D is both the idiomatic rig and the
    // case that exercises that default.
    let id = tree
        .add_child(skel, Node::new("Model", "MeshInstance3D"))
        .expect("model attaches");
    (tree, id)
}

fn deformed_vertex(tree: &SceneTree, id: NodeId) -> Vector3 {
    deformed_mesh_for_node(tree, id, &base_mesh())
        .expect("a skinned mesh under a skeleton always deforms")
        .vertices[0]
}

fn set(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id)
        .expect("model exists")
        .set_property(key, value);
}

/// **The control.** With no `skin` the skeleton's rest-derived skin applies,
/// and each per-bind matrix is `global_rest * global_rest⁻¹` — the identity.
///
/// This is the value every test below must differ from. It is also the number
/// the unfixed engine returns for all of them.
#[test]
fn without_a_skin_the_rest_pose_leaves_the_mesh_undeformed() {
    let (tree, id) = rig();
    let v = deformed_vertex(&tree, id);
    assert!(
        v.length() < 1e-4,
        "the rest-derived skin must collapse to the identity at rest, leaving \
         the vertex at the origin; got {v:?}"
    );
}

/// An authored bind pose that is **not** the bone's rest inverse must move the
/// vertex by the bone's rest transform.
///
/// `bind_pose = identity` with `b0` resting at `(0, 3, 0)` gives
/// `global_pose * bind_pose = translate(0, 3, 0)`, so the vertex lands at
/// `y = 3`. 🔑 The rest fallback yields `y = 0` here — see the control above —
/// which is precisely the difference this bead exists to produce.
#[test]
fn an_authored_bind_pose_overrides_the_rest_derived_one() {
    let (mut tree, id) = rig();
    set(
        &mut tree,
        id,
        "__skin__class",
        Variant::String("Skin".into()),
    );
    set(&mut tree, id, "__skin__bind_count", Variant::Int(1));
    set(&mut tree, id, "__skin__bind/0/bone", Variant::Int(0));
    set(
        &mut tree,
        id,
        "__skin__bind/0/pose",
        Variant::Transform3D(Transform3D::IDENTITY),
    );

    let v = deformed_vertex(&tree, id);
    assert!(
        (v.y - 3.0).abs() < 1e-4 && v.x.abs() < 1e-4 && v.z.abs() < 1e-4,
        "the authored bind pose (identity) composed with b0's rest must put the \
         vertex at (0, 3, 0); (0, 0, 0) means the rest-derived skin was used and \
         the `skin` resource was ignored — got {v:?}"
    );
}

/// A bind may name its bone instead of indexing it (`bind/<i>/name`), which
/// Godot resolves against the skeleton at bind time.
///
/// The fixture names `b1` while leaving `bind/0/bone` at Godot's `-1` default,
/// so a reader that trusts the index lands on bone 0 — and bone 0's rest is the
/// `y = 3` displacement of the test above. The two answers are disjoint, so
/// `x = 5` can only come from the name having been resolved.
#[test]
fn a_named_bind_resolves_against_the_skeletons_bone_names() {
    let (mut tree, id) = rig();
    set(
        &mut tree,
        id,
        "__skin__class",
        Variant::String("Skin".into()),
    );
    set(&mut tree, id, "__skin__bind_count", Variant::Int(1));
    set(&mut tree, id, "__skin__bind/0/bone", Variant::Int(-1));
    set(
        &mut tree,
        id,
        "__skin__bind/0/name",
        Variant::String("b1".into()),
    );
    set(
        &mut tree,
        id,
        "__skin__bind/0/pose",
        Variant::Transform3D(Transform3D::IDENTITY),
    );

    let v = deformed_vertex(&tree, id);
    assert!(
        (v.x - 5.0).abs() < 1e-4 && v.y.abs() < 1e-4,
        "`bind/0/name = \"b1\"` must resolve to bone 1, whose rest is (5, 0, 0); \
         y=3 means the -1 index fell through to bone 0 and the name was never \
         read — got {v:?}"
    );
}

/// A `skin` whose binds *are* the rest inverses must still leave the mesh
/// undeformed.
///
/// ⚠️ This is the assertion that keeps the fix honest in the other direction.
/// The two tests above are satisfied by anything that stops using the rest
/// fallback — including a bug that drops the composition entirely. Here the
/// authored answer and the rest answer coincide, so only a correct composition
/// passes all three.
#[test]
fn an_authored_rest_inverse_bind_reproduces_the_fallback() {
    let (mut tree, id) = rig();
    set(
        &mut tree,
        id,
        "__skin__class",
        Variant::String("Skin".into()),
    );
    set(&mut tree, id, "__skin__bind_count", Variant::Int(1));
    set(&mut tree, id, "__skin__bind/0/bone", Variant::Int(0));
    set(
        &mut tree,
        id,
        "__skin__bind/0/pose",
        Variant::Transform3D(translation(0.0, -3.0, 0.0)),
    );

    let v = deformed_vertex(&tree, id);
    assert!(
        v.length() < 1e-4,
        "an authored bind equal to b0's rest inverse must collapse to the \
         identity exactly as the fallback does; got {v:?}"
    );
}

/// A `.tscn` carrying an inline `Skin` sub-resource, with the same two binds
/// the deformation tests set by hand.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Skin" id="Skin_1"]
bind_count = 2
bind/0/name = "b0"
bind/0/bone = -1
bind/0/pose = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, -3, 0)
bind/1/name = "b1"
bind/1/bone = -1
bind/1/pose = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, -5, 0, 0)

[node name="Root" type="Node3D"]

[node name="Skel" type="Skeleton3D" parent="."]

[node name="Model" type="MeshInstance3D" parent="Skel"]
skin = SubResource("Skin_1")
"#;

/// The bind data has to survive scene loading, and a `SubResource:<id>` string
/// carries none of it: it names a class and an id, not a bind list.
///
/// 🔑 This is the half that makes the deformation reachable from a real scene
/// rather than from a hand-built node, and it fails silently — the property is
/// *present* on the node, it simply has nothing in it.
#[test]
fn an_inline_skin_sub_resource_reaches_the_node() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let model = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == "Model")
                .unwrap_or(false)
        })
        .expect("no node named Model");
    let node = tree.get_node(model).expect("model exists");

    assert!(
        matches!(node.get_property("__skin__bind_count"), Variant::Int(2)),
        "the sub-resource's bind_count must reach the node; absent means the \
         flattening pass never ran and every bind is invisible to the runtime"
    );
    assert!(
        matches!(
            node.get_property("__skin__bind/1/name"),
            Variant::String(ref s) if s == "b1"
        ),
        "per-bind keys must be copied verbatim, not just the count"
    );
    assert_eq!(
        node.get_property("__skin__bind/1/pose"),
        Variant::Transform3D(translation(-5.0, 0.0, 0.0)),
        "the bind pose must arrive as a Transform3D; a String here means the \
         value was copied before the .tscn value parser ran"
    );
    // The real property keeps its reference so the scene still round-trips —
    // the companion is an addition, not a replacement.
    assert!(
        matches!(node.get_property("skin"), Variant::String(ref s) if s.contains("Skin_1")),
        "the `skin` property itself must be left alone"
    );
}

/// A rig authored ENTIRELY in `.tscn`: bones, rests, and a `Skin` sub-resource
/// whose bind is deliberately **not** the rest inverse.
///
/// ⚠️ `SCENE` above cannot be used for a deformation assertion, for two separate
/// reasons worth stating so nobody tries: its `Skel` declares no bones at all,
/// and its two bind poses ARE the rest inverses — the coincidence point where a
/// correct engine and one that ignored the skin both return the identity.
///
/// Bones are plain node properties (`bones/{idx}/…`, `skeleton3d.rs`) and a
/// scene-loaded skeleton has no `_bone_count`, so the count is inferred by
/// walking consecutive `bones/{idx}/name` records — which is exactly what Godot
/// writes and what this fixture declares.
const RIGGED_SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Skin" id="Skin_1"]
bind_count = 1
bind/0/name = "b1"
bind/0/bone = -1
bind/0/pose = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)

[node name="Root" type="Node3D"]

[node name="Skel" type="Skeleton3D" parent="."]
bones/0/name = "b0"
bones/0/parent = -1
bones/0/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 3, 0)
bones/1/name = "b1"
bones/1/parent = -1
bones/1/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 0, 0)

[node name="Model" type="MeshInstance3D" parent="Skel"]
skin = SubResource("Skin_1")
"#;

fn node_named(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {name}"))
}

fn model_of(src: &str) -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let model = node_named(&tree, "Model");
    (tree, model)
}

/// 🔑 **THE ACCEPTANCE, end to end.** A `.tscn` `[sub_resource type="Skin"]`
/// drives the deformation of the mesh it is attached to.
///
/// Every other test in this file exercises one half of that: the flattening
/// tests stop once the `__skin__*` companions reach the node, and the
/// deformation tests start from a rig built by hand with `set()`. Nothing joined
/// them, so a break in the seam between "the companions arrived" and "the
/// companions were read" would leave this whole file green.
///
/// # Why this fixture can tell three answers apart
///
/// The bind names `b1` and leaves `bind/0/bone` at Godot's `-1` default, with an
/// IDENTITY pose. `deformed = global_rest(bone) * bind_pose * vertex`, so:
///
/// | what the engine does                | vertex lands at |
/// |-------------------------------------|-----------------|
/// | resolves the NAME to bone 1 (right)  | `(5, 0, 0)`     |
/// | ignores the skin, rest-derived skin  | `(0, 0, 0)`     |
/// | trusts `bone = -1` and clamps to 0   | `(0, 3, 0)`     |
///
/// Three disjoint coordinates, so one reading names the failure instead of
/// merely reporting one — and the two clauses the acceptance asks for (authored
/// binds drive the deformation, and a named bind resolves to the right bone) are
/// separated rather than conflated.
#[test]
fn mesh_instance_skin_resource_binds_drive_deformation() {
    let (tree, model) = model_of(RIGGED_SCENE);

    // Precondition, asserted first: without it a `(0, 0, 0)` below could mean
    // "the skeleton had no bones" rather than "the skin was ignored", and the
    // headline assertion would be diagnosing the wrong thing.
    let skel = node_named(&tree, "Skel");
    assert_eq!(
        gdscene::skeleton3d::get_bone_count(&tree, skel),
        2,
        "the fixture's two `bones/{{idx}}/…` records must be inferred as two \
         bones; zero means a scene-loaded skeleton reported no bones and no \
         deformation claim below is meaningful"
    );

    let v = deformed_vertex(&tree, model);

    assert!(
        (v.x - 5.0).abs() < 1e-4 && v.y.abs() < 1e-4 && v.z.abs() < 1e-4,
        "the .tscn Skin must drive the deformation: expected (5, 0, 0) from \
         b1's rest composed with the authored identity bind, got {v:?}. \
         (0, 0, 0) means the resource was ignored and the rest-derived skin \
         applied; (0, 3, 0) means `bind/0/bone = -1` was trusted over the name \
         and the bind landed on bone 0"
    );
}

/// The differential control for the test above, sharing its fixture.
///
/// Strips only the `skin` reference, leaving the bones untouched, so the two
/// runs differ in exactly one authored fact. Without this, `(5, 0, 0)` could in
/// principle be what the rest-derived path returns for this rig too, and the
/// headline assertion would prove nothing about the skin.
#[test]
fn the_same_rig_without_a_skin_falls_back_to_the_rest_pose() {
    let stripped = RIGGED_SCENE.replace("skin = SubResource(\"Skin_1\")\n", "");
    assert!(
        !stripped.contains("skin = SubResource"),
        "the control must actually remove the skin reference, or it is a copy \
         of the test it is controlling for"
    );

    let (tree, model) = model_of(&stripped);
    let v = deformed_vertex(&tree, model);

    assert!(
        v.x.abs() < 1e-4 && v.y.abs() < 1e-4 && v.z.abs() < 1e-4,
        "with no skin the rest-derived bind is `global_rest * global_rest⁻¹` — \
         the identity — so the vertex must stay at the origin, got {v:?}. A \
         (5, 0, 0) here would mean the previous test's result came from the \
         rig rather than from the Skin resource"
    );
}
