//! Camera3D full API: projection modes, projection matrix, frustum culling.
//!
//! Mirrors Godot's `Camera3D` API surface including:
//! - Three projection modes: perspective, orthogonal, frustum-offset
//! - Projection matrix computation (4×4)
//! - Frustum plane extraction and AABB culling
//! - `project_position`, `project_ray_origin`, `project_ray_normal`,
//!   `unproject_position`, `is_position_in_frustum`
//! - `make_current` / `clear_current`

use gdcore::math::{Vector2, Vector3};
use gdcore::math3d::{Aabb, Plane, Transform3D};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::scene_tree::SceneTree;

// ===========================================================================
// Enums matching Godot constants
// ===========================================================================

/// Camera3D projection modes, matching Godot's `Camera3D.ProjectionType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i64)]
pub enum ProjectionType {
    /// Standard perspective projection.
    #[default]
    Perspective = 0,
    /// Orthographic (parallel) projection.
    Orthogonal = 1,
    /// Frustum-offset projection (used for stereo rendering / portal effects).
    Frustum = 2,
}

impl ProjectionType {
    /// Converts from the integer representation stored in Variant.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::Orthogonal,
            2 => Self::Frustum,
            _ => Self::Perspective,
        }
    }
}

/// Camera3D keep-aspect modes, matching Godot's `Camera3D.KeepAspect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i64)]
pub enum KeepAspect {
    /// Keep width, adjust height.
    KeepWidth = 0,
    /// Keep height, adjust width (Godot default).
    #[default]
    KeepHeight = 1,
}

impl KeepAspect {
    /// Converts from the integer representation stored in Variant.
    pub fn from_i64(v: i64) -> Self {
        match v {
            0 => Self::KeepWidth,
            _ => Self::KeepHeight,
        }
    }
}

/// Camera3D Doppler tracking mode, matching Godot's `Camera3D.DopplerTracking`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i64)]
pub enum DopplerTracking {
    /// No Doppler tracking.
    #[default]
    Disabled = 0,
    /// Track based on idle step.
    IdleStep = 1,
    /// Track based on physics step.
    PhysicsStep = 2,
}

impl DopplerTracking {
    /// Converts from the integer representation stored in Variant.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::IdleStep,
            2 => Self::PhysicsStep,
            _ => Self::Disabled,
        }
    }
}

// ===========================================================================
// 4×4 Projection matrix
// ===========================================================================

/// A 4×4 column-major projection matrix, matching Godot's `Projection`.
///
/// Stored as `m[col][row]` following OpenGL/Godot convention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// Column-major data: `m[column][row]`.
    pub m: [[f32; 4]; 4],
}

