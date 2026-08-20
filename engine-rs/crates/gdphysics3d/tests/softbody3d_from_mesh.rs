//! pat-s3rrv.193: building a soft body from mesh data.
//!
//! The solver already simulates a cloth given positions and edges, and
//! `softbody3d_sag.rs` proves that behaviour. What nothing did was derive those
//! positions and edges from a *mesh* — the sag test hand-rolls a grid's
//! structural edges in test code, which only works because it knows it is a
//! lattice. A SoftBody3D loaded from a `.tscn` arrives as a vertex buffer plus
//! a triangle list, so that derivation is the missing production step between
//! the scene and the solver.
//!
//! ⚠️ This is NOT the bead's acceptance test. `physics3d_softbody_node` requires
//! the scene bridge in `gdscene`; this file covers the prerequisite it will
//! call.

use gdcore::math::Vector3;
use gdphysics3d::softbody3d::{
    edges_from_triangles, weld_positions, SoftBody3D, SoftBodyId3D, DEFAULT_WELD_EPSILON,
};

/// Four corners of a quad, deliberately away from the origin and not
/// symmetric about any axis: a fixture at the origin hides a missing offset,
/// and a symmetric one cannot catch swapped axes.
const C0: Vector3 = Vector3::new(1.0, 2.0, 3.0);
const C1: Vector3 = Vector3::new(2.0, 2.0, 3.0);
const C2: Vector3 = Vector3::new(1.0, 2.0, 4.0);
const C3: Vector3 = Vector3::new(2.0, 2.0, 4.0);

fn has_edge(edges: &[(usize, usize)], a: usize, b: usize) -> bool {
    let key = if a < b { (a, b) } else { (b, a) };
    edges.iter().any(|&e| e == key)
}

/// Two triangles sharing an edge must yield that edge ONCE.
///
/// This is the whole reason edge derivation is not a one-liner: every interior
/// edge of a closed mesh belongs to two triangles, so collecting three edges
/// per triangle emits it twice. Two distance constraints over one vertex pair
/// is not a duplicate to be ignored — it doubles that edge's stiffness, and on
/// a real mesh nearly every edge is interior.
#[test]
fn a_shared_edge_yields_one_constraint_not_two() {
    let positions = [C0, C1, C2, C3];
    let indices = [0u32, 1, 2, 1, 3, 2];
    let (_, map) = weld_positions(&positions, DEFAULT_WELD_EPSILON);
    let edges = edges_from_triangles(&indices, &map);

    // 6 triangle-edge slots, but (1,2) is shared, so 5 distinct.
    assert_eq!(
        edges.len(),
        5,
        "expected 5 unique edges from two triangles sharing one; got {edges:?}"
    );
    assert!(
        has_edge(&edges, 1, 2),
        "the shared edge itself must still be present: {edges:?}"
    );
    for (a, b) in [(0, 1), (0, 2), (1, 3), (2, 3)] {
        assert!(has_edge(&edges, a, b), "missing boundary edge ({a},{b})");
    }
}

/// The same quad authored with per-face vertices — no sharing at all, which is
/// how a mesh carrying per-face normals or UVs arrives.
fn unwelded_quad() -> Vec<Vector3> {
    vec![C0, C1, C2, C1, C3, C2]
}

/// Welding is what makes an unshared mesh simulate as one sheet.
#[test]
fn welding_reconnects_a_mesh_whose_faces_share_no_vertices() {
    let positions = unwelded_quad();
    let (welded, map) = weld_positions(&positions, DEFAULT_WELD_EPSILON);

    assert_eq!(
        welded.len(),
        4,
        "the quad's 6 authored vertices are 4 corners"
    );
    assert_eq!(
        map.len(),
        positions.len(),
        "every input vertex needs a mapping"
    );
    // The duplicated corners must land on the same simulated particle.
    assert_eq!(map[1], map[3], "both copies of C1 are one particle");
    assert_eq!(map[2], map[5], "both copies of C2 are one particle");

    let edges = edges_from_triangles(&[], &map);
    assert_eq!(
        edges.len(),
        5,
        "welded quad has 5 distinct edges: {edges:?}"
    );
}

