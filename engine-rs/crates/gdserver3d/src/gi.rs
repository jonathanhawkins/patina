//! Global Illumination stub nodes: VoxelGI and LightmapGI.
//!
//! These are stub types for Godot's GI nodes. Full voxel-cone-tracing and
//! lightmap baking are out of scope for the initial port; these types carry
//! the configuration surface so that scenes using VoxelGI or LightmapGI can
//! be loaded, saved, and round-tripped without data loss.
//!
//! What "stub" means here, precisely (pat-u3l8c.41) — do not read these types
//! as global illumination:
//!
//! - [`VoxelGI::bake`] never voxelizes anything. It takes lights and no
//!   geometry, so it cannot bounce, occlude, or bleed colour between
//!   surfaces. It samples DIRECT irradiance on a coarse grid and scales it by
//!   `propagation`. Godot's VoxelGI voxelizes the scene and cone-traces
//!   diffuse bounce; this does not, and no test here claims otherwise.
//! - **SDFGI is not implemented at all** — there is no `Sdfgi` type, no
//!   cascade storage, and no runtime path. A scene requesting SDFGI silently
//!   gets no GI rather than an approximation.
//!
//! Closing either gap is 3D-render scope, which is deferred for the v1
//! milestone (see `prd/PARITY_CODE_AUDIT_2026-07-11.md` "VoxelGI / SDFGI" and
//! `prd/PORT_1TO1_SCORECARD_2026-07-11.md`).

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Aabb, Transform3D};

use crate::light::{Light3D, LightType};
use crate::occluder::OccluderInstance3D;

// ===========================================================================
// VoxelGI
// ===========================================================================

/// Unique identifier for a VoxelGI instance in the rendering server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoxelGIId(pub u64);

/// Quality preset for VoxelGI.
///
/// Maps to Godot's `VoxelGI.Subdiv` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoxelGISubdiv {
    /// 64×64×64 voxel grid.
    Subdiv64 = 0,
    /// 128×128×128 voxel grid.
    #[default]
    Subdiv128 = 1,
    /// 256×256×256 voxel grid.
    Subdiv256 = 2,
    /// 512×512×512 voxel grid.
    Subdiv512 = 3,
}

/// Stub for Godot's VoxelGI node.
///
/// VoxelGI provides real-time global illumination via voxel cone tracing.
/// This stub stores the configuration but does not perform actual baking.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxelGI {
    /// Unique identifier.
    pub id: VoxelGIId,
    /// World-space transform.
    pub transform: Transform3D,
    /// Half-extents of the voxel grid volume.
    pub size: Vector3,
    /// Voxel grid resolution.
    pub subdiv: VoxelGISubdiv,
    /// Camera attributes path (if any).
    pub camera_attributes_path: Option<String>,
    /// Energy multiplier for indirect light.
    pub energy: f32,
    /// Bias to reduce light leaking through thin surfaces.
    pub bias: f32,
    /// Normal bias to reduce self-illumination artifacts.
    pub normal_bias: f32,
    /// Propagation factor controlling how far light bounces spread.
    pub propagation: f32,
    /// Whether interior mode is enabled (no sky contribution).
    pub interior: bool,
    /// Whether the VoxelGI probe data has been baked.
    pub baked: bool,
    /// Cached aggregate indirect-light contribution produced by the most
    /// recent [`VoxelGI::bake`] call (volume average).
    pub baked_indirect: Color,
    /// Coarse spatial grid (N³ = 64 cells) for position-varying samples
    /// (pat-p0s82.9). Empty until bake.
    pub grid: Vec<Color>,
    /// Grid resolution per axis (fixed 4 for the portable slice).
    pub grid_res: u32,
}

impl VoxelGI {
    /// Creates a new VoxelGI with Godot-compatible defaults.
    pub fn new(id: VoxelGIId) -> Self {
        Self {
            id,
            transform: Transform3D::IDENTITY,
            size: Vector3::new(20.0, 20.0, 20.0),
            subdiv: VoxelGISubdiv::default(),
            camera_attributes_path: None,
            energy: 1.0,
            bias: 1.5,
            normal_bias: 0.0,
            propagation: 0.7,
            interior: false,
            baked: false,
            baked_indirect: Color::BLACK,
            grid: Vec::new(),
            grid_res: 4,
        }
    }

