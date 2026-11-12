//! Script-defined `PrimitiveMesh` geometry — `_create_mesh_array` (pat-s3rrv.160).
//!
//! In Godot, `extends PrimitiveMesh` plus `_create_mesh_array()` gives fully
//! procedural geometry usable anywhere a Mesh is. Patina declared none of the
//! Mesh virtuals and dispatched none of them, so a scripted mesh resource
//! loaded fine and rendered nothing.
//!
//! # The array layout is Godot's, not ours
//!
//! `_create_mesh_array` returns one flat `Array` of `ARRAY_MAX` = 13 slots,
//! indexed by `Mesh.ArrayType`: vertices at **0**, normals at **1**, UVs at
//! **4**, indices at **12**. The slots between them are tangents, colours, more
//! UV sets and skinning data. ⚠️ The indices live at 12, NOT at the end of
//! whatever the script happened to fill — a script supplying only vertices and
//! indices still returns a 13-slot array with holes, so reading "the last slot"
//! or counting non-empty entries finds the wrong data. Transcribed from
//! `doc/classes/Mesh.xml`.

use gdserver3d::mesh::{Mesh3D, PrimitiveType};
use gdserver3d::primitive_mesh::ArrayMesh;
use gdvariant::Variant;

/// `Mesh.ArrayType` slots this reads.
// ⚠️ ONE table, `pub(crate)`, transcribed from `doc/classes/Mesh.xml`'s
// `ArrayType` enum — not derived, and not duplicated per consumer.
// `SurfaceTool::commit_to_arrays` writes these slots and
// `mesh_from_mesh_array` reads them; two private copies would be free to
// drift, which is exactly how the two mesh resolvers diverged in
// pat-s3rrv.199.
pub(crate) const ARRAY_VERTEX: usize = 0;
pub(crate) const ARRAY_NORMAL: usize = 1;
#[allow(dead_code)]
pub(crate) const ARRAY_TANGENT: usize = 2;
pub(crate) const ARRAY_COLOR: usize = 3;
pub(crate) const ARRAY_TEX_UV: usize = 4;
#[allow(dead_code)]
pub(crate) const ARRAY_TEX_UV2: usize = 5;
pub(crate) const ARRAY_BONES: usize = 10;
pub(crate) const ARRAY_WEIGHTS: usize = 11;
pub(crate) const ARRAY_INDEX: usize = 12;
/// Slot count of a Godot mesh-array; `ARRAY_MAX` in the enum.
pub(crate) const ARRAY_MAX: usize = 13;

/// Builds a mesh from the array a script's `_create_mesh_array` returned.
///
/// `None` when the array carries no vertices: a mesh with no positions cannot
/// be rasterised, and returning an empty mesh would render nothing while
/// looking like success.
pub fn mesh_from_mesh_array(arrays: &[Variant]) -> Option<Mesh3D> {
    let slot = |i: usize| arrays.get(i);

    let vertices: Vec<gdcore::math::Vector3> = match slot(ARRAY_VERTEX) {
        Some(Variant::PackedVector3Array(v)) => v.clone(),
        _ => return None,
    };
    if vertices.is_empty() {
        return None;
    }

    let normals: Vec<gdcore::math::Vector3> = match slot(ARRAY_NORMAL) {
        Some(Variant::PackedVector3Array(v)) => v.clone(),
        _ => Vec::new(),
    };
    let uvs: Vec<[f32; 2]> = match slot(ARRAY_TEX_UV) {
        Some(Variant::PackedVector2Array(v)) => v.iter().map(|p| [p.x, p.y]).collect(),
        _ => Vec::new(),
    };
    // A script may omit indices entirely, in which case the vertices are taken
    // in order — Godot treats an empty index slot as a non-indexed surface, and
    // synthesising 0..n here keeps every downstream consumer on one path.
    let indices: Vec<u32> = match slot(ARRAY_INDEX) {
        Some(Variant::PackedInt32Array(v)) if !v.is_empty() => {
            v.iter().map(|i| (*i).max(0) as u32).collect()
        }
        _ => (0..vertices.len() as u32).collect(),
    };

    let mut mesh = ArrayMesh::new();
    mesh.add_surface_from_arrays(PrimitiveType::Triangles, vertices, normals, uvs, indices);
    Some(mesh.into_mesh())
}