/// The control for the test above.
///
/// "Welding produced 4 vertices" proves little on its own — so would a builder
/// that always collapsed everything. This pins the opposite direction: with
/// welding off, the two triangles stay disjoint and the sheet is two loose
/// flaps, which is exactly the failure welding exists to prevent.
#[test]
fn without_welding_the_two_faces_stay_disconnected() {
    let positions = unwelded_quad();
    let (unwelded, map) = weld_positions(&positions, 0.0);

    assert_eq!(
        unwelded.len(),
        6,
        "welding disabled must keep all 6 vertices"
    );
    assert_eq!(map, (0..6).collect::<Vec<_>>(), "identity mapping expected");

    let edges = edges_from_triangles(&[], &map);
    assert_eq!(
        edges.len(),
        6,
        "two disjoint triangles have 6 edges: {edges:?}"
    );

    // No edge bridges the first triangle {0,1,2} and the second {3,4,5}.
    assert!(
        !edges.iter().any(|&(a, b)| (a < 3) != (b < 3)),
        "the faces must share no constraint when welding is off: {edges:?}"
    );
}

/// Points within epsilon must weld even when they fall in different spatial
/// hash cells.
///
/// With epsilon 0.1 these two x values quantise to cells 9 and 10, so a builder
/// that only consults a point's own cell leaves them unwelded — a seam that
/// appears at some mesh positions and not others. Testing two *exactly*
/// coincident points would never exercise the tolerance at all.
#[test]
fn points_within_epsilon_weld_across_a_cell_boundary() {
    let epsilon = 0.1;
    let positions = [Vector3::new(0.999, 0.0, 0.0), Vector3::new(1.001, 0.0, 0.0)];
    let (welded, map) = weld_positions(&positions, epsilon);

    assert_eq!(
        welded.len(),
        1,
        "points 0.002 apart must weld under epsilon 0.1 despite spanning cells"
    );
    assert_eq!(map[0], map[1]);
}

/// And the other side of the tolerance: distinct points must survive.
#[test]
fn points_beyond_epsilon_are_kept_apart() {
    let positions = [Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.5, 0.0, 0.0)];
    let (welded, map) = weld_positions(&positions, 0.1);

    assert_eq!(welded.len(), 2, "0.5 apart is well beyond epsilon 0.1");
    assert_ne!(map[0], map[1]);
}

/// A non-indexed mesh must build the same body as the indexed form.
#[test]
fn a_non_indexed_mesh_matches_its_indexed_equivalent() {
    let positions = unwelded_quad();
    let (_, map) = weld_positions(&positions, DEFAULT_WELD_EPSILON);

    let implicit = edges_from_triangles(&[], &map);
    let explicit = edges_from_triangles(&[0u32, 1, 2, 3, 4, 5], &map);

    assert_eq!(
        implicit, explicit,
        "an absent index buffer means consecutive triples, not zero triangles"
    );
    assert!(
        !implicit.is_empty(),
        "a non-indexed mesh must not silently produce a constraint-free body"
    );
}

/// Malformed scene data must not panic the engine.
#[test]
fn out_of_range_indices_skip_their_triangle() {
    let positions = [C0, C1, C2, C3];
    let (_, map) = weld_positions(&positions, DEFAULT_WELD_EPSILON);

    // Second triangle references vertex 99, which does not exist.
    let edges = edges_from_triangles(&[0u32, 1, 2, 1, 99, 2], &map);
    assert_eq!(
        edges.len(),
        3,
        "only the valid triangle contributes: {edges:?}"
    );
}

/// An index count that is not a multiple of three drops the partial triangle.
#[test]
fn a_trailing_partial_triangle_is_ignored() {
    let positions = [C0, C1, C2, C3];
    let (_, map) = weld_positions(&positions, DEFAULT_WELD_EPSILON);

    let edges = edges_from_triangles(&[0u32, 1, 2, 3], &map);
    assert_eq!(edges.len(), 3, "the dangling index 3 forms no triangle");
}