    /// Fills the volume's coarse grid with DIRECT irradiance from `lights`,
    /// scaled by `propagation`.
    ///
    /// This is not indirect light and not Godot's VoxelGI bake, despite the
    /// name it mirrors. It takes no geometry, so nothing is voxelized, nothing
    /// occludes anything, and no light bounces: a lit wall will not bleed
    /// colour onto adjacent surfaces. Each cell centre accumulates
    /// `color * energy * attenuation` per light (inverse-square for
    /// point/spot, unattenuated for directional, skipped when `interior`), and
    /// the whole grid is multiplied by `propagation` to stand in for a bounce.
    ///
    /// The result is a cheap, spatially varying ambient term — enough for
    /// `sample` to differ across the volume, and enough to round-trip the
    /// configuration surface. See the module docs for what real parity needs.
    pub fn bake(&mut self, lights: &[Light3D]) {
        let n = self.grid_res as usize;
        self.grid = vec![Color::BLACK; n * n * n];
        let (min, max) = self.world_aabb_min_max();
        let prop = self.propagation;
        let mut avg = Color::BLACK;
        let mut count = 0.0f32;
        for iz in 0..n {
            for iy in 0..n {
                for ix in 0..n {
                    let fx = (ix as f32 + 0.5) / n as f32;
                    let fy = (iy as f32 + 0.5) / n as f32;
                    let fz = (iz as f32 + 0.5) / n as f32;
                    let cell = Vector3::new(
                        min.x + (max.x - min.x) * fx,
                        min.y + (max.y - min.y) * fy,
                        min.z + (max.z - min.z) * fz,
                    );
                    let mut r = 0.0f32;
                    let mut g = 0.0f32;
                    let mut b = 0.0f32;
                    for light in lights {
                        let attenuation = match light.light_type {
                            LightType::Directional => {
                                if self.interior {
                                    continue;
                                }
                                1.0
                            }
                            LightType::Point | LightType::Spot => {
                                let d = (light.position - cell).length().max(1.0);
                                1.0 / (d * d)
                            }
                        };
                        r += light.color.r * light.energy * attenuation;
                        g += light.color.g * light.energy * attenuation;
                        b += light.color.b * light.energy * attenuation;
                    }
                    let c = Color::new(r * prop, g * prop, b * prop, 1.0);
                    self.grid[iz * n * n + iy * n + ix] = c;
                    avg.r += c.r;
                    avg.g += c.g;
                    avg.b += c.b;
                    count += 1.0;
                }
            }
        }
        if count > 0.0 {
            self.baked_indirect = Color::new(avg.r / count, avg.g / count, avg.b / count, 1.0);
        }
        self.baked = true;
    }

    /// Samples the baked ambient term at `world_pos`, scaled by `energy`.
    ///
    /// Returns `BLACK` when unbaked or outside the volume. The value comes
    /// from the coarse grid [`VoxelGI::bake`] filled with direct irradiance —
    /// it is not cone-traced indirect light. See the module docs.
    pub fn sample(&self, world_pos: Vector3) -> Color {
        if !self.baked || !self.contains_point(world_pos) {
            return Color::BLACK;
        }
        let n = self.grid_res as usize;
        if self.grid.len() != n * n * n {
            return Color::new(
                self.baked_indirect.r * self.energy,
                self.baked_indirect.g * self.energy,
                self.baked_indirect.b * self.energy,
                1.0,
            );
        }
        let (min, max) = self.world_aabb_min_max();
        let fx = ((world_pos.x - min.x) / (max.x - min.x).max(1e-6)).clamp(0.0, 0.999);
        let fy = ((world_pos.y - min.y) / (max.y - min.y).max(1e-6)).clamp(0.0, 0.999);
        let fz = ((world_pos.z - min.z) / (max.z - min.z).max(1e-6)).clamp(0.0, 0.999);
        let ix = (fx * n as f32) as usize;
        let iy = (fy * n as f32) as usize;
        let iz = (fz * n as f32) as usize;
        let c = self.grid[iz * n * n + iy * n + ix];
        Color::new(c.r * self.energy, c.g * self.energy, c.b * self.energy, 1.0)
    }

    /// Returns the axis-aligned bounding box of the VoxelGI volume in world space.
    pub fn world_aabb_min_max(&self) -> (Vector3, Vector3) {
        let center = self.transform.origin;
        let half = self.size * 0.5;
        (center - half, center + half)
    }

    /// Returns `true` if the given world-space point is inside the GI volume.
    pub fn contains_point(&self, point: Vector3) -> bool {
        let (min, max) = self.world_aabb_min_max();
        point.x >= min.x
            && point.x <= max.x
            && point.y >= min.y
            && point.y <= max.y
            && point.z >= min.z
            && point.z <= max.z
    }

    /// Returns the number of voxels per axis for the current subdivision.
    pub fn grid_resolution(&self) -> u32 {
        match self.subdiv {
            VoxelGISubdiv::Subdiv64 => 64,
            VoxelGISubdiv::Subdiv128 => 128,
            VoxelGISubdiv::Subdiv256 => 256,
            VoxelGISubdiv::Subdiv512 => 512,
        }
    }
}