/// Runs a scripted `PrimitiveMesh`'s `_create_mesh_array` and builds its mesh.
///
/// `None` on every failure path — a missing script, one that does not parse, or
/// one that does not define the virtual. Godot only calls the virtual on
/// resources that override it, so not overriding it is not an error.
pub fn mesh_from_script(script_path: &str) -> Option<Mesh3D> {
    let source = gdcore::file_access::FileAccess::read_file_string(script_path).ok()?;
    let mut interp = gdscript_interop::interpreter::Interpreter::new();
    let class_def = interp.run_class(&source).ok()?;
    let mut instance = interp.instantiate_class(&class_def).ok()?;
    let returned = interp
        .call_instance_method(&mut instance, "_create_mesh_array", &[])
        .ok()?;
    match returned {
        Variant::Array(items) => mesh_from_mesh_array(&items.to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector3;

    /// A 13-slot array with only vertices and indices filled — the shape a real
    /// `_create_mesh_array` returns.
    fn triangle_arrays() -> Vec<Variant> {
        let mut slots = vec![Variant::Nil; 13];
        slots[ARRAY_VERTEX] = Variant::PackedVector3Array(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ]);
        slots[ARRAY_INDEX] = Variant::PackedInt32Array(vec![0, 1, 2]);
        slots
    }

    #[test]
    fn three_vertices_make_one_triangle() {
        let mesh = mesh_from_mesh_array(&triangle_arrays()).expect("a mesh is built");
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.triangle_count(), 1, "exactly one triangle");
    }

    /// ⚠️ The index slot is 12, not "the last filled one". A reader that
    /// scanned for the last non-empty entry would pick up the UVs here and
    /// produce garbage indices from float data.
    #[test]
    fn indices_are_read_from_slot_twelve_not_the_last_filled_slot() {
        let mut slots = triangle_arrays();
        slots[ARRAY_TEX_UV] = Variant::PackedVector2Array(vec![
            gdcore::math::Vector2::new(0.0, 0.0),
            gdcore::math::Vector2::new(1.0, 0.0),
            gdcore::math::Vector2::new(0.0, 1.0),
        ]);
        let mesh = mesh_from_mesh_array(&slots).expect("a mesh is built");
        assert_eq!(
            mesh.indices,
            vec![0, 1, 2],
            "indices came from the wrong slot"
        );
        assert_eq!(mesh.uvs.len(), 3, "the UV slot should still be read");
    }

    /// No index slot means a non-indexed surface, not an empty one.
    #[test]
    fn missing_indices_fall_back_to_vertex_order() {
        let mut slots = triangle_arrays();
        slots[ARRAY_INDEX] = Variant::Nil;
        let mesh = mesh_from_mesh_array(&slots).expect("a mesh is built");
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    /// A mesh with no positions cannot be rasterised; an empty mesh would
    /// render nothing while looking like success.
    #[test]
    fn no_vertices_is_none_rather_than_an_empty_mesh() {
        assert!(mesh_from_mesh_array(&vec![Variant::Nil; 13]).is_none());
        assert!(mesh_from_mesh_array(&[]).is_none());
    }
}

/// Packs a [`Mesh3D`] into Godot's `ARRAY_MAX`-slot mesh-array form.
///
/// The inverse of [`mesh_from_mesh_array`], and deliberately adjacent to it:
/// a writer and reader of the same layout that live apart are free to drift.
///
/// Slots the surface has nothing for stay `Nil`, which is how Godot marks an
/// **absent** channel as opposed to an empty one — a reader cannot tell those
/// apart from an empty packed array.
///
/// ⚠️ `colors` is passed separately because [`Mesh3D`] has no colour channel;
/// `SurfaceTool` holds them and the arrays form can carry them, so dropping
/// them here would lose data the script explicitly set.
pub fn mesh_to_mesh_array(mesh: &Mesh3D, colors: &[gdcore::math::Color]) -> Vec<Variant> {
    let mut slots = vec![Variant::Nil; ARRAY_MAX];

    if !mesh.vertices.is_empty() {
        slots[ARRAY_VERTEX] = Variant::PackedVector3Array(mesh.vertices.clone());
    }
    if !mesh.normals.is_empty() {
        slots[ARRAY_NORMAL] = Variant::PackedVector3Array(mesh.normals.clone());
    }
    if !mesh.uvs.is_empty() {
        slots[ARRAY_TEX_UV] = Variant::PackedVector2Array(
            mesh.uvs
                .iter()
                .map(|uv| gdcore::math::Vector2::new(uv[0], uv[1]))
                .collect(),
        );
    }
    if !colors.is_empty() {
        slots[ARRAY_COLOR] = Variant::PackedColorArray(colors.to_vec());
    }
    // Skin data is present only when a script actually set it; see
    // `SurfaceTool`'s `has_skin`. Bones and weights are flattened four-per-vertex,
    // as Godot packs them.
    if !mesh.bone_indices.is_empty() {
        slots[ARRAY_BONES] = Variant::PackedInt32Array(
            mesh.bone_indices
                .iter()
                .flatten()
                .map(|b| *b as i32)
                .collect(),
        );
        slots[ARRAY_WEIGHTS] =
            Variant::PackedFloat32Array(mesh.bone_weights.iter().flatten().copied().collect());
    }
    if !mesh.indices.is_empty() {
        slots[ARRAY_INDEX] =
            Variant::PackedInt32Array(mesh.indices.iter().map(|i| *i as i32).collect());
    }
    slots
}
