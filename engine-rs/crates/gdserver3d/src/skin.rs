//! Skeletal skinning: a [`Skin`] bind-pose resource plus linear-blend
//! skinning ("LBS") of a [`Mesh3D`].
//!
//! This mirrors Godot's `Skin` resource and the `Skeleton3D` skinning path.
//! A [`Skin`] holds an ordered list of [`SkinBind`]s; each bind maps one
//! skeleton bone to an *inverse bind pose* — the transform that takes a
//! rest-space (mesh-space) vertex into that bone's local space. A skinned
//! [`Mesh3D`] stores up to four influences per vertex in its
//! `bone_weights` / `bone_indices` arrays, where each index selects a bind.
//!
//! Given the skeleton's current global bone poses (e.g. from
//! `gdscene::skeleton3d::get_bone_global_pose`, gathered with
//! `bone_pose_array`), [`skin_mesh`] produces a deformed copy of the mesh:
//!
//! ```text
//! v' = Σ_i  w_i · (global_pose[bind_i.bone] · bind_i.bind_pose) · v
//! ```
//!
//! At the rest pose (`global_pose == bone_global_rest` and
//! `bind_pose == bone_global_rest.inverse()`) each per-bind matrix collapses
//! to the identity, so a rest-pose skin leaves the mesh unchanged — the
//! defining correctness property of the pipeline.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;

use crate::mesh::Mesh3D;

/// One bind entry in a [`Skin`]: a skeleton bone plus its inverse bind pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinBind {
    /// Index of the skeleton bone this bind is driven by. Used to look up the
    /// bone's current global pose in the array passed to [`skin_mesh`].
    pub bone: usize,
    /// Inverse bind pose: maps a rest-space mesh vertex into the bone's local
    /// space. Composed with the bone's live global pose to move the vertex.
    pub bind_pose: Transform3D,
}

impl SkinBind {
    /// Builds a bind for `bone` from its rest-space global transform.
    ///
    /// The stored [`bind_pose`](Self::bind_pose) is the inverse of
    /// `bone_global_rest`, matching Godot's
    /// `Skin.add_bind(bone, global_rest.affine_inverse())` convention.
    pub fn from_rest(bone: usize, bone_global_rest: Transform3D) -> Self {
        Self {
            bone,
            bind_pose: bone_global_rest.inverse(),
        }
    }
}

/// A skinning bind set: an ordered list of [`SkinBind`]s.
///
/// A mesh's `bone_indices` reference entries in [`binds`](Self::binds) (not
/// skeleton bones directly), matching Godot's `Skin` indirection so a single
/// skeleton can drive meshes that only use a subset of its bones.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skin {
    /// Ordered bind entries. A mesh `bone_indices[k]` value of `i` selects
    /// `binds[i]`.
    pub binds: Vec<SkinBind>,
}

impl Skin {
    /// Creates an empty skin with no binds.
    pub fn new() -> Self {
        Self { binds: Vec::new() }
    }

    /// Appends a bind mapping `bone` to `bind_pose` (an inverse bind matrix)
    /// and returns the new bind's index.
    pub fn add_bind(&mut self, bone: usize, bind_pose: Transform3D) -> usize {
        let idx = self.binds.len();
        self.binds.push(SkinBind { bone, bind_pose });
        idx
    }

    /// Appends a bind for `bone` derived from its rest-space global transform
    /// (stores the inverse as the bind pose) and returns the bind's index.
    ///
    /// This is the registration entry point the acceptance criteria calls
    /// "Skin bind-pose registration".
    pub fn add_bind_from_rest(&mut self, bone: usize, bone_global_rest: Transform3D) -> usize {
        let idx = self.binds.len();
        self.binds.push(SkinBind::from_rest(bone, bone_global_rest));
        idx
    }

    /// Returns the number of binds.
    pub fn bind_count(&self) -> usize {
        self.binds.len()
    }
}

