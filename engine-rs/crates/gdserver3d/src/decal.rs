//! Decal node for projected texture decals.
//!
//! Implements Godot's `Decal` node — a `VisualInstance3D` that projects
//! textures onto nearby geometry within an oriented bounding box (OBB).
//!
//! The decal projects downward along its local −Y axis. Geometry within
//! the decal's [`size`](Decal3D::size) box receives the projected texture
//! blended according to the configured channels, modulate color, and
//! energy multiplier.
//!
//! Channels:
//! - **Albedo** — base color texture
//! - **Normal** — normal map (tangent-space, projected)
//! - **ORM** — occlusion / roughness / metallic packed texture
//! - **Emission** — emissive texture (multiplied by `emission_energy`)
//!
//! Fade control:
//! - `upper_fade` / `lower_fade` — gradient ramp at the top/bottom of
//!   the projection box so decals don't end with a hard edge.
//! - `distance_fade_*` — camera-distance-based fading.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;

// ---------------------------------------------------------------------------
// Decal texture channels
// ---------------------------------------------------------------------------

/// Identifies which texture slot on the decal is being referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecalTexture {
    /// Base color / albedo channel.
    Albedo,
    /// Tangent-space normal map channel.
    Normal,
    /// Occlusion-Roughness-Metallic packed channel.
    Orm,
    /// Emissive channel.
    Emission,
}

// ---------------------------------------------------------------------------
// Decal3D
// ---------------------------------------------------------------------------

/// A projected-texture decal node (Godot `Decal`).
///
/// The decal projects along its local −Y axis. The [`size`](Self::size)
/// defines the full width (X), height (Y), and depth (Z) of the
/// projection box.
#[derive(Debug, Clone)]
pub struct Decal3D {
    /// Full extents of the decal projection box (meters).
    /// Godot default: `Vector3(2.0, 2.0, 2.0)`.
    pub size: Vector3,

    /// Optional texture resource paths for each channel.
    pub texture_albedo: Option<String>,
    pub texture_normal: Option<String>,
    pub texture_orm: Option<String>,
    pub texture_emission: Option<String>,

    /// Multiplier for the emissive channel.
    pub emission_energy: f64,

    /// Color multiplier applied to the albedo channel.
    pub modulate: Color,

    /// Albedo mix factor — how much of the projected albedo replaces
    /// the underlying surface albedo (`0.0` = fully transparent,
    /// `1.0` = fully opaque).
    pub albedo_mix: f64,

    /// Multiplier for the normal-map influence (`0.0`–`1.0`).
    pub normal_fade: f64,

    /// Gradient fade at the upper edge of the projection box (`0.0`–`1.0`).
    pub upper_fade: f64,

    /// Gradient fade at the lower edge of the projection box (`0.0`–`1.0`).
    pub lower_fade: f64,

    /// Distance (in meters) at which the decal starts fading out.
    pub distance_fade_begin: f64,

    /// Length (in meters) over which the distance fade occurs.
    pub distance_fade_length: f64,

    /// Whether distance-based fading is enabled.
    pub distance_fade_enabled: bool,

    /// Cull mask — only affects geometry on matching visual layers.
    pub cull_mask: u32,

    /// Local-to-world transform (set by the scene tree sync step).
    pub global_transform: Transform3D,

    /// Whether the decal is visible.
    pub visible: bool,
}

impl Default for Decal3D {
    fn default() -> Self {
        Self {
            size: Vector3::new(2.0, 2.0, 2.0),
            texture_albedo: None,
            texture_normal: None,
            texture_orm: None,
            texture_emission: None,
            emission_energy: 1.0,
            modulate: Color::WHITE,
            albedo_mix: 1.0,
            normal_fade: 1.0,
            upper_fade: 0.3,
            lower_fade: 0.3,
            distance_fade_begin: 40.0,
            distance_fade_length: 10.0,
            distance_fade_enabled: false,
            cull_mask: 0xFFFF_FFFF,
            global_transform: Transform3D::IDENTITY,
            visible: true,
        }
    }
}

