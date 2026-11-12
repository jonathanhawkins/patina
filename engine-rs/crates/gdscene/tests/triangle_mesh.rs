//! pat-0q2kx.256 — `TriangleMesh`, the CPU BVH a mesh raycast runs against.
//!
//! Godot ground truth: `scene/resources/triangle_mesh.{h,cpp}`,
//! `doc/classes/TriangleMesh.xml`, `core/register_core_types.cpp:258`
//! (`GDREGISTER_CLASS` — concrete).
//!
//! The class was entirely absent, so there was no CPU ray query against mesh
//! geometry at all. Implemented in `gdserver3d::triangle_mesh`; registered in
//! the ClassDB from gdobject. This test lives in gdscene because it is the
//! nearest crate that depends on BOTH, and the acceptance spans both halves.
//!
//! ⚠️ A BVH is an ACCELERATION structure, not a semantic one: it must return
//! exactly what a linear scan would. `the_bvh_agrees_with_a_linear_scan` is the
//! load-bearing test here — a wrong split plane or a bad slab test produces
//! *occasionally* missed hits, never an error, so a handful of hand-picked rays
//! can easily all pass against a broken tree.

use gdcore::math::Vector3;
use gdserver3d::mesh::{Mesh3D, PrimitiveType};
use gdserver3d::triangle_mesh::TriangleMesh;

/// Positions are compared with a tolerance; these are f32 intersections, and
/// exact equality would make the test about float reproducibility rather than
/// about the geometry.
fn close(a: Vector3, b: Vector3) -> bool {
    (a - b).length() < 1e-4
}

