//! pat-s3rrv.201: `SurfaceTool.commit_to_arrays` and its inverse.
//!
//! `script_mesh::mesh_from_mesh_array` has always read Godot's `ARRAY_MAX`-slot
//! mesh-array form; nothing wrote it. `mesh_to_mesh_array` is that writer, and
//! the pair is tested against each other — a writer and a reader of the same
//! layout that are only tested separately are free to agree on nothing.

use gdcore::math::{Color, Vector3};
use gdscene::script_mesh::{mesh_from_mesh_array, mesh_to_mesh_array};
use gdserver3d::mesh::{Mesh3D, PrimitiveType, SurfaceTool};
use gdvariant::Variant;

fn built_quad() -> SurfaceTool {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    for (i, p) in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
    ]
    .iter()
    .enumerate()
    {
        t.set_uv([i as f32 * 0.5, 1.0]);
        t.set_color(Color::new(i as f32 * 0.25, 0.5, 1.0, 1.0));
        t.add_vertex(*p);
    }
    t
}

/// Geometry survives a pack/unpack cycle.
#[test]
fn a_committed_surface_round_trips_through_the_arrays_form() {
    let t = built_quad();
    let mesh = t.commit();
    let slots = mesh_to_mesh_array(&mesh, t.colors());

    let back = mesh_from_mesh_array(&slots).expect("the arrays form parses back");
    assert_eq!(back.vertices, mesh.vertices, "positions survive");
    assert_eq!(back.normals.len(), mesh.normals.len());
    assert_eq!(back.uvs, mesh.uvs, "UVs survive");
    assert_eq!(back.indices, mesh.indices, "the index buffer survives");
}

/// ⚠️ An absent channel is `Nil`, not an empty packed array.
///
/// Godot distinguishes "this surface has no colours" from "this surface has
/// zero colours", and a reader cannot tell those apart once an empty array is
/// written. Every slot the surface has nothing for must stay Nil.
#[test]
fn absent_channels_stay_nil() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    for p in [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }
    // No skin data was set.
    let mesh = t.commit();
    let slots = mesh_to_mesh_array(&mesh, &[]);

    assert_eq!(slots.len(), 13, "an ARRAY_MAX-slot array");
    assert!(matches!(slots[3], Variant::Nil), "ARRAY_COLOR absent");
    assert!(matches!(slots[10], Variant::Nil), "ARRAY_BONES absent");
    assert!(matches!(slots[11], Variant::Nil), "ARRAY_WEIGHTS absent");
    assert!(
        !matches!(slots[0], Variant::Nil),
        "ARRAY_VERTEX is present, so Nil elsewhere means absent rather than unwritten"
    );
}

/// Colours ride in the arrays form even though `Mesh3D` cannot carry them.
///
/// This is the one channel where the arrays form is strictly richer than the
/// mesh type, so dropping it would silently lose what the script set.
#[test]
fn colours_reach_the_arrays_form() {
    let t = built_quad();
    let slots = mesh_to_mesh_array(&t.commit(), t.colors());

    match &slots[3] {
        Variant::PackedColorArray(c) => {
            assert_eq!(c.len(), 3, "one colour per vertex");
            assert_eq!(c[1], Color::new(0.25, 0.5, 1.0, 1.0));
        }
        other => panic!("ARRAY_COLOR should carry the colours, got {other:?}"),
    }
}

/// Skin data appears only when a script set it.
#[test]
fn skin_slots_appear_only_when_set() {
    let mut t = SurfaceTool::new();
    t.begin(PrimitiveType::Triangles);
    t.set_bones(&[1, 2]);
    t.set_weights(&[0.6, 0.4]);
    for p in [
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ] {
        t.add_vertex(p);
    }
    let slots = mesh_to_mesh_array(&t.commit(), &[]);

    match (&slots[10], &slots[11]) {
        (Variant::PackedInt32Array(b), Variant::PackedFloat32Array(w)) => {
            // Four influences per vertex, flattened, as Godot packs them.
            assert_eq!(b.len(), 12, "3 vertices x 4 bone slots");
            assert_eq!(w.len(), 12);
            assert_eq!(&b[0..4], &[1, 2, 0, 0]);
            assert_eq!(&w[0..4], &[0.6, 0.4, 0.0, 0.0]);
        }
        other => panic!("skin slots should be packed, got {other:?}"),
    }
}

/// An empty tool produces an all-Nil array rather than a malformed one.
#[test]
fn an_empty_surface_produces_an_all_nil_array() {
    let slots = mesh_to_mesh_array(&Mesh3D::new(PrimitiveType::Triangles), &[]);
    assert_eq!(slots.len(), 13);
    assert!(slots.iter().all(|s| matches!(s, Variant::Nil)));
}
