//! pat-s3rrv.201: SurfaceTool's staged per-vertex attributes.
//!
//! `set_color` and `clear` were **ClassDB-registered with no dispatch arm**, so
//! `has_method` answered true and the call failed; `generate_normals` did not
//! exist at all. This covers the three at the level they are implemented.
//!
//! Godot ground truth: `upstream/godot/scene/resources/surface_tool.cpp`.

use gdcore::math::{Color, Vector3};
use gdserver3d::mesh::PrimitiveType;
use gdserver3d::mesh::SurfaceTool;

/// A quad as two triangles in the XY plane, wound counter-clockwise so its
/// face normal points along +Z.
fn quad() -> SurfaceTool {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    for p in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }
    t
}

/// ⚠️ Colours advance on `add_vertex`, not on `set_color`.
///
/// These arrays are read BY INDEX alongside `vertices`. A colour pushed inside
/// `set_color` desynchronises the moment a script sets one twice before a
/// vertex, or adds a vertex without setting one — and every later vertex then
/// carries its neighbour's colour.
#[test]
fn colours_stay_in_lockstep_with_vertices() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);

    // No colour set: the vertex still gets one.
    t.add_vertex(Vector3::ZERO);
    // Set twice before a single vertex — only the last applies, and only once.
    t.set_color(Color::new(1.0, 0.0, 0.0, 1.0));
    t.set_color(Color::new(0.0, 1.0, 0.0, 1.0));
    t.add_vertex(Vector3::new(1.0, 0.0, 0.0));
    // Set once, then two vertices — both take it.
    t.set_color(Color::new(0.0, 0.0, 1.0, 1.0));
    t.add_vertex(Vector3::new(2.0, 0.0, 0.0));
    t.add_vertex(Vector3::new(3.0, 0.0, 0.0));

    assert_eq!(t.vertex_count(), 4);
    assert_eq!(
        t.colors().len(),
        4,
        "one colour per vertex, however many times set_color was called"
    );
    // An unset colour is opaque white, as in Godot — not transparent black.
    assert_eq!(t.colors()[0], Color::WHITE);
    assert_eq!(t.colors()[1], Color::new(0.0, 1.0, 0.0, 1.0));
    assert_eq!(t.colors()[2], Color::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(t.colors()[3], Color::new(0.0, 0.0, 1.0, 1.0));
}

/// `generate_normals` replaces every normal with its face's geometric normal.
#[test]
fn generate_normals_writes_the_face_normal() {
    let mut t = quad();
    // Seed deliberately wrong normals so a no-op implementation is visible.
    let mesh_before = t.commit();
    assert!(
        mesh_before.normals.iter().all(|n| *n == Vector3::ZERO),
        "unset normals start at zero, which is what makes the check below mean something"
    );

    t.generate_normals(false);
    let mesh = t.commit();

    assert_eq!(mesh.normals.len(), 6);
    for (i, n) in mesh.normals.iter().enumerate() {
        assert!(
            (n.z - 1.0).abs() < 1e-5 && n.x.abs() < 1e-5 && n.y.abs() < 1e-5,
            "vertex {i} should face +Z for a CCW quad in XY, got {n:?}"
        );
    }
}

/// `flip` reverses them, which is the parameter's whole purpose.
#[test]
fn generate_normals_can_flip() {
    let mut t = quad();
    t.generate_normals(true);
    let mesh = t.commit();
    for (i, n) in mesh.normals.iter().enumerate() {
        assert!(
            (n.z + 1.0).abs() < 1e-5,
            "vertex {i} should face -Z when flipped, got {n:?}"
        );
    }
}

/// A degenerate triangle has no defined normal and must not poison the mesh.
///
/// Normalising a zero-length cross product yields NaN, which renders as a black
/// or invisible surface — and NaN propagates, so one bad triangle is enough.
#[test]
fn a_degenerate_triangle_produces_no_nan_normals() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    // Three collinear points: zero area.
    for p in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(2.0, 0.0, 0.0),
    ] {
        t.add_vertex(p);
    }
    t.generate_normals(false);
    let mesh = t.commit();

    for (i, n) in mesh.normals.iter().enumerate() {
        assert!(
            n.x.is_finite() && n.y.is_finite() && n.z.is_finite(),
            "vertex {i} normal went non-finite: {n:?}"
        );
    }
}