impl Projection {
    /// The identity projection.
    pub const IDENTITY: Self = Self {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// Creates a perspective projection matrix.
    ///
    /// - `fov_y`: vertical field of view in **degrees**
    /// - `aspect`: width / height
    /// - `z_near`, `z_far`: clipping plane distances
    ///
    /// Matches Godot's `Projection::create_perspective`.
    pub fn create_perspective(fov_y: f32, aspect: f32, z_near: f32, z_far: f32) -> Self {
        let half_fov = (fov_y * 0.5).to_radians();
        let top = z_near * half_fov.tan();
        let bottom = -top;
        let right = top * aspect;
        let left = -right;
        Self::create_frustum(left, right, bottom, top, z_near, z_far)
    }

    /// Creates a perspective projection with frustum offset.
    ///
    /// - `fov_y`: vertical FOV in **degrees**
    /// - `aspect`: width / height
    /// - `offset`: frustum offset (Vector2)
    /// - `z_near`, `z_far`: clipping plane distances
    ///
    /// Matches Godot's `Projection::create_perspective` with offset overload.
    pub fn create_perspective_with_offset(
        fov_y: f32,
        aspect: f32,
        offset: Vector2,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let half_fov = (fov_y * 0.5).to_radians();
        let top = z_near * half_fov.tan();
        let bottom = -top;
        let right = top * aspect;
        let left = -right;

        let left = left + offset.x;
        let right = right + offset.x;
        let top = top + offset.y;
        let bottom = bottom + offset.y;

        Self::create_frustum(left, right, bottom, top, z_near, z_far)
    }

    /// Creates an orthographic projection matrix.
    ///
    /// - `size`: the vertical extent (half-height × 2)
    /// - `aspect`: width / height
    /// - `z_near`, `z_far`: clipping plane distances
    ///
    /// Matches Godot's `Projection::create_orthogonal`.
    /// Godot `Projection.create_orthogonal_aspect(size, aspect, z_near, z_far,
    /// flip_fov)` — the size/aspect form.
    ///
    /// ⚠️ This was called `create_orthogonal` until pat-s3rrv.59, which is
    /// Godot's name for the SIX-argument off-centre form below. Both names
    /// existed here with their meanings swapped, so anyone reading Godot's docs
    /// and calling `create_orthogonal` on this type got the other projection —
    /// and because `gdcore::math3d::Projection` spells them Godot's way, moving
    /// a call between the two identical-looking types silently changed which
    /// matrix it built.
    pub fn create_orthogonal_aspect(size: f32, aspect: f32, z_near: f32, z_far: f32) -> Self {
        let half_h = size * 0.5;
        let half_w = half_h * aspect;
        Self::create_orthogonal(-half_w, half_w, -half_h, half_h, z_near, z_far)
    }

    /// Creates an orthographic projection from explicit bounds.
    /// Godot `Projection.create_orthogonal(left, right, bottom, top, z_near,
    /// z_far)` — the off-centre form, and the one Godot gives this name to.
    pub fn create_orthogonal(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let mut m = [[0.0f32; 4]; 4];

        m[0][0] = 2.0 / (right - left);
        m[1][1] = 2.0 / (top - bottom);
        m[2][2] = -2.0 / (z_far - z_near);

        m[3][0] = -(right + left) / (right - left);
        m[3][1] = -(top + bottom) / (top - bottom);
        m[3][2] = -(z_far + z_near) / (z_far - z_near);
        m[3][3] = 1.0;

        Self { m }
    }

    /// Creates a frustum projection from explicit bounds.
    ///
    /// Matches Godot's `Projection::create_frustum`.
    pub fn create_frustum(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let mut m = [[0.0f32; 4]; 4];

        let x = 2.0 * z_near / (right - left);
        let y = 2.0 * z_near / (top - bottom);
        let a = (right + left) / (right - left);
        let b = (top + bottom) / (top - bottom);
        let c = -(z_far + z_near) / (z_far - z_near);
        let d = -2.0 * z_far * z_near / (z_far - z_near);

        m[0][0] = x;
        m[1][1] = y;
        m[2][0] = a;
        m[2][1] = b;
        m[2][2] = c;
        m[2][3] = -1.0;
        m[3][2] = d;

        Self { m }
    }

    /// Extracts the six frustum planes from this projection matrix.
    ///
    /// Returns planes in order: [left, right, bottom, top, near, far].
    /// Each plane normal points **inward** (toward the visible volume).
    ///
    /// When combined with a view (camera) transform, call
    /// `get_frustum_planes(projection * view_inverse)`.
    pub fn get_frustum_planes(&self) -> [Plane; 6] {
        let m = &self.m;

        // Extract from the combined matrix (column-major).
        // Row 0: m[0][r], Row 1: m[1][r], Row 2: m[2][r], Row 3: m[3][r]
        // But we store column-major: m[col][row].

        let row = |r: usize| -> [f32; 4] { [m[0][r], m[1][r], m[2][r], m[3][r]] };

        let r0 = row(0);
        let r1 = row(1);
        let r2 = row(2);
        let r3 = row(3);

        let make_plane = |a: f32, b: f32, c: f32, d: f32| -> Plane {
            let len = (a * a + b * b + c * c).sqrt();
            if len < 1e-10 {
                return Plane::new(Vector3::new(0.0, 1.0, 0.0), 0.0);
            }
            let inv = 1.0 / len;
            // Gribb-Hartmann gives planes where ax+by+cz+d >= 0 means "inside".
            // Plane::distance_to(p) = normal·p - self.d, so negate d to match.
            Plane::new(Vector3::new(a * inv, b * inv, c * inv), -d * inv)
        };

        // Left:   row3 + row0
        let left = make_plane(r3[0] + r0[0], r3[1] + r0[1], r3[2] + r0[2], r3[3] + r0[3]);
        // Right:  row3 - row0
        let right = make_plane(r3[0] - r0[0], r3[1] - r0[1], r3[2] - r0[2], r3[3] - r0[3]);
        // Bottom: row3 + row1
        let bottom = make_plane(r3[0] + r1[0], r3[1] + r1[1], r3[2] + r1[2], r3[3] + r1[3]);
        // Top:    row3 - row1
        let top = make_plane(r3[0] - r1[0], r3[1] - r1[1], r3[2] - r1[2], r3[3] - r1[3]);
        // Near:   row3 + row2
        let near = make_plane(r3[0] + r2[0], r3[1] + r2[1], r3[2] + r2[2], r3[3] + r2[3]);
        // Far:    row3 - row2
        let far = make_plane(r3[0] - r2[0], r3[1] - r2[1], r3[2] - r2[2], r3[3] - r2[3]);

        [left, right, bottom, top, near, far]
    }

    /// Multiplies a 4D vector (x, y, z, w) by this projection matrix.
    pub fn xform4(&self, x: f32, y: f32, z: f32, w: f32) -> [f32; 4] {
        [
            self.m[0][0] * x + self.m[1][0] * y + self.m[2][0] * z + self.m[3][0] * w,
            self.m[0][1] * x + self.m[1][1] * y + self.m[2][1] * z + self.m[3][1] * w,
            self.m[0][2] * x + self.m[1][2] * y + self.m[2][2] * z + self.m[3][2] * w,
            self.m[0][3] * x + self.m[1][3] * y + self.m[2][3] * z + self.m[3][3] * w,
        ]
    }

    /// Returns the inverse of this projection matrix.
    pub fn inverse(&self) -> Self {
        // For projection matrices we do a full 4×4 inverse via cofactor expansion.
        let m = &self.m;
        let mut inv = [[0.0f32; 4]; 4];

        // Compute cofactors
        inv[0][0] =
            m[1][1] * m[2][2] * m[3][3] - m[1][1] * m[2][3] * m[3][2] - m[2][1] * m[1][2] * m[3][3]
                + m[2][1] * m[1][3] * m[3][2]
                + m[3][1] * m[1][2] * m[2][3]
                - m[3][1] * m[1][3] * m[2][2];

        inv[1][0] = -m[1][0] * m[2][2] * m[3][3]
            + m[1][0] * m[2][3] * m[3][2]
            + m[2][0] * m[1][2] * m[3][3]
            - m[2][0] * m[1][3] * m[3][2]
            - m[3][0] * m[1][2] * m[2][3]
            + m[3][0] * m[1][3] * m[2][2];

        inv[2][0] =
            m[1][0] * m[2][1] * m[3][3] - m[1][0] * m[2][3] * m[3][1] - m[2][0] * m[1][1] * m[3][3]
                + m[2][0] * m[1][3] * m[3][1]
                + m[3][0] * m[1][1] * m[2][3]
                - m[3][0] * m[1][3] * m[2][1];

        inv[3][0] = -m[1][0] * m[2][1] * m[3][2]
            + m[1][0] * m[2][2] * m[3][1]
            + m[2][0] * m[1][1] * m[3][2]
            - m[2][0] * m[1][2] * m[3][1]
            - m[3][0] * m[1][1] * m[2][2]
            + m[3][0] * m[1][2] * m[2][1];

        let det =
            m[0][0] * inv[0][0] + m[0][1] * inv[1][0] + m[0][2] * inv[2][0] + m[0][3] * inv[3][0];

        if det.abs() < 1e-10 {
            return Self::IDENTITY;
        }

        let inv_det = 1.0 / det;

        inv[0][1] = -m[0][1] * m[2][2] * m[3][3]
            + m[0][1] * m[2][3] * m[3][2]
            + m[2][1] * m[0][2] * m[3][3]
            - m[2][1] * m[0][3] * m[3][2]
            - m[3][1] * m[0][2] * m[2][3]
            + m[3][1] * m[0][3] * m[2][2];

        inv[1][1] =
            m[0][0] * m[2][2] * m[3][3] - m[0][0] * m[2][3] * m[3][2] - m[2][0] * m[0][2] * m[3][3]
                + m[2][0] * m[0][3] * m[3][2]
                + m[3][0] * m[0][2] * m[2][3]
                - m[3][0] * m[0][3] * m[2][2];

        inv[2][1] = -m[0][0] * m[2][1] * m[3][3]
            + m[0][0] * m[2][3] * m[3][1]
            + m[2][0] * m[0][1] * m[3][3]
            - m[2][0] * m[0][3] * m[3][1]
            - m[3][0] * m[0][1] * m[2][3]
            + m[3][0] * m[0][3] * m[2][1];

        inv[3][1] =
            m[0][0] * m[2][1] * m[3][2] - m[0][0] * m[2][2] * m[3][1] - m[2][0] * m[0][1] * m[3][2]
                + m[2][0] * m[0][2] * m[3][1]
                + m[3][0] * m[0][1] * m[2][2]
                - m[3][0] * m[0][2] * m[2][1];

        inv[0][2] =
            m[0][1] * m[1][2] * m[3][3] - m[0][1] * m[1][3] * m[3][2] - m[1][1] * m[0][2] * m[3][3]
                + m[1][1] * m[0][3] * m[3][2]
                + m[3][1] * m[0][2] * m[1][3]
                - m[3][1] * m[0][3] * m[1][2];

        inv[1][2] = -m[0][0] * m[1][2] * m[3][3]
            + m[0][0] * m[1][3] * m[3][2]
            + m[1][0] * m[0][2] * m[3][3]
            - m[1][0] * m[0][3] * m[3][2]
            - m[3][0] * m[0][2] * m[1][3]
            + m[3][0] * m[0][3] * m[1][2];

        inv[2][2] =
            m[0][0] * m[1][1] * m[3][3] - m[0][0] * m[1][3] * m[3][1] - m[1][0] * m[0][1] * m[3][3]
                + m[1][0] * m[0][3] * m[3][1]
                + m[3][0] * m[0][1] * m[1][3]
                - m[3][0] * m[0][3] * m[1][1];

        inv[3][2] = -m[0][0] * m[1][1] * m[3][2]
            + m[0][0] * m[1][2] * m[3][1]
            + m[1][0] * m[0][1] * m[3][2]
            - m[1][0] * m[0][2] * m[3][1]
            - m[3][0] * m[0][1] * m[1][2]
            + m[3][0] * m[0][2] * m[1][1];

        inv[0][3] = -m[0][1] * m[1][2] * m[2][3]
            + m[0][1] * m[1][3] * m[2][2]
            + m[1][1] * m[0][2] * m[2][3]
            - m[1][1] * m[0][3] * m[2][2]
            - m[2][1] * m[0][2] * m[1][3]
            + m[2][1] * m[0][3] * m[1][2];

        inv[1][3] =
            m[0][0] * m[1][2] * m[2][3] - m[0][0] * m[1][3] * m[2][2] - m[1][0] * m[0][2] * m[2][3]
                + m[1][0] * m[0][3] * m[2][2]
                + m[2][0] * m[0][2] * m[1][3]
                - m[2][0] * m[0][3] * m[1][2];

        inv[2][3] = -m[0][0] * m[1][1] * m[2][3]
            + m[0][0] * m[1][3] * m[2][1]
            + m[1][0] * m[0][1] * m[2][3]
            - m[1][0] * m[0][3] * m[2][1]
            - m[2][0] * m[0][1] * m[1][3]
            + m[2][0] * m[0][3] * m[1][1];

        inv[3][3] =
            m[0][0] * m[1][1] * m[2][2] - m[0][0] * m[1][2] * m[2][1] - m[1][0] * m[0][1] * m[2][2]
                + m[1][0] * m[0][2] * m[2][1]
                + m[2][0] * m[0][1] * m[1][2]
                - m[2][0] * m[0][2] * m[1][1];

        for col in &mut inv {
            for val in col.iter_mut() {
                *val *= inv_det;
            }
        }

        Self { m: inv }
    }
}

// ===========================================================================
// Frustum culling
// ===========================================================================

/// A view frustum defined by six planes (left, right, bottom, top, near, far).
///
/// Normals point **inward**. A point or AABB is inside the frustum if it is
/// on the positive side of every plane.
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    /// The six frustum planes: [left, right, bottom, top, near, far].
    pub planes: [Plane; 6],
}

impl Frustum {
    /// Extracts a frustum from the combined projection × view matrix.
    pub fn from_projection(proj: &Projection) -> Self {
        Self {
            planes: proj.get_frustum_planes(),
        }
    }

    /// Tests whether a point is inside the frustum.
    pub fn contains_point(&self, point: Vector3) -> bool {
        for plane in &self.planes {
            if plane.distance_to(point) < 0.0 {
                return false;
            }
        }
        true
    }