/// Welding can collapse a sliver triangle; the result must not be a
/// zero-length constraint tying a vertex to itself.
#[test]
fn a_triangle_collapsed_by_welding_emits_no_self_edge() {
    // The first two corners are within epsilon, so this triangle welds to a
    // single edge between two distinct particles.
    let positions = [C0, Vector3::new(1.0, 2.0, 3.0 + 1e-7), C2];
    let (welded, map) = weld_positions(&positions, DEFAULT_WELD_EPSILON);
    assert_eq!(welded.len(), 2, "two of the three corners coincide");

    let edges = edges_from_triangles(&[0u32, 1, 2], &map);
    assert!(
        edges.iter().all(|&(a, b)| a != b),
        "a collapsed corner must not produce a self-edge: {edges:?}"
    );
    assert_eq!(edges.len(), 1, "one real edge survives: {edges:?}");
}

const N: usize = 5;
const SPACING: f32 = 0.25;

/// The 5x5 plane the bead's acceptance describes, expressed the way a mesh
/// carries it: a vertex buffer plus a triangle list.
fn plane_mesh() -> (Vec<Vector3>, Vec<u32>) {
    let mut positions = Vec::with_capacity(N * N);
    for row in 0..N {
        for col in 0..N {
            positions.push(Vector3::new(
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
    (positions, indices)
}

/// A body built from mesh data sags like the hand-built one.
///
/// This is the behavioural tie-in: the derivation above is only correct if the
/// resulting constraint network actually behaves as a cloth. The 0.05 threshold
/// is the acceptance criterion's; `softbody3d_sag.rs` clears 0.3 on the
/// structural-only grid, and a triangulated grid adds diagonals, so it is
/// stiffer than that reference but far above the threshold.
#[test]
fn a_mesh_built_plane_sags_between_its_pinned_corners() {
    let (positions, indices) = plane_mesh();
    let (mut body, map) =
        SoftBody3D::from_mesh(SoftBodyId3D(1), &positions, &indices, DEFAULT_WELD_EPSILON);

    // A well-formed grid shares no positions, so nothing should have merged.
    assert_eq!(
        body.vertex_count(),
        N * N,
        "no grid vertex should weld away"
    );
    assert_eq!(map, (0..N * N).collect::<Vec<_>>(), "expected identity map");

    let top_left = 0;
    let top_right = N - 1;
    let center = (N / 2) * N + (N / 2);

    body.pin_vertex(top_left);
    body.pin_vertex(top_right);

    let start_center = body.vertices[center].position;
    let start_left = body.vertices[top_left].position;

    let gravity = Vector3::new(0.0, -9.8, 0.0);
    for _ in 0..120 {
        body.step(1.0 / 60.0, gravity);
    }

    let left = body.vertices[top_left].position;
    assert_eq!(
        (left.x, left.y, left.z),
        (start_left.x, start_left.y, start_left.z),
        "a pinned vertex must be exactly immobile, on every axis"
    );

    let center_now = body.vertices[center].position;
    assert!(
        center_now.y < start_center.y - 0.05,
        "the centre must sag past the acceptance threshold (start {}, now {})",
        start_center.y,
        center_now.y
    );

    // An unstable spring network also "sags"; without this the assertion above
    // passes on a solver that has blown up.
    for (i, v) in body.vertices.iter().enumerate() {
        assert!(
            v.position.x.is_finite() && v.position.y.is_finite() && v.position.z.is_finite(),
            "vertex {i} went non-finite: {:?}",
            v.position
        );
    }
}

/// `from_mesh` must return a map usable for writing solved positions back onto
/// the original vertex buffer: one entry per authored vertex, all in range.
#[test]
fn the_visual_map_covers_every_authored_vertex() {
    let positions = unwelded_quad();
    let (body, map) = SoftBody3D::from_mesh(SoftBodyId3D(7), &positions, &[], DEFAULT_WELD_EPSILON);

    assert_eq!(map.len(), positions.len());
    for (visual, &physics) in map.iter().enumerate() {
        assert!(
            physics < body.vertex_count(),
            "visual vertex {visual} maps to {physics}, outside the {} simulated",
            body.vertex_count()
        );
    }
}
