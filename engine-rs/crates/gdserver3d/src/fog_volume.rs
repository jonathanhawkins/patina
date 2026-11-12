//! FogVolume node for volumetric fog regions in 3D scenes.
//!
//! Implements Godot's `FogVolume` node which defines a region of volumetric
//! fog in 3D space. Each volume has a shape, size, and material controlling
//! the fog appearance within its bounds.

use gdcore::math::{Color, Vector3};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Texture3DVolume
// ---------------------------------------------------------------------------

/// CPU volume storage backing a `Texture3D` density map (pat-nzgdu.214).
///
/// A `Texture3D` is a stack of `depth` image layers; here each layer is a
/// `width * height` grid of scalar density samples (fog reads one density
/// channel). The runtime `Texture3D` *resource* is registered in ClassDB
/// (gdobject); this is the actual per-voxel data a `FogVolume` samples on the
/// CPU fog path. It lives in `gdserver3d` — not `gdrender2d` as the bead's
/// text suggested — because that is where `FogVolume` consumes it, and
/// `gdserver3d` neither does nor (being 3D) should depend on the 2D crate.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Texture3DVolume {
    width: usize,
    height: usize,
    /// One `width * height` layer per depth slice, front (`w = 0`) to back.
    layers: Vec<Vec<f32>>,
}

impl Texture3DVolume {
    /// Builds a volume from its depth layers. Each layer is normalised to
    /// `width * height` (short layers zero-padded, long ones truncated) so the
    /// sampler can never index out of bounds.
    pub fn from_layers(width: usize, height: usize, mut layers: Vec<Vec<f32>>) -> Self {
        let plane = width * height;
        for layer in &mut layers {
            layer.resize(plane, 0.0);
        }
        Self {
            width,
            height,
            layers,
        }
    }

    /// Number of depth layers.
    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    /// Layer width in samples.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Layer height in samples.
    pub fn height(&self) -> usize {
        self.height
    }

    /// `true` when there is no data to sample.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty() || self.width == 0 || self.height == 0
    }

    /// Nearest-neighbour sample at normalised coordinates `(u, v, w)`, each in
    /// `[0, 1]`. `w` selects the depth layer — the behaviour a `Texture3D` adds
    /// over a flat texture. Coordinates outside `[0, 1]` clamp to the edge.
    pub fn sample(&self, u: f32, v: f32, w: f32) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let z = nearest_index(w, self.layers.len());
        let x = nearest_index(u, self.width);
        let y = nearest_index(v, self.height);
        self.layers[z][y * self.width + x]
    }
}

/// Maps a normalised coordinate in `[0, 1]` to a nearest cell index in
/// `[0, len)`, clamping outside the range.
fn nearest_index(t: f32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    ((t.clamp(0.0, 1.0) * len as f32).floor() as usize).min(len - 1)
}

// ---------------------------------------------------------------------------
// FogVolumeShape
// ---------------------------------------------------------------------------

/// Shape of the fog volume region.
///
/// Maps to Godot's `RenderingServer.FogVolumeShape` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FogVolumeShape {
    /// Ellipsoid fog region (default).
    #[default]
    Ellipsoid,
    /// Cone fog region.
    Cone,
    /// Cylinder fog region.
    Cylinder,
    /// Axis-aligned box fog region.
    Box,
    /// World-spanning fog (ignores size).
    World,
}

impl FogVolumeShape {
    /// Returns the integer value matching Godot's enum ordering.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Ellipsoid => 0,
            Self::Cone => 1,
            Self::Cylinder => 2,
            Self::Box => 3,
            Self::World => 4,
        }
    }

    /// Creates from Godot's integer enum value.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Ellipsoid,
            1 => Self::Cone,
            2 => Self::Cylinder,
            3 => Self::Box,
            4 => Self::World,
            _ => Self::Ellipsoid,
        }
    }
}

// ---------------------------------------------------------------------------
// FogMaterial
// ---------------------------------------------------------------------------