    /// Tests whether an AABB intersects or is inside the frustum.
    ///
    /// Uses the "p-vertex / n-vertex" test for efficiency: for each plane,
    /// find the AABB vertex most in the direction of the plane normal
    /// (p-vertex). If the p-vertex is on the negative side, the entire
    /// AABB is outside.
    pub fn intersects_aabb(&self, aabb: Aabb) -> bool {
        let min = aabb.position;
        let max = aabb.position + aabb.size;

        for plane in &self.planes {
            // p-vertex: for each axis, pick max if normal component is positive,
            // min otherwise.
            let px = if plane.normal.x >= 0.0 { max.x } else { min.x };
            let py = if plane.normal.y >= 0.0 { max.y } else { min.y };
            let pz = if plane.normal.z >= 0.0 { max.z } else { min.z };

            let p_vertex = Vector3::new(px, py, pz);
            if plane.distance_to(p_vertex) < 0.0 {
                return false;
            }
        }
        true
    }
}

// ===========================================================================
// Camera3D property accessors (scene tree)
// ===========================================================================

/// Reads the projection type from a Camera3D node's properties.
pub fn get_projection_type(tree: &SceneTree, node_id: NodeId) -> ProjectionType {
    tree.get_node(node_id)
        .map(|n| match n.get_property("projection") {
            Variant::Int(i) => ProjectionType::from_i64(i),
            Variant::String(ref s) => match s.as_str() {
                "orthographic" | "orthogonal" => ProjectionType::Orthogonal,
                "frustum" => ProjectionType::Frustum,
                _ => ProjectionType::Perspective,
            },
            _ => ProjectionType::Perspective,
        })
        .unwrap_or(ProjectionType::Perspective)
}

/// Sets the projection type on a Camera3D node.
pub fn set_projection_type(tree: &mut SceneTree, node_id: NodeId, mode: ProjectionType) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("projection", Variant::Int(mode as i64));
    }
}

/// Reads the orthographic `"size"` property, defaulting to `1.0`.
pub fn get_size(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size") {
            Variant::Float(f) => f,
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

/// Sets the orthographic `"size"` property.
pub fn set_size(tree: &mut SceneTree, node_id: NodeId, size: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("size", Variant::Float(size));
    }
}

/// Reads the `"frustum_offset"` property, defaulting to `Vector2::ZERO`.
pub fn get_frustum_offset(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("frustum_offset") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

/// Sets the `"frustum_offset"` property.
pub fn set_frustum_offset(tree: &mut SceneTree, node_id: NodeId, offset: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("frustum_offset", Variant::Vector2(offset));
    }
}

/// Reads the `"keep_aspect"` property.
pub fn get_keep_aspect(tree: &SceneTree, node_id: NodeId) -> KeepAspect {
    tree.get_node(node_id)
        .map(|n| match n.get_property("keep_aspect") {
            Variant::Int(i) => KeepAspect::from_i64(i),
            _ => KeepAspect::KeepHeight,
        })
        .unwrap_or(KeepAspect::KeepHeight)
}

/// Sets the `"keep_aspect"` property.
pub fn set_keep_aspect(tree: &mut SceneTree, node_id: NodeId, mode: KeepAspect) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("keep_aspect", Variant::Int(mode as i64));
    }
}

/// Reads the `"doppler_tracking"` property.
pub fn get_doppler_tracking(tree: &SceneTree, node_id: NodeId) -> DopplerTracking {
    tree.get_node(node_id)
        .map(|n| match n.get_property("doppler_tracking") {
            Variant::Int(i) => DopplerTracking::from_i64(i),
            _ => DopplerTracking::Disabled,
        })
        .unwrap_or(DopplerTracking::Disabled)
}

/// Sets the `"doppler_tracking"` property.
pub fn set_doppler_tracking(tree: &mut SceneTree, node_id: NodeId, mode: DopplerTracking) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("doppler_tracking", Variant::Int(mode as i64));
    }
}

/// Reads the `"h_offset"` property, defaulting to `0.0`.
pub fn get_h_offset(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("h_offset") {
            Variant::Float(f) => f,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the `"h_offset"` property.
pub fn set_h_offset(tree: &mut SceneTree, node_id: NodeId, offset: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("h_offset", Variant::Float(offset));
    }
}

/// Reads the `"v_offset"` property, defaulting to `0.0`.
pub fn get_v_offset(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("v_offset") {
            Variant::Float(f) => f,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the `"v_offset"` property.
pub fn set_v_offset(tree: &mut SceneTree, node_id: NodeId, offset: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("v_offset", Variant::Float(offset));
    }
}

/// Reads the `"cull_mask"` property (20-bit bitmask), defaulting to `0xFFFFF`.
pub fn get_cull_mask(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("cull_mask") {
            Variant::Int(i) => i,
            _ => 0xFFFFF,
        })
        .unwrap_or(0xFFFFF)
}

/// Sets the `"cull_mask"` property (20-bit bitmask for visual layer culling).
pub fn set_cull_mask(tree: &mut SceneTree, node_id: NodeId, mask: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("cull_mask", Variant::Int(mask));
    }
}

/// Returns `true` if the given layer bit (1-based) is set in the cull mask.
pub fn get_cull_mask_value(tree: &SceneTree, node_id: NodeId, layer: u32) -> bool {
    if layer == 0 || layer > 20 {
        return false;
    }
    let mask = get_cull_mask(tree, node_id);
    (mask >> (layer - 1)) & 1 != 0
}

/// Sets or clears a specific layer bit (1-based) in the cull mask.
pub fn set_cull_mask_value(tree: &mut SceneTree, node_id: NodeId, layer: u32, value: bool) {
    if layer == 0 || layer > 20 {
        return;
    }
    let mut mask = get_cull_mask(tree, node_id);
    if value {
        mask |= 1 << (layer - 1);
    } else {
        mask &= !(1 << (layer - 1));
    }
    set_cull_mask(tree, node_id, mask);
}

// ===========================================================================
// make_current / clear_current
// ===========================================================================

/// Makes this camera the current active camera by setting `"current"` to true
/// on this node and false on all other Camera3D nodes in the scene.
pub fn make_current(tree: &mut SceneTree, node_id: NodeId) {
    let all = tree.all_nodes_in_tree_order();
    for &nid in &all {
        if let Some(node) = tree.get_node(nid) {
            if node.class_name() == "Camera3D" && nid != node_id {
                // Clear current on other cameras.
                if let Some(n) = tree.get_node_mut(nid) {
                    n.set_property("current", Variant::Bool(false));
                }
            }
        }
    }
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("current", Variant::Bool(true));
    }
}

/// Clears the `"current"` flag on this camera.
pub fn clear_current(tree: &mut SceneTree, node_id: NodeId) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("current", Variant::Bool(false));
    }
}

/// Returns `true` if this camera node is marked as current.
pub fn is_current(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| matches!(n.get_property("current"), Variant::Bool(true)))
        .unwrap_or(false)
}

/// The `Camera3D` a running scene renders from, or `None` if the scene has no
/// camera at all (pat-0q2kx.30).
///
/// Godot's viewport tracks an active camera: a `Camera3D` claims it by being
/// `current` (`make_current`, camera_3d.cpp), the last claimant in tree order
/// wins, and a scene whose cameras never call `make_current` still renders from
/// the first camera that entered the tree — which is why the fallback matters:
/// most `.tscn` files carry exactly one `Camera3D` with no explicit `current`.
pub fn current_camera3d(tree: &SceneTree) -> Option<NodeId> {
    let mut first = None;
    let mut claimed = None;
    for id in tree.all_nodes_in_tree_order() {
        let is_camera = tree
            .get_node(id)
            .map(|n| n.class_name() == "Camera3D")
            .unwrap_or(false);
        if !is_camera {
            continue;
        }
        if first.is_none() {
            first = Some(id);
        }
        if is_current(tree, id) {
            claimed = Some(id);
        }
    }
    claimed.or(first)
}

// ===========================================================================
// Projection matrix from scene tree state
// ===========================================================================

/// Converts the authored `fov` into the vertical FOV the projection needs,
/// honouring `keep_aspect` (pat-s3rrv.186).
///
/// 🔑 Godot's `fov` is VERTICAL under `KEEP_HEIGHT` (the default) and
/// HORIZONTAL under `KEEP_WIDTH`. `camera_3d.cpp:274` passes
/// `keep_aspect == KEEP_WIDTH` as `set_perspective`'s `flip_fov`, and
/// `projection.cpp:253-255` then re-derives the vertical angle with
/// `get_fovy(fov, 1.0 / aspect)`.
///
/// Ignoring this does not merely tweak the framing: on a wide viewport a
/// KEEP_WIDTH camera authored at 75° would render as if 75° were vertical,
/// which is a much narrower view than Godot shows.
fn fovy_for_keep_aspect(tree: &SceneTree, node_id: NodeId, fov: f32, aspect: f32) -> f32 {
    match get_keep_aspect(tree, node_id) {
        KeepAspect::KeepHeight => fov,
        // Godot: get_fovy(p_fovx, p_aspect) =
        //   rad_to_deg(atan(p_aspect * tan(deg_to_rad(p_fovx) * 0.5)) * 2.0)
        // called with p_aspect = 1.0 / aspect.
        KeepAspect::KeepWidth => {
            let inv_aspect = 1.0 / aspect.max(f32::EPSILON);
            (inv_aspect * (fov.to_radians() * 0.5).tan())
                .atan()
                .to_degrees()
                * 2.0
        }
    }
}