impl Decal3D {
    /// Creates a new decal with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the texture resource path for the given channel.
    pub fn set_texture(&mut self, channel: DecalTexture, path: Option<String>) {
        match channel {
            DecalTexture::Albedo => self.texture_albedo = path,
            DecalTexture::Normal => self.texture_normal = path,
            DecalTexture::Orm => self.texture_orm = path,
            DecalTexture::Emission => self.texture_emission = path,
        }
    }

    /// Returns the texture resource path for the given channel.
    pub fn get_texture(&self, channel: DecalTexture) -> Option<&str> {
        match channel {
            DecalTexture::Albedo => self.texture_albedo.as_deref(),
            DecalTexture::Normal => self.texture_normal.as_deref(),
            DecalTexture::Orm => self.texture_orm.as_deref(),
            DecalTexture::Emission => self.texture_emission.as_deref(),
        }
    }

    /// Returns the half-extents of the decal box (size / 2).
    pub fn half_extents(&self) -> Vector3 {
        Vector3::new(self.size.x * 0.5, self.size.y * 0.5, self.size.z * 0.5)
    }

    // -----------------------------------------------------------------------
    // Projection helpers
    // -----------------------------------------------------------------------

    /// Projects a world-space point into decal-local UVW coordinates.
    ///
    /// Returns `Some((u, v, w))` where:
    /// - `u` ∈ [0, 1] across the X axis of the box
    /// - `v` ∈ [0, 1] across the Z axis of the box (depth)
    /// - `w` ∈ [0, 1] from bottom (−Y) to top (+Y) of the box
    ///
    /// Returns `None` if the point is outside the decal box.
    pub fn project_point(&self, world_point: Vector3) -> Option<(f64, f64, f64)> {
        let inv = self.global_transform.inverse();
        let local = inv.xform(world_point);
        let he = self.half_extents();

        if local.x.abs() > he.x || local.y.abs() > he.y || local.z.abs() > he.z {
            return None;
        }

        let u = (local.x / he.x) * 0.5 + 0.5;
        let v = (local.z / he.z) * 0.5 + 0.5;
        let w = (local.y / he.y) * 0.5 + 0.5;

        Some((u as f64, v as f64, w as f64))
    }

    /// Computes the fade factor for a projected point based on its `w`
    /// coordinate (position along the projection axis) and the
    /// `upper_fade` / `lower_fade` settings.
    ///
    /// Returns a value in `[0.0, 1.0]` where `1.0` means fully visible.
    pub fn vertical_fade(&self, w: f64) -> f64 {
        let mut fade = 1.0;

        // Upper fade: w near 1.0
        if self.upper_fade > 0.0 {
            let t = ((1.0 - w) / self.upper_fade).min(1.0);
            fade *= t;
        }

        // Lower fade: w near 0.0
        if self.lower_fade > 0.0 {
            let t = (w / self.lower_fade).min(1.0);
            fade *= t;
        }

        fade.clamp(0.0, 1.0)
    }

    /// Computes the distance fade factor given the camera-to-decal distance.
    ///
    /// Returns `1.0` when distance fading is disabled or the decal is
    /// closer than `distance_fade_begin`.
    pub fn distance_fade(&self, camera_distance: f64) -> f64 {
        if !self.distance_fade_enabled {
            return 1.0;
        }
        if camera_distance < self.distance_fade_begin {
            return 1.0;
        }
        if self.distance_fade_length <= 0.0 {
            return 0.0;
        }
        let t = (camera_distance - self.distance_fade_begin) / self.distance_fade_length;
        (1.0 - t).clamp(0.0, 1.0)
    }