/// ⚠️ `clear` must reset the STAGED attributes too, not just the arrays.
///
/// Godot's does. A colour or normal left staged leaks onto the first vertex of
/// the next surface — a bug that only shows when a tool is reused, which is
/// exactly what `clear` exists for.
#[test]
fn clear_resets_staged_attributes_not_just_the_arrays() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_color(Color::new(1.0, 0.0, 0.0, 1.0));
    t.set_normal(Vector3::new(0.0, 1.0, 0.0));
    t.set_uv([0.5, 0.5]);
    t.add_vertex(Vector3::ZERO);
    assert_eq!(t.vertex_count(), 1);

    t.clear();
    assert_eq!(t.vertex_count(), 0, "clear empties the vertex array");
    assert_eq!(t.colors().len(), 0);

    // Rebuild and check nothing leaked from before the clear.
    t.begin(PrimitiveType::Triangles);
    t.add_vertex(Vector3::ZERO);
    let mesh = t.commit();
    assert_eq!(
        t.colors()[0],
        Color::WHITE,
        "a colour staged before clear must not colour the next surface"
    );
    assert_eq!(
        mesh.normals[0],
        Vector3::ZERO,
        "a normal staged before clear must not survive it"
    );
    assert_eq!(mesh.uvs[0], [0.0, 0.0], "nor a UV");
}

/// An empty tool commits an empty mesh rather than panicking.
#[test]
fn an_empty_tool_commits_an_empty_mesh() {
    let t = SurfaceTool::new();
    let mesh = t.commit();
    assert!(mesh.vertices.is_empty());
    assert!(mesh.indices.is_empty());
}

// ---------------------------------------------------------------------------
// index() / deindex() — a paired invariant, ported from surface_tool.cpp.
// ---------------------------------------------------------------------------

/// Building a quad as six loose vertices, two of which coincide exactly.
fn quad_with_shared_corners() -> SurfaceTool {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    for p in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
        // Second triangle repeats two corners exactly.
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }
    t
}

/// `index()` collapses the duplicated corners and emits an index buffer.
#[test]
fn index_collapses_identical_vertices() {
    let mut t = quad_with_shared_corners();
    assert_eq!(t.vertex_count(), 6);

    t.index();
    assert_eq!(t.vertex_count(), 4, "the two shared corners merge");

    let mesh = t.commit();
    assert_eq!(mesh.indices.len(), 6, "still six triangle corners");
    assert_eq!(mesh.vertices.len(), 4);
}

/// ⚠️ Only vertices identical in EVERY attribute merge.
///
/// Deduping on position alone welds the seam of any UV-unwrapped mesh, where
/// one corner legitimately carries two different UVs — and the texture tears
/// there. This is the same shape as the torus seam: the duplicate exists
/// precisely because its attributes differ.
#[test]
fn index_does_not_merge_vertices_that_differ_in_uv() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);

    t.set_uv([0.0, 0.0]);
    t.add_vertex(Vector3::ZERO);
    t.set_uv([1.0, 0.0]); // same position, different UV — a seam
    t.add_vertex(Vector3::ZERO);
    t.set_uv([0.0, 0.0]); // identical to the first in every attribute
    t.add_vertex(Vector3::ZERO);

    t.index();
    assert_eq!(
        t.vertex_count(),
        2,
        "the seam vertex must survive; only the exact duplicate merges"
    );
}

/// Colour is part of the identity too.
#[test]
fn index_does_not_merge_vertices_that_differ_in_colour() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_color(Color::new(1.0, 0.0, 0.0, 1.0));
    t.add_vertex(Vector3::ZERO);
    t.set_color(Color::new(0.0, 1.0, 0.0, 1.0));
    t.add_vertex(Vector3::ZERO);

    t.index();
    assert_eq!(
        t.vertex_count(),
        2,
        "different colours are different vertices"
    );
}

/// The pair's invariant: index then deindex restores the original sequence.
#[test]
fn index_then_deindex_round_trips() {
    let before = quad_with_shared_corners().commit();

    let mut t = quad_with_shared_corners();
    t.index();
    t.deindex();
    let after = t.commit();

    assert_eq!(
        after.vertices, before.vertices,
        "deindex must restore the original vertex order, not just the count"
    );
    assert_eq!(after.uvs, before.uvs);
    assert_eq!(
        after.indices,
        (0..after.vertices.len() as u32).collect::<Vec<u32>>(),
        "a deindexed surface's implicit indices are sequential"
    );
}