/// Builds the projection matrix for a Camera3D node given viewport dimensions.
///
/// Reads the camera's projection type, FOV, size, near, far, and frustum
/// offset from the scene tree and returns the corresponding matrix.
pub fn get_camera_projection(
    tree: &SceneTree,
    node_id: NodeId,
    viewport_width: u32,
    viewport_height: u32,
) -> Projection {
    let proj_type = get_projection_type(tree, node_id);
    let near = node3d::get_near(tree, node_id) as f32;
    let far = node3d::get_far(tree, node_id) as f32;
    let aspect = viewport_width as f32 / viewport_height.max(1) as f32;

    match proj_type {
        ProjectionType::Perspective => {
            let fov = node3d::get_fov(tree, node_id) as f32;
            Projection::create_perspective(
                fovy_for_keep_aspect(tree, node_id, fov, aspect),
                aspect,
                near,
                far,
            )
        }
        ProjectionType::Orthogonal => {
            let size = get_size(tree, node_id) as f32;
            Projection::create_orthogonal_aspect(size, aspect, near, far)
        }
        ProjectionType::Frustum => {
            let fov = node3d::get_fov(tree, node_id) as f32;
            let offset = get_frustum_offset(tree, node_id);
            Projection::create_perspective_with_offset(
                fovy_for_keep_aspect(tree, node_id, fov, aspect),
                aspect,
                offset,
                near,
                far,
            )
        }
    }
}

/// Builds a [`Frustum`] for a Camera3D node in world space.
///
/// Combines the projection matrix with the camera's inverse global transform
/// so that the resulting frustum planes are in world space, suitable for
/// culling world-space AABBs.
pub fn get_camera_frustum(
    tree: &SceneTree,
    node_id: NodeId,
    viewport_width: u32,
    viewport_height: u32,
) -> Frustum {
    let proj = get_camera_projection(tree, node_id, viewport_width, viewport_height);
    let cam_transform = node3d::get_global_transform(tree, node_id);
    let view = cam_transform.inverse();

    // Combine projection × view to get clip-space transform, then extract planes.
    let combined = multiply_projection_transform(&proj, &view);
    Frustum::from_projection(&combined)
}

/// Tests whether a world-space point is inside the camera's frustum.
pub fn is_position_in_frustum(
    tree: &SceneTree,
    node_id: NodeId,
    point: Vector3,
    viewport_width: u32,
    viewport_height: u32,
) -> bool {
    let frustum = get_camera_frustum(tree, node_id, viewport_width, viewport_height);
    frustum.contains_point(point)
}

// ===========================================================================
// project / unproject
// ===========================================================================

/// Projects a 3D world position to 2D screen coordinates.
///
/// Returns `(x, y)` in pixel coordinates where (0,0) is top-left.
/// Matches Godot's `Camera3D.unproject_position`.
pub fn unproject_position(
    tree: &SceneTree,
    node_id: NodeId,
    world_pos: Vector3,
    viewport_width: u32,
    viewport_height: u32,
) -> Vector2 {
    let proj = get_camera_projection(tree, node_id, viewport_width, viewport_height);
    let cam_transform = node3d::get_global_transform(tree, node_id);
    let view = cam_transform.inverse();

    // Transform to view space.
    let view_pos = view.xform(world_pos);

    // Project through projection matrix.
    let clip = proj.xform4(view_pos.x, view_pos.y, view_pos.z, 1.0);

    if clip[3].abs() < 1e-10 {
        return Vector2::new(viewport_width as f32 * 0.5, viewport_height as f32 * 0.5);
    }

    let ndc_x = clip[0] / clip[3];
    let ndc_y = clip[1] / clip[3];

    // NDC to screen coordinates.
    let screen_x = (ndc_x * 0.5 + 0.5) * viewport_width as f32;
    let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * viewport_height as f32;

    Vector2::new(screen_x, screen_y)
}

/// Projects a 2D screen point to a 3D world position at the given depth.
///
/// `screen_point` is in pixel coordinates, `depth` is the distance from camera.
/// Matches Godot's `Camera3D.project_position`.
pub fn project_position(
    tree: &SceneTree,
    node_id: NodeId,
    screen_point: Vector2,
    depth: f32,
    viewport_width: u32,
    viewport_height: u32,
) -> Vector3 {
    let proj = get_camera_projection(tree, node_id, viewport_width, viewport_height);
    let cam_transform = node3d::get_global_transform(tree, node_id);
    let inv_proj = proj.inverse();

    // Screen to NDC.
    let ndc_x = (screen_point.x / viewport_width as f32) * 2.0 - 1.0;
    let ndc_y = 1.0 - (screen_point.y / viewport_height as f32) * 2.0;

    // Unproject to view space at z = -1 (near plane direction).
    let view_point = inv_proj.xform4(ndc_x, ndc_y, -1.0, 1.0);
    let w = view_point[3];
    if w.abs() < 1e-10 {
        return cam_transform.origin;
    }

    let dir = Vector3::new(view_point[0] / w, view_point[1] / w, view_point[2] / w).normalized();

    // Scale by depth and transform to world space.
    let view_pos = dir * depth;
    cam_transform.xform(view_pos)
}

/// Returns the world-space ray origin for a screen point.
///
/// For perspective cameras, this is the camera position.
/// For orthographic cameras, the origin varies per pixel.
/// Matches Godot's `Camera3D.project_ray_origin`.
pub fn project_ray_origin(
    tree: &SceneTree,
    node_id: NodeId,
    screen_point: Vector2,
    viewport_width: u32,
    viewport_height: u32,
) -> Vector3 {
    let proj_type = get_projection_type(tree, node_id);
    let cam_transform = node3d::get_global_transform(tree, node_id);

    match proj_type {
        ProjectionType::Perspective | ProjectionType::Frustum => cam_transform.origin,
        ProjectionType::Orthogonal => {
            // For ortho cameras, the ray origin is on the near plane.
            let near = node3d::get_near(tree, node_id) as f32;
            project_position(
                tree,
                node_id,
                screen_point,
                near,
                viewport_width,
                viewport_height,
            )
        }
    }
}

/// Returns the world-space ray direction for a screen point.
///
/// The returned vector is normalized.
/// Matches Godot's `Camera3D.project_ray_normal`.
pub fn project_ray_normal(
    tree: &SceneTree,
    node_id: NodeId,
    screen_point: Vector2,
    viewport_width: u32,
    viewport_height: u32,
) -> Vector3 {
    let proj_type = get_projection_type(tree, node_id);
    let cam_transform = node3d::get_global_transform(tree, node_id);

    match proj_type {
        ProjectionType::Perspective | ProjectionType::Frustum => {
            // Ray from camera through the screen point.
            let far = node3d::get_far(tree, node_id) as f32;
            let world_point = project_position(
                tree,
                node_id,
                screen_point,
                far,
                viewport_width,
                viewport_height,
            );
            (world_point - cam_transform.origin).normalized()
        }
        ProjectionType::Orthogonal => {
            // For ortho cameras, all rays are parallel — just the camera's -Z.
            let forward = cam_transform.basis.z * -1.0;
            forward.normalized()
        }
    }
}

// ===========================================================================
// Internal helpers
// ===========================================================================

/// Multiplies a Projection (4×4) by a Transform3D (treated as a 4×4 affine matrix).
///
/// Result = P × T (where T is the view matrix, i.e., camera inverse transform).
#[allow(clippy::needless_range_loop)] // textbook row/col/k matrix product
fn multiply_projection_transform(proj: &Projection, t: &Transform3D) -> Projection {
    // Expand Transform3D to 4×4 column-major.
    let t_m: [[f32; 4]; 4] = [
        [t.basis.x.x, t.basis.x.y, t.basis.x.z, 0.0],
        [t.basis.y.x, t.basis.y.y, t.basis.y.z, 0.0],
        [t.basis.z.x, t.basis.z.y, t.basis.z.z, 0.0],
        [t.origin.x, t.origin.y, t.origin.z, 1.0],
    ];

    let mut result = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0f32;
            for k in 0..4 {
                sum += proj.m[k][row] * t_m[col][k];
            }
            result[col][row] = sum;
        }
    }

    Projection { m: result }
}

// ===========================================================================
// Tests
// ===========================================================================