/// Material controlling the appearance of fog within a FogVolume.
///
/// Maps to Godot's `FogMaterial` resource.
#[derive(Debug, Clone, PartialEq)]
pub struct FogMaterial {
    /// Fog density within this volume. Higher = thicker fog.
    pub density: f32,
    /// Base color of the fog.
    pub albedo: Color,
    /// Emission color (self-illumination of the fog).
    pub emission: Color,
    /// Height falloff factor. Higher = fog fades faster with altitude.
    pub height_falloff: f32,
    /// Edge fade distance. Controls how sharply the fog boundary transitions.
    pub edge_fade: f32,
    /// Density texture (path). Empty means uniform density.
    pub density_texture: String,
    /// pat-nzgdu.214: the resolved `Texture3D` volume data for `density_texture`,
    /// when a loader has resolved the path. Transient — not (de)serialised (the
    /// path in `density_texture` is what round-trips); when present it modulates
    /// the base density per voxel.
    pub density_volume: Option<Texture3DVolume>,
}

impl Default for FogMaterial {
    fn default() -> Self {
        Self {
            density: 1.0,
            albedo: Color::WHITE,
            emission: Color::new(0.0, 0.0, 0.0, 1.0),
            height_falloff: 0.0,
            edge_fade: 0.1,
            density_texture: String::new(),
            density_volume: None,
        }
    }
}

impl FogMaterial {
    /// Builds a material from ClassDB/`.tscn` property pairs (pat-nzgdu.208).
    ///
    /// This is the `set` half of the ClassDB binding. `gdobject` (ClassDB) and
    /// `gdserver3d` deliberately do not depend on each other, so the two meet at
    /// `gdvariant::Variant` — the same shape `ProceduralSkyMaterial` uses
    /// (pat-nzgdu.161), which is how the editor drives sky materials out of a
    /// parsed `.tscn`.
    ///
    /// Unknown keys and type mismatches are ignored, leaving the default: a
    /// `.tscn` only serialises properties that differ from the default, so an
    /// absent key means "default", never "zero".
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut mat = Self::default();
        for (key, value) in props {
            match key {
                "density" => {
                    if let Variant::Float(f) = value {
                        mat.density = *f as f32;
                    }
                }
                "albedo" => {
                    if let Variant::Color(c) = value {
                        mat.albedo = *c;
                    }
                }
                "emission" => {
                    if let Variant::Color(c) = value {
                        mat.emission = *c;
                    }
                }
                "height_falloff" => {
                    if let Variant::Float(f) = value {
                        mat.height_falloff = *f as f32;
                    }
                }
                "edge_fade" => {
                    if let Variant::Float(f) = value {
                        mat.edge_fade = *f as f32;
                    }
                }
                // The Texture3D itself is resolved by a loader; here we keep the
                // resource-ref string, exactly as `sky_cover`/`panorama` do.
                "density_texture" => {
                    if let Variant::String(s) = value {
                        mat.density_texture = s.clone();
                    }
                }
                _ => {}
            }
        }
        mat
    }

    /// Serialises back to property pairs (pat-nzgdu.208) — the `get` half.
    ///
    /// Only properties that differ from the default are emitted, mirroring
    /// Godot's `.tscn` convention (and `ProceduralSkyMaterial::to_properties`).
    /// A default material therefore yields an empty list, which is what makes
    /// `from_properties(to_properties(m))` round-trip.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();
        if (self.density - def.density).abs() > f32::EPSILON {
            props.push(("density".into(), Variant::Float(self.density as f64)));
        }
        if self.albedo != def.albedo {
            props.push(("albedo".into(), Variant::Color(self.albedo)));
        }
        if self.emission != def.emission {
            props.push(("emission".into(), Variant::Color(self.emission)));
        }
        if (self.height_falloff - def.height_falloff).abs() > f32::EPSILON {
            props.push((
                "height_falloff".into(),
                Variant::Float(self.height_falloff as f64),
            ));
        }
        if (self.edge_fade - def.edge_fade).abs() > f32::EPSILON {
            props.push(("edge_fade".into(), Variant::Float(self.edge_fade as f64)));
        }
        if self.density_texture != def.density_texture {
            props.push((
                "density_texture".into(),
                Variant::String(self.density_texture.clone()),
            ));
        }
        props
    }
}