/// ⚠️ `deindex()` on a surface with no index buffer must do NOTHING.
///
/// Godot returns early — "nothing to deindex". An implementation that instead
/// rebuilds the vertex array from an empty index list silently **empties a
/// perfectly good surface**, and the caller sees a mesh that renders nothing.
#[test]
fn deindex_on_an_unindexed_surface_is_a_no_op() {
    let mut t = quad_with_shared_corners();
    let before = t.commit();

    t.deindex();
    let after = t.commit();

    assert_eq!(after.vertices.len(), 6, "the surface must not be emptied");
    assert_eq!(after.vertices, before.vertices);
}

/// `index()` on an already-indexed surface leaves it alone.
#[test]
fn index_on_an_indexed_surface_is_a_no_op() {
    let mut t = quad_with_shared_corners();
    t.index();
    let once = t.commit();

    t.index();
    let twice = t.commit();

    assert_eq!(once.vertices, twice.vertices);
    assert_eq!(once.indices, twice.indices);
}

// ---------------------------------------------------------------------------
// append_from / create_from, ported from surface_tool.cpp.
// ---------------------------------------------------------------------------

use gdcore::math3d::Transform3D;
use gdserver3d::mesh::Mesh3D;

/// A single triangle with an explicit +Z normal on every vertex.
fn triangle_mesh() -> Mesh3D {
    let mut m = Mesh3D::new(PrimitiveType::Triangles);
    m.vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ];
    m.normals = vec![Vector3::new(0.0, 0.0, 1.0); 3];
    m.uvs = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    m.indices = vec![0, 1, 2];
    m
}

/// ⚠️ THE detail worth a test: a normal takes the BASIS, not the transform.
///
/// Upstream is `v.normal = p_xform.basis.xform(v.normal)`. A normal is a
/// direction — running it through the full transform adds the translation, so a
/// mesh appended at an offset would light as though its surface faced the world
/// origin. A translation-only transform makes the mistake unmistakable: the
/// vertex must move and the normal must not.
#[test]
fn append_from_transforms_vertices_but_only_rotates_normals() {
    let mut t = SurfaceTool::new();
    let mut xform = Transform3D::IDENTITY;
    xform.origin = Vector3::new(5.0, 0.0, 0.0);

    t.append_from(&triangle_mesh(), xform);
    let mesh = t.commit();

    assert_eq!(
        mesh.vertices[0],
        Vector3::new(5.0, 0.0, 0.0),
        "the vertex takes the full transform"
    );
    for (i, n) in mesh.normals.iter().enumerate() {
        assert_eq!(
            *n,
            Vector3::new(0.0, 0.0, 1.0),
            "normal {i} must be unmoved by a pure translation; got {n:?} — the \
             full transform was applied to a direction"
        );
    }
}

/// ⚠️ Appended indices must be offset by the vertices already present.
///
/// Without the offset the second mesh's triangles index into the FIRST mesh's
/// vertices — the geometry is still "there", the counts still look right, and
/// the surface is drawn inside out or collapsed.
#[test]
fn append_from_offsets_indices_by_the_existing_vertex_count() {
    let mut t = SurfaceTool::new();
    t.append_from(&triangle_mesh(), Transform3D::IDENTITY);
    t.append_from(&triangle_mesh(), Transform3D::IDENTITY);

    let mesh = t.commit();
    assert_eq!(mesh.vertices.len(), 6, "two triangles' worth of vertices");
    assert_eq!(
        mesh.indices,
        vec![0, 1, 2, 3, 4, 5],
        "the second triangle must index its OWN vertices, not the first's"
    );
}

/// An empty tool adopts the source's primitive type.
#[test]
fn append_from_adopts_the_primitive_type_when_empty() {
    let mut src = triangle_mesh();
    src.primitive_type = PrimitiveType::Lines;

    let mut t = SurfaceTool::new();
    t.append_from(&src, Transform3D::IDENTITY);
    assert_eq!(t.commit().primitive_type, PrimitiveType::Lines);
}

/// `create_from` replaces rather than accumulates — that is the whole
/// difference between it and `append_from`.
#[test]
fn create_from_replaces_existing_contents() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.add_vertex(Vector3::new(9.0, 9.0, 9.0));
    assert_eq!(t.vertex_count(), 1);

    t.create_from(&triangle_mesh());
    let mesh = t.commit();

    assert_eq!(mesh.vertices.len(), 3, "the prior vertex must be gone");
    assert!(
        !mesh.vertices.contains(&Vector3::new(9.0, 9.0, 9.0)),
        "create_from must not accumulate onto what was already there"
    );
}