/// Brute-force reference: the nearest hit over EVERY face, no tree involved.
/// This is what the BVH is required to reproduce.
fn linear_scan(faces: &[Vector3], from: Vector3, dir: Vector3) -> Option<Vector3> {
    let mut best: Option<(f32, Vector3)> = None;
    for t in faces.chunks_exact(3) {
        if let Some(p) = gdcore::geometry3d::ray_intersects_triangle(from, dir, t[0], t[1], t[2]) {
            let d = (p - from).length_squared();
            if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                best = Some((d, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// THE ACCEPTANCE. `generate_triangle_mesh()` yields a TriangleMesh whose
/// `intersect_ray` against a known face returns the right position AND normal,
/// and the class is registered.
#[test]
fn triangle_mesh_intersect_ray_returns_hit() {
    gdobject::class_db::register_3d_classes();
    assert!(
        gdobject::class_db::class_exists("TriangleMesh"),
        "TriangleMesh must be registered for scripts to name the type"
    );

    // A 2-unit cube centred on the origin: the +Z face sits at z = 1.
    let mesh = Mesh3D::cube(2.0);
    let tm = mesh
        .generate_triangle_mesh()
        .expect("a cube has triangles, so this must not be None");

    let hit = tm
        .intersect_ray(Vector3::new(0.0, 0.0, 5.0), Vector3::new(0.0, 0.0, -1.0))
        .expect("a ray down the -Z axis must hit the cube");

    assert!(
        close(hit.position, Vector3::new(0.0, 0.0, 1.0)),
        "must hit the NEAR (+Z) face at z=1, got {:?}. z=-1 would mean the \
         nearest-hit selection picked the far face.",
        hit.position
    );
    assert!(
        close(hit.normal, Vector3::new(0.0, 0.0, 1.0)),
        "the +Z face's outward normal is +Z, got {:?} — a flipped winding \
         would give -Z, which points INTO the surface the ray came from",
        hit.normal
    );
}

/// The nearest hit wins from either side. A tree that returned the first face
/// it happened to visit would pass the test above roughly half the time.
#[test]
fn the_nearest_face_wins_from_both_directions() {
    let tm = Mesh3D::cube(2.0).generate_triangle_mesh().unwrap();

    let front = tm
        .intersect_ray(Vector3::new(0.0, 0.0, 5.0), Vector3::new(0.0, 0.0, -1.0))
        .unwrap();
    assert!(close(front.position, Vector3::new(0.0, 0.0, 1.0)));

    let back = tm
        .intersect_ray(Vector3::new(0.0, 0.0, -5.0), Vector3::new(0.0, 0.0, 1.0))
        .unwrap();
    assert!(
        close(back.position, Vector3::new(0.0, 0.0, -1.0)),
        "approaching from -Z must hit the -Z face, got {:?}",
        back.position
    );
    assert!(
        close(back.normal, Vector3::new(0.0, 0.0, -1.0)),
        "and that face's outward normal points -Z, got {:?}",
        back.normal
    );
}

/// THE LOAD-BEARING TEST. Over a mesh big enough to force several levels of
/// split, the BVH must agree with a linear scan for every ray — including the
/// rays that miss.
#[test]
fn the_bvh_agrees_with_a_linear_scan() {
    // A sphere gives hundreds of faces with varied orientation, so the tree is
    // genuinely deep rather than one or two leaves.
    let mesh = Mesh3D::sphere(1.0, 16);
    let tm = mesh
        .generate_triangle_mesh()
        .expect("a sphere has triangles");
    assert!(
        tm.face_count() > 4 * 8,
        "the mesh must be large enough to force real subdivision, got {} faces",
        tm.face_count()
    );

    let faces = tm.get_faces();
    let mut hits = 0;
    let mut misses = 0;

    // A deterministic spread of origins and directions, including rays aimed
    // away from the mesh and rays parallel to an axis (the slab test's
    // division-by-zero case).
    for i in 0..12 {
        let a = i as f32 * 0.5;
        for (from, dir) in [
            (
                Vector3::new(0.0, 0.0, 3.0),
                Vector3::new(a * 0.1, 0.0, -1.0),
            ),
            (
                Vector3::new(a - 3.0, 0.0, 3.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
            (Vector3::new(0.0, a - 3.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
            (
                Vector3::new(3.0, a - 3.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
            ),
            // Pointing away — must miss.
            (Vector3::new(0.0, 0.0, 3.0), Vector3::new(0.0, 0.0, 1.0)),
        ] {
            let want = linear_scan(&faces, from, dir);
            let got = tm.intersect_ray(from, dir).map(|h| h.position);

            match (want, got) {
                (None, None) => misses += 1,
                (Some(w), Some(g)) => {
                    hits += 1;
                    assert!(
                        close(w, g),
                        "BVH and linear scan disagree for ray {from:?} -> {dir:?}: \
                         scan {w:?}, bvh {g:?}"
                    );
                }
                (w, g) => panic!(
                    "BVH and linear scan disagree on WHETHER ray {from:?} -> {dir:?} \
                     hits: scan {w:?}, bvh {g:?}"
                ),
            }
        }
    }

    // Guard the guard: if every ray missed, the comparison above proved
    // nothing at all.
    assert!(hits > 10, "the sweep must actually hit, got {hits} hits");
    assert!(misses > 0, "and must include genuine misses, got {misses}");
}

/// `intersect_segment` is bounded where `intersect_ray` is not. Sharing one
/// implementation would make the segment report hits it never reaches.
#[test]
fn intersect_segment_stops_at_its_end() {
    let tm = Mesh3D::cube(2.0).generate_triangle_mesh().unwrap();

    // Ends short of the cube (which starts at z = 1).
    assert!(
        tm.intersect_segment(Vector3::new(0.0, 0.0, 5.0), Vector3::new(0.0, 0.0, 3.0))
            .is_none(),
        "a segment that stops before the mesh must not report a hit"
    );

    // Same direction as a ray, which DOES hit — proving the miss above is the
    // bound doing its job and not a broken direction.
    assert!(
        tm.intersect_ray(Vector3::new(0.0, 0.0, 5.0), Vector3::new(0.0, 0.0, -1.0))
            .is_some(),
        "the unbounded ray along the same direction must still hit"
    );

    // Long enough to reach.
    let hit = tm
        .intersect_segment(Vector3::new(0.0, 0.0, 5.0), Vector3::new(0.0, 0.0, -5.0))
        .expect("a segment crossing the cube must hit");
    assert!(close(hit.position, Vector3::new(0.0, 0.0, 1.0)));
}

/// A mesh with no triangles yields `None`, matching Godot's null ref — not an
/// empty structure that every query silently misses.
#[test]
fn a_mesh_with_no_triangles_yields_none() {
    let empty = Mesh3D::new(PrimitiveType::Triangles);
    assert!(empty.generate_triangle_mesh().is_none());

    // A LINE surface has no faces to hit. Treating its vertices as triangles
    // would fabricate geometry that is never rendered.
    let mut lines = Mesh3D::new(PrimitiveType::Lines);
    lines.vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
    ];
    assert!(
        lines.generate_triangle_mesh().is_none(),
        "a line primitive must contribute no faces"
    );
}

/// Every surface contributes, not just the primary one. `Mesh3D::aabb()` and
/// `triangle_count()` look only at the primary surface; a TriangleMesh that
/// copied that narrowness would raycast against a hole where geometry is.
#[test]
fn extra_surfaces_are_included() {
    let mut mesh = Mesh3D::cube(2.0);
    let primary_faces = mesh.generate_triangle_mesh().unwrap().face_count();

    // A second surface, well clear of the cube so a hit against it is
    // unambiguous.
    mesh.surfaces.push(gdserver3d::mesh::Surface3D {
        vertices: vec![
            Vector3::new(-1.0, -1.0, 10.0),
            Vector3::new(1.0, -1.0, 10.0),
            Vector3::new(0.0, 1.0, 10.0),
        ],
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: vec![0, 1, 2],
        primitive_type: PrimitiveType::Triangles,
        bone_weights: Vec::new(),
        bone_indices: Vec::new(),
        lods: Vec::new(),
    });

    let tm = mesh.generate_triangle_mesh().unwrap();
    assert_eq!(
        tm.face_count(),
        primary_faces + 1,
        "the extra surface's triangle must be in the tree"
    );

    let hit = tm
        .intersect_ray(Vector3::new(0.0, 0.0, 20.0), Vector3::new(0.0, 0.0, -1.0))
        .expect("must hit the far surface");
    assert!(
        close(hit.position, Vector3::new(0.0, 0.0, 10.0)),
        "the nearest face along -Z is now the extra surface at z=10, got {:?}",
        hit.position
    );
}

/// `create_from_faces` / `get_faces` round-trip, and a trailing partial
/// triangle is dropped rather than padded with an invented corner.
#[test]
fn create_from_faces_round_trips() {
    let corners = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ];
    let tm = TriangleMesh::create_from_faces(&corners);
    assert_eq!(tm.face_count(), 1);
    assert_eq!(tm.get_faces(), corners);

    // Four corners is one triangle plus a stray vertex.
    let mut ragged = corners.clone();
    ragged.push(Vector3::new(5.0, 5.0, 5.0));
    let tm2 = TriangleMesh::create_from_faces(&ragged);
    assert_eq!(
        tm2.face_count(),
        1,
        "a trailing partial triangle must be dropped, not completed with an \
         invented corner"
    );
}