// ===========================================================================
// Godot's one-call mode constructors, and the behind test
// ===========================================================================

/// Switches the camera to perspective and sets FOV and depth range at once.
///
/// Godot's `Camera3D.set_perspective(fov, z_near, z_far)`. The MODE change is
/// the part a script cannot express by assigning properties: setting `fov` on
/// an orthogonal camera changes a number the orthogonal projection never reads.
pub fn set_perspective(tree: &mut SceneTree, node_id: NodeId, fov: f64, z_near: f64, z_far: f64) {
    set_projection_type(tree, node_id, ProjectionType::Perspective);
    node3d::set_fov(tree, node_id, fov);
    node3d::set_near(tree, node_id, z_near);
    node3d::set_far(tree, node_id, z_far);
}

/// Switches the camera to orthogonal and sets its vertical size and depth range.
///
/// Godot's `Camera3D.set_orthogonal(size, z_near, z_far)`. `size` is the
/// vertical extent in world units; the horizontal one is derived from the
/// viewport aspect by [`get_camera_projection`].
pub fn set_orthogonal(tree: &mut SceneTree, node_id: NodeId, size: f64, z_near: f64, z_far: f64) {
    set_projection_type(tree, node_id, ProjectionType::Orthogonal);
    set_size(tree, node_id, size);
    node3d::set_near(tree, node_id, z_near);
    node3d::set_far(tree, node_id, z_far);
}

/// Switches the camera to a frustum-offset projection.
///
/// Godot's `Camera3D.set_frustum(size, offset, z_near, z_far)`. ⚠️ The offset
/// is the SECOND argument, between the two scalars — not trailing, which is
/// where a reader scanning the other two constructors expects it.
pub fn set_frustum(
    tree: &mut SceneTree,
    node_id: NodeId,
    size: f64,
    offset: Vector2,
    z_near: f64,
    z_far: f64,
) {
    set_projection_type(tree, node_id, ProjectionType::Frustum);
    set_size(tree, node_id, size);
    set_frustum_offset(tree, node_id, offset);
    node3d::set_near(tree, node_id, z_near);
    node3d::set_far(tree, node_id, z_far);
}

/// Whether a world-space point is behind the camera's near plane.
///
/// Godot's `Camera3D.is_position_behind` (`camera_3d.cpp`):
/// `(-basis.column(2)).normalized().dot(point - origin) < near`.
///
/// 🔑 The comparison is against NEAR, not against zero. A point in FRONT of the
/// camera but nearer than the near plane also reads `true`, which is the whole
/// use of the call: it answers "would this be clipped away", not "is it behind
/// me". An implementation testing `< 0.0` agrees with this one everywhere
/// except the thin slab where the answer matters.
pub fn is_position_behind(tree: &SceneTree, node_id: NodeId, point: Vector3) -> bool {
    let cam = node3d::get_global_transform(tree, node_id);
    // `basis.z` is the Z COLUMN, and the camera looks down its negation — the
    // same idiom `project_ray_normal` uses for the ortho forward vector.
    let eyedir = (cam.basis.z * -1.0).normalized();
    let near = node3d::get_near(tree, node_id) as f32;
    eyedir.dot(point - cam.origin) < near
}