/// A source mesh with no index buffer still appends coherently.
#[test]
fn append_from_handles_an_unindexed_source() {
    let mut src = triangle_mesh();
    src.indices.clear();

    let mut t = SurfaceTool::new();
    t.append_from(&src, Transform3D::IDENTITY);
    t.append_from(&src, Transform3D::IDENTITY);

    let mesh = t.commit();
    assert_eq!(mesh.vertices.len(), 6);
    assert_eq!(
        mesh.indices,
        vec![0, 1, 2, 3, 4, 5],
        "implicit source indices must still be offset per append"
    );
}

// ---------------------------------------------------------------------------
// add_triangle_fan
// ---------------------------------------------------------------------------

/// ⚠️ A fan of `n` points produces `(n - 2) * 3` vertices, not `n`.
///
/// Godot expands the fan into loose triangles `(0, i+1, i+2)`, so the hub is
/// re-emitted for every triangle. Expecting `n` back is the natural guess and
/// it is wrong — a caller sizing a buffer by point count under-allocates.
#[test]
fn a_fan_expands_into_loose_triangles() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);

    // A four-point fan: hub plus three rim points => 2 triangles => 6 vertices.
    let pts = [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ];
    assert!(t.add_triangle_fan(&pts, &[], &[], &[]));

    assert_eq!(t.vertex_count(), 6, "(4 - 2) * 3");
    let mesh = t.commit();
    // The hub appears once per triangle.
    let hub = pts[0];
    assert_eq!(
        mesh.vertices.iter().filter(|v| **v == hub).count(),
        2,
        "the hub must be re-emitted for each of the two triangles"
    );
    assert_eq!(mesh.vertices[0], pts[0]);
    assert_eq!(mesh.vertices[1], pts[1]);
    assert_eq!(mesh.vertices[2], pts[2]);
    assert_eq!(
        mesh.vertices[3], pts[0],
        "second triangle restarts at the hub"
    );
    assert_eq!(mesh.vertices[4], pts[2]);
    assert_eq!(mesh.vertices[5], pts[3]);
}

/// Per-point attributes follow their vertex, including on the repeated hub.
#[test]
fn fan_attributes_are_indexed_with_their_point() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);

    let pts = [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ];
    let cols = [
        Color::new(1.0, 0.0, 0.0, 1.0),
        Color::new(0.0, 1.0, 0.0, 1.0),
        Color::new(0.0, 0.0, 1.0, 1.0),
        Color::new(1.0, 1.0, 0.0, 1.0),
    ];
    assert!(t.add_triangle_fan(&pts, &[], &cols, &[]));

    let c = t.colors();
    assert_eq!(c.len(), 6);
    assert_eq!(c[0], cols[0], "hub colour");
    assert_eq!(c[3], cols[0], "the repeated hub carries its colour again");
    assert_eq!(c[1], cols[1]);
    assert_eq!(c[5], cols[3]);
}

/// Shorter attribute slices leave the staged value in place, as upstream's
/// `if (size > n)` guards do — they do not reset to a default.
#[test]
fn fan_tolerates_shorter_attribute_slices() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_color(Color::new(0.25, 0.5, 0.75, 1.0));

    let pts = [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
    ];
    // One colour for three points.
    let cols = [Color::new(1.0, 0.0, 0.0, 1.0)];
    assert!(t.add_triangle_fan(&pts, &[], &cols, &[]));

    let c = t.colors();
    assert_eq!(c[0], cols[0], "point 0 takes the supplied colour");
    assert_eq!(
        c[1], cols[0],
        "points past the slice keep the last staged colour, not a default"
    );
}

/// ⚠️ Godot `ERR_FAIL_COND`s these; nothing must be emitted.
///
/// A partially-built fan is worse than none — the surface would carry
/// triangles the caller never asked for and no error to explain them.
#[test]
fn a_fan_is_refused_rather_than_partially_emitted() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    assert!(
        !t.add_triangle_fan(&[Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0)], &[], &[], &[]),
        "fewer than three points is refused"
    );
    assert_eq!(t.vertex_count(), 0, "and emits nothing");

    let mut lines = SurfaceTool::new();
    lines.begin(PrimitiveType::Lines);
    let pts = [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
    ];
    assert!(
        !lines.add_triangle_fan(&pts, &[], &[], &[]),
        "a fan is only meaningful on a TRIANGLES surface"
    );
    assert_eq!(lines.vertex_count(), 0);
}

