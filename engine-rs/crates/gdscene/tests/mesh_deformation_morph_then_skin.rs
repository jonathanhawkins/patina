//! pat-s3rrv.23: the mesh a `MeshInstance3D` draws is its blend shapes applied
//! and THEN skinned by the bound `Skeleton3D`.
//!
//! Godot ground truth: `upstream/godot/scene/3d/mesh_instance_3d.cpp` — a mesh
//! is deformed by its `blend_shapes/<name>` values and by its `skin` +
//! `skeleton` binding.
//!
//! ⚠️ **THE ORDER IS THE POINT, AND IT FAILS SILENTLY.** Skinning the
//! undeformed base instead of the morphed mesh loses the blend shape entirely,
//! because the skinned result is what gets drawn — and a mesh that is skinned
//! but not morphed looks perfectly correct, so the bug only appears on meshes
//! that are both. The runtime path documents this at its own call site; these
//! tests pin it for the shared helper the editor now uses.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdscene::node::{Node, NodeId};
use gdscene::render_server_3d::deformed_mesh_for_node;
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{add_bone, set_bone_pose, set_bone_rest};
use gdserver3d::mesh::{BlendShape3D, Mesh3D};
use gdvariant::Variant;

/// One vertex at the origin, weighted entirely to bone 0, with a blend shape
/// that lifts it one unit along +Y.
///
/// A single vertex on purpose: the assertions are about WHICH deformations were
/// applied, and one vertex makes each contribution readable as a coordinate
/// rather than inferred from a shape.
fn base_mesh() -> Mesh3D {
    Mesh3D {
        vertices: vec![Vector3::new(0.0, 0.0, 0.0)],
        normals: vec![Vector3::new(0.0, 0.0, 1.0)],
        bone_weights: vec![[1.0, 0.0, 0.0, 0.0]],
        bone_indices: vec![[0, 0, 0, 0]],
        blend_shapes: vec![BlendShape3D {
            name: "lift".into(),
            vertex_deltas: vec![Vector3::new(0.0, 1.0, 0.0)],
        }],
        ..Default::default()
    }
}

fn translation(x: f32) -> Transform3D {
    Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(x, 0.0, 0.0),
    }
}

/// Builds Root -> [Skeleton3D ->] MeshInstance3D.
///
/// ⚠️ When a skeleton is wanted the mesh is parented UNDER it and no `skeleton`
/// property is set — Godot's default for that property is `".."`, so this is
/// both the idiomatic rig and the case that exercises the default. Parenting
/// the mesh under a plain Node3D instead is what makes it unskinned.
fn scene(with_skeleton: bool, weight: Option<f32>, bone_shift: f32) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = if with_skeleton {
        let skel = tree
            .add_child(root, Node::new("Skel", "Skeleton3D"))
            .expect("skeleton attaches");
        let bone = add_bone(&mut tree, skel, "b0");
        set_bone_rest(&mut tree, skel, bone, Transform3D::IDENTITY);
        set_bone_pose(&mut tree, skel, bone, translation(bone_shift));
        skel
    } else {
        tree.add_child(root, Node::new("Holder", "Node3D"))
            .expect("holder attaches")
    };

    let mut model = Node::new("Model", "MeshInstance3D");
    if let Some(w) = weight {
        model.set_property("blend_shapes/lift", Variant::Float(w as f64));
    }
    let id = tree.add_child(parent, model).expect("model attaches");
    (tree, id)
}

fn deformed_vertex(with_skeleton: bool, weight: Option<f32>, bone_shift: f32) -> Option<Vector3> {
    let (tree, id) = scene(with_skeleton, weight, bone_shift);
    deformed_mesh_for_node(&tree, id, &base_mesh()).map(|m| m.vertices[0])
}

/// A mesh with neither a skeleton nor a blend-shape weight is not deformed at
/// all, so the caller keeps its ordinary mesh path.
#[test]
fn an_undeformed_mesh_reports_nothing_to_change() {
    assert!(
        deformed_vertex(false, None, 0.0).is_none(),
        "no skeleton and no weight means nothing to deform; returning a copy \
         here would push every ordinary mesh onto the generated-geometry path"
    );
}

/// ⚠️ Morph must apply WITHOUT a skeleton. The skinning branch is gated on
/// `is_skinned()`, so folding the morph into it would skip every unskinned
/// morphed mesh — a face with blend shapes and no rig is an ordinary case.
#[test]
fn blend_shapes_apply_without_a_skeleton() {
    let v = deformed_vertex(false, Some(1.0), 0.0).expect("a weighted blend shape deforms");
    assert!(
        (v.y - 1.0).abs() < 1e-4,
        "weight 1.0 on a shape that lifts +1 in Y must move the vertex to y=1; \
         got {v:?}"
    );
}

/// Posing the bone moves the mesh.
#[test]
fn a_posed_skeleton_moves_the_mesh() {
    let v = deformed_vertex(true, None, 2.0).expect("a posed skeleton deforms");
    assert!(
        (v.x - 2.0).abs() < 1e-4,
        "the single bone was posed +2 in X and the vertex is fully weighted to \
         it, so the vertex must land at x=2; got {v:?}"
    );
}

/// **The ordering test.** A mesh that is BOTH morphed and skinned must show
/// both, which is only true if the skin was applied to the MORPHED mesh.
#[test]
fn skinning_is_applied_to_the_morphed_mesh_not_the_base() {
    let v = deformed_vertex(true, Some(1.0), 2.0).expect("a morphed, skinned mesh deforms");

    assert!(
        (v.x - 2.0).abs() < 1e-4,
        "the bone pose must still be applied; got {v:?}"
    );
    // ⚠️ THE DISCRIMINATOR. Skinning the base rather than the morphed mesh
    // yields y = 0 here — the blend shape silently dropped — while every other
    // assertion in this file still passes. This is the one that catches it.
    assert!(
        (v.y - 1.0).abs() < 1e-4,
        "the blend shape must survive skinning: y=0 means the skin was applied \
         to the undeformed base and the morph was discarded; got {v:?}"
    );
}