/// Computes the per-bind skinning matrix `global_pose[bone] · bind_pose`.
///
/// `bone_global_poses` is indexed by skeleton bone index (the array returned
/// by `gdscene::skeleton3d::bone_pose_array`). A bind referencing a bone
/// outside the array falls back to an identity global pose.
pub fn skin_matrices(skin: &Skin, bone_global_poses: &[Transform3D]) -> Vec<Transform3D> {
    skin.binds
        .iter()
        .map(|b| {
            let pose = bone_global_poses
                .get(b.bone)
                .copied()
                .unwrap_or(Transform3D::IDENTITY);
            pose * b.bind_pose
        })
        .collect()
}

/// Skins a single position by blending up to four influences.
///
/// `mats` are precomputed per-bind matrices from [`skin_matrices`]. Influences
/// with zero weight, or referencing a bind index outside `mats`, are skipped.
/// The blended result is renormalized by the total contributing weight, so
/// weight arrays that do not sum to exactly `1.0` still produce a sensible
/// position. A vertex with no contributing weight is returned unchanged.
pub fn skin_position(
    vertex: Vector3,
    weights: [f32; 4],
    indices: [u32; 4],
    mats: &[Transform3D],
) -> Vector3 {
    let mut acc = Vector3::ZERO;
    let mut total = 0.0f32;
    for k in 0..4 {
        let w = weights[k];
        if w == 0.0 {
            continue;
        }
        if let Some(m) = mats.get(indices[k] as usize) {
            acc = acc + m.xform(vertex) * w;
            total += w;
        }
    }
    if total > 0.0 {
        acc * (1.0 / total)
    } else {
        vertex
    }
}

/// Skins a single normal by blending the rotation/scale (basis) part of up to
/// four influences, then renormalizing. Translation is intentionally ignored
/// for direction vectors. Returns the input normal when unrigged.
pub fn skin_normal(
    normal: Vector3,
    weights: [f32; 4],
    indices: [u32; 4],
    mats: &[Transform3D],
) -> Vector3 {
    let mut acc = Vector3::ZERO;
    let mut total = 0.0f32;
    for k in 0..4 {
        let w = weights[k];
        if w == 0.0 {
            continue;
        }
        if let Some(m) = mats.get(indices[k] as usize) {
            acc = acc + m.basis.xform(normal) * w;
            total += w;
        }
    }
    if total <= 0.0 {
        return normal;
    }
    let blended = acc * (1.0 / total);
    let len = blended.length();
    if len > 1e-8 {
        blended * (1.0 / len)
    } else {
        normal
    }
}