// ---------------------------------------------------------------------------
// set_bones / set_weights
// ---------------------------------------------------------------------------

/// ⚠️ THE trap: an ordinary mesh must NOT report itself skinned.
///
/// `Mesh3D::is_skinned()` is true whenever the bone arrays merely *match* the
/// vertex count, and `render_server_3d` gates its skinning branch on exactly
/// that. Emitting default bone data for every vertex — the obvious way to keep
/// the parallel arrays in lockstep — would make **every** procedural mesh look
/// skinned, and the render path would deform it against whatever skeleton
/// resolved. With all-zero weights that collapses the mesh onto the origin.
///
/// Godot avoids this with its `ARRAY_FORMAT_BONES` flag; the equivalent here is
/// only emitting the arrays when the script actually set skin data.
#[test]
fn a_mesh_built_without_skin_data_is_not_skinned() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    for p in [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }

    let mesh = t.commit();
    assert!(
        mesh.bone_weights.is_empty() && mesh.bone_indices.is_empty(),
        "no skin data was set, so none may be emitted"
    );
    assert!(
        !mesh.is_skinned(),
        "an unskinned procedural mesh must not enter the skinning path"
    );
}

/// And when the script does set them, they arrive.
#[test]
fn skin_data_reaches_the_committed_mesh() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_bones(&[2, 5]);
    t.set_weights(&[0.75, 0.25]);
    for p in [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }

    let mesh = t.commit();
    assert!(
        mesh.is_skinned(),
        "skin data was set, so the mesh is skinned"
    );
    assert_eq!(mesh.bone_indices.len(), 3, "one entry per vertex");
    assert_eq!(mesh.bone_weights.len(), 3);
    // A short array leaves the remaining slots at zero, as Godot does.
    assert_eq!(mesh.bone_indices[0], [2, 5, 0, 0]);
    assert_eq!(mesh.bone_weights[0], [0.75, 0.25, 0.0, 0.0]);
}

/// Skin influences stage like every other attribute — they apply from the next
/// vertex on, and persist until changed.
#[test]
fn skin_influences_stage_per_vertex() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);

    t.set_bones(&[1, 0, 0, 0]);
    t.set_weights(&[1.0, 0.0, 0.0, 0.0]);
    t.add_vertex(Vector3::ZERO);
    t.set_bones(&[3, 0, 0, 0]);
    t.add_vertex(Vector3::new(1.0, 0.0, 0.0));
    // Not changed again: the third vertex keeps bone 3.
    t.add_vertex(Vector3::new(0.0, 1.0, 0.0));

    let mesh = t.commit();
    assert_eq!(mesh.bone_indices[0], [1, 0, 0, 0]);
    assert_eq!(mesh.bone_indices[1], [3, 0, 0, 0]);
    assert_eq!(
        mesh.bone_indices[2],
        [3, 0, 0, 0],
        "an unchanged influence persists to the next vertex"
    );
    // The weight set before the first vertex still applies to all three.
    assert_eq!(mesh.bone_weights[2], [1.0, 0.0, 0.0, 0.0]);
}

/// More than four influences are truncated, not wrapped or rejected.
#[test]
fn more_than_four_influences_are_truncated() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_bones(&[1, 2, 3, 4, 5, 6]);
    t.set_weights(&[0.1, 0.2, 0.3, 0.4, 0.5]);
    t.add_vertex(Vector3::ZERO);

    let mesh = t.commit();
    assert_eq!(mesh.bone_indices[0], [1, 2, 3, 4], "the fifth is dropped");
    assert_eq!(mesh.bone_weights[0], [0.1, 0.2, 0.3, 0.4]);
}

/// ⚠️ `clear` must reset the skin FLAG, not just the arrays.
///
/// A tool reused after building a skinned surface would otherwise emit bone
/// arrays for the next, unskinned one — putting it back into the skinning path
/// with default influences.
#[test]
fn clear_resets_the_skin_flag() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_bones(&[1]);
    t.set_weights(&[1.0]);
    t.add_vertex(Vector3::ZERO);
    assert!(t.commit().is_skinned());

    t.clear();
    t.begin(PrimitiveType::Triangles);
    for p in [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }
    assert!(
        !t.commit().is_skinned(),
        "a cleared tool must not carry the previous surface's skin flag"
    );
}