// ===========================================================================
// LightmapGI
// ===========================================================================

/// Unique identifier for a LightmapGI instance in the rendering server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LightmapGIId(pub u64);

/// Bake quality preset for LightmapGI.
///
/// Maps to Godot's `LightmapGI.BakeQuality` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightmapBakeQuality {
    /// Fast bake with lower sample count.
    Low = 0,
    /// Balanced quality and speed.
    #[default]
    Medium = 1,
    /// High quality with more samples.
    High = 2,
    /// Maximum quality, very slow.
    Ultra = 3,
}

/// Lightmap generation mode for LightmapGI.
///
/// Maps to Godot's `LightmapGI.GenerateProbes` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightmapProbeGeneration {
    /// Do not generate light probes automatically.
    #[default]
    Disabled = 0,
    /// Generate probes at low density.
    Low = 1,
    /// Generate probes at medium density.
    Medium = 2,
    /// Generate probes at high density.
    High = 3,
}

/// Stub for Godot's LightmapGI node.
///
/// LightmapGI provides baked global illumination using lightmaps.
/// This stub stores the configuration but does not perform actual baking.
#[derive(Debug, Clone, PartialEq)]
pub struct LightmapGI {
    /// Unique identifier.
    pub id: LightmapGIId,
    /// World-space transform.
    pub transform: Transform3D,
    /// Bake quality preset.
    pub quality: LightmapBakeQuality,
    /// Number of light bounces during baking.
    pub bounces: u32,
    /// Lightmap texel density (texels per unit).
    pub texel_scale: f32,
    /// Whether to use denoiser on the baked result.
    pub use_denoiser: bool,
    /// Denoiser strength (0.0 = none, 1.0 = full).
    pub denoiser_strength: f32,
    /// Whether directional lightmaps are generated.
    pub directional: bool,
    /// Whether interior mode is enabled (no sky contribution).
    pub interior: bool,
    /// Energy multiplier for the baked indirect light.
    pub energy: f32,
    /// Bias to reduce shadow acne in baked lightmaps.
    pub bias: f32,
    /// Maximum texture size for the lightmap atlas.
    pub max_texture_size: u32,
    /// Automatic probe generation mode.
    pub generate_probes: LightmapProbeGeneration,
    /// Camera attributes path (if any).
    pub camera_attributes_path: Option<String>,
    /// Whether lightmap data has been baked.
    pub baked: bool,
    /// Path to baked lightmap data resource (if baked).
    pub light_data_path: Option<String>,
    /// Baked sample grid (pat-p0s82.10).
    pub lightmap_samples: Vec<Color>,
}

impl LightmapGI {
    /// Creates a new LightmapGI with Godot-compatible defaults.
    pub fn new(id: LightmapGIId) -> Self {
        Self {
            id,
            transform: Transform3D::IDENTITY,
            quality: LightmapBakeQuality::default(),
            bounces: 3,
            texel_scale: 1.0,
            use_denoiser: true,
            denoiser_strength: 0.1,
            directional: false,
            interior: false,
            energy: 1.0,
            bias: 0.0005,
            max_texture_size: 16384,
            generate_probes: LightmapProbeGeneration::default(),
            camera_attributes_path: None,
            baked: false,
            light_data_path: None,
            lightmap_samples: Vec::new(),
        }
    }

    /// Bake a simple lightmap sample grid from lights (pat-p0s82.10).
    ///
    /// Produces `resolution × resolution` samples in the XZ plane at y=0.
    /// This is the occluder-free path; see [`bake_samples_with_occluders`] to
    /// account for shadow casters.
    ///
    /// [`bake_samples_with_occluders`]: Self::bake_samples_with_occluders
    pub fn bake_samples(&mut self, lights: &[Light3D], resolution: u32) {
        self.bake_samples_with_occluders(lights, &[], resolution);
    }

