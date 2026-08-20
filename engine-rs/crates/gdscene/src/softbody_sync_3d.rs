//! Publishing solved SoftBody3D vertices onto the mesh the renderer draws.
//!
//! A soft body simulates *particles*, and a mesh draws *vertices*; welding means
//! those two sets are not the same size, so the solver's output cannot simply be
//! assigned over a vertex buffer. This module owns that translation — the second
//! half of `pat-s3rrv.193`, whose first half
//! ([`gdphysics3d::softbody3d::SoftBody3D::from_mesh`]) goes the other way.
//!
//! This is a sibling of the morph and skin stages in [`crate::render_server_3d`]:
//! same shape (`base` mesh in, deformed mesh out, `None` for "nothing to do"),
//! so the render collection can apply it the same way it applies those.

use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use gdcore::math3d::Transform3D;
use gdphysics3d::softbody3d::SoftBody3D;
use gdserver3d::mesh::Mesh3D;
use gdvariant::Variant;

/// Companion property names the physics step publishes onto a SoftBody3D node.
///
/// Prefixed so they cannot collide with a real Godot property and read as
/// engine-internal in an inspector dump — same convention as
/// [`crate::navigation_agents::published`].
pub mod published {
    /// Solved vertex positions, in the node's **authored vertex order**.
    ///
    /// Already scattered out of particle order, so a reader needs no weld map.
    pub const VERTICES: &str = "__softbody_vertices__";
}

/// Rewrites `base`'s vertex positions from a stepped soft body.
///
/// `visual_to_physics` is the map returned alongside the body by
/// `SoftBody3D::from_mesh`: one entry per authored vertex, naming the particle
/// that carries it. Several vertices may share a particle — that is what
/// welding produced — so this is a scatter from particles back out to vertices,
/// not a copy.
///
/// Returns `None` when the body does not describe this mesh, rather than
/// deforming it partially: a length mismatch means the map was built from a
/// different mesh, and writing the vertices they happen to share would leave a
/// torn mesh that looks like a physics bug rather than a wiring bug.
///
/// ⚠️ Normals are carried through as authored, not recomputed. That is
/// deliberate: the morph and skin stages in `render_server_3d` do the same, and
/// no normal-recomputation helper exists in the tree. A deforming cloth will
/// therefore light as though flat until one does — a known gap shared with the
/// other deformation stages, not one introduced here.
///
/// ⚠️ Only the primary surface is deformed. `Mesh3D::surfaces`, when populated,
/// carries its own vertex arrays that this does not touch; a multi-surface soft
/// body is not supported and its extra surfaces stay authored.
pub fn softbody_deformed_mesh(
    base: &Mesh3D,
    body: &SoftBody3D,
    visual_to_physics: &[usize],
) -> Option<Mesh3D> {
    if base.vertices.is_empty() || body.vertices.is_empty() {
        return None;
    }
    if visual_to_physics.len() != base.vertices.len() {
        return None;
    }

    let mut out = base.clone();
    for (visual, slot) in out.vertices.iter_mut().enumerate() {
        // Bounds-checked rather than indexed: a map naming a particle the body
        // does not have is corrupt input, and a panic here would take down the
        // frame instead of skipping one bad body.
        let particle = *visual_to_physics.get(visual)?;
        *slot = body.vertices.get(particle)?.position;
    }
    Some(out)
}

/// Publishes a stepped body's solved positions onto its node.
///
/// The scatter from particles back out to vertices happens **here**, so what
/// lands on the node is in authored vertex order and any reader — the render
/// collection, a script, a test — can consume it without knowing that welding
/// happened at all. Doing it the other way round would leak the weld map to
/// every consumer.
///
/// Returns false and publishes nothing if the map does not describe this body,
/// rather than publishing a half-filled array.
pub fn publish_vertices(
    tree: &mut SceneTree,
    nid: NodeId,
    body: &SoftBody3D,
    visual_to_physics: &[usize],
) -> bool {
    publish_vertices_with_transform(tree, nid, body, visual_to_physics, Transform3D::IDENTITY)
}

/// As [`publish_vertices`], but maps each solved position through `to_local`
/// on the way out.
///
/// The solver runs in world space (gravity is a world vector) while the mesh
/// the renderer draws is in the node's local space, so the caller needs the
/// inverse node transform applied to every vertex. Doing it inside the scatter
/// means the body itself is never copied — transforming a cloned `SoftBody3D`
/// instead would duplicate its whole edge list every frame for nothing.
pub fn publish_vertices_with_transform(
    tree: &mut SceneTree,
    nid: NodeId,
    body: &SoftBody3D,
    visual_to_physics: &[usize],
    to_local: Transform3D,
) -> bool {
    if visual_to_physics.is_empty() {
        return false;
    }
    let mut solved = Vec::with_capacity(visual_to_physics.len());
    for &particle in visual_to_physics {
        match body.vertices.get(particle) {
            Some(v) => solved.push(to_local.xform(v.position)),
            // Build the whole array before touching the node: a partial write
            // would leave the node advertising a vertex count it does not have.
            None => return false,
        }
    }

    let Some(node) = tree.get_node_mut(nid) else {
        return false;
    };
    node.set_property(published::VERTICES, Variant::PackedVector3Array(solved));
    true
}

/// Builds the mesh the renderer should draw for a node carrying published
/// soft-body vertices.
///
/// Deliberately the same shape as `render_server_3d`'s `morphed_mesh(tree, nid,
/// base)` so the render collection can apply it as one more deformation stage
/// beside morph and skin.
///
/// `None` means "nothing published for this node" — the overwhelmingly common
/// case, since almost no node is a soft body.
pub fn softbody_mesh_for_node(tree: &SceneTree, nid: NodeId, base: &Mesh3D) -> Option<Mesh3D> {
    let node = tree.get_node(nid)?;
    let solved = match node.get_property(published::VERTICES) {
        Variant::PackedVector3Array(v) => v,
        _ => return None,
    };
    // A count mismatch means the published array belongs to a different mesh;
    // drawing the overlap would tear the surface.
    if solved.is_empty() || solved.len() != base.vertices.len() {
        return None;
    }

    let mut out = base.clone();
    out.vertices = solved;
    Some(out)
}
