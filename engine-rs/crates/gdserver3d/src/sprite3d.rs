//! Sprite3D: 2D-in-3D sprite node with billboard, region, and flip controls.
//!
//! Mirrors Godot's `Sprite3D` / `SpriteBase3D` node surface, providing a
//! textured quad that can be rendered in the 3D pipeline. Supports:
//!
//! - Billboard modes (disabled, enabled, y-billboard, particles)
//! - Region rect (atlas sub-rectangle) with flip H/V
//! - Pixel size, centering, and offset
//! - Alpha cut mode (discard / opaque prepass / hash)
//! - Axis selection (quad plane)
//!
//! The concrete Godot enum integer values are preserved for scene
//! serialization parity (see [`BillboardMode3D::from_godot_int`] and
//! friends). `to_mesh()` builds a [`Mesh3D`] quad suitable for rendering
//! with [`crate::material::Material3D`], and `billboard_basis()` computes
//! the rotation basis for billboard orientation toward a camera.

use crate::material::TextureSlot;
use crate::mesh::{Mesh3D, PrimitiveType};
use gdcore::math::{Color, Rect2, Vector2, Vector2i, Vector3};
use gdcore::math3d::Basis;

// ---------------------------------------------------------------------------
// BillboardMode3D
// ---------------------------------------------------------------------------

/// Billboard mode controlling how the sprite faces the camera.
///
/// Integer values match Godot's `BaseMaterial3D.BillboardMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillboardMode3D {
    /// No billboarding — the sprite uses its local transform.
    Disabled,
    /// Full billboard — sprite always faces the camera.
    Enabled,
    /// Y-axis billboard — sprite rotates around Y to face the camera (keeps Y fixed).
    YBillboard,
    /// Particles billboard — used by particle systems for per-particle facing.
    Particles,
}

impl Default for BillboardMode3D {
    fn default() -> Self {
        Self::Disabled
    }
}

impl BillboardMode3D {
    /// Converts a Godot integer value to a billboard mode.
    ///
    /// Returns [`BillboardMode3D::Disabled`] for unknown values.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            0 => Self::Disabled,
            1 => Self::Enabled,
            2 => Self::YBillboard,
            3 => Self::Particles,
            _ => Self::Disabled,
        }
    }

    /// Converts this billboard mode to its Godot integer value.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Disabled => 0,
            Self::Enabled => 1,
            Self::YBillboard => 2,
            Self::Particles => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// AlphaCutMode
// ---------------------------------------------------------------------------

/// Alpha cut mode for sprite transparency handling.
///
/// Integer values match Godot's `SpriteBase3D.AlphaCutMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaCutMode {
    /// No alpha cutting — standard transparent blend.
    Disabled,
    /// Discard fragments below the scissor threshold.
    Discard,
    /// Opaque prepass — writes depth for fully opaque fragments first.
    OpaquePrepass,
    /// Hashed alpha — stochastic dithered alpha test.
    Hash,
}

impl Default for AlphaCutMode {
    fn default() -> Self {
        Self::Disabled
    }
}

impl AlphaCutMode {
    /// Converts a Godot integer value to an alpha cut mode.
    ///
    /// Returns [`AlphaCutMode::Disabled`] for unknown values.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            0 => Self::Disabled,
            1 => Self::Discard,
            2 => Self::OpaquePrepass,
            3 => Self::Hash,
            _ => Self::Disabled,
        }
    }

    /// Converts this alpha cut mode to its Godot integer value.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Disabled => 0,
            Self::Discard => 1,
            Self::OpaquePrepass => 2,
            Self::Hash => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Axis3D
// ---------------------------------------------------------------------------

/// The axis the sprite's plane normal aligns with.
///
/// Integer values match Godot's `Vector3.Axis` enum (X=0, Y=1, Z=2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis3D {
    /// Sprite lies in the YZ plane (normal along X).
    X,
    /// Sprite lies in the XZ plane (normal along Y) — e.g. a floor decal.
    Y,
    /// Sprite lies in the XY plane (normal along Z) — the default.
    Z,
}

impl Default for Axis3D {
    fn default() -> Self {
        Self::Z
    }
}

impl Axis3D {
    /// Converts a Godot integer value to an axis.
    ///
    /// Returns [`Axis3D::Z`] for unknown values.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            0 => Self::X,
            1 => Self::Y,
            2 => Self::Z,
            _ => Self::Z,
        }
    }

    /// Converts this axis to its Godot integer value.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Sprite3D
