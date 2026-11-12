//! pat-0q2kx.204 (partial): the soft-body solver in its new home.
//!
//! Two things are proven here.
//!
//! **The relocation.** `softbody3d` used to live in `gdphysics2d` even though it
//! is a 3D solver and its only consumer is `gdphysics3d::world`, which had to
//! reach across crates (`gdphysics2d::softbody3d::SoftBody3D`) for one of its
//! own core types. Every path in this file goes through `gdphysics3d`, so it
//! fails to compile if the move is incomplete.
//!
//! **The sag behaviour**, which is the first clause of the bead's acceptance
//! criterion. Until now the solver's only coverage was a `#[cfg(test)]` block
//! inside `world.rs`; nothing exercised pinning against gravity over time.
//!
//! ⚠️ This is NOT the bead's acceptance test. `physics3d_softbody_node` requires
//! scene-level wiring — a `.tscn` SoftBody3D whose mesh drives the solver and
//! whose rendered vertices follow it — which is not implemented. See the
//! completion report for what remains.

use gdcore::math::Vector3;
use gdphysics3d::softbody3d::{SoftBody3D, SoftBodyId3D};

const N: usize = 5;
const SPACING: f32 = 0.25;

/// Builds the 5x5 plane the acceptance criterion describes, in the XZ plane at
/// y = 0, with structural edges to each right/down neighbour.
fn plane() -> SoftBody3D {
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

    let mut edges = Vec::new();
    for row in 0..N {
        for col in 0..N {
            let i = row * N + col;
            if col + 1 < N {
                edges.push((i, i + 1));
            }
            if row + 1 < N {
                edges.push((i, i + N));
            }
        }
    }

    SoftBody3D::new(SoftBodyId3D(1), &positions, &edges)
}

const TOP_LEFT: usize = 0;
const TOP_RIGHT: usize = N - 1;
const CENTER: usize = (N / 2) * N + (N / 2);

/// A cloth pinned at two corners sags under gravity while the pinned corners
/// stay exactly put.
#[test]
fn pinned_plane_sags_between_its_pinned_corners() {
    let mut body = plane();
    body.pin_vertex(TOP_LEFT);
    body.pin_vertex(TOP_RIGHT);

    let start_center = body.vertices[CENTER].position;
    let start_left = body.vertices[TOP_LEFT].position;
    let start_right = body.vertices[TOP_RIGHT].position;

    let gravity = Vector3::new(0.0, -9.8, 0.0);
    for _ in 0..120 {
        body.step(1.0 / 60.0, gravity);
    }

    let left = body.vertices[TOP_LEFT].position;
    let right = body.vertices[TOP_RIGHT].position;
    let center = body.vertices[CENTER].position;

    // A pinned vertex is not "heavily damped" — it must not move at all, on any
    // axis. Checking only y would let a pin drift sideways unnoticed.
    assert_eq!(
        (left.x, left.y, left.z),
        (start_left.x, start_left.y, start_left.z),
        "a pinned vertex must be exactly immobile after 120 ticks"
    );
    assert_eq!(
        (right.x, right.y, right.z),
        (start_right.x, start_right.y, start_right.z),
        "the second pinned corner must be immobile too"
    );

    assert!(
        center.y < start_center.y - 0.3,
        "the unpinned centre must sag more than 0.3 below where it started \
         (start {}, now {})",
        start_center.y,
        center.y
    );
    assert!(
        center.y < left.y - 0.3,
        "the centre must hang below the pinned corners it is suspended from \
         (corner {}, centre {})",
        left.y,
        center.y
    );

    // The solver must not have exploded: an unstable spring network reaches
    // these thresholds too, and would satisfy the sag assertions above while
    // being worthless.
    for (i, v) in body.vertices.iter().enumerate() {
        assert!(
            v.position.x.is_finite() && v.position.y.is_finite() && v.position.z.is_finite(),
            "vertex {i} went non-finite: {:?}",
            v.position
        );
    }
}

/// With nothing pinned the whole sheet falls, so "the centre dropped" alone
/// never proves pinning works. This is the control the sag test needs.
#[test]
fn an_unpinned_plane_falls_as_a_whole() {
    let mut body = plane();
    let start_corner = body.vertices[TOP_LEFT].position;

    let gravity = Vector3::new(0.0, -9.8, 0.0);
    for _ in 0..120 {
        body.step(1.0 / 60.0, gravity);
    }

    assert!(
        body.vertices[TOP_LEFT].position.y < start_corner.y - 0.3,
        "an unpinned corner must fall; if it does not, `step` is ignoring \
         gravity and the sag test proves nothing"
    );
}

/// `pin_vertex` / `unpin_vertex` round-trip: an unpinned vertex resumes moving.
#[test]
fn unpinning_restores_motion() {
    let mut body = plane();
    body.pin_vertex(TOP_LEFT);
    assert!(body.is_vertex_pinned(TOP_LEFT));

    body.unpin_vertex(TOP_LEFT, 1.0);
    assert!(!body.is_vertex_pinned(TOP_LEFT));

    let start = body.vertices[TOP_LEFT].position;
    let gravity = Vector3::new(0.0, -9.8, 0.0);
    for _ in 0..120 {
        body.step(1.0 / 60.0, gravity);
    }

    assert!(
        body.vertices[TOP_LEFT].position.y < start.y - 0.3,
        "a vertex released with `unpin_vertex` must fall again"
    );
}