// ---------------------------------------------------------------------------
// FogVolume
// ---------------------------------------------------------------------------

/// A 3D fog volume node defining a region of volumetric fog.
///
/// FogVolume works with the Environment's volumetric fog system to create
/// localized fog effects. The volume's transform determines its position
/// and orientation in the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct FogVolume {
    /// Shape of the fog region.
    pub shape: FogVolumeShape,
    /// Size of the volume in local space (width, height, depth).
    /// For World shape, this is ignored.
    pub size: Vector3,
    /// Material controlling fog appearance.
    pub material: FogMaterial,
}

impl Default for FogVolume {
    fn default() -> Self {
        Self {
            // ⚠️ BOX, not `FogVolumeShape::default()` (pat-nzgdu.208).
            //
            // The enum's zero value is Ellipsoid — that matches
            // `RS::FOG_VOLUME_SHAPE_ELLIPSOID = 0` and is correct — but a
            // *node* does not default to the enum's zero: Godot's constructor
            // calls `fog_volume_set_shape(volume, RS::FOG_VOLUME_SHAPE_BOX)`
            // (scene/3d/fog_volume.cpp:139), and the doc XML records
            // `shape` default `3`.
            //
            // This matters because a `.tscn` omits properties equal to Godot's
            // default, so a stock FogVolume node carries no `shape` key at all
            // and lands here — rendering an ellipsoid where Godot renders a box.
            shape: FogVolumeShape::Box,
            size: Vector3::new(2.0, 2.0, 2.0),
            material: FogMaterial::default(),
        }
    }
}

impl FogVolume {
    /// Creates a new FogVolume with the given shape and size.
    pub fn new(shape: FogVolumeShape, size: Vector3) -> Self {
        Self {
            shape,
            size,
            material: FogMaterial::default(),
        }
    }

    /// Creates a world-spanning fog volume (size is ignored).
    pub fn world() -> Self {
        Self {
            shape: FogVolumeShape::World,
            size: Vector3::ZERO,
            material: FogMaterial::default(),
        }
    }

    /// Creates a box-shaped fog volume.
    pub fn box_shape(size: Vector3) -> Self {
        Self::new(FogVolumeShape::Box, size)
    }

    /// Creates an ellipsoid fog volume.
    pub fn ellipsoid(size: Vector3) -> Self {
        Self::new(FogVolumeShape::Ellipsoid, size)
    }

    /// Creates a cylinder fog volume.
    pub fn cylinder(radius: f32, height: f32) -> Self {
        Self::new(
            FogVolumeShape::Cylinder,
            Vector3::new(radius * 2.0, height, radius * 2.0),
        )
    }

    /// Creates a cone fog volume.
    pub fn cone(radius: f32, height: f32) -> Self {
        Self::new(
            FogVolumeShape::Cone,
            Vector3::new(radius * 2.0, height, radius * 2.0),
        )
    }

    /// Tests whether a local-space point is inside this fog volume.
    ///
    /// The point should be in the volume's local coordinate space
    /// (pre-transform). For World shape, always returns true.
    pub fn contains_point(&self, local_point: Vector3) -> bool {
        match self.shape {
            FogVolumeShape::World => true,
            FogVolumeShape::Box => {
                let half = self.size * 0.5;
                local_point.x.abs() <= half.x
                    && local_point.y.abs() <= half.y
                    && local_point.z.abs() <= half.z
            }
            FogVolumeShape::Ellipsoid => {
                let half = self.size * 0.5;
                if half.x < 1e-6 || half.y < 1e-6 || half.z < 1e-6 {
                    return false;
                }
                let nx = local_point.x / half.x;
                let ny = local_point.y / half.y;
                let nz = local_point.z / half.z;
                nx * nx + ny * ny + nz * nz <= 1.0
            }
            FogVolumeShape::Cylinder => {
                let half = self.size * 0.5;
                if half.x < 1e-6 || half.y < 1e-6 {
                    return false;
                }
                let radius = half.x; // x and z define the radius
                let nx = local_point.x / radius;
                let nz = local_point.z / radius;
                nx * nx + nz * nz <= 1.0 && local_point.y.abs() <= half.y
            }
            FogVolumeShape::Cone => {
                let half = self.size * 0.5;
                if half.y < 1e-6 || half.x < 1e-6 {
                    return false;
                }
                // Cone narrows from bottom (y = -half.y) to top (y = half.y)
                let t = (local_point.y + half.y) / self.size.y; // 0 at bottom, 1 at top
                if !(0.0..=1.0).contains(&t) {
                    return false;
                }
                let cone_radius = half.x * (1.0 - t);
                if cone_radius < 1e-6 {
                    return false;
                }
                let nx = local_point.x / cone_radius;
                let nz = local_point.z / cone_radius;
                nx * nx + nz * nz <= 1.0
            }
        }
    }

