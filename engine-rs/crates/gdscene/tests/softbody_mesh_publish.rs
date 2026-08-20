//! pat-s3rrv.193 clause (b): solved soft-body vertices reaching the render mesh.
//!
//! Clause (a) — the cloth sags — is covered by `gdphysics3d`. This covers the
//! return trip: the mesh a renderer would read must report the *solved*
//! positions, not the authored ones.
//!
//! The subtle part is that welding makes particles and vertices different sets.
//! Two authored vertices at the same corner become one particle, so publishing
//! is a scatter back out to every vertex that particle carries — a plain
//! element-wise copy silently works on a mesh with no duplicates and tears one
//! that has them.
//!
//! ⚠️ This is NOT the bead's acceptance test. `physics3d_softbody_node` drives a
//! `.tscn` through MainLoop; the scene bridge that would do so lives in
//! `physics_server_3d.rs`, which is leased by another agent.

use gdcore::math::Vector3;
use gdphysics3d::softbody3d::{SoftBody3D, SoftBodyId3D, DEFAULT_WELD_EPSILON};
use gdscene::scene_tree::SceneTree;
use gdscene::softbody_sync_3d::{
    publish_vertices, published, softbody_deformed_mesh, softbody_mesh_for_node,
};
use gdserver3d::mesh::Mesh3D;
use gdvariant::Variant;

fn mesh(vertices: Vec<Vector3>, indices: Vec<u32>) -> Mesh3D {
    Mesh3D {
        vertices,
        indices,
        ..Default::default()
    }
}

const C0: Vector3 = Vector3::new(1.0, 2.0, 3.0);
const C1: Vector3 = Vector3::new(2.0, 2.0, 3.0);
const C2: Vector3 = Vector3::new(1.0, 2.0, 4.0);
const C3: Vector3 = Vector3::new(2.0, 2.0, 4.0);

/// A quad authored with per-face vertices: C1 and C2 each appear twice, so
/// welding collapses 6 vertices onto 4 particles.
fn unwelded_quad() -> Vec<Vector3> {
    vec![C0, C1, C2, C1, C3, C2]
}