    /// Bake a lightmap sample grid, tracing a shadow ray from each texel to
    /// each light and dropping the light's contribution when an occluder
    /// blocks it (pat-p0s82.10).
    ///
    /// Produces `resolution × resolution` samples in the XZ plane at y=0. The
    /// net effect is that the side of a surface facing away from a light — the
    /// side behind an occluder — bakes darker than the directly lit side.
    pub fn bake_samples_with_occluders(
        &mut self,
        lights: &[Light3D],
        occluders: &[OccluderInstance3D],
        resolution: u32,
    ) {
        // Pre-compute world AABBs of occluders that carry a shape.
        let occluder_aabbs: Vec<Aabb> =
            occluders.iter().filter_map(|o| o.get_world_aabb()).collect();

        let res = resolution.max(2);
        self.lightmap_samples = Vec::with_capacity((res * res) as usize);
        for z in 0..res {
            for x in 0..res {
                let fx = (x as f32 / (res - 1) as f32) * 2.0 - 1.0;
                let fz = (z as f32 / (res - 1) as f32) * 2.0 - 1.0;
                let pos = Vector3::new(fx * 10.0, 0.0, fz * 10.0);
                let mut r = 0.0f32;
                let mut g = 0.0f32;
                let mut b = 0.0f32;
                for light in lights {
                    let att = match light.light_type {
                        LightType::Directional => {
                            if self.interior {
                                continue;
                            }
                            1.0
                        }
                        LightType::Point | LightType::Spot => {
                            let d = (light.position - pos).length().max(1.0);
                            1.0 / (d * d)
                        }
                    };
                    // Trace toward the light. `visibility` is 0/1 for a hard
                    // filter and fractional across a soft filter's penumbra
                    // (pat-p0s82.25).
                    let visibility = light_visibility(pos, light, &occluder_aabbs);
                    if visibility <= 0.0 {
                        continue;
                    }
                    // A light projector tints its contribution per-texel.
                    let gel = light.projector_modulate(pos);
                    let scale = light.energy * att * visibility;
                    r += light.color.r * gel.r * scale;
                    g += light.color.g * gel.g * scale;
                    b += light.color.b * gel.b * scale;
                }
                self.lightmap_samples
                    .push(Color::new(r * self.energy, g * self.energy, b * self.energy, 1.0));
            }
        }
        self.baked = true;
        self.light_data_path = Some(format!("res://lightmaps/{}.lmbake", self.id.0));
    }

    /// Sample lightmap by world XZ in \[-10,10\].
    pub fn sample_lightmap(&self, world: Vector3) -> Color {
        if !self.baked || self.lightmap_samples.is_empty() {
            return Color::BLACK;
        }
        let res = (self.lightmap_samples.len() as f32).sqrt() as u32;
        if res < 2 {
            return Color::BLACK;
        }
        let fx = ((world.x + 10.0) / 20.0).clamp(0.0, 0.999);
        let fz = ((world.z + 10.0) / 20.0).clamp(0.0, 0.999);
        let ix = (fx * res as f32) as usize;
        let iz = (fz * res as f32) as usize;
        self.lightmap_samples[iz * res as usize + ix]
    }

    /// Apply the baked lightmap as the indirect-lighting term for a surface
    /// with the given `albedo` at `world` position (pat-p0s82.10 shading path).
    ///
    /// The baked texel stores total irradiance, so the returned color is the
    /// albedo modulated by the sampled lightmap. When the lightmap is not baked
    /// the albedo passes through unchanged so unbaked scenes render normally.
    pub fn shade(&self, albedo: Color, world: Vector3) -> Color {
        if !self.baked || self.lightmap_samples.is_empty() {
            return albedo;
        }
        let gi = self.sample_lightmap(world);
        Color::new(albedo.r * gi.r, albedo.g * gi.g, albedo.b * gi.b, albedo.a)
    }
}

/// Returns `true` if any occluder AABB blocks the straight-line path from a
/// baked texel at `pos` to `light`.
///
/// For point/spot lights the segment runs from the texel to the light's
/// position; for directional lights it runs a long distance up the incoming
/// light direction (there is no finite light position).
fn light_occluded(pos: Vector3, light: &Light3D, occluder_aabbs: &[Aabb]) -> bool {
    if occluder_aabbs.is_empty() {
        return false;
    }
    let light_point = light_origin(pos, light);
    occluder_aabbs
        .iter()
        .any(|aabb| segment_intersects_aabb(pos, light_point, *aabb))
}

/// The point a shadow ray from `pos` aims at for `light`: the light itself for
/// point/spot lights, or a far point up the incoming direction for a
/// directional light (which has no position).
fn light_origin(pos: Vector3, light: &Light3D) -> Vector3 {
    match light.light_type {
        LightType::Directional => pos - light.direction * 1.0e4,
        LightType::Point | LightType::Spot => light.position,
    }
}