    /// Samples the fog density at a local-space point.
    ///
    /// Returns 0.0 if the point is outside the volume. Otherwise returns
    /// the material density, optionally modulated by height falloff and
    /// edge fade.
    pub fn sample_density(&self, local_point: Vector3) -> f32 {
        if !self.contains_point(local_point) {
            return 0.0;
        }

        let mut density = self.material.density;

        // Apply height falloff (exponential decay with altitude)
        if self.material.height_falloff > 0.0 {
            let half_y = self.size.y * 0.5;
            let normalized_height = if half_y > 1e-6 {
                ((local_point.y + half_y) / self.size.y).clamp(0.0, 1.0)
            } else {
                0.5
            };
            density *= (-self.material.height_falloff * normalized_height).exp();
        }

        // Apply edge fade
        if self.material.edge_fade > 0.0 && self.shape != FogVolumeShape::World {
            let edge_factor = self.edge_distance(local_point);
            let fade = (edge_factor / self.material.edge_fade).clamp(0.0, 1.0);
            density *= fade;
        }

        // pat-nzgdu.214: a resolved Texture3D density map modulates the base
        // density by its per-voxel value; the `w` (depth) coordinate selects the
        // correct layer. The local point spans [-size/2, size/2] per axis (see
        // `contains_point`), so it maps to [0, 1] as `(p + size/2) / size`.
        if let Some(vol) = &self.material.density_volume {
            if !vol.is_empty() {
                let half = self.size * 0.5;
                let norm = |p: f32, half: f32, size: f32| {
                    if size.abs() > 1e-6 {
                        (p + half) / size
                    } else {
                        0.5
                    }
                };
                let u = norm(local_point.x, half.x, self.size.x);
                let v = norm(local_point.y, half.y, self.size.y);
                let w = norm(local_point.z, half.z, self.size.z);
                density *= vol.sample(u, v, w);
            }
        }

        density
    }