    /// Tests whether a world-space AABB (given as center + half-extents)
    /// could potentially overlap this decal's projection box.
    ///
    /// Uses a conservative OBB-vs-AABB separating-axis test on the
    /// decal's local axes.
    pub fn overlaps_aabb(&self, aabb_center: Vector3, aabb_half: Vector3) -> bool {
        let inv = self.global_transform.inverse();
        let local_center = inv.xform(aabb_center);
        let he = self.half_extents();

        // Transform AABB half-extents into decal-local space (conservative).
        let basis = &inv.basis;
        let local_half = Vector3::new(
            (basis.x.x.abs() * aabb_half.x)
                + (basis.x.y.abs() * aabb_half.y)
                + (basis.x.z.abs() * aabb_half.z),
            (basis.y.x.abs() * aabb_half.x)
                + (basis.y.y.abs() * aabb_half.y)
                + (basis.y.z.abs() * aabb_half.z),
            (basis.z.x.abs() * aabb_half.x)
                + (basis.z.y.abs() * aabb_half.y)
                + (basis.z.z.abs() * aabb_half.z),
        );

        local_center.x.abs() <= he.x + local_half.x
            && local_center.y.abs() <= he.y + local_half.y
            && local_center.z.abs() <= he.z + local_half.z
    }

    /// Samples the combined fade factor for a world-space point at a
    /// given camera distance.
    ///
    /// Returns `Some(alpha)` in `[0.0, 1.0]` if the point is inside
    /// the decal box, or `None` if outside.
    pub fn sample_alpha(&self, world_point: Vector3, camera_distance: f64) -> Option<f64> {
        if !self.visible {
            return None;
        }
        let (_, _, w) = self.project_point(world_point)?;
        let vf = self.vertical_fade(w);
        let df = self.distance_fade(camera_distance);
        Some(vf * df * self.albedo_mix)
    }
}

// ---------------------------------------------------------------------------
// DecalRegistry — manages active decals for the rendering server
// ---------------------------------------------------------------------------

/// Manages a collection of active [`Decal3D`] instances for the renderer.
///
/// The rendering server can query the registry to gather all decals that
/// might affect a given piece of geometry.
pub struct DecalRegistry {
    decals: Vec<(u64, Decal3D)>,
    next_id: u64,
}

impl Default for DecalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DecalRegistry {
    pub fn new() -> Self {
        Self {
            decals: Vec::new(),
            next_id: 1,
        }
    }

