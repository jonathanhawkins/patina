//! pat-s3rrv.197 — `ImmediateMesh` surface semantics.
//!
//! Godot ground truth: `scene/resources/immediate_mesh.cpp`,
//! `doc/classes/ImmediateMesh.xml`.
//!
//! These pin the behaviour acceptance clauses 3, 4 and 5 turn on — attributes
//! carried into the surface arrays, `clear_surfaces` emptying the mesh, and
//! calls outside a begin/end pair ERRORING rather than silently appending. They
//! are asserted at the resource level because that is where those rules are
//! decided; the script-driven end-to-end test is separate.

use gdcore::math::{Color, Vector3};
use gdserver3d::immediate_mesh::{primitive_from_godot_int, ImmediateMesh, ImmediateMeshError};
use gdserver3d::mesh::PrimitiveType;

/// Godot's `Mesh.PRIMITIVE_TRIANGLES`.
const TRIANGLES: i64 = 3;
/// Godot's `Mesh.PRIMITIVE_LINES`.
const LINES: i64 = 1;

fn triangle() -> ImmediateMesh {
    let mut m = ImmediateMesh::new();
    m.surface_begin(TRIANGLES).expect("surface opens");
    for v in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        m.surface_add_vertex(v).expect("vertex adds");
    }
    m.surface_end().expect("surface closes");
    m
}

/// 🔑 The primitive ordinals are Godot's, not our enum's declaration order.
///
/// ⚠️ This is the one that fails silently. `PrimitiveType` is declared
/// `Triangles, Lines, Points`, while Godot numbers them POINTS 0, LINES 1,
/// LINE_STRIP 2, **TRIANGLES 3**. A positional mapping would accept
/// `PRIMITIVE_TRIANGLES` and build a point cloud — geometry that renders, just
/// not the geometry that was asked for.
#[test]
fn primitive_ordinals_follow_godot_not_declaration_order() {
    assert_eq!(primitive_from_godot_int(0), Some(PrimitiveType::Points));
    assert_eq!(primitive_from_godot_int(1), Some(PrimitiveType::Lines));
    assert_eq!(primitive_from_godot_int(3), Some(PrimitiveType::Triangles));

    // ⚠️ 2 is LINE_STRIP and 4 is TRIANGLE_STRIP — real Godot values this
    // renderer cannot draw. They must be REFUSED, not rounded to a neighbour:
    // silently treating LINE_STRIP as LINES draws a different shape.
    assert_eq!(primitive_from_godot_int(2), None);
    assert_eq!(primitive_from_godot_int(4), None);
    assert_eq!(primitive_from_godot_int(99), None);
}

/// A closed surface holds exactly the vertices it was given.
#[test]
fn a_closed_surface_holds_its_vertices() {
    let m = triangle();
    assert_eq!(m.surface_count(), 1);
    assert!(!m.is_surface_open(), "surface_end closes the surface");

    let s = &m.surfaces()[0];
    assert_eq!(s.primitive_type, PrimitiveType::Triangles);
    assert_eq!(s.vertices.len(), 3);
    assert_eq!(s.vertices[1], Vector3::new(1.0, 0.0, 0.0));
}