/// Fraction of `light` visible from `pos`, in `[0, 1]` (pat-p0s82.25).
///
/// With [`ShadowFilterQuality::Hard`] this takes a single tap and returns
/// exactly 0.0 or 1.0 — the original binary behavior. A soft quality spreads
/// [`Light3D::shadow_blur`] worth of taps across the disc facing the shadow ray
/// and returns the fraction that reach the light, so a texel at the edge of an
/// occluder lands strictly between 0 and 1: a penumbra.
///
/// Taps are placed on a deterministic golden-angle spiral (no RNG), so a bake
/// is reproducible.
fn light_visibility(pos: Vector3, light: &Light3D, occluder_aabbs: &[Aabb]) -> f32 {
    if occluder_aabbs.is_empty() {
        return 1.0;
    }
    let center = light_origin(pos, light);
    let taps = light.shadow_filter_quality.taps();

    // Hard (single tap) or a zero blur radius has no area to sample across:
    // fall back to the binary test so behavior is unchanged.
    if taps <= 1 || light.shadow_blur <= 0.0 {
        return if light_occluded(pos, light, occluder_aabbs) {
            0.0
        } else {
            1.0
        };
    }

    let (right, up) = shadow_tap_basis(center - pos);
    let mut unoccluded = 0u32;
    for i in 0..taps {
        // Golden-angle spiral over the disc: sqrt keeps samples area-uniform
        // rather than clustered at the center.
        let t = (i as f32 + 0.5) / taps as f32;
        let radius = light.shadow_blur * t.sqrt();
        let angle = i as f32 * 2.399_963_2; // ~137.5° in radians
        let offset = right * (radius * angle.cos()) + up * (radius * angle.sin());
        let sample_point = center + offset;
        if !occluder_aabbs
            .iter()
            .any(|aabb| segment_intersects_aabb(pos, sample_point, *aabb))
        {
            unoccluded += 1;
        }
    }
    unoccluded as f32 / taps as f32
}