// ---------------------------------------------------------------------------

/// A 2D-in-3D sprite node.
///
/// Represents a textured quad rendered in the 3D pipeline. Properties
/// match Godot's `Sprite3D` + `SpriteBase3D` surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprite3D {
    /// Optional albedo texture slot.
    pub texture: Option<TextureSlot>,
    /// Natural size of the texture in pixels (used when no region is set).
    pub texture_size: Vector2i,
    /// Whether the sprite is centered on its origin.
    pub centered: bool,
    /// Offset in pixels (multiplied by `pixel_size` for world offset).
    pub offset: Vector2,
    /// Flip horizontally.
    pub flip_h: bool,
    /// Flip vertically.
    pub flip_v: bool,
    /// Color modulation (tint).
    pub modulate: Color,
    /// Pixel-to-world conversion factor.
    pub pixel_size: f32,
    /// Axis whose normal the sprite plane uses.
    pub axis: Axis3D,
    /// Billboard mode.
    pub billboard: BillboardMode3D,
    /// Whether the material is transparent.
    pub transparent: bool,
    /// Whether the sprite receives lighting (vs unlit).
    pub shaded: bool,
    /// Whether the sprite renders both faces.
    pub double_sided: bool,
    /// Whether depth test is disabled (always on top).
    pub no_depth_test: bool,
    /// Whether the sprite maintains a fixed screen-space size.
    pub fixed_size: bool,
    /// Alpha cut mode.
    pub alpha_cut: AlphaCutMode,
    /// Alpha scissor threshold (used with `AlphaCutMode::Discard`).
    pub alpha_scissor_threshold: f32,
    /// Alpha hash scale (used with `AlphaCutMode::Hash`).
    pub alpha_hash_scale: f32,
    /// Render priority — higher values render later.
    pub render_priority: i32,
    /// Whether region-rect (atlas subrect) is enabled.
    pub region_enabled: bool,
    /// Region rect in pixel coordinates.
    pub region_rect: Rect2,
}

impl Default for Sprite3D {
    fn default() -> Self {
        Self {
            texture: None,
            texture_size: Vector2i::new(64, 64),
            centered: true,
            offset: Vector2::ZERO,
            flip_h: false,
            flip_v: false,
            modulate: Color::new(1.0, 1.0, 1.0, 1.0),
            pixel_size: 0.01,
            axis: Axis3D::Z,
            billboard: BillboardMode3D::Disabled,
            transparent: true,
            shaded: false,
            double_sided: true,
            no_depth_test: false,
            fixed_size: false,
            alpha_cut: AlphaCutMode::Disabled,
            alpha_scissor_threshold: 0.5,
            alpha_hash_scale: 1.0,
            render_priority: 0,
            region_enabled: false,
            region_rect: Rect2::new(Vector2::ZERO, Vector2::ZERO),
        }
    }
}

impl Sprite3D {
    /// Creates a new sprite with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the effective source rect used for UV sampling.
    ///
    /// If `region_enabled` is set, returns `region_rect` as-is. Otherwise
    /// returns a rect spanning the full texture size (origin `(0, 0)`).
    pub fn effective_region(&self) -> Rect2 {
        if self.region_enabled {
            self.region_rect
        } else {
            Rect2::new(
                Vector2::ZERO,
                Vector2::new(self.texture_size.x as f32, self.texture_size.y as f32),
            )
        }
    }

    /// Returns the sprite's world-space size as (width, height).
    ///
    /// Computed as `effective_region().size * pixel_size`.
    pub fn world_size(&self) -> Vector2 {
        let r = self.effective_region();
        Vector2::new(r.size.x * self.pixel_size, r.size.y * self.pixel_size)
    }