    /// Returns the approximate distance from a local-space point to the
    /// nearest edge of the volume. Used for edge fade calculations.
    fn edge_distance(&self, local_point: Vector3) -> f32 {
        match self.shape {
            FogVolumeShape::World => f32::MAX,
            FogVolumeShape::Box => {
                let half = self.size * 0.5;
                let dx = half.x - local_point.x.abs();
                let dy = half.y - local_point.y.abs();
                let dz = half.z - local_point.z.abs();
                dx.min(dy).min(dz).max(0.0)
            }
            FogVolumeShape::Ellipsoid => {
                let half = self.size * 0.5;
                if half.x < 1e-6 || half.y < 1e-6 || half.z < 1e-6 {
                    return 0.0;
                }
                let nx = local_point.x / half.x;
                let ny = local_point.y / half.y;
                let nz = local_point.z / half.z;
                let r = (nx * nx + ny * ny + nz * nz).sqrt();
                if r < 1e-6 {
                    return half.x.min(half.y).min(half.z);
                }
                (1.0 - r) * half.x.min(half.y).min(half.z)
            }
            FogVolumeShape::Cylinder | FogVolumeShape::Cone => {
                let half = self.size * 0.5;
                let dy = half.y - local_point.y.abs();
                let radius = half.x;
                let dr =
                    radius - (local_point.x * local_point.x + local_point.z * local_point.z).sqrt();
                dy.min(dr).max(0.0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // -- FogVolumeShape ----------------------------------------------------

    #[test]
    fn shape_default_is_ellipsoid() {
        assert_eq!(FogVolumeShape::default(), FogVolumeShape::Ellipsoid);
    }

    #[test]
    fn shape_godot_int_roundtrip() {
        for shape in [
            FogVolumeShape::Ellipsoid,
            FogVolumeShape::Cone,
            FogVolumeShape::Cylinder,
            FogVolumeShape::Box,
            FogVolumeShape::World,
        ] {
            assert_eq!(FogVolumeShape::from_godot_int(shape.to_godot_int()), shape);
        }
    }

    #[test]
    fn shape_unknown_int_defaults_ellipsoid() {
        assert_eq!(
            FogVolumeShape::from_godot_int(99),
            FogVolumeShape::Ellipsoid
        );
    }

    // -- FogMaterial -------------------------------------------------------

    #[test]
    fn material_defaults() {
        let m = FogMaterial::default();
        assert!(approx(m.density, 1.0));
        assert_eq!(m.albedo, Color::WHITE);
        assert!(approx(m.height_falloff, 0.0));
        assert!(approx(m.edge_fade, 0.1));
        assert!(m.density_texture.is_empty());
    }

    // -- FogVolume constructors -------------------------------------------

    /// ⚠️ A FogVolume defaults to BOX (fog_volume.cpp:139, doc XML `default="3"`),
    /// even though the *enum's* zero value is Ellipsoid (see
    /// `shape_default_is_ellipsoid`, which is a different question and still
    /// correct). Both facts are true at once; conflating them is what made a
    /// stock `.tscn` fog volume render as an ellipsoid (pat-nzgdu.208).
    #[test]
    fn default_volume() {
        let v = FogVolume::default();
        assert_eq!(
            v.shape,
            FogVolumeShape::Box,
            "Godot's FogVolume ctor sets FOG_VOLUME_SHAPE_BOX"
        );
        assert_ne!(
            v.shape,
            FogVolumeShape::default(),
            "the node default deliberately differs from the enum's zero value"
        );
        assert!(approx(v.size.x, 2.0));
        assert!(approx(v.size.y, 2.0));
        assert!(approx(v.size.z, 2.0));
    }

    #[test]
    fn box_constructor() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 6.0, 8.0));
        assert_eq!(v.shape, FogVolumeShape::Box);
        assert!(approx(v.size.x, 4.0));
        assert!(approx(v.size.y, 6.0));
    }

    #[test]
    fn ellipsoid_constructor() {
        let v = FogVolume::ellipsoid(Vector3::new(3.0, 3.0, 3.0));
        assert_eq!(v.shape, FogVolumeShape::Ellipsoid);
    }

    #[test]
    fn cylinder_constructor() {
        let v = FogVolume::cylinder(2.0, 5.0);
        assert_eq!(v.shape, FogVolumeShape::Cylinder);
        assert!(approx(v.size.x, 4.0)); // diameter = 2 * radius
        assert!(approx(v.size.y, 5.0));
    }

    #[test]
    fn cone_constructor() {
        let v = FogVolume::cone(3.0, 10.0);
        assert_eq!(v.shape, FogVolumeShape::Cone);
        assert!(approx(v.size.x, 6.0));
        assert!(approx(v.size.y, 10.0));
    }

    #[test]
    fn world_constructor() {
        let v = FogVolume::world();
        assert_eq!(v.shape, FogVolumeShape::World);
    }

    // -- contains_point: Box -----------------------------------------------

    #[test]
    fn box_contains_origin() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::ZERO));
    }

    #[test]
    fn box_contains_interior() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::new(1.0, 1.0, 1.0)));
    }

    #[test]
    fn box_excludes_outside() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(!v.contains_point(Vector3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn box_edge_is_inside() {
        let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::new(2.0, 0.0, 0.0)));
    }

    // -- contains_point: Ellipsoid -----------------------------------------

    #[test]
    fn ellipsoid_contains_origin() {
        let v = FogVolume::ellipsoid(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::ZERO));
    }

    #[test]
    fn ellipsoid_excludes_outside() {
        let v = FogVolume::ellipsoid(Vector3::new(2.0, 2.0, 2.0));
        assert!(!v.contains_point(Vector3::new(1.5, 0.0, 0.0)));
    }

    #[test]
    fn ellipsoid_contains_near_surface() {
        let v = FogVolume::ellipsoid(Vector3::new(4.0, 4.0, 4.0));
        assert!(v.contains_point(Vector3::new(1.9, 0.0, 0.0)));
    }

    // -- contains_point: World ---------------------------------------------

    #[test]
    fn world_contains_everything() {
        let v = FogVolume::world();
        assert!(v.contains_point(Vector3::ZERO));
        assert!(v.contains_point(Vector3::new(1e6, 1e6, 1e6)));
        assert!(v.contains_point(Vector3::new(-1e6, -1e6, -1e6)));
    }

    // -- contains_point: Cylinder ------------------------------------------

    #[test]
    fn cylinder_contains_center() {
        let v = FogVolume::cylinder(2.0, 4.0);
        assert!(v.contains_point(Vector3::ZERO));
    }

    #[test]
    fn cylinder_excludes_outside_radius() {
        let v = FogVolume::cylinder(2.0, 4.0);
        assert!(!v.contains_point(Vector3::new(2.5, 0.0, 0.0)));
    }

    #[test]
    fn cylinder_excludes_above_height() {
        let v = FogVolume::cylinder(2.0, 4.0);
        assert!(!v.contains_point(Vector3::new(0.0, 3.0, 0.0)));
    }

    // -- contains_point: Cone ----------------------------------------------

    #[test]
    fn cone_contains_bottom_center() {
        let v = FogVolume::cone(3.0, 10.0);
        // Bottom is at y = -half.y = -5
        assert!(v.contains_point(Vector3::new(0.0, -4.9, 0.0)));
    }

    #[test]
    fn cone_excludes_top_edge() {
        let v = FogVolume::cone(3.0, 10.0);
        // At top (y = +5), radius should be ~0
        assert!(!v.contains_point(Vector3::new(1.0, 4.9, 0.0)));
    }

    #[test]
    fn cone_contains_wide_bottom() {
        let v = FogVolume::cone(3.0, 10.0);
        // At bottom (y = -5), radius = 3.0
        assert!(v.contains_point(Vector3::new(2.5, -4.5, 0.0)));
    }

    // -- sample_density ----------------------------------------------------

    #[test]
    fn density_zero_outside() {
        let v = FogVolume::box_shape(Vector3::new(2.0, 2.0, 2.0));
        assert!(approx(v.sample_density(Vector3::new(5.0, 0.0, 0.0)), 0.0));
    }

    #[test]
    fn density_matches_material_inside() {
        let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        v.material.density = 0.5;
        v.material.height_falloff = 0.0;
        v.material.edge_fade = 0.0;
        let d = v.sample_density(Vector3::ZERO);
        assert!(approx(d, 0.5), "Expected 0.5, got {d}");
    }

    #[test]
    fn density_with_height_falloff() {
        let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        v.material.density = 1.0;
        v.material.height_falloff = 2.0;
        v.material.edge_fade = 0.0;
        let bottom = v.sample_density(Vector3::new(0.0, -1.9, 0.0));
        let top = v.sample_density(Vector3::new(0.0, 1.9, 0.0));
        assert!(
            bottom > top,
            "Fog should be denser at bottom: bottom={bottom}, top={top}"
        );
    }

    #[test]
    fn density_world_always_material_density() {
        let mut v = FogVolume::world();
        v.material.density = 0.3;
        v.material.height_falloff = 0.0;
        let d = v.sample_density(Vector3::new(100.0, 200.0, 300.0));
        assert!(approx(d, 0.3));
    }

    #[test]
    fn edge_fade_reduces_density_near_boundary() {
        let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        v.material.density = 1.0;
        v.material.height_falloff = 0.0;
        v.material.edge_fade = 0.5;
        let center = v.sample_density(Vector3::ZERO);
        let near_edge = v.sample_density(Vector3::new(1.9, 0.0, 0.0));
        assert!(
            near_edge < center,
            "Edge fade should reduce density near boundary: center={center}, edge={near_edge}"
        );
    }

    // -- from_properties / to_properties (pat-nzgdu.208) --------------------

    #[test]
    fn from_properties_maps_every_member() {
        let density = Variant::Float(4.5);
        let albedo = Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0));
        let emission = Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0));
        let height_falloff = Variant::Float(2.0);
        let edge_fade = Variant::Float(0.75);
        let density_texture = Variant::String("res://fog.png".to_string());

        let mat = FogMaterial::from_properties(
            [
                ("density", &density),
                ("albedo", &albedo),
                ("emission", &emission),
                ("height_falloff", &height_falloff),
                ("edge_fade", &edge_fade),
                ("density_texture", &density_texture),
            ]
            .into_iter(),
        );

        assert!(approx(mat.density, 4.5));
        assert_eq!(mat.albedo, Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(mat.emission, Color::new(0.0, 1.0, 0.0, 1.0));
        assert!(approx(mat.height_falloff, 2.0));
        assert!(approx(mat.edge_fade, 0.75));
        assert_eq!(mat.density_texture, "res://fog.png");
    }

    /// ⚠️ An absent key means "default", NOT "zero". A `.tscn` only serialises
    /// properties that differ from the default, so a material parsed from a
    /// scene that never mentions `density` must still be dense — zeroing it
    /// would silently render no fog at all.
    #[test]
    fn from_properties_empty_gives_default_not_zero() {
        let mat = FogMaterial::from_properties(std::iter::empty());
        assert_eq!(mat, FogMaterial::default());
        assert!(
            approx(mat.density, 1.0),
            "density must default to 1.0, not 0"
        );
        assert!(approx(mat.edge_fade, 0.1));
    }

    #[test]
    fn from_properties_ignores_unknown_keys_and_wrong_types() {
        // A wrong-typed value must leave the default rather than panic: the
        // property bag comes from a parsed scene and is not trusted to be
        // well-typed.
        let wrong = Variant::String("not a float".to_string());
        let unknown = Variant::Float(9.0);
        let mat = FogMaterial::from_properties(
            [("density", &wrong), ("no_such_property", &unknown)].into_iter(),
        );
        assert_eq!(mat, FogMaterial::default());
    }

    /// Mirrors Godot's `.tscn` convention: only non-defaults are written.
    #[test]
    fn to_properties_emits_only_non_defaults() {
        assert!(
            FogMaterial::default().to_properties().is_empty(),
            "a default material serialises to nothing"
        );

        let mut mat = FogMaterial::default();
        mat.density = 3.0;
        let props = mat.to_properties();
        assert_eq!(props.len(), 1, "only the changed property is emitted");
        assert_eq!(props[0].0, "density");
        assert_eq!(props[0].1, Variant::Float(3.0));
    }

    #[test]
    fn properties_round_trip_through_the_binding() {
        let mut mat = FogMaterial::default();
        mat.density = 2.5;
        mat.albedo = Color::new(0.25, 0.5, 0.75, 1.0);
        mat.emission = Color::new(0.1, 0.2, 0.3, 1.0);
        mat.height_falloff = 1.5;
        mat.edge_fade = 0.9;
        mat.density_texture = "res://noise.png".to_string();

        let props = mat.to_properties();
        let restored = FogMaterial::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
        assert_eq!(restored, mat, "from_properties(to_properties(m)) must be m");
    }

    /// The binding is only worth anything if the values reach the render math.
    #[test]
    fn density_set_through_properties_changes_sampled_density() {
        let dense = Variant::Float(5.0);
        let mut volume = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
        volume.material.edge_fade = 0.0;
        let before = volume.sample_density(Vector3::ZERO);

        volume.material = FogMaterial::from_properties([("density", &dense)].into_iter());
        volume.material.edge_fade = 0.0;
        let after = volume.sample_density(Vector3::ZERO);

        assert!(approx(before, 1.0), "default density samples 1.0");
        assert!(
            approx(after, 5.0),
            "a density set through the property binding must reach sample_density, got {after}"
        );
    }
}