/// Orthonormal basis spanning the disc perpendicular to `axis`, used to offset
/// soft-shadow taps around the light.
fn shadow_tap_basis(axis: Vector3) -> (Vector3, Vector3) {
    let a = axis.normalized();
    let reference = if a.y.abs() > 0.99 {
        Vector3::new(1.0, 0.0, 0.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let right = reference.cross(a).normalized();
    let up = a.cross(right).normalized();
    (right, up)
}

/// Slab-test a segment `p0`→`p1` against an axis-aligned box.
///
/// Returns `true` when the segment enters the box for some parameter
/// `t ∈ [0, 1]`.
fn segment_intersects_aabb(p0: Vector3, p1: Vector3, aabb: Aabb) -> bool {
    let min = aabb.position;
    let max = aabb.position + aabb.size;
    let dir = p1 - p0;
    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;
    for axis in 0..3 {
        let (o, d, lo, hi) = match axis {
            0 => (p0.x, dir.x, min.x, max.x),
            1 => (p0.y, dir.y, min.y, max.y),
            _ => (p0.z, dir.z, min.z, max.z),
        };
        if d.abs() < 1e-8 {
            // Segment is parallel to this slab; reject if the origin lies
            // outside the slab's extent.
            if o < lo || o > hi {
                return false;
            }
        } else {
            let inv = 1.0 / d;
            let mut t1 = (lo - o) * inv;
            let mut t2 = (hi - o) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::light::{Light3DId, LightProjector, ShadowFilterQuality};
    use crate::occluder::Occluder3D;

    // -- pat-p0s82.25: soft shadow filter + light projector -------------------

    /// Fixture matching `lightmapgi_occluder_darkens_opposite_side`: a box over
    /// x∈[0,10], y∈[4,6] lit from straight above, so the shadow edge falls at
    /// x = 0 and a texel there straddles it.
    fn edge_shadow_occluders() -> Vec<Aabb> {
        let mut occ = OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(
            10.0, 2.0, 40.0,
        )));
        occ.transform.origin = Vector3::new(5.0, 5.0, 0.0);
        occ.get_world_aabb().into_iter().collect()
    }

    /// A hard filter keeps the binary edge: every texel is fully lit or fully
    /// shadowed, never in between. This is the pre-bead behavior and the
    /// default, so existing bakes are unchanged.
    #[test]
    fn hard_shadow_filter_is_binary() {
        let light = Light3D::directional(Light3DId(1));
        assert_eq!(light.shadow_filter_quality, ShadowFilterQuality::Hard);
        let occluders = edge_shadow_occluders();

        // On the shadow edge, and deep under the box: fully shadowed.
        assert_eq!(
            light_visibility(Vector3::new(0.0, 0.0, 0.0), &light, &occluders),
            0.0,
        );
        assert_eq!(
            light_visibility(Vector3::new(5.0, 0.0, 0.0), &light, &occluders),
            0.0,
        );
        // Clear of the box: fully lit.
        assert_eq!(
            light_visibility(Vector3::new(-8.0, 0.0, 0.0), &light, &occluders),
            1.0,
        );
    }

    /// The bead's soft option: with a soft quality and a blur radius, a texel on
    /// the shadow edge is only PARTIALLY occluded — a penumbra — rather than
    /// snapping to 0 or 1.
    #[test]
    fn pcf_soft_filter_produces_penumbra_at_shadow_edge() {
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_filter_quality = ShadowFilterQuality::High;
        light.shadow_blur = 3.0;
        let occluders = edge_shadow_occluders();

        let edge = light_visibility(Vector3::new(0.0, 0.0, 0.0), &light, &occluders);
        assert!(
            edge > 0.0 && edge < 1.0,
            "a texel on the shadow edge must be partially lit (penumbra), got {edge}",
        );

        // The penumbra must not leak into the umbra or dim the open area.
        assert_eq!(
            light_visibility(Vector3::new(5.0, 0.0, 0.0), &light, &occluders),
            0.0,
            "deep under the occluder must stay fully shadowed",
        );
        assert_eq!(
            light_visibility(Vector3::new(-8.0, 0.0, 0.0), &light, &occluders),
            1.0,
            "well clear of the occluder must stay fully lit",
        );
    }

    /// A soft quality with no blur radius has no area to filter across, so it
    /// degrades to the hard binary edge rather than silently sampling a point.
    #[test]
    fn soft_filter_without_blur_radius_stays_binary() {
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_filter_quality = ShadowFilterQuality::High;
        light.shadow_blur = 0.0;
        let occluders = edge_shadow_occluders();

        assert_eq!(
            light_visibility(Vector3::new(0.0, 0.0, 0.0), &light, &occluders),
            0.0,
        );
    }

    /// Higher quality means more taps, so the edge is quantized more finely.
    #[test]
    fn shadow_filter_quality_tap_counts_match_levels() {
        assert_eq!(ShadowFilterQuality::Hard.taps(), 1);
        assert_eq!(ShadowFilterQuality::Low.taps(), 4);
        assert_eq!(ShadowFilterQuality::Medium.taps(), 8);
        assert_eq!(ShadowFilterQuality::High.taps(), 16);
        assert!(!ShadowFilterQuality::Hard.is_soft());
        assert!(ShadowFilterQuality::High.is_soft());
    }

    /// Visual test: a spot light with a red projector bakes a red-tinted
    /// lightmap, where the same light without a projector bakes neutral.
    #[test]
    fn light_projector_modulates_baked_lightmap() {
        let spot = |projector: Option<LightProjector>| {
            let mut l = Light3D::spot(
                Light3DId(1),
                Vector3::new(0.0, 5.0, 0.0),
                Vector3::new(0.0, -1.0, 0.0),
            );
            l.projector = projector;
            l
        };

        let mut plain = LightmapGI::new(LightmapGIId(1));
        plain.bake_samples(&[spot(None)], 16);
        let neutral = plain.sample_lightmap(Vector3::ZERO);

        let mut tinted = LightmapGI::new(LightmapGIId(2));
        tinted.bake_samples(
            &[spot(Some(LightProjector::solid(Color::new(1.0, 0.0, 0.0, 1.0))))],
            16,
        );
        let red = tinted.sample_lightmap(Vector3::ZERO);

        assert!(
            neutral.g > 0.0 && neutral.b > 0.0,
            "without a projector the light stays neutral, got {neutral:?}",
        );
        assert!(
            red.r > 0.0,
            "a red projector must still light the red channel, got {red:?}",
        );
        assert_eq!(
            (red.g, red.b),
            (0.0, 0.0),
            "a red projector must cut the green/blue contribution, got {red:?}",
        );
    }

    // -- VoxelGI tests --------------------------------------------------------

    #[test]
    fn voxelgi_defaults_match_godot() {
        let gi = VoxelGI::new(VoxelGIId(1));
        assert_eq!(gi.size, Vector3::new(20.0, 20.0, 20.0));
        assert_eq!(gi.subdiv, VoxelGISubdiv::Subdiv128);
        assert!((gi.energy - 1.0).abs() < f32::EPSILON);
        assert!((gi.bias - 1.5).abs() < f32::EPSILON);
        assert!((gi.normal_bias - 0.0).abs() < f32::EPSILON);
        assert!((gi.propagation - 0.7).abs() < f32::EPSILON);
        assert!(!gi.interior);
        assert!(!gi.baked);
        assert!(gi.camera_attributes_path.is_none());
    }

    #[test]
    fn voxelgi_id_equality() {
        assert_eq!(VoxelGIId(10), VoxelGIId(10));
        assert_ne!(VoxelGIId(10), VoxelGIId(20));
    }

    #[test]
    fn voxelgi_contains_point_at_center() {
        let gi = VoxelGI::new(VoxelGIId(1));
        assert!(gi.contains_point(Vector3::ZERO));
    }

    #[test]
    fn voxelgi_contains_point_at_edge() {
        let gi = VoxelGI::new(VoxelGIId(1));
        assert!(gi.contains_point(Vector3::new(10.0, 10.0, 10.0)));
        assert!(!gi.contains_point(Vector3::new(10.1, 0.0, 0.0)));
    }

    #[test]
    fn voxelgi_contains_point_with_transform() {
        let mut gi = VoxelGI::new(VoxelGIId(1));
        gi.transform.origin = Vector3::new(100.0, 0.0, 0.0);
        let (min, max) = gi.world_aabb_min_max();
        assert!((min.x - 90.0).abs() < f32::EPSILON);
        assert!((max.x - 110.0).abs() < f32::EPSILON);
        assert!(gi.contains_point(Vector3::new(100.0, 0.0, 0.0)));
        assert!(!gi.contains_point(Vector3::ZERO));
    }

    #[test]
    fn voxelgi_grid_resolution() {
        let mut gi = VoxelGI::new(VoxelGIId(1));
        assert_eq!(gi.grid_resolution(), 128);

        gi.subdiv = VoxelGISubdiv::Subdiv64;
        assert_eq!(gi.grid_resolution(), 64);

        gi.subdiv = VoxelGISubdiv::Subdiv256;
        assert_eq!(gi.grid_resolution(), 256);

        gi.subdiv = VoxelGISubdiv::Subdiv512;
        assert_eq!(gi.grid_resolution(), 512);
    }

    #[test]
    fn voxelgi_subdiv_values() {
        assert_eq!(VoxelGISubdiv::Subdiv64 as u32, 0);
        assert_eq!(VoxelGISubdiv::Subdiv128 as u32, 1);
        assert_eq!(VoxelGISubdiv::Subdiv256 as u32, 2);
        assert_eq!(VoxelGISubdiv::Subdiv512 as u32, 3);
    }

    #[test]
    fn voxelgi_sample_varies_spatially_near_point_light() {
        let mut gi = VoxelGI::new(VoxelGIId(1));
        gi.size = Vector3::new(20.0, 20.0, 20.0);
        let mut light = Light3D::directional(Light3DId(1));
        light.light_type = LightType::Point;
        light.position = Vector3::new(8.0, 0.0, 0.0);
        light.color = Color::WHITE;
        light.energy = 10.0;
        gi.bake(&[light]);
        let near = gi.sample(Vector3::new(7.0, 0.0, 0.0));
        let far = gi.sample(Vector3::new(-7.0, 0.0, 0.0));
        assert!(
            near.r > far.r,
            "near GI {} should exceed far {}",
            near.r,
            far.r
        );
    }

    // -- LightmapGI tests ----------------------------------------------------

    #[test]
    fn lightmapgi_defaults_match_godot() {
        let gi = LightmapGI::new(LightmapGIId(1));
        assert_eq!(gi.quality, LightmapBakeQuality::Medium);
        assert_eq!(gi.bounces, 3);
        assert!((gi.texel_scale - 1.0).abs() < f32::EPSILON);
        assert!(gi.use_denoiser);
        assert!((gi.denoiser_strength - 0.1).abs() < f32::EPSILON);
        assert!(!gi.directional);
        assert!(!gi.interior);
        assert!((gi.energy - 1.0).abs() < f32::EPSILON);
        assert!((gi.bias - 0.0005).abs() < f32::EPSILON);
        assert_eq!(gi.max_texture_size, 16384);
        assert_eq!(gi.generate_probes, LightmapProbeGeneration::Disabled);
        assert!(gi.camera_attributes_path.is_none());
        assert!(!gi.baked);
        assert!(gi.light_data_path.is_none());
    }

    #[test]
    fn lightmapgi_id_equality() {
        assert_eq!(LightmapGIId(10), LightmapGIId(10));
        assert_ne!(LightmapGIId(10), LightmapGIId(20));
    }

    #[test]
    fn lightmapgi_bake_quality_values() {
        assert_eq!(LightmapBakeQuality::Low as u32, 0);
        assert_eq!(LightmapBakeQuality::Medium as u32, 1);
        assert_eq!(LightmapBakeQuality::High as u32, 2);
        assert_eq!(LightmapBakeQuality::Ultra as u32, 3);
    }

    #[test]
    fn lightmapgi_probe_generation_values() {
        assert_eq!(LightmapProbeGeneration::Disabled as u32, 0);
        assert_eq!(LightmapProbeGeneration::Low as u32, 1);
        assert_eq!(LightmapProbeGeneration::Medium as u32, 2);
        assert_eq!(LightmapProbeGeneration::High as u32, 3);
    }

    #[test]
    fn lightmapgi_set_light_data() {
        let mut gi = LightmapGI::new(LightmapGIId(1));
        assert!(gi.light_data_path.is_none());

        gi.light_data_path = Some("res://lightmap_data.lmbake".to_string());
        gi.baked = true;
        assert!(gi.baked);
        assert_eq!(
            gi.light_data_path.as_deref(),
            Some("res://lightmap_data.lmbake")
        );
    }

    #[test]
    fn lightmapgi_interior_disables_sky() {
        let mut gi = LightmapGI::new(LightmapGIId(1));
        assert!(!gi.interior);
        gi.interior = true;
        assert!(gi.interior);
    }

    #[test]
    fn lightmapgi_bake_and_sample_roundtrip() {
        let mut gi = LightmapGI::new(LightmapGIId(1));
        let mut light = Light3D::directional(Light3DId(1));
        light.color = Color::WHITE;
        light.energy = 1.0;
        gi.bake_samples(&[light], 8);
        assert!(gi.baked);
        assert_eq!(gi.lightmap_samples.len(), 64);
        assert!(gi.light_data_path.is_some());
        // A fully-lit directional bake reads bright everywhere.
        let c = gi.sample_lightmap(Vector3::new(0.0, 0.0, 0.0));
        assert!(c.r > 0.5, "directional bake should be lit, got {}", c.r);
    }

    #[test]
    fn lightmapgi_unbaked_sample_is_black() {
        let gi = LightmapGI::new(LightmapGIId(1));
        assert_eq!(gi.sample_lightmap(Vector3::ZERO), Color::BLACK);
    }

    /// Fixture: a box occluder floating above the +X half of the bake plane.
    /// A directional light straight down should leave the shadowed (+X) side
    /// markedly darker than the exposed (-X) side.
    #[test]
    fn lightmapgi_occluder_darkens_opposite_side() {
        let mut gi = LightmapGI::new(LightmapGIId(1));
        let light = Light3D::directional(Light3DId(1)); // points (0,-1,0)

        // Box over x∈[0,10], y∈[4,6], spanning z, casting a downward shadow.
        let mut occ = OccluderInstance3D::with_occluder(Occluder3D::new_box(
            Vector3::new(10.0, 2.0, 40.0),
        ));
        occ.transform.origin = Vector3::new(5.0, 5.0, 0.0);

        gi.bake_samples_with_occluders(&[light], &[occ], 16);

        let lit = gi.sample_lightmap(Vector3::new(-8.0, 0.0, 0.0));
        let shadowed = gi.sample_lightmap(Vector3::new(8.0, 0.0, 0.0));
        assert!(
            lit.r > shadowed.r + 0.5,
            "exposed side ({}) should be much brighter than occluded side ({})",
            lit.r,
            shadowed.r
        );
        assert!(
            shadowed.r < 0.01,
            "fully occluded texel should be dark, got {}",
            shadowed.r
        );
    }

    #[test]
    fn lightmapgi_bake_without_occluders_is_uniform() {
        // No occluders → the occluder-aware path matches the plain path.
        let mut a = LightmapGI::new(LightmapGIId(1));
        let mut b = LightmapGI::new(LightmapGIId(1));
        let light = Light3D::directional(Light3DId(1));
        a.bake_samples(&[light.clone()], 8);
        b.bake_samples_with_occluders(&[light], &[], 8);
        assert_eq!(a.lightmap_samples, b.lightmap_samples);
        // A uniform directional bake has no dark spots.
        assert!(a.lightmap_samples.iter().all(|c| c.r > 0.5));
    }

    #[test]
    fn lightmapgi_shade_modulates_albedo_by_lightmap() {
        let mut gi = LightmapGI::new(LightmapGIId(1));
        let light = Light3D::directional(Light3DId(1));
        let mut occ = OccluderInstance3D::with_occluder(Occluder3D::new_box(
            Vector3::new(10.0, 2.0, 40.0),
        ));
        occ.transform.origin = Vector3::new(5.0, 5.0, 0.0);
        gi.bake_samples_with_occluders(&[light], &[occ], 16);

        let albedo = Color::new(0.8, 0.4, 0.2, 1.0);
        let lit = gi.shade(albedo, Vector3::new(-8.0, 0.0, 0.0));
        let shadowed = gi.shade(albedo, Vector3::new(8.0, 0.0, 0.0));
        // Shaded albedo keeps its hue where lit and goes dark in shadow.
        assert!(lit.r > shadowed.r + 0.3);
        assert!(shadowed.r < 0.01);
        assert!((lit.a - 1.0).abs() < f32::EPSILON, "alpha preserved");
    }

    #[test]
    fn lightmapgi_shade_passthrough_when_unbaked() {
        let gi = LightmapGI::new(LightmapGIId(1));
        let albedo = Color::new(0.3, 0.6, 0.9, 1.0);
        assert_eq!(gi.shade(albedo, Vector3::ZERO), albedo);
    }
}