    /// Returns the UV coordinates for the four quad vertices in order:
    /// `[bottom_left, bottom_right, top_right, top_left]`.
    ///
    /// Accounts for region rect (if enabled) and flip H/V.
    pub fn uv_coords(&self) -> [[f32; 2]; 4] {
        let tex_w = self.texture_size.x.max(1) as f32;
        let tex_h = self.texture_size.y.max(1) as f32;

        let (u0, u1, v0, v1) = if self.region_enabled {
            let r = self.region_rect;
            (
                r.position.x / tex_w,
                (r.position.x + r.size.x) / tex_w,
                r.position.y / tex_h,
                (r.position.y + r.size.y) / tex_h,
            )
        } else {
            (0.0, 1.0, 0.0, 1.0)
        };

        let (lu, ru) = if self.flip_h { (u1, u0) } else { (u0, u1) };
        let (tv, bv) = if self.flip_v { (v1, v0) } else { (v0, v1) };

        // Order: BL, BR, TR, TL
        [[lu, bv], [ru, bv], [ru, tv], [lu, tv]]
    }

    /// Returns the four quad vertices in the sprite's local space.
    ///
    /// Order: `[bottom_left, bottom_right, top_right, top_left]`.
    /// Accounts for `centered`, `offset`, `pixel_size`, and `axis`.
    pub fn quad_vertices(&self) -> [Vector3; 4] {
        let ws = self.world_size();
        let half_w = ws.x * 0.5;
        let half_h = ws.y * 0.5;

        let (cx, cy) = if self.centered {
            (0.0, 0.0)
        } else {
            // Top-left at origin.
            (half_w, -half_h)
        };

        let ox = cx + self.offset.x * self.pixel_size;
        let oy = cy + self.offset.y * self.pixel_size;

        let bl = (ox - half_w, oy - half_h);
        let br = (ox + half_w, oy - half_h);
        let tr = (ox + half_w, oy + half_h);
        let tl = (ox - half_w, oy + half_h);

        let to_v3 = |(x, y): (f32, f32)| match self.axis {
            Axis3D::Z => Vector3::new(x, y, 0.0),
            Axis3D::Y => Vector3::new(x, 0.0, y),
            Axis3D::X => Vector3::new(0.0, y, x),
        };

        [to_v3(bl), to_v3(br), to_v3(tr), to_v3(tl)]
    }

    /// Returns the plane normal for the sprite's current axis.
    pub fn plane_normal(&self) -> Vector3 {
        match self.axis {
            Axis3D::X => Vector3::new(1.0, 0.0, 0.0),
            Axis3D::Y => Vector3::new(0.0, 1.0, 0.0),
            Axis3D::Z => Vector3::new(0.0, 0.0, 1.0),
        }
    }

    /// Computes the rotation basis for billboard orientation.
    ///
    /// `sprite_pos` is the sprite's world position and `camera_pos` is the
    /// camera's world position. Returns `Basis::IDENTITY` when billboarding
    /// is disabled or the camera/sprite are coincident.
    pub fn billboard_basis(&self, sprite_pos: Vector3, camera_pos: Vector3) -> Basis {
        let world_up = Vector3::UP;
        let delta = camera_pos - sprite_pos;

        match self.billboard {
            BillboardMode3D::Disabled => Basis::IDENTITY,
            BillboardMode3D::Enabled | BillboardMode3D::Particles => {
                if delta.length_squared() < 1e-12 {
                    return Basis::IDENTITY;
                }
                let forward = delta.normalized();
                // Degenerate case: forward is parallel to world up.
                let right_unnorm = world_up.cross(forward);
                if right_unnorm.length_squared() < 1e-12 {
                    return Basis::IDENTITY;
                }
                let right = right_unnorm.normalized();
                let up = forward.cross(right).normalized();
                Basis {
                    x: right,
                    y: up,
                    z: forward,
                }
            }
            BillboardMode3D::YBillboard => {
                // Project the view direction onto the sprite's XZ plane,
                // keeping world Y fixed.
                let flat = Vector3::new(delta.x, 0.0, delta.z);
                if flat.length_squared() < 1e-12 {
                    return Basis::IDENTITY;
                }
                let forward = flat.normalized();
                let right = world_up.cross(forward).normalized();
                Basis {
                    x: right,
                    y: world_up,
                    z: forward,
                }
            }
        }
    }