/// Camera3D's script-callable method surface.
///
/// Returns `None` for every non-Camera3D receiver and for every name it does
/// not serve, so `call_node_method`'s table stays reachable unchanged.
///
/// ⚠️ Before this existed, all 19 methods Camera3D registers in ClassDB had no
/// dispatch arm anywhere. `node_has_method` answers from the registration table
/// alone (`scene_tree.rs` -> `class_db::class_has_method`), so the interpreter
/// took its "the method exists" branch and returned `Variant::Nil` — a falsy
/// value, silently — for every one of them. `camera.make_current()` did nothing
/// and reported nothing, and a script could not even detect it, because the
/// thing it would ask (`has_method`) was the thing that was wrong (pat-nzgdu).
///
/// 🔑 The projection family is served here too, since pat-mgixk.35. It used to
/// be unwired for a reason worth keeping on the record: `get_camera_projection`,
/// `is_position_in_frustum`, `project_position`, `unproject_position`,
/// `project_ray_origin` and `project_ray_normal` all take explicit
/// `viewport_width`/`viewport_height`, which Godot's script API does not pass.
/// The size lived only on [`crate::main_loop`]'s `viewport_size()`, so wiring
/// them from here would have meant inventing a default and returning
/// confidently wrong screen coordinates. The size was in fact already
/// reachable — `tree.root_window().visible_size()`, which `visibility_3d.rs`
/// uses for exactly this — so the arms read that.
///
/// ⚠️ STILL UNWIRED, deliberately, and each for its own reason — all four are
/// registered in ClassDB, so each is a silent `Nil` today:
/// - `get_frustum` returns `Array[Plane]` in Godot, whose plane ORDER is
///   `[near, far, left, top, right, bottom]` (`Projection::get_projection_planes`).
///   [`Frustum`] here stores `[left, right, bottom, top, near, far]`. Exposing
///   it without settling that order would hand scripts a correctly-shaped array
///   with the planes in the wrong slots — worse than Nil, because it looks right.
/// - `project_local_ray_normal` is the camera-LOCAL counterpart of
///   `project_ray_normal`; there is no local-space variant to call yet.
/// - `get_camera_transform` and `get_camera_rid` are not projection.
pub fn call_camera3d_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if !matches!(
        method,
        "make_current"
            | "clear_current"
            | "get_cull_mask_value"
            | "set_cull_mask_value"
            | "get_camera_projection"
            | "is_position_behind"
            | "is_position_in_frustum"
            | "project_position"
            | "project_ray_normal"
            | "project_ray_origin"
            | "unproject_position"
            | "set_frustum"
            | "set_orthogonal"
            | "set_perspective"
    ) {
        return None;
    }
    if tree.get_node(node_id)?.class_name() != "Camera3D" {
        return None;
    }

    // Godot's `layer_number` is 1-based and valid in 1..=20.
    // `get_cull_mask_value`/`set_cull_mask_value` already reject 0 and >20, so
    // mapping anything non-positive to 0 reads back `false` instead of
    // panicking on a negative-to-u32 cast.
    let layer = |v: Option<&Variant>| -> u32 {
        match v {
            Some(Variant::Int(i)) if *i > 0 => *i as u32,
            Some(Variant::Float(f)) if *f > 0.0 => *f as u32,
            _ => 0,
        }
    };

    // ⚠️ A GDScript number literal without a decimal point is an `Int`, so
    // `set_orthogonal(10, 0.1, 100)` arrives as Int, Float, Int. Matching only
    // `Variant::Float` would read 0.0 for the size and the far plane and build
    // a degenerate projection that still "worked" — so both numeric variants
    // are accepted. Kept in f64 (`Variant::Float`'s own width) and cast to f32
    // only where the geometry needs it.
    let num = |v: Option<&Variant>, default: f64| -> f64 {
        match v {
            Some(Variant::Float(f)) => *f,
            Some(Variant::Int(i)) => *i as f64,
            _ => default,
        }
    };
    let vec2 = |v: Option<&Variant>| -> Option<Vector2> {
        match v {
            Some(Variant::Vector2(v)) => Some(*v),
            _ => None,
        }
    };
    let vec3 = |v: Option<&Variant>| -> Option<Vector3> {
        match v {
            Some(Variant::Vector3(v)) => Some(*v),
            _ => None,
        }
    };

    // 🔑 The viewport size Godot's script API never passes, read from the
    // tree rather than invented (pat-mgixk.35). This is the SAME source
    // `visibility_3d.rs` already builds a camera frustum from, which is what
    // makes the two agree about what is on screen.
    //
    // ⚠️ `visible_size()`, NOT the raw window size. Once content scaling is on
    // they differ: a 640x360 base resolution in a 1920x1080 window reports
    // 640x360, and that is the rect Godot's own Camera3D projects against
    // (`get_viewport()->get_visible_rect().size`). Taking the window size
    // instead would hand every projection the wrong aspect ratio on exactly
    // the projects that configure stretching.
    let viewport = tree.root_window().visible_size();
    let (vw, vh) = (viewport.x.max(1.0) as u32, viewport.y.max(1.0) as u32);

    match method {
        // Not the same as `current = true`: Godot also clears `current` on every
        // other Camera3D, which is what this helper does by walking the tree.
        "make_current" => {
            make_current(tree, node_id);
            Some(Variant::Nil)
        }
        "clear_current" => {
            clear_current(tree, node_id);
            Some(Variant::Nil)
        }
        "get_cull_mask_value" => Some(Variant::Bool(get_cull_mask_value(
            tree,
            node_id,
            layer(args.first()),
        ))),
        "set_cull_mask_value" => {
            let value = matches!(args.get(1), Some(Variant::Bool(true)));
            set_cull_mask_value(tree, node_id, layer(args.first()), value);
            Some(Variant::Nil)
        }

        // -- projection / unprojection ---------------------------------------
        //
        // ⚠️ A wrong-typed argument returns `Some(Variant::Nil)`, NOT `None`.
        // `None` means "not a native method", which would make the interpreter
        // raise `UndefinedFunction` and blame the method rather than the
        // argument. Nil is not silent either: it propagates to the use site and
        // fails loudly there, whereas substituting `Vector3::ZERO` would return
        // a plausible screen position for a point the script never named.
        "unproject_position" => Some(match vec3(args.first()) {
            Some(point) => Variant::Vector2(unproject_position(tree, node_id, point, vw, vh)),
            None => Variant::Nil,
        }),
        "project_position" => Some(match vec2(args.first()) {
            Some(screen) => Variant::Vector3(project_position(
                tree,
                node_id,
                screen,
                num(args.get(1), 0.0) as f32,
                vw,
                vh,
            )),
            None => Variant::Nil,
        }),
        "project_ray_origin" => Some(match vec2(args.first()) {
            Some(screen) => Variant::Vector3(project_ray_origin(tree, node_id, screen, vw, vh)),
            None => Variant::Nil,
        }),
        "project_ray_normal" => Some(match vec2(args.first()) {
            Some(screen) => Variant::Vector3(project_ray_normal(tree, node_id, screen, vw, vh)),
            None => Variant::Nil,
        }),
        "is_position_in_frustum" => Some(match vec3(args.first()) {
            Some(point) => Variant::Bool(is_position_in_frustum(tree, node_id, point, vw, vh)),
            None => Variant::Nil,
        }),
        // Needs no viewport — a half-space test against the near plane.
        "is_position_behind" => Some(match vec3(args.first()) {
            Some(point) => Variant::Bool(is_position_behind(tree, node_id, point)),
            None => Variant::Nil,
        }),
        "get_camera_projection" => {
            let proj = get_camera_projection(tree, node_id, vw, vh);
            // ⚠️ TWO types named `Projection` are in play: the one this module
            // defines (above) and `gdcore::math3d::Projection`, which is what
            // `Variant::Projection` carries. They are field-for-field identical
            // (`pub m: [[f32; 4]; 4]`, same column-major convention), so this
            // is a re-wrap and not a conversion — but nothing in the NAME warns
            // you, and passing `proj` straight in does not compile.
            Some(Variant::Projection(gdcore::math3d::Projection {
                m: proj.m,
            }))
        }

        // -- Godot's one-call mode constructors ------------------------------
        //
        // Defaults match `doc/classes/Camera3D.xml` (fov 75, near 0.05,
        // far 4000) so a short call does what Godot's short call does.
        "set_perspective" => {
            set_perspective(
                tree,
                node_id,
                num(args.first(), 75.0),
                num(args.get(1), 0.05),
                num(args.get(2), 4000.0),
            );
            Some(Variant::Nil)
        }
        "set_orthogonal" => {
            set_orthogonal(
                tree,
                node_id,
                num(args.first(), 1.0),
                num(args.get(1), 0.05),
                num(args.get(2), 4000.0),
            );
            Some(Variant::Nil)
        }
        "set_frustum" => {
            set_frustum(
                tree,
                node_id,
                num(args.first(), 1.0),
                vec2(args.get(1)).unwrap_or(Vector2::ZERO),
                num(args.get(2), 0.05),
                num(args.get(3), 4000.0),
            );
            Some(Variant::Nil)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    /// 🔑 The two orthogonal constructors are Godot's, spelled Godot's way.
    ///
    /// ⚠️ Until pat-s3rrv.59 these names were SWAPPED relative to Godot: the
    /// size/aspect form was called `create_orthogonal` (Godot's name for the
    /// six-argument off-centre form) and the off-centre form was called
    /// `create_orthogonal_off_center` (a name Godot does not use). Because
    /// `gdcore::math3d::Projection` is a field-for-field identical type that
    /// spells them correctly, a call moved between the two silently built a
    /// different matrix. This asserts the aspect form is exactly the off-centre
    /// form over the derived half-extents, the relationship Godot documents.
    #[test]
    fn orthogonal_aspect_is_the_off_centre_form_over_half_extents() {
        let (size, aspect, near, far) = (10.0f32, 2.0f32, 0.1f32, 100.0f32);
        let half_h = size * 0.5;
        let half_w = half_h * aspect;

        let by_aspect = Projection::create_orthogonal_aspect(size, aspect, near, far);
        let by_extents = Projection::create_orthogonal(-half_w, half_w, -half_h, half_h, near, far);
        assert_eq!(
            by_aspect, by_extents,
            "create_orthogonal_aspect(size, aspect, ..) must equal \
             create_orthogonal(-w, w, -h, h, ..) — Godot's own relationship",
        );

        // ⚠️ A non-square aspect is what makes this test able to fail. At
        // aspect 1.0 the half-width equals the half-height, so an
        // implementation that dropped `aspect` entirely would still match.
        assert!(
            (by_aspect.m[0][0] - 1.0 / half_w).abs() < 1e-6,
            "the x scale must come from the ASPECT-derived half-width, got {}",
            by_aspect.m[0][0],
        );
        assert!(
            (by_aspect.m[1][1] - 1.0 / half_h).abs() < 1e-6,
            "…and the y scale from the half-height, got {}",
            by_aspect.m[1][1],
        );
    }

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn approx_vec3(a: Vector3, b: Vector3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    fn make_tree() -> SceneTree {
        SceneTree::new()
    }

    fn add_camera(tree: &mut SceneTree, name: &str) -> NodeId {
        let root = tree.root_id();
        let node = Node::new(name, "Camera3D");
        tree.add_child(root, node).unwrap()
    }

    // -- script dispatch (pat-nzgdu) -----------------------------------------

    /// `make_current()` must reach the helper that clears the OTHER cameras.
    ///
    /// This is the assertion that would have caught the original bug: before a
    /// dispatch arm existed the call returned `Nil` and changed nothing, so
    /// `b` would still have been current.
    #[test]
    fn script_make_current_switches_the_active_camera() {
        let mut tree = make_tree();
        let a = add_camera(&mut tree, "A");
        let b = add_camera(&mut tree, "B");
        make_current(&mut tree, b);
        assert!(is_current(&tree, b), "precondition: B is current");

        let out = call_camera3d_method(&mut tree, a, "make_current", &[]);
        assert_eq!(out, Some(Variant::Nil), "make_current must be dispatched");
        assert!(is_current(&tree, a), "A must become current");
        assert!(!is_current(&tree, b), "B must stop being current");
    }

    #[test]
    fn script_clear_current_clears_it() {
        let mut tree = make_tree();
        let a = add_camera(&mut tree, "A");
        make_current(&mut tree, a);
        let out = call_camera3d_method(&mut tree, a, "clear_current", &[]);
        assert_eq!(out, Some(Variant::Nil), "clear_current must be dispatched");
        assert!(!is_current(&tree, a));
    }

    /// 🔑 The default cull mask is `0xFFFFF` — ALL twenty layers on, as in Godot
    /// (`get_cull_mask`, this file). An earlier version of this test asserted an
    /// untouched layer reads `false`, which assumed a zero default and failed
    /// against the correct engine behaviour. Pinning the real default is the
    /// more useful assertion: a regression to `0` would make every camera cull
    /// everything, and nothing else here would catch it.
    #[test]
    fn script_cull_mask_value_round_trips() {
        let mut tree = make_tree();
        let a = add_camera(&mut tree, "A");

        assert_eq!(
            call_camera3d_method(&mut tree, a, "get_cull_mask_value", &[Variant::Int(4)]),
            Some(Variant::Bool(true)),
            "every layer starts enabled (Godot's 0xFFFFF default)"
        );

        // Clearing is what exercises the write path against an all-on default;
        // setting a layer that is already on could not fail.
        assert_eq!(
            call_camera3d_method(
                &mut tree,
                a,
                "set_cull_mask_value",
                &[Variant::Int(3), Variant::Bool(false)]
            ),
            Some(Variant::Nil)
        );
        assert_eq!(
            call_camera3d_method(&mut tree, a, "get_cull_mask_value", &[Variant::Int(3)]),
            Some(Variant::Bool(false)),
            "layer 3 must read back cleared"
        );
        assert_eq!(
            call_camera3d_method(&mut tree, a, "get_cull_mask_value", &[Variant::Int(4)]),
            Some(Variant::Bool(true)),
            "clearing layer 3 must not disturb layer 4"
        );

        // …and back on again, so the round trip is both directions.
        assert_eq!(
            call_camera3d_method(
                &mut tree,
                a,
                "set_cull_mask_value",
                &[Variant::Int(3), Variant::Bool(true)]
            ),
            Some(Variant::Nil)
        );
        assert_eq!(
            call_camera3d_method(&mut tree, a, "get_cull_mask_value", &[Variant::Int(3)]),
            Some(Variant::Bool(true)),
            "layer 3 must read back re-enabled"
        );
    }

    /// Godot's `layer_number` is 1..=20; out-of-range must read `false`, not panic.
    ///
    /// ⚠️ The negative case is the point: `layer_number` becomes a `u32`, so a
    /// raw `as` cast on -1 would wrap to 4294967295 rather than being rejected.
    #[test]
    fn script_cull_mask_value_rejects_out_of_range_layers() {
        let mut tree = make_tree();
        let a = add_camera(&mut tree, "A");
        for bad in [Variant::Int(0), Variant::Int(-1), Variant::Int(21)] {
            assert_eq!(
                call_camera3d_method(&mut tree, a, "get_cull_mask_value", &[bad.clone()]),
                Some(Variant::Bool(false)),
                "out-of-range layer {bad:?} must read false"
            );
        }
    }

    /// The handler must DECLINE anything it does not serve, or it would shadow
    /// the generic node table for every other method and node class.
    #[test]
    fn script_dispatch_declines_other_nodes_and_methods() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        let root = tree.root_id();
        let other = tree
            .add_child(root, Node::new("Mesh", "MeshInstance3D"))
            .unwrap();

        assert_eq!(
            call_camera3d_method(&mut tree, other, "make_current", &[]),
            None,
            "a non-Camera3D receiver must be declined"
        );
        assert_eq!(
            call_camera3d_method(&mut tree, cam, "get_frustum", &[]),
            None,
            "a name this handler does not serve must be declined so the \
             generic table stays reachable"
        );
    }

    // -- ProjectionType enum ------------------------------------------------

    #[test]
    fn projection_type_from_i64() {
        assert_eq!(ProjectionType::from_i64(0), ProjectionType::Perspective);
        assert_eq!(ProjectionType::from_i64(1), ProjectionType::Orthogonal);
        assert_eq!(ProjectionType::from_i64(2), ProjectionType::Frustum);
        assert_eq!(ProjectionType::from_i64(99), ProjectionType::Perspective);
    }

    #[test]
    fn keep_aspect_from_i64() {
        assert_eq!(KeepAspect::from_i64(0), KeepAspect::KeepWidth);
        assert_eq!(KeepAspect::from_i64(1), KeepAspect::KeepHeight);
        assert_eq!(KeepAspect::from_i64(42), KeepAspect::KeepHeight);
    }

    #[test]
    fn doppler_tracking_from_i64() {
        assert_eq!(DopplerTracking::from_i64(0), DopplerTracking::Disabled);
        assert_eq!(DopplerTracking::from_i64(1), DopplerTracking::IdleStep);
        assert_eq!(DopplerTracking::from_i64(2), DopplerTracking::PhysicsStep);
        assert_eq!(DopplerTracking::from_i64(99), DopplerTracking::Disabled);
    }

    // -- Projection type property -------------------------------------------

    #[test]
    fn set_get_projection_type_enum() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        assert_eq!(get_projection_type(&tree, cam), ProjectionType::Perspective);

        set_projection_type(&mut tree, cam, ProjectionType::Orthogonal);
        assert_eq!(get_projection_type(&tree, cam), ProjectionType::Orthogonal);

        set_projection_type(&mut tree, cam, ProjectionType::Frustum);
        assert_eq!(get_projection_type(&tree, cam), ProjectionType::Frustum);
    }

    #[test]
    fn projection_type_from_string_variant() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        // Simulate old-style string storage.
        tree.get_node_mut(cam)
            .unwrap()
            .set_property("projection", Variant::String("orthographic".into()));
        assert_eq!(get_projection_type(&tree, cam), ProjectionType::Orthogonal);

        tree.get_node_mut(cam)
            .unwrap()
            .set_property("projection", Variant::String("frustum".into()));
        assert_eq!(get_projection_type(&tree, cam), ProjectionType::Frustum);
    }

    // -- Orthographic size --------------------------------------------------

    #[test]
    fn set_get_size() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        assert!((get_size(&tree, cam) - 1.0).abs() < 1e-6);
        set_size(&mut tree, cam, 10.0);
        assert!((get_size(&tree, cam) - 10.0).abs() < 1e-6);
    }

    // -- Frustum offset -----------------------------------------------------

    #[test]
    fn set_get_frustum_offset() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        assert_eq!(get_frustum_offset(&tree, cam), Vector2::ZERO);
        set_frustum_offset(&mut tree, cam, Vector2::new(0.5, -0.3));
        let off = get_frustum_offset(&tree, cam);
        assert!(approx_eq(off.x, 0.5));
        assert!(approx_eq(off.y, -0.3));
    }

    // -- Keep aspect --------------------------------------------------------

    #[test]
    fn set_get_keep_aspect() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        assert_eq!(get_keep_aspect(&tree, cam), KeepAspect::KeepHeight);
        set_keep_aspect(&mut tree, cam, KeepAspect::KeepWidth);
        assert_eq!(get_keep_aspect(&tree, cam), KeepAspect::KeepWidth);
    }

    // -- Doppler tracking ---------------------------------------------------

    #[test]
    fn set_get_doppler_tracking() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        assert_eq!(get_doppler_tracking(&tree, cam), DopplerTracking::Disabled);
        set_doppler_tracking(&mut tree, cam, DopplerTracking::PhysicsStep);
        assert_eq!(
            get_doppler_tracking(&tree, cam),
            DopplerTracking::PhysicsStep
        );
    }

    // -- H/V offset ---------------------------------------------------------

    #[test]
    fn set_get_h_v_offset() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        assert!((get_h_offset(&tree, cam)).abs() < 1e-6);
        assert!((get_v_offset(&tree, cam)).abs() < 1e-6);

        set_h_offset(&mut tree, cam, 1.5);
        set_v_offset(&mut tree, cam, -2.0);
        assert!((get_h_offset(&tree, cam) - 1.5).abs() < 1e-6);
        assert!((get_v_offset(&tree, cam) + 2.0).abs() < 1e-6);
    }

    // -- Cull mask ----------------------------------------------------------

    #[test]
    fn cull_mask_default_all_layers() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        assert_eq!(get_cull_mask(&tree, cam), 0xFFFFF);
    }

    #[test]
    fn set_get_cull_mask() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        set_cull_mask(&mut tree, cam, 0b101);
        assert_eq!(get_cull_mask(&tree, cam), 0b101);
    }

    #[test]
    fn cull_mask_value_layer_access() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        // Default: all 20 layers on.
        assert!(get_cull_mask_value(&tree, cam, 1));
        assert!(get_cull_mask_value(&tree, cam, 20));

        // Layer 0 and >20 are always false.
        assert!(!get_cull_mask_value(&tree, cam, 0));
        assert!(!get_cull_mask_value(&tree, cam, 21));

        // Clear layer 5.
        set_cull_mask_value(&mut tree, cam, 5, false);
        assert!(!get_cull_mask_value(&tree, cam, 5));
        assert!(get_cull_mask_value(&tree, cam, 4));
        assert!(get_cull_mask_value(&tree, cam, 6));

        // Re-enable layer 5.
        set_cull_mask_value(&mut tree, cam, 5, true);
        assert!(get_cull_mask_value(&tree, cam, 5));
    }

    // -- make_current / clear_current / is_current --------------------------

    #[test]
    fn make_current_clears_others() {
        let mut tree = make_tree();
        let cam1 = add_camera(&mut tree, "Cam1");
        let cam2 = add_camera(&mut tree, "Cam2");

        make_current(&mut tree, cam1);
        assert!(is_current(&tree, cam1));
        assert!(!is_current(&tree, cam2));

        make_current(&mut tree, cam2);
        assert!(!is_current(&tree, cam1));
        assert!(is_current(&tree, cam2));
    }

    #[test]
    fn clear_current_sets_false() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");

        make_current(&mut tree, cam);
        assert!(is_current(&tree, cam));

        clear_current(&mut tree, cam);
        assert!(!is_current(&tree, cam));
    }

    // -- Perspective projection matrix --------------------------------------

    #[test]
    fn perspective_projection_identity_point() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        // A point at the center of the near plane should project to NDC (0, 0).
        let clip = proj.xform4(0.0, 0.0, -0.1, 1.0);
        let ndc_x = clip[0] / clip[3];
        let ndc_y = clip[1] / clip[3];
        assert!(approx_eq(ndc_x, 0.0));
        assert!(approx_eq(ndc_y, 0.0));
    }

    #[test]
    fn perspective_fov_affects_width() {
        let narrow = Projection::create_perspective(30.0, 1.0, 0.1, 100.0);
        let wide = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        // Wider FOV means smaller m[0][0] (less magnification).
        assert!(wide.m[0][0] < narrow.m[0][0]);
    }

    // -- Orthographic projection matrix -------------------------------------

    #[test]
    fn orthographic_projection_center_maps_to_origin() {
        let proj = Projection::create_orthogonal_aspect(10.0, 1.0, 0.1, 100.0);
        let clip = proj.xform4(0.0, 0.0, -50.0, 1.0);
        let ndc_x = clip[0] / clip[3];
        let ndc_y = clip[1] / clip[3];
        assert!(approx_eq(ndc_x, 0.0));
        assert!(approx_eq(ndc_y, 0.0));
    }

    #[test]
    fn orthographic_size_determines_visible_extent() {
        let proj = Projection::create_orthogonal_aspect(10.0, 1.0, 0.1, 100.0);
        // A point at y=5 (half the size) should map to NDC y=1.0.
        let clip = proj.xform4(0.0, 5.0, -50.0, 1.0);
        let ndc_y = clip[1] / clip[3];
        assert!(approx_eq(ndc_y, 1.0));
    }

    // -- Frustum extraction -------------------------------------------------

    #[test]
    fn perspective_frustum_contains_center_point() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        // A point directly in front of the camera (view space) should be inside.
        assert!(frustum.contains_point(Vector3::new(0.0, 0.0, -10.0)));
    }

    #[test]
    fn perspective_frustum_rejects_behind_camera() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        // A point behind the camera.
        assert!(!frustum.contains_point(Vector3::new(0.0, 0.0, 10.0)));
    }

    #[test]
    fn perspective_frustum_rejects_beyond_far() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        assert!(!frustum.contains_point(Vector3::new(0.0, 0.0, -200.0)));
    }

    #[test]
    fn perspective_frustum_rejects_outside_left() {
        let proj = Projection::create_perspective(45.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        // Far off to the left at moderate depth.
        assert!(!frustum.contains_point(Vector3::new(-100.0, 0.0, -5.0)));
    }

    // -- Frustum AABB culling -----------------------------------------------

    #[test]
    fn aabb_inside_frustum_intersects() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        let aabb = Aabb::new(Vector3::new(-1.0, -1.0, -11.0), Vector3::new(2.0, 2.0, 2.0));
        assert!(frustum.intersects_aabb(aabb));
    }

    #[test]
    fn aabb_behind_camera_does_not_intersect() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        let aabb = Aabb::new(Vector3::new(-1.0, -1.0, 5.0), Vector3::new(2.0, 2.0, 2.0));
        assert!(!frustum.intersects_aabb(aabb));
    }

    #[test]
    fn aabb_beyond_far_does_not_intersect() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        let aabb = Aabb::new(
            Vector3::new(-1.0, -1.0, -200.0),
            Vector3::new(2.0, 2.0, 2.0),
        );
        assert!(!frustum.intersects_aabb(aabb));
    }

    #[test]
    fn large_aabb_straddling_near_intersects() {
        let proj = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
        let frustum = Frustum::from_projection(&proj);
        let aabb = Aabb::new(
            Vector3::new(-5.0, -5.0, -5.0),
            Vector3::new(10.0, 10.0, 10.0),
        );
        assert!(frustum.intersects_aabb(aabb));
    }

    // -- Projection matrix from scene tree ----------------------------------

    #[test]
    fn camera_projection_perspective_default() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        node3d::set_fov(&mut tree, cam, 75.0);

        let proj = get_camera_projection(&tree, cam, 800, 600);
        // Should be a valid perspective matrix (m[2][3] == -1.0).
        assert!(approx_eq(proj.m[2][3], -1.0));
    }

    #[test]
    fn camera_projection_orthogonal() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        set_projection_type(&mut tree, cam, ProjectionType::Orthogonal);
        set_size(&mut tree, cam, 10.0);

        let proj = get_camera_projection(&tree, cam, 800, 600);
        // Orthographic: m[2][3] == 0.0 (no perspective divide), m[3][3] == 1.0.
        assert!(approx_eq(proj.m[2][3], 0.0));
        assert!(approx_eq(proj.m[3][3], 1.0));
    }

    // -- Frustum from scene tree --------------------------------------------

    #[test]
    fn camera_frustum_world_space() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        node3d::set_fov(&mut tree, cam, 90.0);
        node3d::set_near(&mut tree, cam, 0.1);
        node3d::set_far(&mut tree, cam, 100.0);
        // Camera at origin looking down -Z (default).

        let frustum = get_camera_frustum(&tree, cam, 800, 600);
        // Point in front of camera.
        assert!(frustum.contains_point(Vector3::new(0.0, 0.0, -10.0)));
        // Point behind.
        assert!(!frustum.contains_point(Vector3::new(0.0, 0.0, 10.0)));
    }

    #[test]
    fn camera_frustum_with_transform() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        node3d::set_fov(&mut tree, cam, 90.0);
        node3d::set_near(&mut tree, cam, 0.1);
        node3d::set_far(&mut tree, cam, 100.0);
        // Move camera to (0, 0, 10), still looking down -Z.
        node3d::set_position(&mut tree, cam, Vector3::new(0.0, 0.0, 10.0));

        let frustum = get_camera_frustum(&tree, cam, 800, 600);
        // Point at (0, 0, 0) is in front of the camera (10 units ahead).
        assert!(frustum.contains_point(Vector3::new(0.0, 0.0, 0.0)));
        // Point at (0, 0, 20) is behind the camera.
        assert!(!frustum.contains_point(Vector3::new(0.0, 0.0, 20.0)));
    }

    // -- is_position_in_frustum from scene tree -----------------------------

    #[test]
    fn is_position_in_frustum_basic() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        node3d::set_fov(&mut tree, cam, 90.0);

        assert!(is_position_in_frustum(
            &tree,
            cam,
            Vector3::new(0.0, 0.0, -10.0),
            800,
            600,
        ));
        assert!(!is_position_in_frustum(
            &tree,
            cam,
            Vector3::new(0.0, 0.0, 10.0),
            800,
            600,
        ));
    }

    // -- unproject / project roundtrip --------------------------------------

    #[test]
    fn unproject_center_point() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        node3d::set_fov(&mut tree, cam, 90.0);

        // A point directly in front of the camera should project to screen center.
        let screen = unproject_position(&tree, cam, Vector3::new(0.0, 0.0, -10.0), 800, 600);
        assert!(approx_eq(screen.x, 400.0));
        assert!(approx_eq(screen.y, 300.0));
    }

    #[test]
    fn project_ray_origin_perspective() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        node3d::set_fov(&mut tree, cam, 90.0);

        let origin = project_ray_origin(&tree, cam, Vector2::new(400.0, 300.0), 800, 600);
        // For perspective, ray origin is the camera position (origin).
        assert!(approx_vec3(origin, Vector3::ZERO));
    }

    #[test]
    fn project_ray_normal_center() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        node3d::set_fov(&mut tree, cam, 90.0);

        let normal = project_ray_normal(&tree, cam, Vector2::new(400.0, 300.0), 800, 600);
        // Center screen should point down -Z.
        assert!(approx_eq(normal.x, 0.0));
        assert!(approx_eq(normal.y, 0.0));
        assert!(normal.z < -0.9);
    }

    #[test]
    fn project_ray_ortho_all_parallel() {
        let mut tree = make_tree();
        let cam = add_camera(&mut tree, "Cam");
        set_projection_type(&mut tree, cam, ProjectionType::Orthogonal);
        set_size(&mut tree, cam, 10.0);

        let n1 = project_ray_normal(&tree, cam, Vector2::new(0.0, 0.0), 800, 600);
        let n2 = project_ray_normal(&tree, cam, Vector2::new(800.0, 600.0), 800, 600);
        // Both should point down -Z.
        assert!(approx_vec3(n1, n2));
        assert!(n1.z < -0.9);
    }

    // -- Projection matrix inverse ------------------------------------------

    #[test]
    fn projection_inverse_roundtrip() {
        let proj = Projection::create_perspective(75.0, 1.333, 0.05, 4000.0);
        let inv = proj.inverse();
        // P * P^-1 should be approximately identity.
        let mut result = [[0.0f32; 4]; 4];
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0f32;
                for k in 0..4 {
                    sum += proj.m[k][row] * inv.m[col][k];
                }
                result[col][row] = sum;
            }
        }
        for i in 0..4 {
            for j in 0..4 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    approx_eq(result[i][j], expected),
                    "result[{}][{}] = {}, expected {}",
                    i,
                    j,
                    result[i][j],
                    expected
                );
            }
        }
    }
}