/// The core: solved positions replace authored ones.
#[test]
fn solved_positions_replace_the_authored_ones() {
    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    let (mut body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(1),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    body.vertices[map[1]].position = Vector3::new(9.0, -9.0, 9.0);

    let out = softbody_deformed_mesh(&base, &body, &map).expect("the body describes this mesh");
    assert_eq!(out.vertices[1], Vector3::new(9.0, -9.0, 9.0));
    assert_eq!(
        out.vertices[0], C0,
        "untouched particles keep their position"
    );
    assert_ne!(
        out.vertices, base.vertices,
        "the published mesh must differ from the authored one"
    );
}

/// Every vertex sharing a welded particle must receive that particle's position.
///
/// This is the test that separates a scatter from a copy. With `map` being the
/// identity — which it is for any mesh without duplicate vertices — an
/// element-wise `out.vertices[i] = body.vertices[i]` passes every other test in
/// this file. Here it would move only one of the two vertices at each welded
/// corner and tear the quad open along its shared edge.
#[test]
fn both_vertices_of_a_welded_corner_follow_their_particle() {
    let base = mesh(unwelded_quad(), vec![]);
    let (mut body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(2),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    assert_eq!(body.vertex_count(), 4, "the 6 authored vertices weld to 4");
    assert_eq!(map[1], map[3], "both copies of C1 share a particle");

    let moved = Vector3::new(-5.0, -6.0, -7.0);
    body.vertices[map[1]].position = moved;

    let out = softbody_deformed_mesh(&base, &body, &map).expect("mesh is described");
    assert_eq!(out.vertices[1], moved, "first copy of the corner moved");
    assert_eq!(
        out.vertices[3], moved,
        "the duplicate vertex at that corner must move with it, or the mesh tears"
    );
    assert_eq!(
        out.vertices[2], out.vertices[5],
        "the other welded corner stays consistent"
    );
}

/// A map from a different mesh must be refused outright.
#[test]
fn a_mismatched_map_deforms_nothing() {
    let base = mesh(vec![C0, C1, C2, C3], vec![]);
    let (body, _) = SoftBody3D::from_mesh(
        SoftBodyId3D(3),
        &[C0, C1, C2],
        &[0, 1, 2],
        DEFAULT_WELD_EPSILON,
    );

    // Three entries for a four-vertex mesh.
    assert!(
        softbody_deformed_mesh(&base, &body, &[0, 1, 2]).is_none(),
        "a partially-applicable map must be refused, not partially applied"
    );
}

/// Corrupt map entries must not panic a frame.
#[test]
fn a_map_naming_a_missing_particle_is_refused() {
    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    let (body, _) = SoftBody3D::from_mesh(
        SoftBodyId3D(4),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    assert!(softbody_deformed_mesh(&base, &body, &[0, 1, 99]).is_none());
}

/// Nothing to publish is `None`, matching the morph/skin stages' convention.
#[test]
fn an_empty_mesh_or_body_publishes_nothing() {
    let empty = Mesh3D::default();
    let (body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(5),
        &[C0, C1, C2],
        &[0, 1, 2],
        DEFAULT_WELD_EPSILON,
    );

    assert!(softbody_deformed_mesh(&empty, &body, &[]).is_none());

    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    let bodyless = SoftBody3D::new(SoftBodyId3D(6), &[], &[]);
    assert!(
        softbody_deformed_mesh(&base, &bodyless, &map).is_none(),
        "a body with no particles has nothing to say about this mesh"
    );
}

/// Everything the renderer needs besides positions must survive.
///
/// Publishing positions by rebuilding a mesh rather than copying one would drop
/// the index buffer, and the cloth would vanish rather than deform — a failure
/// that looks like the physics, not like the publish step.
#[test]
fn indices_and_attributes_survive_publication() {
    let mut base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    base.normals = vec![Vector3::new(0.0, 1.0, 0.0); 3];
    base.uvs = vec![[0.25, 0.75]; 3];

    let (body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(8),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );
    let out = softbody_deformed_mesh(&base, &body, &map).expect("mesh is described");

    assert_eq!(out.indices, base.indices, "the index buffer must survive");
    assert_eq!(out.uvs, base.uvs, "UVs must survive");
    assert_eq!(
        out.normals.len(),
        base.normals.len(),
        "normals must survive"
    );
    assert_eq!(out.primitive_type, base.primitive_type);
}

const N: usize = 5;
const SPACING: f32 = 0.25;

/// The full round trip: mesh in, simulate, mesh out.
///
/// This is as close to the bead's acceptance as this crate can get without the
/// scene bridge — it exercises both halves I own together, which neither
/// half's own tests do. The 0.05 threshold is the acceptance criterion's.
#[test]
fn a_stepped_cloth_publishes_sagged_vertices_to_its_mesh() {
    let mut vertices = Vec::with_capacity(N * N);
    for row in 0..N {
        for col in 0..N {
            vertices.push(Vector3::new(
                col as f32 * SPACING,
                0.0,
                row as f32 * SPACING,
            ));
        }
    }
    let mut indices = Vec::new();
    for row in 0..N - 1 {
        for col in 0..N - 1 {
            let i = (row * N + col) as u32;
            let n = N as u32;
            indices.extend_from_slice(&[i, i + 1, i + n]);
            indices.extend_from_slice(&[i + 1, i + n + 1, i + n]);
        }
    }
    let base = mesh(vertices, indices);

    let (mut body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(9),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    let top_left = 0;
    let top_right = N - 1;
    let center = (N / 2) * N + (N / 2);
    body.pin_vertex(map[top_left]);
    body.pin_vertex(map[top_right]);

    let gravity = Vector3::new(0.0, -9.8, 0.0);
    for _ in 0..120 {
        body.step(1.0 / 60.0, gravity);
    }

    let out = softbody_deformed_mesh(&base, &body, &map).expect("mesh is described");

    assert_eq!(
        out.vertices[top_left], base.vertices[top_left],
        "a pinned corner must publish its authored position, unchanged"
    );
    assert_eq!(out.vertices[top_right], base.vertices[top_right]);
    assert!(
        out.vertices[center].y < base.vertices[center].y - 0.05,
        "the published centre must carry the sag (authored {}, published {})",
        base.vertices[center].y,
        out.vertices[center].y
    );

    // The published mesh must still be drawable, not a bag of NaNs.
    for (i, v) in out.vertices.iter().enumerate() {
        assert!(
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
            "published vertex {i} is non-finite: {v:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Publishing onto the node — the interface between the physics step and the
// render collection. The acceptance criterion is written in these terms
// ("published back onto the node", "the mesh the render collection reads"),
// so this is the seam the two halves of the bridge meet at.
// ---------------------------------------------------------------------------

/// A stepped body reaches the renderer through the node.
#[test]
fn published_vertices_become_the_mesh_the_renderer_reads() {
    let mut tree = SceneTree::new();
    let nid = tree.create_node("SoftBody3D", "cloth");

    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    let (mut body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(20),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    let moved = Vector3::new(4.0, -8.0, 6.0);
    body.vertices[map[2]].position = moved;

    assert!(
        publish_vertices(&mut tree, nid, &body, &map),
        "publish succeeds"
    );

    let drawn = softbody_mesh_for_node(&tree, nid, &base).expect("a mesh is published");
    assert_eq!(
        drawn.vertices[2], moved,
        "the renderer must see the solved position"
    );
    assert_eq!(drawn.vertices[0], C0);
    assert_eq!(
        drawn.indices, base.indices,
        "the index buffer must survive the round trip"
    );
}

/// The scatter happens at publish time, so the array on the node is already in
/// authored vertex order.
///
/// This is what lets the render side stay ignorant of welding. If the scatter
/// were deferred to the reader, every consumer would need the weld map, and a
/// consumer that did not have it would silently draw a torn mesh.
#[test]
fn the_published_array_is_in_authored_vertex_order() {
    let mut tree = SceneTree::new();
    let nid = tree.create_node("SoftBody3D", "cloth");

    let authored = unwelded_quad();
    let (mut body, map) =
        SoftBody3D::from_mesh(SoftBodyId3D(21), &authored, &[], DEFAULT_WELD_EPSILON);
    assert_eq!(
        body.vertex_count(),
        4,
        "6 authored vertices weld to 4 particles"
    );

    let moved = Vector3::new(-1.0, -2.0, -3.0);
    body.vertices[map[1]].position = moved;
    assert!(publish_vertices(&mut tree, nid, &body, &map));

    let published_positions = match tree
        .get_node(nid)
        .expect("node exists")
        .get_property(published::VERTICES)
    {
        Variant::PackedVector3Array(v) => v,
        other => panic!("expected a packed vector array, got {other:?}"),
    };

    assert_eq!(
        published_positions.len(),
        authored.len(),
        "one entry per AUTHORED vertex, not per particle"
    );
    assert_eq!(published_positions[1], moved);
    assert_eq!(
        published_positions[3], moved,
        "the duplicate vertex at that corner must carry the same solved position"
    );
}

/// Almost no node is a soft body, so the common answer is "nothing to do".
#[test]
fn a_node_with_nothing_published_deforms_nothing() {
    let mut tree = SceneTree::new();
    let nid = tree.create_node("MeshInstance3D", "plain");
    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);

    assert!(softbody_mesh_for_node(&tree, nid, &base).is_none());
}

/// A published array from a different mesh must not be drawn.
#[test]
fn a_published_count_mismatch_is_refused() {
    let mut tree = SceneTree::new();
    let nid = tree.create_node("SoftBody3D", "cloth");
    tree.get_node_mut(nid).expect("node exists").set_property(
        published::VERTICES,
        Variant::PackedVector3Array(vec![C0, C1]),
    );

    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    assert!(
        softbody_mesh_for_node(&tree, nid, &base).is_none(),
        "two published positions cannot describe a three-vertex mesh"
    );
}

/// A refused publish must leave the node untouched.
///
/// Returning false is not enough: a half-written array would leave the node
/// advertising a vertex count it does not have, and the reader's length check
/// would then reject a mesh that is actually fine.
#[test]
fn a_refused_publish_writes_nothing_at_all() {
    let mut tree = SceneTree::new();
    let nid = tree.create_node("SoftBody3D", "cloth");

    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    let (body, _) = SoftBody3D::from_mesh(
        SoftBodyId3D(22),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    // The last entry names a particle the body does not have.
    assert!(!publish_vertices(&mut tree, nid, &body, &[0, 1, 99]));

    assert!(
        matches!(
            tree.get_node(nid)
                .expect("node exists")
                .get_property(published::VERTICES),
            Variant::Nil
        ),
        "a refused publish must not leave a partial array on the node"
    );
}

/// Clause (a) and clause (b) together, as far as this crate can carry them:
/// simulate, publish onto the node, and read the drawn mesh back.
#[test]
fn a_stepped_cloth_reaches_the_renderer_through_its_node() {
    let mut tree = SceneTree::new();
    let nid = tree.create_node("SoftBody3D", "cloth");

    let mut vertices = Vec::with_capacity(N * N);
    for row in 0..N {
        for col in 0..N {
            vertices.push(Vector3::new(
                col as f32 * SPACING,
                0.0,
                row as f32 * SPACING,
            ));
        }
    }
    let mut indices = Vec::new();
    for row in 0..N - 1 {
        for col in 0..N - 1 {
            let i = (row * N + col) as u32;
            let n = N as u32;
            indices.extend_from_slice(&[i, i + 1, i + n]);
            indices.extend_from_slice(&[i + 1, i + n + 1, i + n]);
        }
    }
    let base = mesh(vertices, indices);

    let (mut body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(23),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    let top_left = 0;
    let top_right = N - 1;
    let center = (N / 2) * N + (N / 2);
    body.pin_vertex(map[top_left]);
    body.pin_vertex(map[top_right]);

    let gravity = Vector3::new(0.0, -9.8, 0.0);
    for _ in 0..120 {
        body.step(1.0 / 60.0, gravity);
    }
    assert!(publish_vertices(&mut tree, nid, &body, &map));

    let drawn = softbody_mesh_for_node(&tree, nid, &base).expect("a mesh is published");

    assert!(
        drawn.vertices[center].y < base.vertices[center].y - 0.05,
        "the drawn centre must carry the sag (authored {}, drawn {})",
        base.vertices[center].y,
        drawn.vertices[center].y
    );
    assert_eq!(
        drawn.vertices[top_left], base.vertices[top_left],
        "a pinned corner must draw at its authored position"
    );
    assert_eq!(drawn.vertices[top_right], base.vertices[top_right]);
    assert_ne!(
        drawn.vertices, base.vertices,
        "the drawn mesh must not be the authored one"
    );
}