    /// Builds a [`Mesh3D`] quad from this sprite's configuration.
    ///
    /// The mesh has 4 vertices, 2 triangles, a shared plane normal, and
    /// UVs accounting for region and flip state. The billboard basis is
    /// *not* baked into the mesh — callers apply it via the instance's
    /// [`gdcore::math3d::Transform3D`] at render time.
    pub fn to_mesh(&self) -> Mesh3D {
        let verts = self.quad_vertices();
        let normal = self.plane_normal();
        let uvs = self.uv_coords();

        Mesh3D {
            vertices: verts.to_vec(),
            normals: vec![normal; 4],
            uvs: uvs.to_vec(),
            indices: vec![0, 1, 2, 0, 2, 3],
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn v3_approx_eq(a: Vector3, b: Vector3, tol: f32) -> bool {
        approx_eq(a.x, b.x, tol) && approx_eq(a.y, b.y, tol) && approx_eq(a.z, b.z, tol)
    }

    // ── Enum roundtrips ─────────────────────────────────────────────────

    #[test]
    fn billboard_mode_roundtrip() {
        for mode in [
            BillboardMode3D::Disabled,
            BillboardMode3D::Enabled,
            BillboardMode3D::YBillboard,
            BillboardMode3D::Particles,
        ] {
            assert_eq!(BillboardMode3D::from_godot_int(mode.to_godot_int()), mode);
        }
    }

    #[test]
    fn billboard_mode_unknown_defaults_to_disabled() {
        assert_eq!(BillboardMode3D::from_godot_int(99), BillboardMode3D::Disabled);
        assert_eq!(BillboardMode3D::from_godot_int(-1), BillboardMode3D::Disabled);
    }

    #[test]
    fn alpha_cut_mode_roundtrip() {
        for mode in [
            AlphaCutMode::Disabled,
            AlphaCutMode::Discard,
            AlphaCutMode::OpaquePrepass,
            AlphaCutMode::Hash,
        ] {
            assert_eq!(AlphaCutMode::from_godot_int(mode.to_godot_int()), mode);
        }
    }

    #[test]
    fn axis3d_roundtrip() {
        for axis in [Axis3D::X, Axis3D::Y, Axis3D::Z] {
            assert_eq!(Axis3D::from_godot_int(axis.to_godot_int()), axis);
        }
    }

    // ── Defaults ────────────────────────────────────────────────────────

    #[test]
    fn sprite3d_defaults() {
        let s = Sprite3D::new();
        assert!(s.centered);
        assert!(!s.flip_h);
        assert!(!s.flip_v);
        assert_eq!(s.offset, Vector2::ZERO);
        assert_eq!(s.modulate, Color::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(s.axis, Axis3D::Z);
        assert_eq!(s.billboard, BillboardMode3D::Disabled);
        assert_eq!(s.alpha_cut, AlphaCutMode::Disabled);
        assert!(approx_eq(s.alpha_scissor_threshold, 0.5, 1e-6));
        assert!(!s.region_enabled);
        assert!(s.texture.is_none());
        assert!(s.double_sided);
    }

    // ── Effective region ────────────────────────────────────────────────

    #[test]
    fn effective_region_uses_texture_size_by_default() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(128, 64);
        let r = s.effective_region();
        assert_eq!(r.position, Vector2::ZERO);
        assert_eq!(r.size, Vector2::new(128.0, 64.0));
    }

    #[test]
    fn effective_region_uses_region_rect_when_enabled() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(256, 256);
        s.region_enabled = true;
        s.region_rect = Rect2::new(Vector2::new(32.0, 16.0), Vector2::new(64.0, 48.0));
        let r = s.effective_region();
        assert_eq!(r.position, Vector2::new(32.0, 16.0));
        assert_eq!(r.size, Vector2::new(64.0, 48.0));
    }

    // ── World size ──────────────────────────────────────────────────────

    #[test]
    fn world_size_scales_by_pixel_size() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 50);
        s.pixel_size = 0.02;
        let ws = s.world_size();
        assert!(approx_eq(ws.x, 2.0, 1e-5));
        assert!(approx_eq(ws.y, 1.0, 1e-5));
    }

    #[test]
    fn world_size_uses_region_size() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(512, 512);
        s.region_enabled = true;
        s.region_rect = Rect2::new(Vector2::ZERO, Vector2::new(64.0, 32.0));
        s.pixel_size = 0.1;
        let ws = s.world_size();
        assert!(approx_eq(ws.x, 6.4, 1e-5));
        assert!(approx_eq(ws.y, 3.2, 1e-5));
    }

    // ── UV coords ───────────────────────────────────────────────────────

    #[test]
    fn uv_coords_default() {
        let s = Sprite3D::new();
        let uvs = s.uv_coords();
        // BL, BR, TR, TL
        assert_eq!(uvs[0], [0.0, 1.0]);
        assert_eq!(uvs[1], [1.0, 1.0]);
        assert_eq!(uvs[2], [1.0, 0.0]);
        assert_eq!(uvs[3], [0.0, 0.0]);
    }

    #[test]
    fn uv_coords_flip_h_swaps_u() {
        let mut s = Sprite3D::new();
        s.flip_h = true;
        let uvs = s.uv_coords();
        assert_eq!(uvs[0], [1.0, 1.0]);
        assert_eq!(uvs[1], [0.0, 1.0]);
        assert_eq!(uvs[2], [0.0, 0.0]);
        assert_eq!(uvs[3], [1.0, 0.0]);
    }

    #[test]
    fn uv_coords_flip_v_swaps_v() {
        let mut s = Sprite3D::new();
        s.flip_v = true;
        let uvs = s.uv_coords();
        assert_eq!(uvs[0], [0.0, 0.0]);
        assert_eq!(uvs[1], [1.0, 0.0]);
        assert_eq!(uvs[2], [1.0, 1.0]);
        assert_eq!(uvs[3], [0.0, 1.0]);
    }

    #[test]
    fn uv_coords_region_rect_normalizes_to_texture() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 200);
        s.region_enabled = true;
        s.region_rect = Rect2::new(Vector2::new(25.0, 50.0), Vector2::new(50.0, 100.0));
        let uvs = s.uv_coords();
        // u: 25/100..75/100 = 0.25..0.75; v: 50/200..150/200 = 0.25..0.75
        assert!(approx_eq(uvs[0][0], 0.25, 1e-5));
        assert!(approx_eq(uvs[0][1], 0.75, 1e-5));
        assert!(approx_eq(uvs[2][0], 0.75, 1e-5));
        assert!(approx_eq(uvs[2][1], 0.25, 1e-5));
    }

    // ── Quad vertices ───────────────────────────────────────────────────

    #[test]
    fn quad_vertices_centered_z_axis() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 100);
        s.pixel_size = 0.01; // world size 1x1
        let v = s.quad_vertices();
        assert!(v3_approx_eq(v[0], Vector3::new(-0.5, -0.5, 0.0), 1e-5));
        assert!(v3_approx_eq(v[1], Vector3::new(0.5, -0.5, 0.0), 1e-5));
        assert!(v3_approx_eq(v[2], Vector3::new(0.5, 0.5, 0.0), 1e-5));
        assert!(v3_approx_eq(v[3], Vector3::new(-0.5, 0.5, 0.0), 1e-5));
    }

    #[test]
    fn quad_vertices_axis_y_lies_in_xz_plane() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 100);
        s.pixel_size = 0.01;
        s.axis = Axis3D::Y;
        let v = s.quad_vertices();
        for p in &v {
            assert!(approx_eq(p.y, 0.0, 1e-5), "axis Y quad should have y=0");
        }
    }

    #[test]
    fn quad_vertices_axis_x_lies_in_yz_plane() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 100);
        s.pixel_size = 0.01;
        s.axis = Axis3D::X;
        let v = s.quad_vertices();
        for p in &v {
            assert!(approx_eq(p.x, 0.0, 1e-5), "axis X quad should have x=0");
        }
    }

    #[test]
    fn quad_vertices_offset_shifts_all_corners() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 100);
        s.pixel_size = 0.01;
        s.offset = Vector2::new(50.0, 0.0);
        let v = s.quad_vertices();
        // offset 50px * 0.01 = 0.5 world shift on x
        assert!(approx_eq(v[0].x, 0.0, 1e-5));
        assert!(approx_eq(v[1].x, 1.0, 1e-5));
        assert!(approx_eq(v[2].x, 1.0, 1e-5));
    }

    #[test]
    fn quad_vertices_non_centered_moves_top_left_to_origin() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 100);
        s.pixel_size = 0.01;
        s.centered = false;
        let v = s.quad_vertices();
        // Top-left corner should be at origin.
        assert!(v3_approx_eq(v[3], Vector3::ZERO, 1e-5));
    }

    // ── Billboard basis ─────────────────────────────────────────────────

    #[test]
    fn billboard_disabled_returns_identity() {
        let s = Sprite3D::new();
        let b = s.billboard_basis(Vector3::ZERO, Vector3::new(10.0, 0.0, 10.0));
        assert_eq!(b, Basis::IDENTITY);
    }

    #[test]
    fn billboard_enabled_aligns_forward_to_camera() {
        let mut s = Sprite3D::new();
        s.billboard = BillboardMode3D::Enabled;
        // Camera directly above-front of sprite, non-colinear with up.
        let cam = Vector3::new(3.0, 0.0, 4.0);
        let b = s.billboard_basis(Vector3::ZERO, cam);
        let expected_forward = cam.normalized();
        assert!(v3_approx_eq(b.z, expected_forward, 1e-5));
        // Axes should be orthonormal.
        assert!(approx_eq(b.x.length(), 1.0, 1e-5));
        assert!(approx_eq(b.y.length(), 1.0, 1e-5));
        assert!(approx_eq(b.x.dot(b.y), 0.0, 1e-5));
        assert!(approx_eq(b.x.dot(b.z), 0.0, 1e-5));
        assert!(approx_eq(b.y.dot(b.z), 0.0, 1e-5));
    }

    #[test]
    fn billboard_enabled_degenerate_parallel_up_returns_identity() {
        let mut s = Sprite3D::new();
        s.billboard = BillboardMode3D::Enabled;
        // Camera straight above the sprite — forward parallel to world up.
        let b = s.billboard_basis(Vector3::ZERO, Vector3::new(0.0, 5.0, 0.0));
        assert_eq!(b, Basis::IDENTITY);
    }

    #[test]
    fn billboard_enabled_coincident_returns_identity() {
        let mut s = Sprite3D::new();
        s.billboard = BillboardMode3D::Enabled;
        let b = s.billboard_basis(Vector3::new(2.0, 2.0, 2.0), Vector3::new(2.0, 2.0, 2.0));
        assert_eq!(b, Basis::IDENTITY);
    }

    #[test]
    fn billboard_y_keeps_y_axis_vertical() {
        let mut s = Sprite3D::new();
        s.billboard = BillboardMode3D::YBillboard;
        // Camera off at an angle with some vertical lift.
        let b = s.billboard_basis(Vector3::ZERO, Vector3::new(5.0, 3.0, 5.0));
        assert!(v3_approx_eq(b.y, Vector3::UP, 1e-5));
        // Forward should lie flat (y=0).
        assert!(approx_eq(b.z.y, 0.0, 1e-5));
        // Right should lie flat (y=0).
        assert!(approx_eq(b.x.y, 0.0, 1e-5));
    }

    #[test]
    fn billboard_particles_behaves_like_enabled() {
        let mut a = Sprite3D::new();
        a.billboard = BillboardMode3D::Enabled;
        let mut b = Sprite3D::new();
        b.billboard = BillboardMode3D::Particles;
        let cam = Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(a.billboard_basis(Vector3::ZERO, cam), b.billboard_basis(Vector3::ZERO, cam));
    }

    // ── to_mesh ─────────────────────────────────────────────────────────

    #[test]
    fn to_mesh_produces_quad_triangles() {
        let mut s = Sprite3D::new();
        s.texture_size = Vector2i::new(100, 100);
        s.pixel_size = 0.01;
        let m = s.to_mesh();
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.normals.len(), 4);
        assert_eq!(m.uvs.len(), 4);
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(m.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn to_mesh_normals_point_along_axis() {
        let mut s = Sprite3D::new();
        s.axis = Axis3D::Y;
        let m = s.to_mesh();
        for n in &m.normals {
            assert!(v3_approx_eq(*n, Vector3::new(0.0, 1.0, 0.0), 1e-5));
        }
    }

    // ── Texture / region assignment ─────────────────────────────────────

    #[test]
    fn texture_slot_assignment() {
        let mut s = Sprite3D::new();
        s.texture = Some(TextureSlot::new("res://sprite.png"));
        assert_eq!(s.texture.as_ref().unwrap().path, "res://sprite.png");
    }

    #[test]
    fn region_rect_stores_rect() {
        let mut s = Sprite3D::new();
        s.region_enabled = true;
        s.region_rect = Rect2::new(Vector2::new(10.0, 20.0), Vector2::new(30.0, 40.0));
        assert!(s.region_enabled);
        assert_eq!(s.region_rect.position, Vector2::new(10.0, 20.0));
        assert_eq!(s.region_rect.size, Vector2::new(30.0, 40.0));
    }
}