// ---------------------------------------------------------------------------
// Smoothing groups. ⚠️ Every generate_normals test above uses a PLANAR fixture,
// where both triangles share a face normal — so smooth and flat give the same
// answer and none of them can tell the two apart. These use a folded pair.
// ---------------------------------------------------------------------------

use gdserver3d::mesh::NO_SMOOTH_GROUP;

/// Two triangles sharing the edge (0,0,0)-(1,0,0), folded 90 degrees:
/// the first lies in XY (face normal +Z), the second in XZ (face normal +Y).
fn folded_pair() -> SurfaceTool {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    for p in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ] {
        t.add_vertex(p);
    }
    t
}

/// ⚠️ Godot's DEFAULT is to smooth, not to flat-shade.
///
/// `last_smooth_group` defaults to 0 — a real group — so `generate_normals()`
/// accumulates the faces meeting at each position and normalises. Defaulting to
/// flat is the intuitive reading and diverges from the engine on every mesh
/// whose faces meet at an angle.
///
/// Here the shared edge carries +Z and +Y, so its smoothed normal is the
/// normalised sum, (0, √½, √½).
#[test]
fn generate_normals_smooths_by_default() {
    let mut t = folded_pair();
    t.generate_normals(false);
    let mesh = t.commit();

    let half = std::f32::consts::FRAC_1_SQRT_2;
    // Vertices 0 and 1 sit on the shared edge and belong to both faces.
    for i in [0usize, 1] {
        let n = mesh.normals[i];
        assert!(
            n.x.abs() < 1e-4 && (n.y - half).abs() < 1e-4 && (n.z - half).abs() < 1e-4,
            "shared vertex {i} should carry the AVERAGED normal (0, {half}, {half}), \
             got {n:?} — a flat-shaded result means the default group was wrong"
        );
    }
    // Vertex 2 belongs only to the first face, so it keeps +Z.
    let n2 = mesh.normals[2];
    assert!(
        (n2.z - 1.0).abs() < 1e-4,
        "a vertex on one face only keeps that face's normal, got {n2:?}"
    );
}

/// `NO_SMOOTH_GROUP` opts out, and then the same fixture flat-shades.
#[test]
fn no_smooth_group_flat_shades_the_same_geometry() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_smooth_group(NO_SMOOTH_GROUP);
    for p in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ] {
        t.add_vertex(p);
    }
    t.generate_normals(false);
    let mesh = t.commit();

    // First face: +Z on all three of its own vertices.
    for i in 0..3 {
        assert!(
            (mesh.normals[i].z - 1.0).abs() < 1e-4,
            "flat: vertex {i} takes its OWN face normal +Z, got {:?}",
            mesh.normals[i]
        );
    }
    // Second face: +Y, including on the copies at the shared positions.
    for i in 3..6 {
        assert!(
            (mesh.normals[i].y - 1.0).abs() < 1e-4,
            "flat: vertex {i} takes its own face normal +Y, got {:?}",
            mesh.normals[i]
        );
    }
}

/// Different groups do not smooth together even at the same position.
#[test]
fn different_smooth_groups_do_not_merge() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_smooth_group(1);
    for p in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }
    t.set_smooth_group(2);
    for p in [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ] {
        t.add_vertex(p);
    }
    t.generate_normals(false);
    let mesh = t.commit();

    assert!(
        (mesh.normals[0].z - 1.0).abs() < 1e-4,
        "group 1 keeps +Z, got {:?}",
        mesh.normals[0]
    );
    assert!(
        (mesh.normals[3].y - 1.0).abs() < 1e-4,
        "group 2 keeps +Y at the same position, got {:?}",
        mesh.normals[3]
    );
}

/// ⚠️ `generate_normals` deindexes first, so a shared vertex gets one copy per
/// face rather than the last face silently overwriting it.
#[test]
fn generate_normals_deindexes_before_shading() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    for p in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ] {
        t.add_vertex(p);
    }
    // Two triangles over four vertices, sharing an edge.
    for i in [0u32, 1, 2, 1, 0, 3] {
        t.add_index(i);
    }
    t.generate_normals(false);

    let mesh = t.commit();
    assert_eq!(
        mesh.vertices.len(),
        6,
        "the surface must be expanded to one vertex per face corner"
    );
    assert!(
        mesh.normals.iter().all(|n| n.length() > 0.5),
        "every vertex shaded"
    );
}