    /// Registers a decal and returns its unique handle.
    pub fn add(&mut self, decal: Decal3D) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.decals.push((id, decal));
        id
    }

    /// Removes a decal by handle. Returns `true` if found.
    pub fn remove(&mut self, handle: u64) -> bool {
        let len_before = self.decals.len();
        self.decals.retain(|(id, _)| *id != handle);
        self.decals.len() < len_before
    }

    /// Returns a mutable reference to a decal by handle.
    pub fn get_mut(&mut self, handle: u64) -> Option<&mut Decal3D> {
        self.decals
            .iter_mut()
            .find(|(id, _)| *id == handle)
            .map(|(_, d)| d)
    }

    /// Returns a reference to a decal by handle.
    pub fn get(&self, handle: u64) -> Option<&Decal3D> {
        self.decals
            .iter()
            .find(|(id, _)| *id == handle)
            .map(|(_, d)| d)
    }

    /// Returns the number of registered decals.
    pub fn len(&self) -> usize {
        self.decals.len()
    }

    /// Returns `true` if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.decals.is_empty()
    }

    /// Iterates over all registered decals.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &Decal3D)> {
        self.decals.iter().map(|(id, d)| (*id, d))
    }

    /// Returns all visible decals whose projection box overlaps the given
    /// world-space AABB. Used by the renderer to gather decals that might
    /// affect a mesh.
    pub fn query_overlapping(
        &self,
        aabb_center: Vector3,
        aabb_half: Vector3,
    ) -> Vec<(u64, &Decal3D)> {
        self.decals
            .iter()
            .filter(|(_, d)| d.visible && d.overlaps_aabb(aabb_center, aabb_half))
            .map(|(id, d)| (*id, d))
            .collect()
    }

    /// Clears all decals from the registry.
    pub fn clear(&mut self) {
        self.decals.clear();
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math3d::Basis;

    #[test]
    fn default_decal_has_correct_size() {
        let d = Decal3D::new();
        assert_eq!(d.size.x, 2.0);
        assert_eq!(d.size.y, 2.0);
        assert_eq!(d.size.z, 2.0);
    }

    #[test]
    fn default_modulate_is_white() {
        let d = Decal3D::new();
        assert_eq!(d.modulate.r, 1.0);
        assert_eq!(d.modulate.g, 1.0);
        assert_eq!(d.modulate.b, 1.0);
        assert_eq!(d.modulate.a, 1.0);
    }

    #[test]
    fn half_extents_is_half_size() {
        let mut d = Decal3D::new();
        d.size = Vector3::new(4.0, 6.0, 8.0);
        let he = d.half_extents();
        assert_eq!(he.x, 2.0);
        assert_eq!(he.y, 3.0);
        assert_eq!(he.z, 4.0);
    }

    #[test]
    fn set_get_texture_channels() {
        let mut d = Decal3D::new();
        assert!(d.get_texture(DecalTexture::Albedo).is_none());

        d.set_texture(DecalTexture::Albedo, Some("res://decal.png".into()));
        d.set_texture(DecalTexture::Normal, Some("res://decal_n.png".into()));
        d.set_texture(DecalTexture::Orm, Some("res://decal_orm.png".into()));
        d.set_texture(DecalTexture::Emission, Some("res://decal_e.png".into()));

        assert_eq!(d.get_texture(DecalTexture::Albedo), Some("res://decal.png"));
        assert_eq!(
            d.get_texture(DecalTexture::Normal),
            Some("res://decal_n.png")
        );
        assert_eq!(
            d.get_texture(DecalTexture::Orm),
            Some("res://decal_orm.png")
        );
        assert_eq!(
            d.get_texture(DecalTexture::Emission),
            Some("res://decal_e.png")
        );

        d.set_texture(DecalTexture::Albedo, None);
        assert!(d.get_texture(DecalTexture::Albedo).is_none());
    }

    #[test]
    fn project_point_at_origin() {
        let d = Decal3D::new(); // identity transform, size 2x2x2
        let result = d.project_point(Vector3::ZERO);
        let (u, v, w) = result.unwrap();
        assert!((u - 0.5).abs() < 1e-9);
        assert!((v - 0.5).abs() < 1e-9);
        assert!((w - 0.5).abs() < 1e-9);
    }

    #[test]
    fn project_point_at_corner() {
        let d = Decal3D::new();
        // Point at (+1, +1, +1) — exactly at the box corner
        let result = d.project_point(Vector3::new(1.0, 1.0, 1.0));
        let (u, v, w) = result.unwrap();
        assert!((u - 1.0).abs() < 1e-9);
        assert!((v - 1.0).abs() < 1e-9);
        assert!((w - 1.0).abs() < 1e-9);
    }

    #[test]
    fn project_point_outside_box_returns_none() {
        let d = Decal3D::new();
        assert!(d.project_point(Vector3::new(2.0, 0.0, 0.0)).is_none());
        assert!(d.project_point(Vector3::new(0.0, 5.0, 0.0)).is_none());
    }

    #[test]
    fn project_point_with_translated_transform() {
        let mut d = Decal3D::new();
        d.global_transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(10.0, 0.0, 0.0),
        };
        // Point at world (10, 0, 0) is at decal-local origin
        let (u, v, w) = d.project_point(Vector3::new(10.0, 0.0, 0.0)).unwrap();
        assert!((u - 0.5).abs() < 1e-9);
        assert!((v - 0.5).abs() < 1e-9);
        assert!((w - 0.5).abs() < 1e-9);

        // Point at world (0, 0, 0) is outside the translated box
        assert!(d.project_point(Vector3::ZERO).is_none());
    }

    #[test]
    fn vertical_fade_center_is_full() {
        let d = Decal3D::new(); // upper_fade=0.3, lower_fade=0.3
        let f = d.vertical_fade(0.5);
        assert!((f - 1.0).abs() < 1e-9);
    }

    #[test]
    fn vertical_fade_at_edges() {
        let mut d = Decal3D::new();
        d.upper_fade = 0.5;
        d.lower_fade = 0.5;

        // At w=0.0 (bottom edge), lower fade should be 0
        assert!(d.vertical_fade(0.0).abs() < 1e-9);

        // At w=1.0 (top edge), upper fade should be 0
        assert!(d.vertical_fade(1.0).abs() < 1e-9);

        // At w=0.25, lower fade = 0.25/0.5 = 0.5, upper is 1.0
        let f = d.vertical_fade(0.25);
        assert!((f - 0.5).abs() < 1e-9);
    }

    #[test]
    fn vertical_fade_zero_settings_means_no_fade() {
        let mut d = Decal3D::new();
        d.upper_fade = 0.0;
        d.lower_fade = 0.0;
        assert!((d.vertical_fade(0.0) - 1.0).abs() < 1e-9);
        assert!((d.vertical_fade(1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn distance_fade_disabled_returns_one() {
        let d = Decal3D::new(); // distance_fade_enabled = false
        assert!((d.distance_fade(1000.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn distance_fade_within_begin_returns_one() {
        let mut d = Decal3D::new();
        d.distance_fade_enabled = true;
        d.distance_fade_begin = 40.0;
        d.distance_fade_length = 10.0;
        assert!((d.distance_fade(30.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn distance_fade_beyond_range_returns_zero() {
        let mut d = Decal3D::new();
        d.distance_fade_enabled = true;
        d.distance_fade_begin = 40.0;
        d.distance_fade_length = 10.0;
        assert!((d.distance_fade(55.0)).abs() < 1e-9);
    }

    #[test]
    fn distance_fade_midpoint() {
        let mut d = Decal3D::new();
        d.distance_fade_enabled = true;
        d.distance_fade_begin = 40.0;
        d.distance_fade_length = 10.0;
        let f = d.distance_fade(45.0); // halfway => 0.5
        assert!((f - 0.5).abs() < 1e-9);
    }

    #[test]
    fn distance_fade_zero_length_returns_zero_beyond_begin() {
        let mut d = Decal3D::new();
        d.distance_fade_enabled = true;
        d.distance_fade_begin = 10.0;
        d.distance_fade_length = 0.0;
        assert!(d.distance_fade(10.0).abs() < 1e-9);
    }

    #[test]
    fn sample_alpha_inside_box() {
        let d = Decal3D::new();
        let alpha = d.sample_alpha(Vector3::ZERO, 5.0).unwrap();
        assert!(alpha > 0.0);
        assert!(alpha <= 1.0);
    }

    #[test]
    fn sample_alpha_outside_box_returns_none() {
        let d = Decal3D::new();
        assert!(d.sample_alpha(Vector3::new(5.0, 0.0, 0.0), 1.0).is_none());
    }

    #[test]
    fn sample_alpha_invisible_returns_none() {
        let mut d = Decal3D::new();
        d.visible = false;
        assert!(d.sample_alpha(Vector3::ZERO, 1.0).is_none());
    }

    #[test]
    fn overlaps_aabb_exact_overlap() {
        let d = Decal3D::new(); // size 2x2x2, identity transform
        // AABB centered at origin with half-extents 1,1,1 — same box
        assert!(d.overlaps_aabb(Vector3::ZERO, Vector3::ONE));
    }

    #[test]
    fn overlaps_aabb_no_overlap() {
        let d = Decal3D::new();
        // AABB centered far away
        assert!(!d.overlaps_aabb(Vector3::new(10.0, 0.0, 0.0), Vector3::new(0.5, 0.5, 0.5)));
    }

    #[test]
    fn overlaps_aabb_partial_overlap() {
        let d = Decal3D::new();
        // AABB centered at (1.5, 0, 0) with half-extent 1 → overlaps on X axis
        assert!(d.overlaps_aabb(Vector3::new(1.5, 0.0, 0.0), Vector3::ONE));
    }

    #[test]
    fn overlaps_aabb_with_translated_decal() {
        let mut d = Decal3D::new();
        d.global_transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(20.0, 0.0, 0.0),
        };
        // Near the decal
        assert!(d.overlaps_aabb(Vector3::new(20.0, 0.0, 0.0), Vector3::new(0.5, 0.5, 0.5)));
        // Far from the decal
        assert!(!d.overlaps_aabb(Vector3::ZERO, Vector3::ONE));
    }

    // -----------------------------------------------------------------------
    // Registry tests
    // -----------------------------------------------------------------------

    #[test]
    fn registry_add_and_get() {
        let mut reg = DecalRegistry::new();
        let h = reg.add(Decal3D::new());
        assert_eq!(reg.len(), 1);
        assert!(reg.get(h).is_some());
    }

    #[test]
    fn registry_remove() {
        let mut reg = DecalRegistry::new();
        let h = reg.add(Decal3D::new());
        assert!(reg.remove(h));
        assert_eq!(reg.len(), 0);
        assert!(!reg.remove(h)); // double-remove returns false
    }

    #[test]
    fn registry_get_mut() {
        let mut reg = DecalRegistry::new();
        let h = reg.add(Decal3D::new());
        reg.get_mut(h).unwrap().size = Vector3::new(10.0, 10.0, 10.0);
        assert_eq!(reg.get(h).unwrap().size.x, 10.0);
    }

    #[test]
    fn registry_query_overlapping() {
        let mut reg = DecalRegistry::new();
        let h1 = reg.add(Decal3D::new()); // at origin
        let mut far_decal = Decal3D::new();
        far_decal.global_transform.origin = Vector3::new(100.0, 0.0, 0.0);
        let _h2 = reg.add(far_decal);

        let hits = reg.query_overlapping(Vector3::ZERO, Vector3::new(0.5, 0.5, 0.5));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, h1);
    }

    #[test]
    fn registry_query_excludes_invisible() {
        let mut reg = DecalRegistry::new();
        let mut d = Decal3D::new();
        d.visible = false;
        reg.add(d);

        let hits = reg.query_overlapping(Vector3::ZERO, Vector3::ONE);
        assert!(hits.is_empty());
    }

    #[test]
    fn registry_is_empty_and_clear() {
        let mut reg = DecalRegistry::new();
        assert!(reg.is_empty());
        reg.add(Decal3D::new());
        assert!(!reg.is_empty());
        reg.clear();
        assert!(reg.is_empty());
    }

    #[test]
    fn registry_iter() {
        let mut reg = DecalRegistry::new();
        reg.add(Decal3D::new());
        reg.add(Decal3D::new());
        let ids: Vec<u64> = reg.iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn registry_unique_handles() {
        let mut reg = DecalRegistry::new();
        let h1 = reg.add(Decal3D::new());
        let h2 = reg.add(Decal3D::new());
        let h3 = reg.add(Decal3D::new());
        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
    }

    #[test]
    fn albedo_mix_affects_sample_alpha() {
        let mut d = Decal3D::new();
        d.upper_fade = 0.0;
        d.lower_fade = 0.0;
        d.albedo_mix = 0.5;
        let alpha = d.sample_alpha(Vector3::ZERO, 1.0).unwrap();
        assert!((alpha - 0.5).abs() < 1e-9);
    }

    #[test]
    fn emission_energy_default() {
        let d = Decal3D::new();
        assert!((d.emission_energy - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cull_mask_default_is_all_bits() {
        let d = Decal3D::new();
        assert_eq!(d.cull_mask, 0xFFFF_FFFF);
    }
}