/// 🔑 Clause 3: colour, normal and uv set before a vertex are carried into the
/// surface arrays, and apply to the NEXT vertex rather than the previous one.
///
/// ⚠️ The off-by-one is the whole risk here. Godot documents these as "call
/// before `surface_add_vertex`", so an implementation that decorated the vertex
/// just added would produce arrays shifted by one — which looks like a winding
/// or index bug, not an attribute bug, and is correspondingly hard to find.
#[test]
fn attributes_apply_to_the_next_vertex_and_reach_the_arrays() {
    let mut m = ImmediateMesh::new();
    m.surface_begin(TRIANGLES).unwrap();

    m.surface_set_color(Color::new(1.0, 0.0, 0.0, 1.0)).unwrap();
    m.surface_set_normal(Vector3::new(0.0, 1.0, 0.0)).unwrap();
    m.surface_set_uv([0.25, 0.5]).unwrap();
    m.surface_add_vertex(Vector3::ZERO).unwrap();

    // Change the colour only; normal and uv must PERSIST, as Godot's do.
    m.surface_set_color(Color::new(0.0, 0.0, 1.0, 1.0)).unwrap();
    m.surface_add_vertex(Vector3::new(1.0, 0.0, 0.0)).unwrap();
    m.surface_end().unwrap();

    let s = &m.surfaces()[0];
    assert_eq!(s.colors.len(), 2, "one colour per vertex");
    assert!(
        s.colors[0].r > 0.9 && s.colors[1].b > 0.9,
        "the FIRST vertex is red and the SECOND is blue; reversed means the \
         attribute decorated the previous vertex instead of the next"
    );
    assert_eq!(
        s.normals.len(),
        2,
        "the normal set once must persist onto the second vertex too"
    );
    assert_eq!(s.normals[1], Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(s.uvs[1], [0.25, 0.5]);
}

/// 🔑 Clause 4: `clear_surfaces` empties the mesh, and it then renders nothing.
#[test]
fn clear_surfaces_empties_the_mesh() {
    let mut m = triangle();
    assert!(m.to_mesh().is_some(), "fixture: it renders before clearing");

    m.clear_surfaces();
    assert_eq!(m.surface_count(), 0);
    assert!(
        m.to_mesh().is_none(),
        "a cleared ImmediateMesh must render NOTHING — an empty Mesh3D husk \
         would still be handed to the renderer as a live instance"
    );
}

/// ⚠️ `clear_surfaces` also discards a surface left OPEN.
///
/// Godot clears everything. Keeping the open surface would let a half-built
/// surface from before the clear reappear at the next `surface_end`.
#[test]
fn clear_surfaces_also_drops_an_open_surface() {
    let mut m = ImmediateMesh::new();
    m.surface_begin(LINES).unwrap();
    m.surface_add_vertex(Vector3::ZERO).unwrap();
    m.clear_surfaces();

    assert!(!m.is_surface_open());
    assert_eq!(
        m.surface_end(),
        Err(ImmediateMeshError::NoSurfaceOpen),
        "after a clear there is no surface to end"
    );
}

/// 🔑 Clause 5: calls outside a begin/end pair ERROR rather than silently
/// appending.
///
/// ⚠️ Silently appending is the dangerous alternative, not a harmless one: the
/// vertex would land in whatever surface happened to be open last, corrupting
/// geometry that was already correct.
#[test]
fn calls_outside_a_begin_end_pair_are_errors() {
    let mut m = ImmediateMesh::new();

    assert_eq!(
        m.surface_add_vertex(Vector3::ZERO),
        Err(ImmediateMeshError::NoSurfaceOpen)
    );
    assert_eq!(
        m.surface_set_color(Color::new(1.0, 1.0, 1.0, 1.0)),
        Err(ImmediateMeshError::NoSurfaceOpen)
    );
    assert_eq!(m.surface_end(), Err(ImmediateMeshError::NoSurfaceOpen));

    // And nothing was recorded by the rejected calls.
    assert_eq!(m.surface_count(), 0);

    // A second begin without an end is equally an error, not a silent restart —
    // a restart would discard vertices the caller had already added.
    m.surface_begin(TRIANGLES).unwrap();
    assert_eq!(
        m.surface_begin(TRIANGLES),
        Err(ImmediateMeshError::SurfaceAlreadyOpen)
    );

    // An unsupported primitive is refused at begin.
    let mut other = ImmediateMesh::new();
    assert_eq!(
        other.surface_begin(2),
        Err(ImmediateMeshError::UnsupportedPrimitive),
        "LINE_STRIP is a real Godot primitive this renderer cannot draw"
    );
}

/// A surface closed with no vertices is refused rather than committed empty.
#[test]
fn an_empty_surface_cannot_be_committed() {
    let mut m = ImmediateMesh::new();
    m.surface_begin(TRIANGLES).unwrap();
    assert_eq!(m.surface_end(), Err(ImmediateMeshError::EmptySurface));
    assert_eq!(m.surface_count(), 0);
}

/// Several surfaces flatten into one renderable mesh: the first as the primary
/// vertex data, the rest as `Mesh3D::surfaces`.
#[test]
fn multiple_surfaces_flatten_into_one_mesh() {
    let mut m = triangle();
    m.surface_begin(LINES).unwrap();
    m.surface_add_vertex(Vector3::new(0.0, 0.0, 0.0)).unwrap();
    m.surface_add_vertex(Vector3::new(0.0, 0.0, 1.0)).unwrap();
    m.surface_end().unwrap();

    assert_eq!(m.surface_count(), 2);
    let mesh = m.to_mesh().expect("renders");
    assert_eq!(mesh.vertices.len(), 3, "surface 0 is the primary data");
    assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    assert_eq!(
        mesh.surfaces.len(),
        1,
        "surface 1 rides as an extra surface"
    );
    assert_eq!(mesh.surfaces[0].primitive_type, PrimitiveType::Lines);
    assert_eq!(
        mesh.indices.len(),
        3,
        "an ImmediateMesh surface is implicitly indexed in add order"
    );
}