/// Produces a deformed copy of `mesh` via linear-blend skinning using the
/// skeleton's current global bone poses.
///
/// Positions and (when present and parallel to positions) normals are blended
/// over up to four influences per vertex. UVs, indices, primitive type and
/// extra surfaces are copied unchanged. When the mesh is not skinned
/// (`mesh.is_skinned() == false`) an unchanged clone is returned. The returned
/// mesh has its own `bone_weights`/`bone_indices` cleared, since it has already
/// been baked into pose space.
///
/// This is the CPU ("software") skinning path; a GPU path may follow.
pub fn skin_mesh(mesh: &Mesh3D, skin: &Skin, bone_global_poses: &[Transform3D]) -> Mesh3D {
    if !mesh.is_skinned() {
        return mesh.clone();
    }

    let mats = skin_matrices(skin, bone_global_poses);
    let skin_normals = mesh.normals.len() == mesh.vertices.len();

    let mut out = mesh.clone();
    for (vi, &v) in mesh.vertices.iter().enumerate() {
        let w = mesh.bone_weights[vi];
        let idx = mesh.bone_indices[vi];
        out.vertices[vi] = skin_position(v, w, idx, &mats);
        if skin_normals {
            out.normals[vi] = skin_normal(mesh.normals[vi], w, idx, &mats);
        }
    }
    out.bone_weights.clear();
    out.bone_indices.clear();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::PrimitiveType;
    use gdcore::math3d::Basis;

    const EPS: f32 = 1e-5;

    fn approx(a: Vector3, b: Vector3) -> bool {
        (a - b).length() < 1e-4
    }

    /// Builds a 4-vertex quad whose bottom two verts follow bind 0 and top two
    /// verts follow bind 1 (single full-weight influence each).
    fn rigged_quad() -> Mesh3D {
        let mut mesh = Mesh3D::new(PrimitiveType::Triangles);
        mesh.vertices = vec![
            Vector3::new(-0.5, 0.0, 0.0), // 0 bottom-left  -> bind 0
            Vector3::new(0.5, 0.0, 0.0),  // 1 bottom-right -> bind 0
            Vector3::new(0.5, 1.0, 0.0),  // 2 top-right    -> bind 1
            Vector3::new(-0.5, 1.0, 0.0), // 3 top-left     -> bind 1
        ];
        mesh.normals = vec![Vector3::new(0.0, 0.0, 1.0); 4];
        mesh.uvs = vec![[0.0, 0.0]; 4];
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        mesh.bone_weights = vec![
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ];
        mesh.bone_indices = vec![[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]];
        mesh
    }

    fn translate(x: f32, y: f32, z: f32) -> Transform3D {
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(x, y, z),
        }
    }

    #[test]
    fn bind_from_rest_is_inverse() {
        let rest = translate(0.0, 2.0, 0.0);
        let bind = SkinBind::from_rest(3, rest);
        assert_eq!(bind.bone, 3);
        // rest * bind_pose == identity
        let composed = rest * bind.bind_pose;
        assert!(approx(composed.xform(Vector3::new(1.0, 1.0, 1.0)), Vector3::new(1.0, 1.0, 1.0)));
    }

    #[test]
    fn add_bind_registration_returns_indices() {
        let mut skin = Skin::new();
        assert_eq!(skin.bind_count(), 0);
        let a = skin.add_bind(0, Transform3D::IDENTITY);
        let b = skin.add_bind_from_rest(1, translate(0.0, 1.0, 0.0));
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        assert_eq!(skin.bind_count(), 2);
    }

    #[test]
    fn unskinned_mesh_returns_unchanged_clone() {
        let mesh = Mesh3D::cube(1.0);
        let skin = Skin::new();
        let out = skin_mesh(&mesh, &skin, &[]);
        assert_eq!(out, mesh);
    }

    #[test]
    fn rest_pose_leaves_mesh_unchanged() {
        // Binds registered from bone rests, then posed AT rest → identity skin.
        let mesh = rigged_quad();
        let bone0_rest = Transform3D::IDENTITY;
        let bone1_rest = translate(0.0, 1.0, 0.0);
        let mut skin = Skin::new();
        skin.add_bind_from_rest(0, bone0_rest);
        skin.add_bind_from_rest(1, bone1_rest);

        let poses = vec![bone0_rest, bone1_rest];
        let out = skin_mesh(&mesh, &skin, &poses);
        for (a, b) in mesh.vertices.iter().zip(out.vertices.iter()) {
            assert!(approx(*a, *b), "rest-pose skin moved a vertex: {a:?} -> {b:?}");
        }
        // Skinned output is baked, so its bone arrays are cleared.
        assert!(out.bone_weights.is_empty());
        assert!(out.bone_indices.is_empty());
    }

    #[test]
    fn single_bone_pose_deforms_only_its_verts() {
        // Identity binds; bone 1 translates. Only the top verts (bind 1) move.
        let mesh = rigged_quad();
        let mut skin = Skin::new();
        skin.add_bind(0, Transform3D::IDENTITY);
        skin.add_bind(1, Transform3D::IDENTITY);

        let poses = vec![Transform3D::IDENTITY, translate(2.0, 0.0, 0.0)];
        let out = skin_mesh(&mesh, &skin, &poses);

        // Bottom verts unchanged.
        assert!(approx(out.vertices[0], mesh.vertices[0]));
        assert!(approx(out.vertices[1], mesh.vertices[1]));
        // Top verts shifted +2 in x.
        assert!(approx(out.vertices[2], mesh.vertices[2] + Vector3::new(2.0, 0.0, 0.0)));
        assert!(approx(out.vertices[3], mesh.vertices[3] + Vector3::new(2.0, 0.0, 0.0)));
    }

    #[test]
    fn four_influence_blend_averages_bind_transforms() {
        // One vertex equally weighted across four binds that each translate
        // along a different axis; blended result is the mean translation.
        let mut mesh = Mesh3D::new(PrimitiveType::Points);
        mesh.vertices = vec![Vector3::ZERO];
        mesh.normals = vec![Vector3::new(0.0, 0.0, 1.0)];
        mesh.uvs = vec![[0.0, 0.0]];
        mesh.indices = vec![0];
        mesh.bone_weights = vec![[0.25, 0.25, 0.25, 0.25]];
        mesh.bone_indices = vec![[0, 1, 2, 3]];

        let mut skin = Skin::new();
        skin.add_bind(0, translate(4.0, 0.0, 0.0));
        skin.add_bind(1, translate(0.0, 8.0, 0.0));
        skin.add_bind(2, translate(0.0, 0.0, 12.0));
        skin.add_bind(3, translate(-4.0, 0.0, 0.0));

        let poses = vec![Transform3D::IDENTITY; 4];
        let out = skin_mesh(&mesh, &skin, &poses);
        // Mean of (4,0,0),(0,8,0),(0,0,12),(-4,0,0) = (0, 2, 3).
        assert!(approx(out.vertices[0], Vector3::new(0.0, 2.0, 3.0)));
    }

    #[test]
    fn partial_weights_are_renormalized() {
        // Weights summing to 0.5 still land on the bind translation.
        let mut mesh = Mesh3D::new(PrimitiveType::Points);
        mesh.vertices = vec![Vector3::ZERO];
        mesh.indices = vec![0];
        mesh.bone_weights = vec![[0.5, 0.0, 0.0, 0.0]];
        mesh.bone_indices = vec![[0, 0, 0, 0]];

        let mut skin = Skin::new();
        skin.add_bind(0, translate(10.0, 0.0, 0.0));

        let poses = vec![Transform3D::IDENTITY];
        let out = skin_mesh(&mesh, &skin, &poses);
        assert!(approx(out.vertices[0], Vector3::new(10.0, 0.0, 0.0)));
    }

    #[test]
    fn out_of_range_bind_index_is_ignored() {
        // Bind index 5 does not exist → that influence is skipped; the vertex
        // falls back to its input position (no contributing weight).
        let out = skin_position(
            Vector3::new(1.0, 2.0, 3.0),
            [1.0, 0.0, 0.0, 0.0],
            [5, 0, 0, 0],
            &[Transform3D::IDENTITY], // only bind 0 exists
        );
        assert!(approx(out, Vector3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn normals_are_rotated_by_pose() {
        // A 90° rotation about Z maps a +X normal onto the Y axis (±Y depending
        // on the basis handedness) and leaves it a unit vector. We assert the
        // sign-agnostic properties so the test does not depend on euler sign
        // conventions: unit length, zero X and Z, unit |Y|.
        let rot = Transform3D {
            basis: Basis::from_euler(Vector3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2)),
            origin: Vector3::ZERO,
        };
        let n = skin_normal(
            Vector3::new(1.0, 0.0, 0.0),
            [1.0, 0.0, 0.0, 0.0],
            [0, 0, 0, 0],
            &[rot],
        );
        assert!((n.length() - 1.0).abs() < EPS, "normal not unit: {n:?}");
        assert!(n.x.abs() < 1e-4, "expected ~0 x: {n:?}");
        assert!(n.z.abs() < 1e-4, "expected ~0 z: {n:?}");
        assert!((n.y.abs() - 1.0).abs() < 1e-4, "expected |y|~1: {n:?}");
    }
}
